import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../daemon/app_shutdown.dart';
import '../daemon/daemon_provider.dart';
import '../i18n/l10n.dart';
import '../prefs.dart';
import '../transport/connection.dart';
import 'update_manifest.dart';
import 'update_service.dart';

// Persisted across runs. The app lives in the tray and gets shown/hidden all
// day, so "check on start" without a timestamp would mean checking constantly.
const kUpdateLastCheckKey = 'senclaw:update:last-check';
const kUpdateSkippedKey = 'senclaw:update:skipped-version';
const kUpdateAutoCheckKey = 'senclaw:update:auto-check';
// "Remind me later" — stored WITH the version it applies to, so a release
// published during the snooze window is still announced.
const kUpdateSnoozeUntilKey = 'senclaw:update:snooze-until';
const kUpdateSnoozeVersionKey = 'senclaw:update:snooze-version';

const kUpdateCheckInterval = Duration(hours: 24);

/// How long "Remind me later" silences the startup popup for one version.
/// A day, matching the check cadence: the next launch after that re-announces.
const kUpdateSnoozeDuration = Duration(hours: 24);

/// Floor for [UpdateNotifier.startupCheck], which otherwise ignores
/// [kUpdateCheckInterval]. Without it, an app the supervisor keeps restarting
/// would hit GitHub on every relaunch.
const kUpdateStartupMinInterval = Duration(minutes: 30);

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
    this.skippedVersion,
    this.snoozeVersion,
    this.snoozeUntil,
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

  /// Version the user chose never to be reminded about ("Skip this version").
  final String? skippedVersion;

  /// Version the user postponed, and until when. Both null unless a snooze is
  /// active; the pair is checked together so a *newer* release still speaks up.
  final String? snoozeVersion;
  final DateTime? snoozeUntil;

  bool get hasUpdate =>
      manifest != null &&
      (phase == UpdatePhase.available ||
          phase == UpdatePhase.downloading ||
          phase == UpdatePhase.ready);

  /// The startup popup for the pending release has been silenced — skipped
  /// outright, or snoozed and the snooze has not expired yet. The nav-rail dot
  /// and the Updates page stay visible either way; only the popup is muted.
  bool get announcementSilenced {
    final m = manifest;
    if (m == null) return false;
    final v = '${m.version}';
    if (skippedVersion == v) return true;
    final until = snoozeUntil;
    return snoozeVersion == v && until != null && DateTime.now().isBefore(until);
  }

  UpdateState copyWith({
    UpdatePhase? phase,
    UpdateManifest? manifest,
    double? progress,
    String? error,
    DateTime? lastCheck,
    bool? autoCheck,
    String? skippedVersion,
    String? snoozeVersion,
    DateTime? snoozeUntil,
    bool clearError = false,
    bool clearSilence = false,
  }) =>
      UpdateState(
        phase: phase ?? this.phase,
        manifest: manifest ?? this.manifest,
        progress: progress ?? this.progress,
        error: clearError ? null : (error ?? this.error),
        lastCheck: lastCheck ?? this.lastCheck,
        autoCheck: autoCheck ?? this.autoCheck,
        skippedVersion:
            clearSilence ? null : (skippedVersion ?? this.skippedVersion),
        snoozeVersion:
            clearSilence ? null : (snoozeVersion ?? this.snoozeVersion),
        snoozeUntil: clearSilence ? null : (snoozeUntil ?? this.snoozeUntil),
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
    final snooze = prefs.getString(kUpdateSnoozeUntilKey);
    return UpdateState(
      lastCheck: raw == null ? null : DateTime.tryParse(raw),
      autoCheck: prefs.getBool(kUpdateAutoCheckKey) ?? true,
      skippedVersion: prefs.getString(kUpdateSkippedKey),
      snoozeVersion: prefs.getString(kUpdateSnoozeVersionKey),
      snoozeUntil: snooze == null ? null : DateTime.tryParse(snooze),
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

  /// The check that runs once per launch, from [_startUpdateChecks] in app.dart.
  ///
  /// Deliberately ignores [kUpdateCheckInterval]: "did anything ship since I
  /// last had the app open?" is exactly the question a launch should answer,
  /// and this app can sit in the tray for a week between restarts, so the daily
  /// debounce would routinely swallow the one check the user notices. Still
  /// silent — no error toast for a check nobody asked for — and still floored by
  /// [kUpdateStartupMinInterval] so a restart loop cannot hammer GitHub.
  Future<void> startupCheck() async {
    if (!_enabled) return;
    if (!state.autoCheck) return;
    final last = state.lastCheck;
    if (last != null &&
        DateTime.now().difference(last) < kUpdateStartupMinInterval) {
      return;
    }
    await check(silent: true);
  }

  Future<void> check({bool silent = false}) async {
    if (!_enabled) {
      if (!silent) {
        state = state.copyWith(
          phase: UpdatePhase.error,
          error: L10n.global.t('Updates are disabled in a dev build.'),
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
        error: silent
            ? null
            : L10n.global.t('Could not reach the update server.'),
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

  /// Whether to put the "update available" popup on screen. Respects both ways
  /// the user can decline it: skipping this exact version, or postponing it.
  bool shouldAnnounce() {
    final m = state.manifest;
    if (m == null || state.phase != UpdatePhase.available) return false;
    return !state.announcementSilenced;
  }

  /// "Don't tell me about this version again." A later release still announces.
  Future<void> skipCurrent() async {
    final m = state.manifest;
    if (m == null) return;
    final v = '${m.version}';
    await ref.read(prefsProvider).setString(kUpdateSkippedKey, v);
    state = state.copyWith(skippedVersion: v);
  }

  /// "Remind me later" — silences this version's popup for [after].
  Future<void> remindLater({Duration after = kUpdateSnoozeDuration}) async {
    final m = state.manifest;
    if (m == null) return;
    final v = '${m.version}';
    final until = DateTime.now().add(after);
    final prefs = ref.read(prefsProvider);
    await prefs.setString(kUpdateSnoozeVersionKey, v);
    await prefs.setString(kUpdateSnoozeUntilKey, until.toIso8601String());
    state = state.copyWith(snoozeVersion: v, snoozeUntil: until);
  }

  /// Undo either of the above, from Settings → Updates. Clears both records
  /// rather than only the matching one: the user is asking to be told again,
  /// and a leftover snooze on the same version would quietly re-mute it.
  Future<void> resumeAnnouncements() async {
    final prefs = ref.read(prefsProvider);
    await prefs.remove(kUpdateSkippedKey);
    await prefs.remove(kUpdateSnoozeVersionKey);
    await prefs.remove(kUpdateSnoozeUntilKey);
    state = state.copyWith(clearSilence: true);
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
      state = state.copyWith(
          phase: UpdatePhase.available,
          error: L10n.global.tArgs('Download failed: {e}', {'e': e}));
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
      state = state.copyWith(
          phase: UpdatePhase.ready,
          error: L10n.global
              .tArgs('Could not start the updater: {e}', {'e': e}));
      return;
    }
    await shutdownApp(
      supervisor: ref.read(daemonSupervisorProvider),
      uiPort: ref.read(appConfigProvider).uiPort,
    );
  }
}
