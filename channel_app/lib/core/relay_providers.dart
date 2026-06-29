import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../models/agent_model.dart';
import '../services/relay_manager.dart';

/// Riverpod surface over the existing [RelayManager] singleton.
///
/// The transport layer (RelayManager / RelayService / ConfigService) is left
/// untouched — these providers just expose it to the widget tree so screens can
/// be `ConsumerWidget`s that rebuild on connection / agent-list changes instead
/// of reaching for the singleton + `AnimatedBuilder` directly.
///
/// Non-autoDispose on purpose: the relay lives for the whole app session.
final relayManagerProvider = ChangeNotifierProvider<RelayManager>(
  (ref) => RelayManager(),
);

/// True once the encrypted relay tunnel is up.
final relayConnectedProvider = Provider<bool>(
  (ref) => ref.watch(relayManagerProvider).connected,
);

/// Latest agent list cached by the relay manager.
final relayAgentsProvider = Provider<List<AgentInfo>>(
  (ref) => ref.watch(relayManagerProvider).agents,
);
