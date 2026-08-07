/// Interactive screen capture, taken from the tray menu or the global shortcut.
///
/// The native side (`AppDelegate.handleCapture`) shells out to
/// `/usr/sbin/screencapture -i` and writes a PNG into the daemon's screenshots
/// directory; the daemon serves it back over HTTP so note bodies can reference
/// the shot as plain markdown. See `space_screenshot_get` in
/// `src/gateway/ui_server/space.rs`.
library;

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/i18n/l10n.dart';

/// A capture that landed on disk.
class CaptureResult {
  /// Absolute path on disk — what OCR (which is path-based) would take.
  final String path;

  /// Bare filename; the key the daemon serves the shot under.
  final String name;

  const CaptureResult({required this.path, required this.name});

  /// URL the daemon serves this shot at. Usable directly in a markdown note
  /// body and by `NetworkImage`, which is HTTP-only and cannot load `file://`.
  String url(String host, int uiPort) =>
      'http://$host:$uiPort/api/space/screenshots/$name';
}

/// macOS withheld Screen Recording. The capture cannot succeed until the user
/// grants it in System Settings — see [openScreenRecordingSettings].
class ScreenCapturePermissionDenied implements Exception {
  const ScreenCapturePermissionDenied();
  @override
  String toString() => 'Screen Recording permission not granted';
}

/// The capture failed for a reason the user can't fix by granting permission
/// (spawn failure, unwritable directory).
class ScreenCaptureFailed implements Exception {
  final String message;
  const ScreenCaptureFailed(this.message);
  @override
  String toString() => 'Screen capture failed: $message';
}

const _channel = MethodChannel('senclaw/app');

/// Only macOS has a native capture bridge today. The tray item is hidden
/// elsewhere rather than failing at click time.
bool get isCaptureSupported =>
    !kIsWeb && defaultTargetPlatform == TargetPlatform.macOS;

/// Run the interactive capture: the user drags a region, presses SPACE for
/// window mode, or ESC to cancel.
///
/// Returns null when the user cancelled. Throws
/// [ScreenCapturePermissionDenied] or [ScreenCaptureFailed] otherwise.
///
/// [dir] must be the daemon's screenshots directory (`screenshotsDir` from
/// `GET /api/config`) — writing anywhere else means the daemon won't serve it.
Future<CaptureResult?> captureScreen({String? dir}) async {
  try {
    final res = await _channel.invokeMapMethod<String, dynamic>(
      'capture',
      {'dir': ?dir},
    );
    if (res == null) return null; // user cancelled
    return CaptureResult(
      path: res['path'] as String,
      name: res['name'] as String,
    );
  } on PlatformException catch (e) {
    if (e.code == 'permission_required') {
      throw const ScreenCapturePermissionDenied();
    }
    throw ScreenCaptureFailed(e.message ?? e.code);
  } on MissingPluginException {
    throw ScreenCaptureFailed(
        L10n.global.t('Capture is not supported on this platform'));
  }
}

/// Deep-link straight to Privacy → Screen Recording. macOS only prompts once,
/// so after a denial the user has no way back without this.
Future<void> openScreenRecordingSettings() => launchUrl(Uri.parse(
    'x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture'));
