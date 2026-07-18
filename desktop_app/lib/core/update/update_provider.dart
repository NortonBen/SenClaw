import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../daemon/app_shutdown.dart';
import '../daemon/daemon_provider.dart';
import '../prefs.dart';
import '../transport/connection.dart';
import 'update_manifest.dart';
import 'update_service.dart';

// Persisted across runs. The app lives in the tray and gets shown/hidden all
// day, so "check on start" without a timestamp would mean checking constantly.
const kUpdateLastCheckKey = 'senclaw:update:last-check';
const kUpdateSkippedKey = 'senclaw:update:skipped-version';
const kUpdateAutoCheckKey = 'senclaw:update:auto-check';

const kUpdateCheckInterval = Duration(hours: 24);

enum UpdatePhase { idle, checking, upToDate, available, downloading, ready, applying, error }

@immutable
class UpdateState {
  const UpdateState({
    this.phase = UpdatePhase.idle,
    this.manifest,
    this.progress = 0,
    this.error,
    this.lastCheck,
    this.autoCheck = true,
  });

  final UpdatePhase phase;
  final UpdateManifest? manifest;

  /// 0..1 while [UpdatePhase.downloading].
  final double progress;
  final String? error;
  final DateTime? lastCheck;

  /// Mirrors the persisted pref. Kept IN the state (rather than read from
  /// SharedPreferences on demand) so toggling it rebuilds the switch — a getter
  /// reading prefs behind Riverpod's back would need a manual notify to show.
  final bool autoCheck;

  bool get hasUpdate =>
      manifest != null &&
      (phase == UpdatePhase.available ||
          phase == UpdatePhase.downloading ||
          phase == UpdatePhase.ready);

  UpdateState copyWith({
    UpdatePhase? phase,
    UpdateManifest? manifest,
    double? progress,
    String? error,
    DateTime? lastCheck,
    bool? autoCheck,
    bool clearError = false,
  }) =>
      UpdateState(
        phase: phase ?? this.phase,
        manifest: manifest ?? this.manifest,
        progress: progress ?? this.progress,
        error: clearError ? null : (error ?? this.error),
        lastCheck: lastCheck ?? this.lastCheck,
        autoCheck: autoCheck ?? this.autoCheck,
      );
}

final updateServiceProvider = Provider<UpdateService>((ref) {
  final supervisor = ref.watch(daemonSupervisorProvider);
  return UpdateService(resolveDaemonBinary: supervisor.resolveBinary);
});

final updateProvider =
    NotifierProvider<UpdateNotifier, UpdateState>(UpdateNotifier.new);

class UpdateNotifier extends Notifier<UpdateState> {
  File? _staged;
  CancelToken? _cancel;

  @override
  UpdateState build() {
    final prefs = ref.read(prefsProvider);
    final raw = prefs.getString(kUpdateLastCheckKey);
    return UpdateState(
      lastCheck: raw == null ? null : DateTime.tryParse(raw),
      autoCheck: prefs.getBool(kUpdateAutoCheckKey) ?? true,
    );
  }

  // Asks the service rather than reading kIsDevBuild directly, so overriding
  // updateServiceProvider in a test drives the whole path — otherwise every
  // test would stop here, since a test binary is always version 'dev'.
  bool get _enabled => !kIsWeb && !ref.read(updateServiceProvider).isDevBuild;

  /// Background check on a schedule. Silent by design: a machine that is
  /// offline, or behind a proxy that eats GitHub, must not get an error toast
  /// for a check nobody asked for.
  Future<void> maybeCheck() async {
    if (!_enabled) return;
    if (!state.autoCheck) return;
    final last = state.lastCheck;
    if (last != null && DateTime.now().difference(last) < kUpdateCheckInterval) {
      return;
    }
    await check(silent: true);
  }

  Future<void> check({bool silent = false}) async {
    if (!_enabled) {
      if (!silent) {
        state = state.copyWith(
          phase: UpdatePhase.error,
          error: 'Updates are disabled in a dev build.',
        );
      }
      return;
    }
    state = state.copyWith(phase: UpdatePhase.checking, clearError: true);

    final now = DateTime.now();
    await ref.read(prefsProvider).setString(kUpdateLastCheckKey, now.toIso8601String());

    final svc = ref.read(updateServiceProvider);
    final m = await svc.fetchManifest();
    if (m == null) {
      state = state.copyWith(
        phase: silent ? UpdatePhase.idle : UpdatePhase.error,
        error: silent ? null : 'Could not reach the update server.',
        lastCheck: now,
      );
      return;
    }
    if (!svc.isNewer(m)) {
      state = state.copyWith(phase: UpdatePhase.upToDate, manifest: m, lastCheck: now);
      return;
    }
    state = state.copyWith(phase: UpdatePhase.available, manifest: m, lastCheck: now);
  }

  /// Whether to surface the one-time "update available" prompt. Respects the
  /// user having skipped this exact version.
  bool shouldAnnounce() {
    final m = state.manifest;
    if (m == null || state.phase != UpdatePhase.available) return false;
    return ref.read(prefsProvider).getString(kUpdateSkippedKey) != '${m.version}';
  }

  Future<void> skipCurrent() async {
    final m = state.manifest;
    if (m == null) return;
    await ref.read(prefsProvider).setString(kUpdateSkippedKey, '${m.version}');
  }

  Future<void> setAutoCheck(bool on) async {
    await ref.read(prefsProvider).setBool(kUpdateAutoCheckKey, on);
    state = state.copyWith(autoCheck: on);
  }

  Future<void> download() async {
    final m = state.manifest;
    if (m == null) return;
    final svc = ref.read(updateServiceProvider);
    try {
      await svc.ensureInstallable(m);
    } on UpdateUnavailable catch (e) {
      state = state.copyWith(phase: UpdatePhase.error, error: e.message);
      return;
    }

    final asset = svc.assetFor(m)!;
    _cancel = CancelToken();
    state = state.copyWith(phase: UpdatePhase.downloading, progress: 0, clearError: true);
    try {
      _staged = await svc.download(
        asset,
        cancel: _cancel,
        onProgress: (v) {
          if (state.phase == UpdatePhase.downloading) {
            state = state.copyWith(progress: v);
          }
        },
      );
      state = state.copyWith(phase: UpdatePhase.ready, progress: 1);
    } on UpdateUnavailable catch (e) {
      state = state.copyWith(phase: UpdatePhase.available, error: e.message);
    } catch (e) {
      state = state.copyWith(phase: UpdatePhase.available, error: 'Download failed: $e');
    }
  }

  void cancelDownload() {
    _cancel?.cancel();
    state = state.copyWith(phase: UpdatePhase.available, progress: 0);
  }

  /// Point of no return: hands the staged bundle to a detached updater, then
  /// shuts the app down — which is the signal `apply-update` is blocked on.
  /// Nothing after [shutdownApp] runs.
  Future<void> applyAndRestart() async {
    final m = state.manifest;
    final staged = _staged;
    if (m == null || staged == null) return;
    state = state.copyWith(phase: UpdatePhase.applying, clearError: true);
    try {
      final svc = ref.read(updateServiceProvider);
      await svc.spawnUpdater(staged, svc.assetFor(m)!);
    } on UpdateUnavailable catch (e) {
      state = state.copyWith(phase: UpdatePhase.ready, error: e.message);
      return;
    } catch (e) {
      state = state.copyWith(phase: UpdatePhase.ready, error: 'Could not start the updater: $e');
      return;
    }
    await shutdownApp(
      supervisor: ref.read(daemonSupervisorProvider),
      uiPort: ref.read(appConfigProvider).uiPort,
    );
  }
}
