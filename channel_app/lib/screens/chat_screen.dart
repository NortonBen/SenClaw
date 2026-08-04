import 'dart:convert';
import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:image_picker/image_picker.dart';
import 'package:speech_to_text/speech_to_text.dart' as stt;
import '../models/agent_model.dart';
import '../models/api_models.dart';
import '../models/session_model.dart';
import '../services/relay_service.dart';
import '../services/relay_manager.dart';
import '../services/config_service.dart';
import '../services/chat_api.dart';
import '../services/language_service.dart';
import '../services/local_cache.dart';
import '../services/llm_api.dart';
import '../services/logger_service.dart';
import '../services/sessions_provider.dart';
import '../theme/tokens.dart';
import '../widgets/app_drawer.dart';
import '../widgets/interaction_cards.dart';
import '../widgets/markdown_text.dart';
import '../widgets/widget_card.dart';
import 'agent_select_screen.dart';
import 'new_chat_screen.dart';

class ChatMessage {
  final String text;
  final bool isFromMe;
  final bool isHistory;
  final DateTime? timestamp;
  final Duration? latency;
  final String role; // 'user', 'agent', 'other', 'tool', 'permission', 'question', 'form', 'plan'

  // Tool-execution card fields (role == 'tool').
  final String? toolName;
  final String? toolSummary;
  final bool toolOk;

  // Interaction card fields (role == 'permission' | 'question').
  final String? requestId;
  final Map<String, dynamic>? interaction;
  bool resolved;
  String? resolvedText;

  ChatMessage(
    this.text,
    this.isFromMe, {
    this.isHistory = false,
    this.timestamp,
    this.latency,
    String? role,
    this.toolName,
    this.toolSummary,
    this.toolOk = true,
    this.requestId,
    this.interaction,
    this.resolved = false,
    this.resolvedText,
  }) : role = role ?? (isFromMe ? 'user' : 'agent');
}

class ChatScreen extends ConsumerStatefulWidget {
  const ChatScreen({super.key});

  @override
  ConsumerState<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends ConsumerState<ChatScreen> {
  static const _agentListTimeout = Duration(seconds: 40);

  /// Max time the busy/typing indicator may stay on without any fresh
  /// state/reply event before we re-check the daemon (see [_onBusyTimeout]).
  static const _busyTimeout = Duration(seconds: 90);

  final _config = ConfigService();
  final _relayManager = RelayManager();
  final _chatApi = ChatApi();
  final _cache = LocalCache();
  final _messageController = TextEditingController();
  final _scrollController = ScrollController();
  final List<StreamSubscription> _subs = [];

  RelayService? _relay;
  Timer? _loadTimeout;
  Timer? _agentListPoll;
  Timer? _busyWatchdog;
  Timer? _deltaSyncDebounce;

  /// The active session's JID on the daemon. Defaults to this device's single
  /// session (`app:<channelId>:user:mobile-app`) and follows the selected
  /// session once multi-session is in play. Keys the local message cache,
  /// sync cursor and agent-state lookups.
  String? _chatJid;

  /// A first message queued to send once a freshly-created session becomes
  /// active (see [_openNewChat] → [_switchSession]).
  String? _pendingFirstMessage;

  /// Server message ids already rendered — dedups overlap between the local
  /// cache, HISTORY_RESP pages and REST delta fetches.
  final Set<String> _seenMessageIds = {};
  bool _deltaSyncInFlight = false;
  bool _pendingDeltaSync = false;

  // Composer: image attachments (base64 data URLs), model picker, voice input.
  final _picker = ImagePicker();
  final List<String> _attachments = [];
  final _llmApi = LlmApi();
  String _modelLabel = 'Model';
  final stt.SpeechToText _speech = stt.SpeechToText();
  bool _sttAvailable = false;
  bool _recording = false;

  static final _dataUrlRe =
      RegExp(r'data:image/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=]+');

  final List<ChatMessage> _messages = [];
  bool _isTyping = false;

  List<AgentInfo> _agents = [];
  AgentInfo? _selectedAgent;
  String _agentState = '';
  bool _agentLoaded = false;
  int _currentPage = 1;
  bool _hasMoreHistory = true;
  bool _isLoadingMore = false;

  String _statusText = tr('Đang kết nối tới relay…', 'Connecting to relay…');
  bool _loadTimedOut = false;
  DateTime? _lastSendTime;

  @override
  void initState() {
    super.initState();
    _initRelay();
    _loadActiveModel();
    _initSpeech();
  }

  Future<void> _initRelay() async {
    final started = await _relayManager.ensureStarted();
    if (!started) {
      if (!mounted) return;
      setState(() {
        _loadTimedOut = true;
        _statusText = tr(
            'Thiếu dữ liệu ghép cặp — hãy quét lại mã QR để kết nối.',
            'Missing pairing data — scan the QR code again to connect.');
      });
      return;
    }

    final relay = _relayManager.relay!;
    _relay = relay;
    Log.i('[Chat] Dùng relay chung từ RelayManager');

    // JID the daemon files this device's conversation under — keys the local
    // cache and the /api/chat/states lookup.
    final cid = await _config.channelId;
    _chatJid = cid == null ? null : 'app:$cid:user:${RelayManager.senderId}';

    _subs.add(relay.incomingMessages.listen((text) {
      if (!mounted) return;
      Log.d(
        '[Chat] Tin nhắn mới từ agent: "${text.length > 60 ? text.substring(0, 60) : text}…"',
      );

      Duration? latency;
      if (_lastSendTime != null) {
        latency = DateTime.now().difference(_lastSendTime!);
        _lastSendTime = null; // reset sau khi nhận được chunk đầu tiên
      }

      setState(
        () => _messages.add(
          ChatMessage(
            text,
            false,
            latency: latency,
            timestamp: DateTime.now(),
            role: 'agent',
          ),
        ),
      );
      // A reply is activity: keep the busy watchdog fresh, and pull the
      // server-authoritative row into the local cache shortly after.
      _restartBusyWatchdog();
      _scheduleDeltaSync();
      _scrollToBottom();
    }));

    _subs.add(relay.typingUpdates.listen((typing) {
      if (!mounted) return;
      setState(() => _isTyping = typing);
      _restartBusyWatchdog();
    }));

    // Events emitted while the relay socket was down (~50s reconnect cycle)
    // are lost — on every (re)connect reconcile agent state + message delta.
    _subs.add(relay.connectionUpdates.listen((connected) {
      if (!mounted || !connected) return;
      unawaited(_onRelayConnected());
    }));

    _subs.add(relay.agentListUpdates.listen(_onAgentList));
    _subs.add(relay.historyUpdates.listen(_onHistory));
    _subs.add(relay.apiEvents.listen(_onApiEvent));

    _scrollController.addListener(() {
      if (_scrollController.position.pixels >=
              _scrollController.position.maxScrollExtent - 200 &&
          !_isLoadingMore &&
          _hasMoreHistory &&
          _selectedAgent != null) {
        _loadMoreHistory();
      }
    });

    // The shared relay may have already received the agent list before this
    // screen mounted — replay it; otherwise render the local-cache snapshot
    // for an instant UI and (re)request the live list.
    if (_relayManager.agents.isNotEmpty && _relayManager.agentsFresh) {
      _onAgentList(_relayManager.agents);
    } else {
      unawaited(_applyCachedAgents());
      _relayManager.requestAgentList();
    }

    // Keep asking until the list arrives — a request sent while the Senclaw
    // daemon is mid-reconnect on the hub is silently dropped, so a one-shot can
    // leave the UI stuck on "daemon không phản hồi". This makes it self-heal.
    _startAgentListPoll();

    _loadTimeout = Timer(_agentListTimeout, () {
      if (!mounted || _agentLoaded) return;
      final hubOk = _relay?.hasReceivedInboundHubData ?? false;
      if (hubOk) {
        Log.w(
          '[Chat] Timeout — hub đã phản hồi nhưng chưa có AGENT_LIST (cần Senclaw kết nối cùng kênh)',
        );
      } else {
        Log.w(
          '[Chat] Timeout — không có tin từ hub (mạng, domain, ghép cặp hoặc hub)',
        );
      }
      setState(() {
        _loadTimedOut = true;
        _statusText = hubOk
            ? tr(
                'Hub đã kết nối — chưa có Senclaw trên kênh này. Hãy chạy Senclaw với relay tới cùng hub.',
                'Hub connected — no Senclaw on this channel yet. Run Senclaw with a relay to the same hub.')
            : tr(
                'Không nhận được phản hồi từ hub — kiểm tra mạng, domain và ghép cặp (token/kênh).',
                'No response from the hub — check network, domain and pairing (token/channel).');
      });
    });
  }

  /// While the agent list hasn't arrived, periodically re-request it so the
  /// screen recovers automatically once the Senclaw daemon is back on the
  /// channel (its relay socket may have dropped & reconnected, dropping the
  /// initial request). Stops as soon as a list — even an empty one — arrives.
  void _startAgentListPoll() {
    _agentListPoll?.cancel();
    _agentListPoll = Timer.periodic(const Duration(seconds: 6), (_) {
      if (!mounted || _agentLoaded) {
        _agentListPoll?.cancel();
        _agentListPoll = null;
        return;
      }
      if (_relayManager.connected) {
        Log.d('[Chat] Chưa có agent — tự xin lại danh sách');
        _relayManager.requestAgentList();
      }
    });
  }

  Future<void> _onAgentList(List<AgentInfo> agents) async {
    if (!mounted) return;

    _loadTimeout?.cancel();
    _agentListPoll?.cancel();
    _agentListPoll = null;
    Log.i(
      '[Chat] Nhận danh sách agent: ${agents.length} — ${agents.map((a) => a.name).join(', ')}',
    );

    setState(() {
      _agents = agents;
      _agentLoaded = true;
      _loadTimedOut = false;
      _statusText = agents.isEmpty
          ? tr('Không có profile nào được bind với kênh này',
              'No profile is bound to this channel')
          : tr('Đã tải ${agents.length} profile',
              'Loaded ${agents.length} profiles');
    });

    if (agents.isEmpty) return;

    // Already chatting with an agent that's still bound? Do NOT re-select — that
    // would clear the messages and reload history, making the screen flicker
    // every time the agent list re-arrives (relay reconnect, snapshot replay,
    // the waiting-poll). Just keep the current conversation.
    if (_selectedAgent != null &&
        agents.any((a) => a.folder == _selectedAgent!.folder)) {
      return;
    }

    final savedFolder = await _config.selectedAgentFolder;
    if (savedFolder != null) {
      final saved = agents.where((a) => a.folder == savedFolder).firstOrNull;
      if (saved != null) {
        _selectAgent(saved, sendSelect: false);
        return;
      }
    }

    if (agents.length == 1) {
      _selectAgent(agents.first);
    } else if (mounted) {
      final chosen = await AgentSelectScreen.show(
        context,
        agents: agents,
        selected: _selectedAgent,
      );
      if (chosen != null) _selectAgent(chosen);
    }
  }

  /// Render the cached agent list (and the saved agent's cached history)
  /// immediately while the live AGENT_LIST_RESP is still in flight over the
  /// relay. Deliberately does NOT set [_agentLoaded]: the connecting banner
  /// stays up and the agent-list poll keeps running until real data arrives.
  Future<void> _applyCachedAgents() async {
    final cached = _relayManager.agents.isNotEmpty
        ? _relayManager.agents
        : await _cache.getAgents();
    if (!mounted || _agentLoaded || cached.isEmpty) return;
    Log.i('[Chat] Hiển thị ${cached.length} agent từ bộ nhớ đệm');
    setState(() => _agents = cached);

    final savedFolder = await _config.selectedAgentFolder;
    AgentInfo? pick = savedFolder == null
        ? null
        : cached.where((a) => a.folder == savedFolder).firstOrNull;
    pick ??= cached.length == 1 ? cached.first : null;
    if (!mounted || _agentLoaded || _selectedAgent != null || pick == null) {
      return;
    }
    _selectAgent(pick, sendSelect: false);
  }

  void _onHistory(List<HistoryMessage> history) {
    if (!mounted) return;
    Log.i(
      '[Chat] Nhận lịch sử: ${history.length} tin cho agent "${_selectedAgent?.name}"',
    );

    // Overlap with the local cache / delta fetches is deduped by message id
    // (the page-offset math is approximate once the cache pre-filled the UI).
    final fresh = history
        .where((m) => m.id.isEmpty || !_seenMessageIds.contains(m.id))
        .toList();
    for (final m in fresh) {
      if (m.id.isNotEmpty) _seenMessageIds.add(m.id);
    }

    final histMsgs = fresh.map((m) {
      final ts = DateTime.tryParse(m.timestamp)?.toLocal();
      return ChatMessage(
        m.content,
        m.isFromMe,
        isHistory: true,
        timestamp: ts,
        role: m.role.isEmpty ? (m.isBotReply ? 'agent' : 'user') : m.role,
      );
    }).toList();

    setState(() {
      _messages.insertAll(0, histMsgs.reversed);
      // The daemon mixes timestamp formats — bot replies as local
      // "yyyy-MM-dd HH:mm:ss", user messages as ISO-UTC — and orders them by
      // STRING, which mis-sorts the page (e.g. a reply lands before its prompt).
      // Re-sort the whole conversation by real (parsed) time so positions line
      // up oldest → newest.
      final epoch0 = DateTime.fromMillisecondsSinceEpoch(0);
      _messages.sort((a, b) =>
          (a.timestamp ?? epoch0).compareTo(b.timestamp ?? epoch0));
      _isLoadingMore = false;
      if (history.isEmpty || history.length < 20) {
        _hasMoreHistory = false;
      }
    });

    if (_currentPage == 1) {
      _scrollToBottom(animate: false);
    }
  }

  /// Server-pushed agent activity (tool executions, live state) forwarded over
  /// the relay as API_EVENT frames. agent:reply / incoming are NOT here — those
  /// still arrive via the encrypted chat path.
  void _onApiEvent(ApiEvent event) {
    if (!mounted) return;
    final data = event.data;
    if (event.topic == 'tool:execution' && data is Map) {
      final m = data.cast<String, dynamic>();
      setState(() {
        _messages.add(ChatMessage(
          '',
          false,
          role: 'tool',
          timestamp: DateTime.now(),
          toolName: (m['toolName'] ?? 'tool').toString(),
          toolSummary: (m['summary'] ?? m['title'] ?? '').toString(),
          toolOk: m['ok'] as bool? ?? true,
        ));
      });
      _restartBusyWatchdog(); // tool activity — the agent is alive
      _scrollToBottom();
    } else if (event.topic == 'agent:state' && data is Map) {
      setState(() => _agentState = (data['state'] ?? '').toString());
      _restartBusyWatchdog();
    } else if (event.topic == 'permission:request' && data is Map) {
      final m = data.cast<String, dynamic>();
      _addInteraction('permission', (m['requestId'] ?? '').toString(), m);
    } else if (event.topic == 'question:request' && data is Map) {
      final m = data.cast<String, dynamic>();
      _addInteraction('question', (m['requestId'] ?? '').toString(), m);
    } else if (event.topic == 'form:request' && data is Map) {
      final m = data.cast<String, dynamic>();
      _addInteraction('form', (m['requestId'] ?? '').toString(), m);
    } else if (event.topic == 'form:resolved' && data is Map) {
      _markResolved((data['requestId'] ?? '').toString(), null);
    } else if (event.topic == 'permission:resolved' && data is Map) {
      _markResolved((data['requestId'] ?? '').toString(),
          (data['optionLabel'] ?? data['optionKey'] ?? '').toString());
    } else if (event.topic == 'question:resolved' && data is Map) {
      _markResolved((data['requestId'] ?? '').toString(), null);
    } else if (event.topic == 'chat:widget' && data is Map) {
      // One-way rich widget push. Key by widget id so a snapshot replay or
      // re-broadcast doesn't duplicate the card.
      final m = data.cast<String, dynamic>();
      final widgetSpec = m['widget'];
      if (widgetSpec is Map) {
        _addInteraction('widget', 'widget:${m['id'] ?? ''}',
            widgetSpec.cast<String, dynamic>());
      }
    } else if (event.topic == 'plan:exit:request' && data is Map) {
      final m = data.cast<String, dynamic>();
      _addInteraction('plan', _planKey(m), m);
    } else if (event.topic == 'plan:exit:response' && data is Map) {
      _markResolved(
        _planKey(data.cast<String, dynamic>()),
        (data['selected'] ?? '').toString(),
      );
    }
  }

  // Plan events carry no requestId; key by group+agent so the response matches.
  String _planKey(Map<String, dynamic> m) =>
      '${m['groupJid'] ?? ''}|${m['agentId'] ?? 'main'}';

  void _addInteraction(String role, String requestId, Map<String, dynamic> data) {
    if (requestId.isEmpty) return;
    // Avoid duplicate cards (snapshot replay / re-broadcast).
    if (_messages.any((m) => m.requestId == requestId)) return;
    setState(() {
      _messages.add(ChatMessage(
        '',
        false,
        role: role,
        timestamp: DateTime.now(),
        requestId: requestId,
        interaction: data,
      ));
    });
    _scrollToBottom();
  }

  void _markResolved(String requestId, String? label) {
    if (requestId.isEmpty) return;
    final idx = _messages.indexWhere((m) => m.requestId == requestId);
    if (idx < 0) return;
    setState(() {
      _messages[idx].resolved = true;
      if (label != null && label.isNotEmpty) {
        _messages[idx].resolvedText = label;
      }
    });
  }

  Future<void> _respondPermission(
      ChatMessage msg, String key, String label) async {
    setState(() {
      msg.resolved = true;
      msg.resolvedText = label;
    });
    try {
      await ChatApi().respondPermission(msg.requestId!, key);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content:
                    Text(tr('Lỗi gửi phản hồi: $e', 'Error sending response: $e'))));
      }
    }
  }

  Future<void> _respondQuestion(
      ChatMessage msg, Map<String, dynamic> answers) async {
    setState(() => msg.resolved = true);
    try {
      await ChatApi().respondQuestion(msg.requestId!, answers);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content:
                    Text(tr('Lỗi gửi trả lời: $e', 'Error sending answer: $e'))));
      }
    }
  }

  Future<void> _respondForm(
      ChatMessage msg, Map<String, dynamic> values, bool submitted) async {
    setState(() => msg.resolved = true);
    try {
      await ChatApi().respondForm(msg.requestId!, values, submitted: submitted);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Lỗi gửi biểu mẫu: $e')));
      }
    }
  }

  Future<void> _respondPlan(ChatMessage msg, String selected) async {
    final data = msg.interaction ?? const {};
    final groupJid = (data['groupJid'] ?? '').toString();
    final agentId = (data['agentId'] ?? 'main').toString();
    setState(() {
      msg.resolved = true;
      msg.resolvedText = selected;
    });
    try {
      await ChatApi().respondPlan(groupJid, agentId, selected);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(
                    tr('Lỗi gửi lựa chọn: $e', 'Error sending selection: $e'))));
      }
    }
  }

  bool get _agentBusy {
    final s = _agentState.toLowerCase();
    return s == 'processing' ||
        s == 'thinking' ||
        s == 'running' ||
        s == 'working' ||
        s == 'busy';
  }

  // ── Typing/busy reconciliation ─────────────────────────────────────────────
  //
  // The busy indicator is fed by TYPING_START/STOP control frames and
  // `agent:state` API_EVENTs. The relay socket drops roughly every 50s, and
  // events emitted during the gap are lost — when that gap swallows the
  // TYPING_STOP / idle transition, the indicator sticks forever. Two guards:
  //  1. on every relay (re)connect, fetch GET /api/chat/states and reconcile;
  //  2. a watchdog clears/re-checks any busy state older than [_busyTimeout].

  void _restartBusyWatchdog() {
    _busyWatchdog?.cancel();
    _busyWatchdog = null;
    if (!(_isTyping || _agentBusy)) return;
    _busyWatchdog = Timer(_busyTimeout, () => unawaited(_onBusyTimeout()));
  }

  /// Busy for > [_busyTimeout] without any fresh event — ask the daemon for
  /// the authoritative state; clear to idle when it (or an unreachable
  /// daemon) says nothing is running.
  Future<void> _onBusyTimeout() async {
    if (!mounted || !(_isTyping || _agentBusy)) return;
    var state = 'idle';
    try {
      state = _stateForThisChat(await _chatApi.fetchAgentStates());
    } catch (_) {/* daemon unreachable — clear rather than spin forever */}
    if (!mounted) return;
    Log.i('[Chat] Busy watchdog: đối chiếu trạng thái → "$state"');
    setState(() {
      _agentState = state;
      if (!_agentBusy) _isTyping = false;
    });
    _restartBusyWatchdog(); // still busy per the daemon → keep watching
  }

  String _stateForThisChat(Map<String, String> states) {
    final jid = _chatJid;
    if (jid == null) return 'idle';
    return states[jid] ?? 'idle';
  }

  /// On relay (re)connect: events sent during the outage are gone. Fetch the
  /// authoritative agent state (fixes a stuck typing indicator) and pull the
  /// message delta the gap may have swallowed.
  Future<void> _onRelayConnected() async {
    _scheduleDeltaSync();
    try {
      final states = await _chatApi.fetchAgentStates();
      if (!mounted) return;
      setState(() {
        _agentState = _stateForThisChat(states);
        if (!_agentBusy) _isTyping = false;
      });
      _restartBusyWatchdog();
    } catch (e) {
      Log.w('[Chat] Không đối chiếu được agent state: $e');
    }
  }

  // ── Local cache + incremental sync ─────────────────────────────────────────

  /// Debounced [_deltaSync] — batches the burst of reply/tool events around a
  /// turn into one REST round-trip over the relay.
  void _scheduleDeltaSync() {
    _deltaSyncDebounce?.cancel();
    _deltaSyncDebounce = Timer(const Duration(milliseconds: 800), () {
      if (mounted) unawaited(_deltaSync());
    });
  }

  /// Fetch messages newer than the per-jid sync cursor, persist them to the
  /// local cache, advance the cursor, and append anything not yet rendered.
  Future<void> _deltaSync() async {
    final jid = _chatJid;
    if (jid == null) return;
    if (_deltaSyncInFlight) {
      _pendingDeltaSync = true;
      return;
    }
    _deltaSyncInFlight = true;
    try {
      final cursor = await _cache.getSyncCursor(jid);
      // `cursor − 1` re-fetches the boundary millisecond so same-ms messages
      // can't be skipped; the overlap is deduped by id.
      final rows = await _chatApi.fetchHistoryAfter(
        jid,
        cursor > 0 ? cursor - 1 : 0,
      );
      if (rows.isEmpty) return;

      await _cache.upsertMessages(jid, [
        for (final r in rows)
          CachedMessage(
            id: r.id,
            ts: r.ts,
            role: r.role,
            content: r.content,
            sender: r.sender,
            isFromMe: r.isFromMe,
            isBotReply: r.isBotReply,
          ),
      ]);
      var maxTs = cursor;
      for (final r in rows) {
        if (r.ts > maxTs) maxTs = r.ts;
      }
      await _cache.setSyncCursor(jid, maxTs);
      if (!mounted) return;

      final added = <ChatMessage>[];
      for (final r in rows) {
        if (r.id.isEmpty || !_seenMessageIds.add(r.id)) continue;
        if (_matchesRecentLiveBubble(r)) continue;
        added.add(ChatMessage(
          r.content,
          r.role == 'user',
          timestamp: DateTime.fromMillisecondsSinceEpoch(r.ts),
          role: r.role,
        ));
      }
      Log.i(
        '[Chat] Delta sync: ${rows.length} tin từ server, ${added.length} tin mới hiển thị',
      );
      if (added.isEmpty) return;
      final epoch0 = DateTime.fromMillisecondsSinceEpoch(0);
      setState(() {
        _messages.addAll(added);
        _messages.sort((a, b) =>
            (a.timestamp ?? epoch0).compareTo(b.timestamp ?? epoch0));
      });
      _scrollToBottom();
    } catch (e) {
      Log.w('[Chat] Delta sync lỗi: $e');
    } finally {
      _deltaSyncInFlight = false;
      if (_pendingDeltaSync) {
        _pendingDeltaSync = false;
        unawaited(_deltaSync());
      }
    }
  }

  /// True when a server row duplicates a live bubble already rendered this
  /// session — the message reached us over the encrypted chat path before its
  /// server row was synced. Matched by role + exact text among recent
  /// non-history rows (live bubbles carry no server id to compare).
  bool _matchesRecentLiveBubble(ChatHistoryEntry r) {
    final text = r.content.trim();
    if (text.isEmpty) return false;
    final start = _messages.length > 40 ? _messages.length - 40 : 0;
    for (var i = _messages.length - 1; i >= start; i--) {
      final m = _messages[i];
      if (m.isHistory || m.requestId != null || m.role == 'tool') continue;
      if (m.role == r.role && m.text.trim() == text) return true;
    }
    return false;
  }

  /// Cache-first history load for the selected agent: render cached rows
  /// instantly, then fetch only the delta. Falls back to the legacy full
  /// page-1 HISTORY_REQ control frame when the cache is empty (first run,
  /// or web where sqflite is unavailable).
  Future<void> _loadHistoryForSelected() async {
    final jid = _chatJid;
    final cached =
        jid == null ? const <CachedMessage>[] : await _cache.getMessages(jid);
    if (!mounted) return;

    if (cached.isEmpty) {
      _relay?.sendControl(
        RelayControlType.historyReq,
        jsonEncode({'page': 1, 'pageSize': 20}),
      );
    } else {
      Log.i('[Chat] Hiển thị ${cached.length} tin từ bộ nhớ đệm');
      setState(() {
        for (final m in cached) {
          if (m.id.isNotEmpty) _seenMessageIds.add(m.id);
          _messages.add(ChatMessage(
            m.content,
            m.role == 'user',
            isHistory: true,
            timestamp: DateTime.fromMillisecondsSinceEpoch(m.ts),
            role: m.role,
          ));
        }
        // Approximate the server page the cache already covers so "load
        // more" continues with older pages (overlap dedups by id).
        _currentPage = (cached.length / 20).ceil().clamp(1, 1 << 20);
        _statusText = tr('Đã tải ${cached.length} tin từ bộ nhớ đệm',
            'Loaded ${cached.length} messages from cache');
      });
      _scrollToBottom(animate: false);
    }
    unawaited(_deltaSync());
  }

  void _loadMoreHistory() {
    if (_selectedAgent == null || !_hasMoreHistory || _isLoadingMore) return;
    Log.i('[Chat] Tải thêm trang lịch sử: ${_currentPage + 1}');
    setState(() {
      _isLoadingMore = true;
      _currentPage++;
    });
    _relay?.sendControl(
      RelayControlType.historyReq,
      jsonEncode({'page': _currentPage, 'pageSize': 20}),
    );
  }

  void _selectAgent(AgentInfo agent, {bool sendSelect = true, String? mode}) {
    Log.i('[Chat] Chọn agent: ${agent.name} (folder=${agent.folder})');

    setState(() {
      _selectedAgent = agent;
      _currentPage = 1;
      _hasMoreHistory = true;
      _messages.clear();
      _seenMessageIds.clear();
      _statusText = tr('Đang tải lịch sử cho "${agent.name}"…',
          'Loading history for "${agent.name}"…');
    });

    _config.setSelectedAgentFolder(agent.folder);
    _config.setSelectedAgentName(agent.name);

    if (sendSelect) {
      _relay?.sendControl(
        RelayControlType.agentSelect,
        jsonEncode({
          'folder': agent.folder,
          if (mode != null && mode.isNotEmpty) 'mode': mode,
        }),
      );
      // AGENT_SELECT rebinds the active session's folder — refresh the list so
      // its agent label stays in sync.
      RelayManager().requestSessionList();
    }
    unawaited(_loadHistoryForSelected());
  }

  void _reloadAgentList() {
    Log.i('[Chat] Người dùng yêu cầu tải lại danh sách agent');
    setState(() {
      _agentLoaded = false;
      _statusText =
          tr('Đang tải lại danh sách profile…', 'Reloading profile list…');
    });
    _relay?.sendControl(RelayControlType.agentListReq, '{}');
  }

  void _reloadHistory() {
    if (_selectedAgent == null) return;
    Log.i(
      '[Chat] Người dùng yêu cầu tải lại lịch sử cho "${_selectedAgent!.name}"',
    );
    setState(() {
      _currentPage = 1;
      _hasMoreHistory = true;
      _messages.clear();
      _seenMessageIds.clear();
    });
    // Forced reload = drop the cache + cursor and re-fetch from the daemon;
    // the delta sync repopulates the cache from the authoritative rows.
    final jid = _chatJid;
    Future<void> reload() async {
      if (jid != null) await _cache.clearMessages(jid);
      _relay?.sendControl(
        RelayControlType.historyReq,
        jsonEncode({'page': 1, 'pageSize': 20}),
      );
      unawaited(_deltaSync());
    }

    unawaited(reload());
  }

  void _retryLoad() {
    Log.i('[Chat] Thử lại kết nối');
    setState(() {
      _loadTimedOut = false;
      _agentLoaded = false;
      _statusText = tr('Đang kết nối lại…', 'Reconnecting…');
    });
    _loadTimeout?.cancel();
    // The shared relay auto-reconnects; just re-request the agent list and
    // restart the load-timeout watchdog.
    _relayManager.requestAgentList();
    _startAgentListPoll();
    _loadTimeout = Timer(_agentListTimeout, () {
      if (!mounted || _agentLoaded) return;
      setState(() {
        _loadTimedOut = true;
        _statusText = tr(
            'Vẫn chưa nhận được phản hồi — kiểm tra mạng và Senclaw daemon.',
            'Still no response — check the network and the Senclaw daemon.');
      });
    });
  }

  void _scrollToBottom({bool animate = true}) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!_scrollController.hasClients) return;
      final max = _scrollController.position.maxScrollExtent;
      // Initial history load jumps instantly (no 250ms slide / flicker); live
      // messages animate.
      if (animate) {
        _scrollController.animateTo(max,
            duration: const Duration(milliseconds: 250), curve: Curves.easeOut);
      } else {
        _scrollController.jumpTo(max);
      }
    });
  }

  Future<void> _openAgentPicker() async {
    if (_agents.isEmpty) return;
    final chosen = await AgentSelectScreen.show(
      context,
      agents: _agents,
      selected: _selectedAgent,
    );
    if (chosen != null && chosen.folder != _selectedAgent?.folder) {
      _selectAgent(chosen);
    }
  }

  /// Desktop-style "New" composer: pick Chat / Code / Cowork, an agent or a
  /// limited project, and a first message. Code/Cowork self-navigate to their
  /// detail screens; a chat result creates a NEW session bound to that agent
  /// and sends the first message once the session becomes active.
  Future<void> _openNewChat() async {
    final result = await Navigator.of(context).push<NewChatResult>(
      MaterialPageRoute(builder: (_) => NewChatScreen(agents: _agents)),
    );
    if (result == null || !mounted) return;
    final agent =
        _agents.where((a) => a.folder == result.agentFolder).firstOrNull;
    final name = agent?.name ?? tr('Phiên mới', 'New session');
    // Create a fresh session; the daemon makes it active and re-sends the
    // session list, which [_reconcileSession] follows.
    ref.read(sessionsProvider.notifier).create(
          folder: result.agentFolder,
          name: name,
          mode: result.mode,
        );
    // Clear any explicit selection so the chat follows the new active session.
    ref.read(selectedSessionJidProvider.notifier).state = null;
    final text = result.message.trim();
    _pendingFirstMessage = text.isEmpty ? null : text;
  }

  // ── Session reconciliation (multi-session) ─────────────────────────────────

  /// Compute the session the chat should be showing — the explicit UI
  /// selection, else the daemon's active session, else the device default —
  /// and switch to it if it differs from the current [_chatJid].
  void _reconcileSession() {
    if (!mounted) return;
    final selected = ref.read(selectedSessionJidProvider);
    final sessions = ref.read(sessionsProvider);
    var target = selected;
    target ??= sessions.where((s) => s.active).firstOrNull?.jid ??
        RelayManager().defaultSessionJid;
    if (target != null && target != _chatJid) {
      _switchSession(target, sessions);
    }
  }

  /// Point the chat at session [jid]: reset the transcript, reflect the
  /// session's agent in the app bar, ensure the daemon routes here, and reload
  /// history. Flushes any queued first message afterwards.
  void _switchSession(String jid, List<SessionInfo> sessions) {
    final s = sessions.where((x) => x.jid == jid).firstOrNull;
    Log.i('[Chat] Chuyển phiên → $jid');
    setState(() {
      _chatJid = jid;
      _currentPage = 1;
      _hasMoreHistory = true;
      _messages.clear();
      _seenMessageIds.clear();
      if (s != null && s.folder.isNotEmpty) {
        final a = _agents.where((x) => x.folder == s.folder).firstOrNull;
        if (a != null) _selectedAgent = a;
      }
      _statusText = tr('Đang tải phiên…', 'Loading session…');
    });
    // Idempotent: make sure the daemon files new messages under this session.
    RelayManager().selectSession(jid, folder: s?.folder);
    unawaited(_loadHistoryForSelected());

    final pending = _pendingFirstMessage;
    if (pending != null) {
      _pendingFirstMessage = null;
      _messageController.text = pending;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_send());
      });
    }
  }

  Future<void> _send() async {
    final text = _messageController.text.trim();
    if ((text.isEmpty && _attachments.isEmpty) || _relay == null) return;
    // Images ride along as base64 data: URLs appended to the body — the daemon's
    // agent input builder detects them and strips them from the model text.
    final buf = StringBuffer(text);
    for (final a in _attachments) {
      if (buf.isNotEmpty) buf.write('\n');
      buf.write(a);
    }
    final wire = buf.toString();
    try {
      await _relay!.sendMessage(wire);
      setState(() {
        _lastSendTime = DateTime.now();
        _messages.add(
          ChatMessage(wire, true, timestamp: DateTime.now(), role: 'user'),
        );
        _messageController.clear();
        _attachments.clear();
      });
      // Pull the server-authoritative row into the cache once it's stored.
      _scheduleDeltaSync();
      _scrollToBottom();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(
            SnackBar(content: Text(tr('Lỗi gửi: $e', 'Send error: $e'))));
      }
    }
  }

  // ── Composer actions: image attach, voice (on-device STT), model picker ────

  Future<void> _pickImage() async {
    try {
      final x = await _picker.pickImage(
          source: ImageSource.gallery, maxWidth: 1024, imageQuality: 70);
      if (x == null) return;
      final bytes = await x.readAsBytes();
      final ext = x.name.contains('.') ? x.name.split('.').last.toLowerCase() : '';
      final mime = switch (ext) {
        'png' => 'image/png',
        'gif' => 'image/gif',
        'webp' => 'image/webp',
        _ => 'image/jpeg',
      };
      if (!mounted) return;
      setState(() =>
          _attachments.add('data:$mime;base64,${base64Encode(bytes)}'));
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi chọn ảnh: $e', 'Error picking image: $e'))));
      }
    }
  }

  Future<void> _initSpeech() async {
    try {
      _sttAvailable = await _speech.initialize(
        onError: (_) {},
        onStatus: (s) {
          if ((s == 'done' || s == 'notListening') && mounted && _recording) {
            setState(() => _recording = false);
          }
        },
      );
      if (mounted) setState(() {});
    } catch (_) {/* mic unavailable */}
  }

  Future<void> _toggleMic() async {
    if (_recording) {
      await _speech.stop();
      if (mounted) setState(() => _recording = false);
      return;
    }
    final base = _messageController.text;
    setState(() => _recording = true);
    await _speech.listen(
      // Speech recognition follows the app language.
      listenOptions: stt.SpeechListenOptions(
          localeId: LanguageService().isVietnamese ? 'vi_VN' : 'en_US'),
      onResult: (r) {
        final sep = base.isEmpty || base.endsWith(' ') ? '' : ' ';
        _messageController.text = '$base$sep${r.recognizedWords}';
        _messageController.selection = TextSelection.fromPosition(
            TextPosition(offset: _messageController.text.length));
        setState(() {});
      },
    );
  }

  Future<void> _loadActiveModel() async {
    try {
      final l = await _llmApi.list();
      if (!mounted) return;
      final m = l.configs.where((x) => x.id == l.activeId).toList();
      setState(() {
        _modelLabel = m.isNotEmpty ? m.first.label : (l.activeId ?? 'Model');
      });
    } catch (_) {/* model list optional */}
  }

  Future<void> _pickModel() async {
    LlmConfigList list;
    try {
      list = await _llmApi.list();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi tải model: $e', 'Error loading models: $e'))));
      }
      return;
    }
    if (!mounted) return;
    final c = context.colors;
    final chosen = await showModalBottomSheet<LlmOption>(
      context: context,
      backgroundColor: c.surface,
      shape: const RoundedRectangleBorder(
        borderRadius:
            BorderRadius.vertical(top: Radius.circular(AppTokens.rXl)),
      ),
      builder: (ctx) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox(height: 10),
            Container(
                width: 40,
                height: 4,
                decoration: BoxDecoration(
                    color: c.borderStrong,
                    borderRadius: BorderRadius.circular(2))),
            Padding(
              padding: const EdgeInsets.all(14),
              child: Row(children: [
                Icon(Icons.memory, color: c.accent, size: 18),
                const SizedBox(width: 8),
                Text(tr('Chọn model', 'Select model'),
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
              ]),
            ),
            if (list.configs.isEmpty)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 16),
                child: Text(tr('Chưa có cấu hình model', 'No model configured'),
                    style: TextStyle(color: c.textMuted, fontSize: 13)),
              )
            else
              Flexible(
                child: ListView(
                  shrinkWrap: true,
                  children: [
                    for (final m in list.configs)
                      ListTile(
                        leading: Icon(
                            m.id == list.activeId
                                ? Icons.radio_button_checked
                                : Icons.radio_button_off,
                            color: m.id == list.activeId
                                ? c.accent
                                : c.textMuted,
                            size: 18),
                        title: Text(m.label,
                            style: TextStyle(color: c.textPrimary)),
                        onTap: () => Navigator.pop(ctx, m),
                      ),
                  ],
                ),
              ),
            const SizedBox(height: 8),
          ],
        ),
      ),
    );
    if (chosen == null) return;
    try {
      await _llmApi.setActive(chosen.id);
      if (mounted) {
        setState(() => _modelLabel = chosen.label);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content: Text(tr('Lỗi đặt model: $e', 'Error setting model: $e'))));
      }
    }
  }

  Uint8List? _decodeDataUrl(String dataUrl) {
    final i = dataUrl.indexOf('base64,');
    if (i < 0) return null;
    try {
      return base64Decode(dataUrl.substring(i + 7));
    } catch (_) {
      return null;
    }
  }

  /// Split a message body into (clean text, decoded images) for rendering.
  (String, List<Uint8List>) _splitImages(String text) {
    final imgs = <Uint8List>[];
    for (final m in _dataUrlRe.allMatches(text)) {
      final b = _decodeDataUrl(m.group(0)!);
      if (b != null) imgs.add(b);
    }
    final clean = text.replaceAll(_dataUrlRe, '').trim();
    return (clean, imgs);
  }

  // ── Build ────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    // Follow the selected session (drawer / sessions screen) and the daemon's
    // active-session changes.
    ref.listen<String?>(
        selectedSessionJidProvider, (_, next) => _reconcileSession());
    ref.listen<List<SessionInfo>>(
        sessionsProvider, (_, next) => _reconcileSession());
    return Scaffold(
      backgroundColor: c.bg,
      drawer: const AppDrawer(),
      appBar: _buildAppBar(),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: Column(
          children: [
            if (!_agentLoaded) _buildConnectingBanner(),
            // Tapping the chat area (or scrolling it) dismisses the keyboard.
            Expanded(
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onTap: () => FocusScope.of(context).unfocus(),
                child: _buildMessageList(),
              ),
            ),
            _buildInputArea(),
          ],
        ),
      ),
    );
  }

  AppBar _buildAppBar() {
    final c = context.colors;
    final agentName = _selectedAgent?.name ?? (_agentLoaded ? '—' : '…');

    return AppBar(
      backgroundColor: c.surface,
      elevation: 0,
      // Hamburger opens drawer
      leading: Builder(
        builder: (ctx) => IconButton(
          icon: Icon(Icons.menu, color: c.textSecondary),
          onPressed: () => Scaffold.of(ctx).openDrawer(),
        ),
      ),
      // Center: agent selector
      title: GestureDetector(
        onTap: _agents.length > 1 ? _openAgentPicker : null,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            CircleAvatar(
              radius: 15,
              backgroundColor: c.accent.withValues(alpha: 0.2),
              child: Text(
                agentName.isNotEmpty ? agentName[0].toUpperCase() : 'A',
                style: TextStyle(
                  color: c.accent,
                  fontSize: 13,
                  fontWeight: FontWeight.bold,
                ),
              ),
            ),
            const SizedBox(width: 8),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  agentName,
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 14,
                    fontWeight: FontWeight.bold,
                  ),
                ),
                if (_selectedAgent != null)
                  Text(
                    _selectedAgent!.folder,
                    style: TextStyle(color: c.textMuted, fontSize: 10),
                  ),
              ],
            ),
            if (_agents.length > 1) ...[
              const SizedBox(width: 4),
              Icon(Icons.expand_more, color: c.textMuted, size: 16),
            ],
          ],
        ),
      ),
      centerTitle: true,
      // Reload button
      actions: [
        IconButton(
          icon: Icon(Icons.add_comment_outlined, color: c.textSecondary),
          tooltip: tr('Tạo mới', 'New'),
          onPressed: _openNewChat,
        ),
        IconButton(
          icon: Icon(Icons.refresh, color: c.textMuted),
          tooltip: tr('Tải lại', 'Reload'),
          onPressed: () {
            _reloadAgentList();
            if (_selectedAgent != null) _reloadHistory();
          },
        ),
      ],
    );
  }

  Widget _buildConnectingBanner() {
    final c = context.colors;
    return Container(
      color: (_loadTimedOut ? AppTokens.danger : AppTokens.cyan)
          .withValues(alpha: 0.07),
      padding: const EdgeInsets.symmetric(vertical: 7, horizontal: 16),
      child: Row(
        children: [
          if (!_loadTimedOut)
            const SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                valueColor: AlwaysStoppedAnimation<Color>(AppTokens.cyan),
              ),
            )
          else
            const Icon(
              Icons.warning_amber_rounded,
              color: AppTokens.warning,
              size: 14,
            ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              _statusText,
              style: TextStyle(
                color: _loadTimedOut ? AppTokens.warning : c.textMuted,
                fontSize: 12,
              ),
            ),
          ),
          if (_loadTimedOut)
            TextButton(
              onPressed: _retryLoad,
              style: TextButton.styleFrom(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                minimumSize: Size.zero,
              ),
              child: Text(
                tr('Thử lại', 'Retry'),
                style: const TextStyle(color: AppTokens.cyan, fontSize: 12),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildMessageList() {
    final c = context.colors;
    if (!_agentLoaded && _messages.isEmpty) {
      return Center(
        child: _loadTimedOut
            ? _buildTimeoutState()
            : Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  CircularProgressIndicator(
                    valueColor: AlwaysStoppedAnimation<Color>(c.accent),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    _statusText,
                    style: TextStyle(color: c.textMuted, fontSize: 13),
                    textAlign: TextAlign.center,
                  ),
                ],
              ),
      );
    }

    if (_agentLoaded && _agents.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(
              Icons.person_outline,
              color: c.textMuted,
              size: 48,
            ),
            const SizedBox(height: 12),
            Text(
              tr('Không có profile nào được bind với kênh này.',
                  'No profile is bound to this channel.'),
              style: TextStyle(color: c.textMuted),
            ),
            const SizedBox(height: 6),
            Text(
              tr('Vào Web UI → Channels → bind profile cho kênh app này',
                  'Open Web UI → Channels → bind a profile to this app channel'),
              style: TextStyle(color: c.textMuted, fontSize: 12),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 16),
            OutlinedButton.icon(
              onPressed: _reloadAgentList,
              icon: Icon(
                Icons.refresh,
                color: c.accent,
                size: 16,
              ),
              label: Text(
                tr('Tải lại', 'Reload'),
                style: TextStyle(color: c.accent, fontSize: 13),
              ),
              style: OutlinedButton.styleFrom(
                side: BorderSide(color: c.accent),
              ),
            ),
          ],
        ),
      );
    }

    final showBusy = _isTyping || _agentBusy;
    // Group consecutive tool messages into a single collapsible card
    // (web ToolGroupCard "Đã dùng công cụ ×N").
    final rows = _buildRows();
    final totalCount = rows.length + (showBusy ? 1 : 0);
    return ListView.builder(
      controller: _scrollController,
      keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 12),
      itemCount: totalCount,
      itemBuilder: (ctx, i) {
        if (i == rows.length) return _buildTypingIndicator();

        final row = rows[i];

        if (row is List<ChatMessage>) {
          return Column(
            children: [
              if (i == 0 && _isLoadingMore) _loadingMoreSpinner(),
              _ToolGroupCard(messages: row),
            ],
          );
        }

        final msg = row as ChatMessage;
        // History separator after the last history message.
        final flatIdx = _messages.indexOf(msg);
        final isLastHistory = msg.isHistory &&
            (flatIdx + 1 >= _messages.length ||
                !_messages[flatIdx + 1].isHistory);

        return Column(
          children: [
            if (i == 0 && _isLoadingMore) _loadingMoreSpinner(),
            _buildBubble(msg),
            if (isLastHistory) _buildHistorySeparator(),
          ],
        );
      },
    );
  }

  Widget _loadingMoreSpinner() => const Padding(
        padding: EdgeInsets.symmetric(vertical: 8),
        child: SizedBox(
          width: 20,
          height: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      );

  /// Collapse consecutive `role == 'tool'` messages into grouped rows; every
  /// other message stays a standalone row. Mirrors web ChatView aggregation.
  List<Object> _buildRows() {
    final rows = <Object>[];
    List<ChatMessage>? toolRun;
    for (final m in _messages) {
      if (m.role == 'tool') {
        (toolRun ??= <ChatMessage>[]).add(m);
      } else {
        if (toolRun != null) {
          rows.add(toolRun);
          toolRun = null;
        }
        rows.add(m);
      }
    }
    if (toolRun != null) rows.add(toolRun);
    return rows;
  }

  Widget _buildHistorySeparator() {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Row(
        children: [
          Expanded(child: Divider(color: c.border)),
          const SizedBox(width: 8),
          Text(
            tr('Lịch sử', 'History'),
            style: TextStyle(color: c.textMuted, fontSize: 11),
          ),
          const SizedBox(width: 8),
          Expanded(child: Divider(color: c.border)),
        ],
      ),
    );
  }

  String _formatTime(DateTime dt) {
    final now = DateTime.now();
    final today = DateTime(now.year, now.month, now.day);
    final msgDay = DateTime(dt.year, dt.month, dt.day);
    final hh = dt.hour.toString().padLeft(2, '0');
    final mm = dt.minute.toString().padLeft(2, '0');
    if (msgDay == today) return '$hh:$mm';
    final dd = dt.day.toString().padLeft(2, '0');
    final mo = dt.month.toString().padLeft(2, '0');
    return '$dd/$mo $hh:$mm';
  }

  Widget _buildBubble(ChatMessage msg) {
    if (msg.role == 'permission' && msg.interaction != null) {
      return PermissionCard(
        data: msg.interaction!,
        resolved: msg.resolved,
        resolvedText: msg.resolvedText,
        onRespond: (key, label) => _respondPermission(msg, key, label),
      );
    }
    if (msg.role == 'question' && msg.interaction != null) {
      return QuestionCard(
        data: msg.interaction!,
        resolved: msg.resolved,
        onSubmit: (answers) => _respondQuestion(msg, answers),
      );
    }
    if (msg.role == 'form' && msg.interaction != null) {
      return FormCard(
        data: msg.interaction!,
        resolved: msg.resolved,
        onSubmit: (values, submitted) => _respondForm(msg, values, submitted),
      );
    }
    if (msg.role == 'plan' && msg.interaction != null) {
      return PlanCard(
        data: msg.interaction!,
        resolved: msg.resolved,
        resolvedText: msg.resolvedText,
        onRespond: (selected) => _respondPlan(msg, selected),
      );
    }
    if (msg.role == 'widget' && msg.interaction != null) {
      // One-way rich widget (chart/image/clock/weather) — display only.
      return Align(
        alignment: Alignment.centerLeft,
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxWidth: MediaQuery.of(context).size.width * 0.92,
          ),
          child: WidgetCard(spec: msg.interaction!),
        ),
      );
    }

    final isUser = msg.role == 'user';
    if (isUser) return _userBubble(msg);
    return _agentBubble(msg);
  }

  // ── User message: right-aligned filled bubble (web UserBubble) ─────────────
  Widget _userBubble(ChatMessage msg) {
    final c = context.colors;
    final timeStr = _formatTime(msg.timestamp ?? DateTime.now());
    final (clean, images) = _splitImages(msg.text);
    return Align(
      alignment: Alignment.centerRight,
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: MediaQuery.of(context).size.width * 0.82,
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.end,
          children: [
            if (images.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: 6),
                child: Wrap(
                  alignment: WrapAlignment.end,
                  spacing: 6,
                  runSpacing: 6,
                  children: [
                    for (final b in images)
                      ClipRRect(
                        borderRadius: BorderRadius.circular(12),
                        child: Image.memory(b, width: 180, fit: BoxFit.cover),
                      ),
                  ],
                ),
              ),
            if (clean.isNotEmpty)
              Container(
                margin: const EdgeInsets.only(top: 6),
                padding:
                    const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
                decoration: BoxDecoration(
                  color: c.bubbleUser
                      .withValues(alpha: msg.isHistory ? 0.6 : 1.0),
                  borderRadius: const BorderRadius.only(
                    topLeft: Radius.circular(16),
                    topRight: Radius.circular(4),
                    bottomLeft: Radius.circular(16),
                    bottomRight: Radius.circular(16),
                  ),
                  border: Border.all(color: c.border),
                ),
                child: Text(clean,
                    style: TextStyle(color: c.textPrimary, fontSize: 14)),
              ),
            _metaRow(timeStr, isUser: true, text: clean.isEmpty ? null : clean),
          ],
        ),
      ),
    );
  }

  // ── Agent message: avatar + subtle bubble (web AgentBubble) ────────────────
  Widget _agentBubble(ChatMessage msg) {
    final c = context.colors;
    final timeStr = _formatTime(msg.timestamp ?? DateTime.now());
    final parts = _extractReasoning(msg.text);
    final reasoning = parts.$1;
    final body = parts.$2;
    final textColor = msg.isHistory ? c.textSecondary : c.textPrimary;

    // Reasoning-only message → render just the collapsible (web fast-path).
    if (reasoning.isNotEmpty && body.trim().isEmpty) {
      return Padding(
        padding: const EdgeInsets.only(left: 38, top: 2, bottom: 2),
        child: _ReasoningCollapsible(text: reasoning),
      );
    }

    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _aiAvatar(),
          const SizedBox(width: 10),
          Flexible(
            child: ConstrainedBox(
              constraints: BoxConstraints(
                maxWidth: MediaQuery.of(context).size.width * 0.82,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
                    decoration: BoxDecoration(
                      color: c.bubbleAgent,
                      borderRadius: const BorderRadius.only(
                        topLeft: Radius.circular(4),
                        topRight: Radius.circular(16),
                        bottomLeft: Radius.circular(16),
                        bottomRight: Radius.circular(16),
                      ),
                      border: Border.all(color: c.border),
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        if (reasoning.isNotEmpty) ...[
                          _ReasoningCollapsible(text: reasoning),
                          const SizedBox(height: 6),
                          Divider(color: c.border, height: 1),
                          const SizedBox(height: 6),
                        ],
                        MarkdownText(body, color: textColor),
                      ],
                    ),
                  ),
                  _metaRow(timeStr,
                      isUser: false, latency: msg.latency, text: msg.text),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _aiAvatar() {
    final c = context.colors;
    return Container(
      width: 28,
      height: 28,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        color: c.accent,
        boxShadow: [
          BoxShadow(
            color: c.accent.withValues(alpha: 0.2),
            blurRadius: 10,
            offset: const Offset(0, 3),
          ),
        ],
      ),
      alignment: Alignment.center,
      child: const Text('AI',
          style: TextStyle(
              color: Colors.white, fontSize: 10, fontWeight: FontWeight.bold)),
    );
  }

  /// Time + latency + copy action row under a bubble (web action row).
  Widget _metaRow(String timeStr,
      {required bool isUser, Duration? latency, String? text}) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(top: 3, left: 4, right: 4),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(timeStr,
              style: TextStyle(color: c.textMuted, fontSize: 10)),
          if (!isUser && latency != null) ...[
            const SizedBox(width: 6),
            Text(
              '• ${(latency.inMilliseconds / 1000).toStringAsFixed(1)}s',
              style: TextStyle(
                  color: AppTokens.cyan.withValues(alpha: 0.6),
                  fontSize: 10,
                  fontWeight: FontWeight.w500),
            ),
          ],
          if (text != null && text.isNotEmpty) ...[
            const SizedBox(width: 4),
            InkWell(
              onTap: () {
                Clipboard.setData(ClipboardData(text: text));
                ScaffoldMessenger.of(context).showSnackBar(
                  SnackBar(
                      content: Text(tr('Đã sao chép', 'Copied')),
                      duration: const Duration(seconds: 1)),
                );
              },
              borderRadius: BorderRadius.circular(4),
              child: Padding(
                padding: const EdgeInsets.all(2),
                child: Icon(Icons.copy, size: 12, color: c.textMuted),
              ),
            ),
          ],
        ],
      ),
    );
  }

  /// Strip leading `<think>…</think>` / reasoning wrappers (web reasoningBlocks).
  /// Returns (reasoning, body).
  (String, String) _extractReasoning(String full) {
    // Matches a leading reasoning block. Superset of the web regex — also
    // accepts the full-word `<thinking>` some local models emit.
    final re = RegExp(
        r'^\s*<(thinking|think|redacted_reasoning|redacted_thinking)\b[^>]*>([\s\S]*?)<\/(thinking|think|redacted_reasoning|redacted_thinking)>',
        caseSensitive: false);
    final parts = <String>[];
    var rest = full;
    while (true) {
      final head = rest.trimLeft();
      final m = re.firstMatch(head);
      if (m == null || m.start != 0) break;
      final inner = (m.group(2) ?? '').trim();
      if (inner.isNotEmpty) parts.add(inner);
      rest = head.substring(m.end);
    }
    return (parts.join('\n\n'), rest.trimLeft());
  }

  Widget _buildTypingIndicator() {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(top: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _aiAvatar(),
          const SizedBox(width: 10),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
            decoration: BoxDecoration(
              color: c.bubbleAgent,
              borderRadius: const BorderRadius.only(
                topLeft: Radius.circular(4),
                topRight: Radius.circular(16),
                bottomLeft: Radius.circular(16),
                bottomRight: Radius.circular(16),
              ),
              border: Border.all(color: c.border),
            ),
            child: const _TypingDots(),
          ),
        ],
      ),
    );
  }

  Widget _buildInputArea() {
    final c = context.colors;
    final enabled = _selectedAgent != null;
    final canSend = enabled &&
        (_messageController.text.trim().isNotEmpty || _attachments.isNotEmpty);
    return Container(
      padding: const EdgeInsets.fromLTRB(12, 8, 8, 8),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        border: Border(top: BorderSide(color: c.border)),
      ),
      child: SafeArea(
        top: false,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (_attachments.isNotEmpty) _attachmentPreviews(c),
            TextField(
              controller: _messageController,
              enabled: enabled,
              minLines: 1,
              maxLines: 5,
              onChanged: (_) => setState(() {}),
              style: TextStyle(color: c.textPrimary),
              decoration: InputDecoration(
                hintText: enabled
                    ? tr('Nhắn tin…', 'Type a message…')
                    : tr('Chọn profile để bắt đầu', 'Select a profile to start'),
                hintStyle: TextStyle(color: c.textMuted),
                border: InputBorder.none,
                isCollapsed: true,
                contentPadding:
                    const EdgeInsets.symmetric(horizontal: 4, vertical: 8),
              ),
            ),
            const SizedBox(height: 4),
            Row(
              children: [
                Expanded(
                  child: SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(children: [
                      _composerChip(
                          c,
                          Icons.person_outline,
                          _selectedAgent?.name ?? 'Profile',
                          _agents.isNotEmpty ? _openAgentPicker : null),
                      const SizedBox(width: 6),
                      _composerChip(c, Icons.memory, _modelLabel, _pickModel),
                    ]),
                  ),
                ),
                IconButton(
                  tooltip: tr('Đính kèm ảnh', 'Attach image'),
                  visualDensity: VisualDensity.compact,
                  icon: Icon(Icons.image_outlined,
                      color: enabled ? c.textSecondary : c.textMuted),
                  onPressed: enabled ? _pickImage : null,
                ),
                IconButton(
                  tooltip: _recording
                      ? tr('Dừng ghi', 'Stop recording')
                      : tr('Nói', 'Speak'),
                  visualDensity: VisualDensity.compact,
                  icon: Icon(_recording ? Icons.stop_circle : Icons.mic_none,
                      color: _recording
                          ? AppTokens.danger
                          : (enabled && _sttAvailable
                              ? c.textSecondary
                              : c.textMuted)),
                  onPressed: enabled && _sttAvailable ? _toggleMic : null,
                ),
                const SizedBox(width: 2),
                Container(
                  decoration: BoxDecoration(
                      color: canSend ? c.accent : c.surface,
                      shape: BoxShape.circle),
                  child: IconButton(
                    tooltip: tr('Gửi', 'Send'),
                    visualDensity: VisualDensity.compact,
                    icon: Icon(Icons.arrow_upward,
                        size: 18,
                        color: canSend ? Colors.white : c.textMuted),
                    onPressed: canSend ? _send : null,
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _composerChip(
      AppColors c, IconData icon, String label, VoidCallback? onTap) {
    final active = onTap != null;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: c.border),
        ),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Icon(icon, size: 14, color: active ? c.textSecondary : c.textMuted),
          const SizedBox(width: 5),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 130),
            child: Text(label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                    color: active ? c.textSecondary : c.textMuted,
                    fontSize: 12)),
          ),
          if (active) Icon(Icons.expand_more, size: 14, color: c.textMuted),
        ]),
      ),
    );
  }

  Widget _attachmentPreviews(AppColors c) {
    return SizedBox(
      height: 64,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        padding: const EdgeInsets.only(bottom: 8),
        itemCount: _attachments.length,
        separatorBuilder: (_, _) => const SizedBox(width: 8),
        itemBuilder: (ctx, i) {
          final bytes = _decodeDataUrl(_attachments[i]);
          return Stack(
            clipBehavior: Clip.none,
            children: [
              ClipRRect(
                borderRadius: BorderRadius.circular(8),
                child: bytes != null
                    ? Image.memory(bytes, width: 56, height: 56, fit: BoxFit.cover)
                    : Container(width: 56, height: 56, color: c.surface),
              ),
              Positioned(
                right: -6,
                top: -6,
                child: GestureDetector(
                  onTap: () => setState(() => _attachments.removeAt(i)),
                  child: Container(
                    padding: const EdgeInsets.all(2),
                    decoration: const BoxDecoration(
                        color: Colors.black54, shape: BoxShape.circle),
                    child: const Icon(Icons.close, size: 13, color: Colors.white),
                  ),
                ),
              ),
            ],
          );
        },
      ),
    );
  }

  Widget _buildTimeoutState() {
    final c = context.colors;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Icon(
          Icons.wifi_off_rounded,
          color: AppTokens.warning,
          size: 48,
        ),
        const SizedBox(height: 16),
        Text(
          _statusText,
          style: TextStyle(color: c.textSecondary, fontSize: 13),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 20),
        OutlinedButton.icon(
          onPressed: _retryLoad,
          icon: Icon(Icons.refresh, color: c.accent),
          label: Text(
            tr('Thử lại', 'Retry'),
            style: TextStyle(color: c.accent),
          ),
          style: OutlinedButton.styleFrom(
            side: BorderSide(color: c.accent),
          ),
        ),
      ],
    );
  }

  @override
  void dispose() {
    _loadTimeout?.cancel();
    _agentListPoll?.cancel();
    _busyWatchdog?.cancel();
    _deltaSyncDebounce?.cancel();
    _speech.cancel();
    for (final s in _subs) {
      s.cancel();
    }
    _subs.clear();
    _messageController.dispose();
    _scrollController.dispose();
    // The relay is owned by RelayManager and shared across tabs — don't dispose.
    super.dispose();
  }
}

// ─── Tool group card (web ToolGroupCard "Đã dùng công cụ ×N") ─────────────────

/// Vietnamese human verb for a tool name, used in the collapsed summary.
String _toolVerb(String raw) {
  final n = raw.contains('__') ? raw.split('__').last : raw;
  switch (n) {
    case 'Read':
      return tr('Đọc tệp', 'Read file');
    case 'Write':
      return tr('Tạo tệp', 'Create file');
    case 'Edit':
    case 'NotebookEdit':
      return tr('Sửa tệp', 'Edit file');
    case 'Bash':
      return tr('Chạy lệnh', 'Run command');
    case 'Glob':
      return tr('Tìm tệp', 'Find files');
    case 'Grep':
      return tr('Tìm nội dung', 'Search content');
    case 'WebFetch':
      return tr('Tải URL', 'Fetch URL');
    case 'Task':
      return tr('Gọi subagent', 'Call subagent');
    case 'Skill':
      return tr('Dùng skill', 'Use skill');
  }
  if (raw.startsWith('mcp__browser__')) {
    return tr('Thao tác trình duyệt', 'Browser action');
  }
  if (raw.startsWith('mcp__memory__')) return tr('Tra trí nhớ', 'Memory lookup');
  if (raw.startsWith('mcp__wiki__')) return tr('Thao tác wiki', 'Wiki action');
  if (raw.startsWith('mcp__')) return n.replaceAll('_', ' ');
  return n;
}

class _ToolGroupCard extends StatefulWidget {
  final List<ChatMessage> messages;
  const _ToolGroupCard({required this.messages});

  @override
  State<_ToolGroupCard> createState() => _ToolGroupCardState();
}

class _ToolGroupCardState extends State<_ToolGroupCard> {
  bool _expanded = false;

  String _summary() {
    final n = widget.messages.length;
    if (n == 1) return _toolVerb(widget.messages.first.toolName ?? 'tool');
    return tr('Đã dùng công cụ ×$n', 'Used tools ×$n');
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final anyError = widget.messages.any((m) => !m.toolOk);
    final color = anyError ? AppTokens.danger : c.textSecondary;
    return Padding(
      padding: const EdgeInsets.only(left: 38, top: 2, bottom: 2),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            onTap: () => setState(() => _expanded = !_expanded),
            borderRadius: BorderRadius.circular(6),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 4),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Icon(
                      anyError
                          ? Icons.cancel
                          : Icons.check_circle,
                      size: 13,
                      color: anyError ? AppTokens.danger : AppTokens.success),
                  const SizedBox(width: 8),
                  Flexible(
                    child: Text(_summary(),
                        style: TextStyle(color: color, fontSize: 13)),
                  ),
                  const SizedBox(width: 6),
                  Icon(_expanded ? Icons.expand_more : Icons.chevron_right,
                      size: 16, color: c.textMuted),
                ],
              ),
            ),
          ),
          if (_expanded)
            Container(
              margin: const EdgeInsets.only(left: 6, top: 2),
              padding: const EdgeInsets.only(left: 12),
              decoration: BoxDecoration(
                border: Border(
                    left: BorderSide(color: c.border, width: 2)),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: widget.messages.map((m) {
                  return Padding(
                    padding: const EdgeInsets.symmetric(vertical: 3),
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Icon(m.toolOk ? Icons.circle : Icons.error_outline,
                            size: m.toolOk ? 6 : 13,
                            color: m.toolOk ? c.textMuted : AppTokens.danger),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(_toolVerb(m.toolName ?? 'tool'),
                                  style: TextStyle(
                                      color: c.textSecondary,
                                      fontSize: 12,
                                      fontWeight: FontWeight.w600)),
                              if ((m.toolSummary ?? '').isNotEmpty)
                                Text(m.toolSummary!,
                                    style: TextStyle(
                                        color: c.textMuted, fontSize: 11)),
                            ],
                          ),
                        ),
                      ],
                    ),
                  );
                }).toList(),
              ),
            ),
        ],
      ),
    );
  }
}

// ─── Reasoning collapsible (web ReasoningCollapsible) ─────────────────────────

class _ReasoningCollapsible extends StatefulWidget {
  final String text;
  const _ReasoningCollapsible({required this.text});

  @override
  State<_ReasoningCollapsible> createState() => _ReasoningCollapsibleState();
}

class _ReasoningCollapsibleState extends State<_ReasoningCollapsible> {
  bool _expanded = false;

  String get _preview {
    final flat = widget.text.replaceAll(RegExp(r'\s+'), ' ').trim();
    return flat.length > 90 ? '${flat.substring(0, 90)}…' : flat;
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        InkWell(
          onTap: () => setState(() => _expanded = !_expanded),
          borderRadius: BorderRadius.circular(6),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 3),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Padding(
                  padding: EdgeInsets.only(top: 1),
                  child: Icon(Icons.lightbulb_outline,
                      size: 13, color: AppTokens.warning),
                ),
                const SizedBox(width: 6),
                Expanded(
                  child: Text.rich(
                    TextSpan(children: [
                      TextSpan(
                        text: tr('Suy luận  ', 'Reasoning  '),
                        style: TextStyle(
                            color: c.textSecondary,
                            fontSize: 12,
                            fontWeight: FontWeight.w600),
                      ),
                      if (!_expanded)
                        TextSpan(
                          text: _preview,
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 12,
                              fontStyle: FontStyle.italic),
                        ),
                    ]),
                    maxLines: _expanded ? null : 1,
                    overflow: _expanded
                        ? TextOverflow.clip
                        : TextOverflow.ellipsis,
                  ),
                ),
                const SizedBox(width: 4),
                Icon(_expanded ? Icons.expand_more : Icons.chevron_right,
                    size: 15, color: c.textMuted),
              ],
            ),
          ),
        ),
        if (_expanded)
          Container(
            margin: const EdgeInsets.only(top: 4, left: 4),
            padding: const EdgeInsets.only(left: 10),
            decoration: BoxDecoration(
              border: Border(
                  left: BorderSide(color: c.border, width: 2)),
            ),
            child:
                MarkdownText(widget.text, color: c.textSecondary, fontSize: 12),
          ),
      ],
    );
  }
}

// ─── Typing dots (web bouncing dots) ──────────────────────────────────────────

class _TypingDots extends StatefulWidget {
  const _TypingDots();

  @override
  State<_TypingDots> createState() => _TypingDotsState();
}

class _TypingDotsState extends State<_TypingDots>
    with SingleTickerProviderStateMixin {
  late final AnimationController _ctrl = AnimationController(
      vsync: this, duration: const Duration(milliseconds: 1000))
    ..repeat();

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AnimatedBuilder(
      animation: _ctrl,
      builder: (context, _) {
        return Row(
          mainAxisSize: MainAxisSize.min,
          children: List.generate(3, (i) {
            final t = (_ctrl.value - i * 0.15) % 1.0;
            final scale = t < 0.5 ? 0.6 + t * 0.8 : 1.0 - (t - 0.5) * 0.8;
            return Padding(
              padding: const EdgeInsets.symmetric(horizontal: 2),
              child: Transform.scale(
                scale: scale.clamp(0.6, 1.0),
                child: Container(
                  width: 6,
                  height: 6,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: c.accent.withValues(alpha: 0.6),
                  ),
                ),
              ),
            );
          }),
        );
      },
    );
  }
}
