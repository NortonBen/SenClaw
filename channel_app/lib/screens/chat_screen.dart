import 'dart:convert';
import 'dart:async';
import 'package:flutter/material.dart';
import '../models/agent_model.dart';
import '../models/api_models.dart';
import '../services/relay_service.dart';
import '../services/relay_manager.dart';
import '../services/config_service.dart';
import '../services/language_service.dart';
import '../services/chat_api.dart';
import '../services/logger_service.dart';
import '../widgets/interaction_cards.dart';
import '../widgets/markdown_text.dart';
import 'welcome_screen.dart';
import 'connection_qr_screen.dart';
import 'agent_select_screen.dart';

class ChatMessage {
  final String text;
  final bool isFromMe;
  final bool isHistory;
  final DateTime? timestamp;
  final Duration? latency;
  final String role; // 'user', 'agent', 'other', 'tool', 'permission', 'question'

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

class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key});

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  static const _agentListTimeout = Duration(seconds: 40);

  final _config = ConfigService();
  final _relayManager = RelayManager();
  final _messageController = TextEditingController();
  final _scrollController = ScrollController();
  final List<StreamSubscription> _subs = [];

  RelayService? _relay;
  Timer? _loadTimeout;

  final List<ChatMessage> _messages = [];
  bool _isTyping = false;

  List<AgentInfo> _agents = [];
  AgentInfo? _selectedAgent;
  String _agentState = '';
  bool _agentLoaded = false;
  bool _historyLoaded = false;
  int _currentPage = 1;
  bool _hasMoreHistory = true;
  bool _isLoadingMore = false;

  String _statusText = 'Đang kết nối tới relay…';
  bool _loadTimedOut = false;
  DateTime? _lastSendTime;

  bool _featureMemory = true;
  bool _featureScheduler = true;
  bool _featureWiki = true;

  @override
  void initState() {
    super.initState();
    _loadFeatureToggles();
    _initRelay();
  }

  Future<void> _loadFeatureToggles() async {
    final mem = await _config.featureMemory;
    final sch = await _config.featureScheduler;
    final wiki = await _config.featureWiki;
    if (!mounted) return;
    setState(() {
      _featureMemory = mem;
      _featureScheduler = sch;
      _featureWiki = wiki;
    });
  }

  Future<void> _initRelay() async {
    final started = await _relayManager.ensureStarted();
    if (!started) {
      if (!mounted) return;
      setState(() {
        _loadTimedOut = true;
        _statusText =
            'Thiếu dữ liệu ghép cặp — hãy quét lại mã QR để kết nối.';
      });
      return;
    }

    final relay = _relayManager.relay!;
    _relay = relay;
    Log.i('[Chat] Dùng relay chung từ RelayManager');

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
      _scrollToBottom();
    }));

    _subs.add(relay.typingUpdates.listen((typing) {
      if (!mounted) return;
      setState(() => _isTyping = typing);
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
    // screen mounted — replay the cache; otherwise (re)request it.
    if (_relayManager.agents.isNotEmpty) {
      _onAgentList(_relayManager.agents);
    } else {
      _relayManager.requestAgentList();
    }

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
            ? 'Hub đã kết nối — chưa có Senclaw trên kênh này. Hãy chạy Senclaw với relay tới cùng hub.'
            : 'Không nhận được phản hồi từ hub — kiểm tra mạng, domain và ghép cặp (token/kênh).';
      });
    });
  }

  Future<void> _onAgentList(List<AgentInfo> agents) async {
    if (!mounted) return;

    _loadTimeout?.cancel();
    Log.i(
      '[Chat] Nhận danh sách agent: ${agents.length} — ${agents.map((a) => a.name).join(', ')}',
    );

    setState(() {
      _agents = agents;
      _agentLoaded = true;
      _loadTimedOut = false;
      _statusText = agents.isEmpty
          ? 'Không có agent nào được bind với kênh này'
          : 'Đã tải ${agents.length} agent';
    });

    if (agents.isEmpty) return;

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

  void _onHistory(List<HistoryMessage> history) {
    if (!mounted) return;
    Log.i(
      '[Chat] Nhận lịch sử: ${history.length} tin cho agent "${_selectedAgent?.name}"',
    );

    final histMsgs = history.map((m) {
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
      // Vì backend trả về ORDER BY timestamp DESC (mới nhất đầu tiên)
      // Chúng ta muốn hiển thị cũ nhất đầu danh sách để khi ListView reverse=true, mới nhất ở dưới cùng.
      // Do đó: danh sách _messages sẽ chứa: [cũ nhất, ..., mới nhất]
      // Khi nhận thêm trang mới (cũ hơn), ta insert vào đầu danh sách.
      _messages.insertAll(0, histMsgs.reversed);
      _historyLoaded = true;
      _isLoadingMore = false;
      if (history.isEmpty || history.length < 20) {
        _hasMoreHistory = false;
      }
    });

    if (_currentPage == 1) {
      _scrollToBottom();
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
      _scrollToBottom();
    } else if (event.topic == 'agent:state' && data is Map) {
      setState(() => _agentState = (data['state'] ?? '').toString());
    } else if (event.topic == 'permission:request' && data is Map) {
      final m = data.cast<String, dynamic>();
      _addInteraction('permission', (m['requestId'] ?? '').toString(), m);
    } else if (event.topic == 'question:request' && data is Map) {
      final m = data.cast<String, dynamic>();
      _addInteraction('question', (m['requestId'] ?? '').toString(), m);
    } else if (event.topic == 'permission:resolved' && data is Map) {
      _markResolved((data['requestId'] ?? '').toString(),
          (data['optionLabel'] ?? data['optionKey'] ?? '').toString());
    } else if (event.topic == 'question:resolved' && data is Map) {
      _markResolved((data['requestId'] ?? '').toString(), null);
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
            .showSnackBar(SnackBar(content: Text('Lỗi gửi phản hồi: $e')));
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
            .showSnackBar(SnackBar(content: Text('Lỗi gửi trả lời: $e')));
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
            .showSnackBar(SnackBar(content: Text('Lỗi gửi lựa chọn: $e')));
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

  void _selectAgent(AgentInfo agent, {bool sendSelect = true}) {
    Log.i('[Chat] Chọn agent: ${agent.name} (folder=${agent.folder})');

    setState(() {
      _selectedAgent = agent;
      _historyLoaded = false;
      _currentPage = 1;
      _hasMoreHistory = true;
      _messages.clear();
      _statusText = 'Đang tải lịch sử cho "${agent.name}"…';
    });

    _config.setSelectedAgentFolder(agent.folder);
    _config.setSelectedAgentName(agent.name);

    if (sendSelect) {
      _relay?.sendControl(
        RelayControlType.agentSelect,
        jsonEncode({'folder': agent.folder}),
      );
    }
    _relay?.sendControl(
      RelayControlType.historyReq,
      jsonEncode({'page': 1, 'pageSize': 20}),
    );
  }

  void _reloadAgentList() {
    Log.i('[Chat] Người dùng yêu cầu tải lại danh sách agent');
    setState(() {
      _agentLoaded = false;
      _statusText = 'Đang tải lại danh sách agent…';
    });
    _relay?.sendControl(RelayControlType.agentListReq, '{}');
  }

  void _reloadHistory() {
    if (_selectedAgent == null) return;
    Log.i(
      '[Chat] Người dùng yêu cầu tải lại lịch sử cho "${_selectedAgent!.name}"',
    );
    setState(() {
      _historyLoaded = false;
      _currentPage = 1;
      _hasMoreHistory = true;
      _messages.clear();
    });
    _relay?.sendControl(
      RelayControlType.historyReq,
      jsonEncode({'page': 1, 'pageSize': 20}),
    );
  }

  void _retryLoad() {
    Log.i('[Chat] Thử lại kết nối');
    setState(() {
      _loadTimedOut = false;
      _agentLoaded = false;
      _statusText = 'Đang kết nối lại…';
    });
    _loadTimeout?.cancel();
    // The shared relay auto-reconnects; just re-request the agent list and
    // restart the load-timeout watchdog.
    _relayManager.requestAgentList();
    _loadTimeout = Timer(_agentListTimeout, () {
      if (!mounted || _agentLoaded) return;
      setState(() {
        _loadTimedOut = true;
        _statusText =
            'Vẫn chưa nhận được phản hồi — kiểm tra mạng và Senclaw daemon.';
      });
    });
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.animateTo(
          _scrollController.position.maxScrollExtent,
          duration: const Duration(milliseconds: 250),
          curve: Curves.easeOut,
        );
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

  Future<void> _confirmDisconnect() async {
    if (Navigator.canPop(context)) Navigator.pop(context); // close drawer
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: const Color(0xFF16162E),
        title: Text(
          t('logout_confirm_title'),
          style: const TextStyle(color: Colors.white),
        ),
        content: Text(
          t('logout_confirm_msg'),
          style: const TextStyle(color: Colors.white70),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(t('cancel')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(
              t('logout'),
              style: const TextStyle(color: Colors.redAccent),
            ),
          ),
        ],
      ),
    );
    if (ok == true) {
      await _relayManager.shutdown();
      await _config.clearAll();
      if (!mounted) return;
      Navigator.pushAndRemoveUntil(
        context,
        MaterialPageRoute(builder: (_) => const WelcomeScreen()),
        (_) => false,
      );
    }
  }

  Future<void> _send() async {
    final text = _messageController.text.trim();
    if (text.isEmpty || _relay == null) return;
    try {
      await _relay!.sendMessage(text);
      setState(() {
        _lastSendTime = DateTime.now();
        _messages.add(
          ChatMessage(text, true, timestamp: DateTime.now(), role: 'user'),
        );
        _messageController.clear();
      });
      _scrollToBottom();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(SnackBar(content: Text('Lỗi gửi: $e')));
      }
    }
  }

  // ── Build ────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D0D1F),
      drawer: _buildDrawer(),
      appBar: _buildAppBar(),
      body: Container(
        decoration: const BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [Color(0xFF0D0D1F), Color(0xFF16162E), Color(0xFF0D0D1F)],
          ),
        ),
        child: Column(
          children: [
            if (!_agentLoaded) _buildConnectingBanner(),
            Expanded(child: _buildMessageList()),
            _buildInputArea(),
          ],
        ),
      ),
    );
  }

  Widget _buildDrawer() {
    return Drawer(
      backgroundColor: const Color(0xFF16162E),
      child: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 24, 20, 16),
              child: Row(
                children: [
                  Container(
                    width: 40,
                    height: 40,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: Colors.purpleAccent.withOpacity(0.2),
                    ),
                    child: const Icon(
                      Icons.smart_toy_outlined,
                      color: Colors.purpleAccent,
                      size: 22,
                    ),
                  ),
                  const SizedBox(width: 12),
                  const Text(
                    'SenClaw',
                    style: TextStyle(
                      color: Colors.white,
                      fontSize: 18,
                      fontWeight: FontWeight.bold,
                    ),
                  ),
                ],
              ),
            ),

            // Feature toggles
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 4, 20, 8),
              child: Text(
                'TÍNH NĂNG',
                style: TextStyle(
                  color: Colors.white.withOpacity(0.3),
                  fontSize: 11,
                  fontWeight: FontWeight.w600,
                  letterSpacing: 1.2,
                ),
              ),
            ),
            _featureToggle(
              icon: Icons.memory_outlined,
              label: 'Bộ nhớ',
              description: 'Lưu trữ ngữ cảnh hội thoại',
              color: const Color(0xFF5BBFE8),
              value: _featureMemory,
              onChanged: (v) {
                setState(() => _featureMemory = v);
                _config.setFeatureMemory(v);
              },
            ),
            _featureToggle(
              icon: Icons.schedule_outlined,
              label: 'Lịch trình',
              description: 'Tác vụ định kỳ & lập lịch',
              color: const Color(0xFFFFB74D),
              value: _featureScheduler,
              onChanged: (v) {
                setState(() => _featureScheduler = v);
                _config.setFeatureScheduler(v);
              },
            ),
            _featureToggle(
              icon: Icons.menu_book_outlined,
              label: 'Wiki',
              description: 'Kho kiến thức của agent',
              color: const Color(0xFF66BB6A),
              value: _featureWiki,
              onChanged: (v) {
                setState(() => _featureWiki = v);
                _config.setFeatureWiki(v);
              },
            ),

            Divider(color: Colors.white.withOpacity(0.08)),
            const SizedBox(height: 4),

            // Reload agent list
            _drawerItem(
              icon: Icons.manage_accounts_outlined,
              label: 'Tải lại danh sách agent',
              onTap: () {
                Navigator.pop(context);
                _reloadAgentList();
              },
            ),

            // Reload history
            _drawerItem(
              icon: Icons.history,
              label: 'Tải lại lịch sử chat',
              onTap: _selectedAgent == null
                  ? null
                  : () {
                      Navigator.pop(context);
                      _reloadHistory();
                    },
              disabled: _selectedAgent == null,
            ),

            const Spacer(),
            Divider(color: Colors.white.withOpacity(0.08)),

            // QR code
            _drawerItem(
              icon: Icons.qr_code,
              iconColor: Colors.cyanAccent,
              label: 'Kết nối QR',
              onTap: () {
                Navigator.pop(context);
                Navigator.push(
                  context,
                  MaterialPageRoute(builder: (_) => const ConnectionQRScreen()),
                );
              },
            ),

            // Logout
            _drawerItem(
              icon: Icons.logout,
              iconColor: Colors.redAccent,
              label: t('logout'),
              labelColor: Colors.redAccent,
              onTap: _confirmDisconnect,
            ),

            const SizedBox(height: 12),
          ],
        ),
      ),
    );
  }

  Widget _featureToggle({
    required IconData icon,
    required String label,
    required String description,
    required Color color,
    required bool value,
    required ValueChanged<bool> onChanged,
  }) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 3),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        decoration: BoxDecoration(
          color: value ? color.withOpacity(0.08) : Colors.transparent,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: value
                ? color.withOpacity(0.25)
                : Colors.white.withOpacity(0.06),
          ),
        ),
        child: ListTile(
          contentPadding: const EdgeInsets.symmetric(
            horizontal: 12,
            vertical: 2,
          ),
          leading: Container(
            width: 34,
            height: 34,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: value
                  ? color.withOpacity(0.18)
                  : Colors.white.withOpacity(0.05),
            ),
            child: Icon(icon, color: value ? color : Colors.white38, size: 18),
          ),
          title: Text(
            label,
            style: TextStyle(
              color: value ? Colors.white : Colors.white54,
              fontSize: 13,
              fontWeight: FontWeight.w600,
            ),
          ),
          subtitle: Text(
            description,
            style: TextStyle(
              color: value ? Colors.white38 : Colors.white24,
              fontSize: 11,
            ),
          ),
          trailing: Switch(
            value: value,
            onChanged: onChanged,
            activeColor: color,
            activeTrackColor: color.withOpacity(0.3),
            inactiveThumbColor: Colors.white38,
            inactiveTrackColor: Colors.white12,
            materialTapTargetSize: MaterialTapTargetSize.shrinkWrap,
          ),
          dense: true,
          onTap: () => onChanged(!value),
        ),
      ),
    );
  }

  Widget _drawerItem({
    required IconData icon,
    required String label,
    VoidCallback? onTap,
    Color iconColor = Colors.white70,
    Color labelColor = Colors.white70,
    bool disabled = false,
  }) {
    return Opacity(
      opacity: disabled ? 0.35 : 1.0,
      child: ListTile(
        leading: Icon(icon, color: iconColor, size: 22),
        title: Text(label, style: TextStyle(color: labelColor, fontSize: 14)),
        onTap: disabled ? null : onTap,
        contentPadding: const EdgeInsets.symmetric(horizontal: 20),
        dense: true,
      ),
    );
  }

  AppBar _buildAppBar() {
    final agentName = _selectedAgent?.name ?? (_agentLoaded ? '—' : '…');

    return AppBar(
      backgroundColor: const Color(0xFF16162E),
      elevation: 0,
      // Hamburger opens drawer
      leading: Builder(
        builder: (ctx) => IconButton(
          icon: const Icon(Icons.menu, color: Colors.white70),
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
              backgroundColor: Colors.purpleAccent.withOpacity(0.2),
              child: Text(
                agentName.isNotEmpty ? agentName[0].toUpperCase() : 'A',
                style: const TextStyle(
                  color: Colors.purpleAccent,
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
                  style: const TextStyle(
                    color: Colors.white,
                    fontSize: 14,
                    fontWeight: FontWeight.bold,
                  ),
                ),
                if (_selectedAgent != null)
                  Text(
                    _selectedAgent!.folder,
                    style: const TextStyle(color: Colors.white38, fontSize: 10),
                  ),
              ],
            ),
            if (_agents.length > 1) ...[
              const SizedBox(width: 4),
              const Icon(Icons.expand_more, color: Colors.white38, size: 16),
            ],
          ],
        ),
      ),
      centerTitle: true,
      // Reload button
      actions: [
        IconButton(
          icon: const Icon(Icons.refresh, color: Colors.white54),
          tooltip: 'Tải lại',
          onPressed: () {
            _reloadAgentList();
            if (_selectedAgent != null) _reloadHistory();
          },
        ),
      ],
    );
  }

  Widget _buildConnectingBanner() {
    return Container(
      color: (_loadTimedOut ? Colors.redAccent : Colors.cyanAccent).withOpacity(
        0.07,
      ),
      padding: const EdgeInsets.symmetric(vertical: 7, horizontal: 16),
      child: Row(
        children: [
          if (!_loadTimedOut)
            const SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                valueColor: AlwaysStoppedAnimation<Color>(Colors.cyanAccent),
              ),
            )
          else
            const Icon(
              Icons.warning_amber_rounded,
              color: Colors.orangeAccent,
              size: 14,
            ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              _statusText,
              style: TextStyle(
                color: _loadTimedOut ? Colors.orangeAccent : Colors.white54,
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
              child: const Text(
                'Thử lại',
                style: TextStyle(color: Colors.cyanAccent, fontSize: 12),
              ),
            ),
        ],
      ),
    );
  }

  Widget _buildMessageList() {
    if (!_agentLoaded && _messages.isEmpty) {
      return Center(
        child: _loadTimedOut
            ? _buildTimeoutState()
            : Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const CircularProgressIndicator(
                    valueColor: AlwaysStoppedAnimation<Color>(
                      Colors.purpleAccent,
                    ),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    _statusText,
                    style: const TextStyle(color: Colors.white38, fontSize: 13),
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
            const Icon(
              Icons.smart_toy_outlined,
              color: Colors.white24,
              size: 48,
            ),
            const SizedBox(height: 12),
            const Text(
              'Không có agent nào được bind với kênh này.',
              style: TextStyle(color: Colors.white38),
            ),
            const SizedBox(height: 6),
            const Text(
              'Vào Web UI → Channels → bind agent cho kênh app này',
              style: TextStyle(color: Colors.white24, fontSize: 12),
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 16),
            OutlinedButton.icon(
              onPressed: _reloadAgentList,
              icon: const Icon(
                Icons.refresh,
                color: Colors.purpleAccent,
                size: 16,
              ),
              label: const Text(
                'Tải lại',
                style: TextStyle(color: Colors.purpleAccent, fontSize: 13),
              ),
              style: OutlinedButton.styleFrom(
                side: const BorderSide(color: Colors.purpleAccent),
              ),
            ),
          ],
        ),
      );
    }

    final showBusy = _isTyping || _agentBusy;
    final totalCount = _messages.length + (showBusy ? 1 : 0);
    // Sử dụng reverse: false để tin nhắn mới ở dưới cùng
    // Chúng ta đã sắp xếp danh sách _messages theo thứ tự thời gian tăng dần [cũ -> mới]
    return ListView.builder(
      controller: _scrollController,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
      itemCount: totalCount,
      itemBuilder: (ctx, i) {
        if (i == _messages.length) return _buildTypingIndicator();

        final msg = _messages[i];

        // Hiện phân cách lịch sử nếu đây là tin nhắn cuối cùng từ lịch sử
        final isLastHistory =
            msg.isHistory &&
            (i + 1 >= _messages.length || !_messages[i + 1].isHistory);

        return Column(
          children: [
            if (i == 0 && _isLoadingMore)
              const Padding(
                padding: EdgeInsets.symmetric(vertical: 8),
                child: SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(strokeWidth: 2),
                ),
              ),
            _buildBubble(msg),
            if (isLastHistory) _buildHistorySeparator(),
          ],
        );
      },
    );
  }

  Widget _buildHistorySeparator() {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Row(
        children: [
          Expanded(child: Divider(color: Colors.white12)),
          const SizedBox(width: 8),
          const Text(
            'Lịch sử',
            style: TextStyle(color: Colors.white24, fontSize: 11),
          ),
          const SizedBox(width: 8),
          Expanded(child: Divider(color: Colors.white12)),
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

  Widget _buildToolCard(ChatMessage msg) {
    final ok = msg.toolOk;
    final color = ok ? Colors.cyanAccent : Colors.redAccent;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3, horizontal: 8),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
        decoration: BoxDecoration(
          color: Colors.white.withOpacity(0.04),
          borderRadius: BorderRadius.circular(10),
          border: Border.all(color: color.withOpacity(0.25)),
        ),
        child: Row(
          children: [
            Icon(ok ? Icons.build_circle_outlined : Icons.error_outline,
                color: color, size: 16),
            const SizedBox(width: 8),
            Text(
              msg.toolName ?? 'tool',
              style: TextStyle(
                color: color,
                fontSize: 12,
                fontWeight: FontWeight.w600,
                fontFamily: 'monospace',
              ),
            ),
            if ((msg.toolSummary ?? '').isNotEmpty) ...[
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  msg.toolSummary!,
                  style: const TextStyle(color: Colors.white54, fontSize: 12),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildBubble(ChatMessage msg) {
    if (msg.role == 'tool') return _buildToolCard(msg);
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
    if (msg.role == 'plan' && msg.interaction != null) {
      return PlanCard(
        data: msg.interaction!,
        resolved: msg.resolved,
        resolvedText: msg.resolvedText,
        onRespond: (selected) => _respondPlan(msg, selected),
      );
    }
    // role: 'user' -> RIGHT (phải)
    // role: 'agent' -> LEFT (trái)
    final isUser = msg.role == 'user';
    final isAgent = msg.role == 'agent';
    final timeStr = _formatTime(msg.timestamp ?? DateTime.now());

    return Align(
      alignment: isUser ? Alignment.centerRight : Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 3),
        constraints: BoxConstraints(
          maxWidth: MediaQuery.of(context).size.width * 0.75,
        ),
        child: Column(
          crossAxisAlignment: isUser
              ? CrossAxisAlignment.end
              : CrossAxisAlignment.start,
          children: [
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
              decoration: BoxDecoration(
                color: isUser
                    ? Colors.purpleAccent.withOpacity(
                        msg.isHistory ? 0.1 : 0.18,
                      )
                    : Colors.white.withOpacity(msg.isHistory ? 0.05 : 0.1),
                borderRadius: BorderRadius.only(
                  topLeft: const Radius.circular(16),
                  topRight: const Radius.circular(16),
                  bottomLeft: Radius.circular(isUser ? 16 : 0),
                  bottomRight: Radius.circular(isUser ? 0 : 16),
                ),
                border: Border.all(
                  color: isUser
                      ? Colors.purpleAccent.withOpacity(
                          msg.isHistory ? 0.15 : 0.3,
                        )
                      : Colors.white.withOpacity(0.06),
                ),
              ),
              child: isUser
                  ? Text(
                      msg.text,
                      style: TextStyle(
                        color: msg.isHistory ? Colors.white60 : Colors.white,
                        fontSize: 14,
                      ),
                    )
                  : MarkdownText(
                      msg.text,
                      color: msg.isHistory ? Colors.white60 : Colors.white,
                    ),
            ),
            const SizedBox(height: 2),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 4),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    timeStr,
                    style: const TextStyle(color: Colors.white38, fontSize: 10),
                  ),
                  if (!isUser && msg.latency != null) ...[
                    const SizedBox(width: 6),
                    Text(
                      '•  Phản hồi: ${(msg.latency!.inMilliseconds / 1000).toStringAsFixed(1)}s',
                      style: TextStyle(
                        color: Colors.cyanAccent.withOpacity(0.4),
                        fontSize: 10,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ],
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildTypingIndicator() {
    return Align(
      alignment: Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(
          color: Colors.white.withOpacity(0.05),
          borderRadius: BorderRadius.circular(16),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(
                strokeWidth: 2,
                valueColor: AlwaysStoppedAnimation<Color>(Colors.white54),
              ),
            ),
            const SizedBox(width: 8),
            Text(
              '${_selectedAgent?.name ?? 'Agent'} đang soạn…',
              style: const TextStyle(
                color: Colors.white54,
                fontSize: 12,
                fontStyle: FontStyle.italic,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildInputArea() {
    final enabled = _selectedAgent != null;
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 8, 8, 16),
      decoration: BoxDecoration(
        color: Colors.black.withOpacity(0.3),
        border: Border(top: BorderSide(color: Colors.white.withOpacity(0.08))),
      ),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: _messageController,
              enabled: enabled,
              onSubmitted: (_) => _send(),
              style: const TextStyle(color: Colors.white),
              decoration: InputDecoration(
                hintText: enabled ? 'Nhắn tin…' : 'Chọn agent để bắt đầu',
                hintStyle: TextStyle(color: Colors.white.withOpacity(0.3)),
                border: InputBorder.none,
              ),
            ),
          ),
          IconButton(
            icon: Icon(
              Icons.send,
              color: enabled ? Colors.purpleAccent : Colors.white24,
            ),
            onPressed: enabled ? _send : null,
          ),
        ],
      ),
    );
  }

  Widget _buildTimeoutState() {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Icon(
          Icons.wifi_off_rounded,
          color: Colors.orangeAccent,
          size: 48,
        ),
        const SizedBox(height: 16),
        Text(
          _statusText,
          style: const TextStyle(color: Colors.white60, fontSize: 13),
          textAlign: TextAlign.center,
        ),
        const SizedBox(height: 20),
        OutlinedButton.icon(
          onPressed: _retryLoad,
          icon: const Icon(Icons.refresh, color: Colors.purpleAccent),
          label: const Text(
            'Thử lại',
            style: TextStyle(color: Colors.purpleAccent),
          ),
          style: OutlinedButton.styleFrom(
            side: const BorderSide(color: Colors.purpleAccent),
          ),
        ),
      ],
    );
  }

  @override
  void dispose() {
    _loadTimeout?.cancel();
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
