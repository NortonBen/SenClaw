import 'dart:async';
import 'package:flutter/foundation.dart';
import '../models/agent_model.dart';
import 'config_service.dart';
import 'crypto_service.dart';
import 'local_cache.dart';
import 'relay_service.dart';
import 'logger_service.dart';

/// App-level owner of the single shared [RelayService].
///
/// All feature tabs (Chat, Code, Space, Cowork) share one relay connection so
/// the encrypted tunnel — and the REST-over-relay bridge on top of it — is
/// established once. The previous design had [ChatScreen] own the relay; that
/// no longer works once other tabs also need to issue API calls.
///
/// Caches the latest agent list and connection state so a tab that mounts after
/// those arrived still sees them immediately.
class RelayManager extends ChangeNotifier {
  static final RelayManager _instance = RelayManager._internal();
  factory RelayManager() => _instance;
  RelayManager._internal();

  /// Sender id this app registers on the relay with. Also the last JID
  /// segment the daemon uses for this device's chat: `app:<cid>:user:<this>`.
  static const String senderId = 'mobile-app';

  final _config = ConfigService();
  final _cache = LocalCache();

  RelayService? _relay;
  RelayService? get relay => _relay;

  bool _connected = false;
  bool get connected => _connected;

  List<AgentInfo> _agents = [];
  List<AgentInfo> get agents => List.unmodifiable(_agents);

  /// False while [agents] only holds the local-cache snapshot; true once a
  /// real AGENT_LIST_RESP arrived from the daemon this session.
  bool _agentsFresh = false;
  bool get agentsFresh => _agentsFresh;

  StreamSubscription? _connSub;
  StreamSubscription? _agentSub;
  Completer<bool>? _startCompleter;

  /// Whether a relay instance exists (started at least once this session).
  bool get hasRelay => _relay != null;

  /// Create and start the shared relay if it isn't already running.
  /// Returns false when pairing data is missing.
  Future<bool> ensureStarted() async {
    if (_relay != null) return true;

    final inFlight = _startCompleter;
    if (inFlight != null) return inFlight.future;

    final completer = Completer<bool>();
    _startCompleter = completer;
    try {
      final hub = await _config.hubUrl;
      final relayUrl = await _config.relayUrl;
      final cid = await _config.channelId;
      final token = await _config.accessToken;
      final key = await _config.encryptionKey;

      final url = (relayUrl ?? hub)?.trim();
      if (url == null ||
          url.isEmpty ||
          cid == null ||
          token == null ||
          key == null) {
        Log.w('[RelayManager] Missing pairing data; not starting relay');
        completer.complete(false);
        return false;
      }

      final encKey = await CryptoService.deriveKey(key);
      Log.i('[RelayManager] Starting shared relay — channel=$cid url=$url');

      final relay = RelayService(
        hubUrl: url,
        channelId: cid,
        senderId: senderId,
        accessToken: token,
        encryptionKey: encKey,
      );

      _connSub = relay.connectionUpdates.listen((c) {
        _connected = c;
        notifyListeners();
      });
      _agentSub = relay.agentListUpdates.listen((list) {
        _agents = list;
        _agentsFresh = true;
        // Persist so the next launch renders the list instantly.
        unawaited(_cache.upsertAgents(list));
        notifyListeners();
      });

      _relay = relay;
      relay.start();
      // Instant render: surface the cached agent list while the live
      // AGENT_LIST_RESP is still in flight over the relay.
      unawaited(_loadCachedAgents());
      notifyListeners();
      completer.complete(true);
      return true;
    } catch (e, st) {
      if (!completer.isCompleted) completer.completeError(e, st);
      rethrow;
    } finally {
      if (identical(_startCompleter, completer)) {
        _startCompleter = null;
      }
    }
  }

  /// Ask the daemon to (re)send the agent list.
  void requestAgentList() {
    _relay?.sendControl(RelayControlType.agentListReq, '{}');
  }

  Future<void> _loadCachedAgents() async {
    if (_agentsFresh || _agents.isNotEmpty) return;
    final cached = await _cache.getAgents();
    // A live list may have raced us — never clobber fresh data with cache.
    if (_agentsFresh || _agents.isNotEmpty || cached.isEmpty) return;
    Log.i('[RelayManager] Serving ${cached.length} agent(s) from local cache');
    _agents = cached;
    notifyListeners();
  }

  /// Tear down and recreate the relay (used by retry / re-pair flows).
  Future<void> reset() async {
    await _disposeRelay();
    await ensureStarted();
  }

  /// Tear down completely (used on logout).
  Future<void> shutdown() async {
    await _disposeRelay();
    _agents = [];
    _agentsFresh = false;
    _connected = false;
    notifyListeners();
  }

  Future<void> _disposeRelay() async {
    await _connSub?.cancel();
    await _agentSub?.cancel();
    _connSub = null;
    _agentSub = null;
    final r = _relay;
    _relay = null;
    await r?.dispose();
  }
}
