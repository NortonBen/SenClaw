import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/prefs.dart';

// ── Persisted keys ────────────────────────────────────────────────────────
const kNotificationsKey = 'senclaw:notifications-enabled';
const kBgSyncKey = 'senclaw:bgsync-enabled';
const kBgSyncIntervalKey = 'senclaw:bgsync-interval-min';

/// Selectable background-sync intervals, in minutes.
const kSyncIntervals = <int>[1, 5, 15, 30, 60];

/// A simple persisted boolean setting.
class _BoolPref extends StateNotifier<bool> {
  _BoolPref(this._ref, this._key, bool fallback)
      : super(_ref.read(prefsHelperProvider).boolean(_key, fallback));
  final Ref _ref;
  final String _key;

  void set(bool v) {
    state = v;
    _ref.read(prefsHelperProvider).setBool(_key, v);
  }
}

/// Show OS notifications for new agent messages. Default on.
final notificationsEnabledProvider =
    StateNotifierProvider<_BoolPref, bool>(
  (ref) => _BoolPref(ref, kNotificationsKey, true),
);

/// Periodically pull new info while the app is running. Default off.
final backgroundSyncEnabledProvider =
    StateNotifierProvider<_BoolPref, bool>(
  (ref) => _BoolPref(ref, kBgSyncKey, false),
);

/// The background-sync interval in minutes (one of [kSyncIntervals]).
class SyncIntervalNotifier extends StateNotifier<int> {
  SyncIntervalNotifier(this._ref)
      : super(_clamp(int.tryParse(
                _ref.read(prefsHelperProvider).string(kBgSyncIntervalKey, '15')) ??
            15));
  final Ref _ref;

  static int _clamp(int v) => kSyncIntervals.contains(v) ? v : 15;

  void set(int v) {
    final n = _clamp(v);
    state = n;
    _ref.read(prefsHelperProvider).setString(kBgSyncIntervalKey, '$n');
  }
}

final syncIntervalProvider =
    StateNotifierProvider<SyncIntervalNotifier, int>(
  (ref) => SyncIntervalNotifier(ref),
);
