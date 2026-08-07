import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;

import '../config/app_config.dart';
import '../i18n/l10n.dart';
import 'update_manifest.dart';
import 'version.dart';

const _repo = 'NortonBen/SenClaw';

/// Manifest URL. Deliberately a release ASSET, not `api.github.com`: the API
/// rate-limits at 60 requests/hour per IP, which every machine behind one
/// office NAT shares. `latest/download/` is a CDN redirect with no such limit,
/// and it resolves to the newest NON-prerelease — so `v*-beta.*` tags stay
/// invisible to normal users without any channel logic here.
const _manifestUrl = 'https://github.com/$_repo/releases/latest/download/latest.json';

String assetUrl(String name) =>
    'https://github.com/$_repo/releases/latest/download/$name';

/// Derive the bundle to replace from the running executable's path.
///
/// Split out from [bundlePath] so the arithmetic is testable — getting the
/// number of `..` wrong points the updater at the PARENT of the app bundle,
/// which would have it replace whatever else lives in /Applications.
@visibleForTesting
String bundlePathFrom(String resolvedExecutable, {required bool isMacOS}) {
  final exeDir = p.dirname(resolvedExecutable);
  if (isMacOS) {
    // …/SenClaw Desktop.app/Contents/MacOS/senclaw_desktop
    //   dirname     → …/SenClaw Desktop.app/Contents/MacOS
    //   ..          → …/SenClaw Desktop.app/Contents
    //   ..          → …/SenClaw Desktop.app   ← the bundle
    return p.normalize(p.join(exeDir, '..', '..'));
  }
  // Windows/Linux: the bundle IS the directory holding the executable.
  return exeDir;
}

/// The bundle this app is running from — passed to `apply-update --target`.
/// Never re-probed on the Rust side: the user may run the app from anywhere.
String bundlePath() =>
    bundlePathFrom(Platform.resolvedExecutable, isMacOS: Platform.isMacOS);

Directory senclawTmpDir() {
  final home = Platform.environment['HOME'] ??
      Platform.environment['USERPROFILE'] ??
      Directory.systemTemp.path;
  return Directory(p.join(home, '.senclaw', 'tmp'));
}

/// Why an update cannot proceed, phrased for the user.
class UpdateUnavailable implements Exception {
  UpdateUnavailable(this.message);
  final String message;
  @override
  String toString() => message;
}

class UpdateService {
  /// [currentVersion] and [buildTarget] default to this build's compile-time
  /// identity and are injectable only so tests can drive the decision that
  /// matters — "is there an update?" — which is otherwise unreachable, since a
  /// test binary is always version 'dev' and would answer "no" every time.
  UpdateService({
    http.Client? client,
    Future<File?> Function()? resolveDaemonBinary,
    String? currentVersion,
    String? buildTarget,
  })  : _client = client ?? http.Client(),
        _resolveDaemonBinary = resolveDaemonBinary,
        _version = currentVersion ?? kAppVersion,
        _target = buildTarget ?? kBuildTarget;

  final http.Client _client;
  final Future<File?> Function()? _resolveDaemonBinary;
  final String _version;
  final String _target;

  /// This build's identity. Read these instead of the `kAppVersion` /
  /// `kBuildTarget` globals anywhere the updater reasons about itself, so the
  /// answer comes from one place — and so tests can drive it.
  String get currentVersion => _version;
  String get buildTarget => _target;

  bool get isDevBuild => _version == 'dev';

  /// The asset matching this build, or null when the release has none for it.
  UpdateAsset? assetFor(UpdateManifest m) => m.assetFor(_target);

  /// Fetch and parse latest.json. Returns null when there is simply nothing to
  /// say — offline, 404 (releases older than the manifest step), malformed
  /// body. A background check the user never asked for must not raise errors.
  Future<UpdateManifest?> fetchManifest() async {
    try {
      final resp = await _client
          .get(Uri.parse(_manifestUrl))
          .timeout(const Duration(seconds: 15));
      if (resp.statusCode != 200) return null;
      // NOT resp.body: GitHub serves release assets as application/octet-stream
      // with no charset, and package:http then decodes as latin-1 — which turns
      // the Vietnamese release notes into mojibake ("chuẩn hoá" → "chuá°©n hoÃ¡").
      return UpdateManifest.tryParse(utf8.decode(resp.bodyBytes, allowMalformed: true));
    } catch (_) {
      return null;
    }
  }

  /// True when [m] is strictly newer than this build.
  ///
  /// A dev build always returns false: [Version.tryParse] rejects `'dev'`, and
  /// treating that as "unknown, so probably old" would have the updater offer
  /// to overwrite a developer's own working build with a release.
  bool isNewer(UpdateManifest m) {
    final current = Version.tryParse(_version);
    if (current == null) return false;
    return m.version > current;
  }

  /// Fail before downloading ~200 MB the swap could never install anyway.
  Future<void> ensureInstallable(UpdateManifest m) async {
    if (isDevBuild) {
      throw UpdateUnavailable(
          L10n.global.t('This is a dev build — updates are disabled.'));
    }
    if (_target.isEmpty || assetFor(m) == null) {
      throw UpdateUnavailable(
        L10n.global.tArgs(
          'Release {v} has no bundle for this platform ({target}).',
          {'v': m.version, 'target': _target},
        ),
      );
    }
    final min = m.minVersion;
    final current = Version.tryParse(_version);
    if (min != null && current != null && current < min) {
      throw UpdateUnavailable(
        L10n.global.tArgs(
          'Version {from} is too old to update directly to {to} — reinstall '
          'from the SenClaw website.',
          {'from': _version, 'to': m.version},
        ),
      );
    }
    await _ensureBundleWritable();
  }

  /// Check the install location NOW rather than after the download: finding out
  /// that /Applications belongs to another admin is a far better first message
  /// than a failure at the end of a long transfer.
  Future<void> _ensureBundleWritable() async {
    final dir = Directory(p.dirname(bundlePath()));
    final probe = File(p.join(dir.path, '.senclaw-write-probe'));
    try {
      await probe.writeAsString('');
      await probe.delete();
    } catch (_) {
      throw UpdateUnavailable(
        L10n.global.tArgs(
          'Cannot write to {dir} — the app was installed by another user. '
          'Update from a terminal instead: senclaw update',
          {'dir': dir.path},
        ),
      );
    }
  }

  /// Stream the bundle to ~/.senclaw/tmp, reporting 0..1 progress.
  Future<File> download(
    UpdateAsset asset, {
    void Function(double)? onProgress,
    CancelToken? cancel,
  }) async {
    final tmp = senclawTmpDir();
    await tmp.create(recursive: true);
    final dest = File(p.join(tmp.path, asset.name));
    if (await dest.exists()) await dest.delete();

    final req = http.Request('GET', Uri.parse(assetUrl(asset.name)));
    final resp = await _client.send(req);
    if (resp.statusCode != 200) {
      throw UpdateUnavailable(L10n.global
          .tArgs('Download failed (HTTP {code}).', {'code': resp.statusCode}));
    }
    final total = resp.contentLength ?? asset.size;

    final sink = dest.openWrite();
    var received = 0;
    try {
      await for (final chunk in resp.stream) {
        if (cancel?.isCancelled ?? false) {
          throw UpdateUnavailable(L10n.global.t('Download cancelled.'));
        }
        sink.add(chunk);
        received += chunk.length;
        if (total > 0) onProgress?.call(received / total);
      }
      await sink.flush();
    } catch (_) {
      await sink.close();
      // A partial file would fail the checksum later anyway; drop it now so a
      // retry starts clean.
      if (await dest.exists()) await dest.delete();
      rethrow;
    }
    await sink.close();
    return dest;
  }

  /// Hand off to the detached updater and return — the CALLER then shuts the
  /// app down, which is the event the updater is waiting on.
  ///
  /// The updater must run from OUTSIDE the bundle it replaces: Windows locks a
  /// running .exe, and on macOS the app's own Resources would be pulled out
  /// from under it. Copying to ~/.senclaw/tmp first is what makes it safe on
  /// every platform.
  Future<void> spawnUpdater(File staged, UpdateAsset asset) async {
    final (updater, argPrefix) = await _stageUpdaterOutsideBundle();
    await Process.start(
      updater.path,
      [
        ...argPrefix,
        '--staged', staged.path,
        '--target', bundlePath(),
        '--pid', '$pid',
        if (asset.sha256.isNotEmpty) ...['--sha256', asset.sha256],
        '--relaunch',
      ],
      mode: ProcessStartMode.detached,
    );
  }

  /// The `update_desktop` helper shipped in this bundle, or null on an install
  /// that predates it. Note the updater that runs is always the one from the
  /// version being REPLACED — the helper only exists once the user is ON a
  /// build that bundles it, hence the legacy fallback below.
  File? _bundledUpdateHelper() {
    final name = Platform.isWindows ? 'update_desktop.exe' : 'update_desktop';
    final fromEnv = Platform.environment['SENCLAW_UPDATER'];
    if (fromEnv != null && File(fromEnv).existsSync()) return File(fromEnv);
    final exeDir = p.dirname(Platform.resolvedExecutable);
    final candidates = <String>[
      p.join(exeDir, name), // Windows/Linux: alongside the app binary
      // macOS .app bundle: Contents/MacOS/<app> → Contents/Resources/<name>
      p.normalize(p.join(exeDir, '..', 'Resources', name)),
    ];
    for (final c in candidates) {
      if (File(c).existsSync()) return File(c);
    }
    return null;
  }

  /// Copy the updater out of the bundle and return it plus the argument
  /// prefix it expects: `update_desktop` takes the flags directly (a windowed
  /// no-console binary on Windows, with a small progress window), while the
  /// legacy fallback — the full `senclaw` daemon binary — needs its
  /// `apply-update` subcommand and flashes a console on Windows. The fallback
  /// keeps updates working for installs whose current bundle predates the
  /// helper.
  Future<(File, List<String>)> _stageUpdaterOutsideBundle() async {
    final helper = _bundledUpdateHelper();
    final src =
        helper ?? await (_resolveDaemonBinary?.call() ?? Future.value(null));
    if (src == null) {
      throw UpdateUnavailable(
        L10n.global.t('Cannot find the senclaw binary to run the update with.'),
      );
    }
    final tmp = senclawTmpDir();
    await tmp.create(recursive: true);
    final dest = File(
      p.join(tmp.path, Platform.isWindows ? 'senclaw-updater.exe' : 'senclaw-updater'),
    );
    if (await dest.exists()) await dest.delete();
    await src.copy(dest.path);
    if (!Platform.isWindows) {
      // File.copy does not carry the executable bit across.
      await Process.run('chmod', ['+x', dest.path]);
    }
    return (dest, helper != null ? const <String>[] : const ['apply-update']);
  }
}

/// Trivial cooperative cancellation for an in-flight download.
class CancelToken {
  bool _cancelled = false;
  bool get isCancelled => _cancelled;
  void cancel() => _cancelled = true;
}
