import 'dart:async';
import 'package:flutter/foundation.dart';
import '../models/agent_model.dart';
import 'config_service.dart';
import 'crypto_service.dart';
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

  final _config = ConfigService();

  RelayService? _relay;
  RelayService? get relay => _relay;

  bool _connected = false;
  bool get connected => _connected;

  List<AgentInfo> _agents = [];
  List<AgentInfo> get agents => List.unmodifiable(_agents);

  StreamSubscription? _connSub;
  StreamSubscription? _agentSub;
  bool _starting = false;

  /// Whether a relay instance exists (started at least once this session).
  bool get hasRelay => _relay != null;

  /// Create and start the shared relay if it isn't already running.
  /// Returns false when pairing data is missing.
  Future<bool> ensureStarted() async {
    if (_relay != null || _starting) return _relay != null;
    _starting = true;
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
        return false;
      }

      final encKey = await CryptoService.deriveKey(key);
      Log.i('[RelayManager] Starting shared relay — channel=$cid url=$url');

      final relay = RelayService(
        hubUrl: url,
        channelId: cid,
        senderId: 'mobile-app',
        accessToken: token,
        encryptionKey: encKey,
      );

      _connSub = relay.connectionUpdates.listen((c) {
        _connected = c;
        notifyListeners();
      });
      _agentSub = relay.agentListUpdates.listen((list) {
        _agents = list;
        notifyListeners();
      });

      _relay = relay;
      relay.start();
      notifyListeners();
      return true;
    } finally {
      _starting = false;
    }
  }

  /// Ask the daemon to (re)send the agent list.
  void requestAgentList() {
    _relay?.sendControl(RelayControlType.agentListReq, '{}');
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
