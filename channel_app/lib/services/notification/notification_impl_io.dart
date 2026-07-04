import 'dart:io' show Platform;
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import '../logger_service.dart';

/// Real OS-tray notifications on Android / iOS / macOS via
/// flutter_local_notifications. Other native platforms (Windows/Linux) fall
/// through as unsupported no-ops.
class NotificationService {
  static final NotificationService _i = NotificationService._();
  factory NotificationService() => _i;
  NotificationService._();

  final _plugin = FlutterLocalNotificationsPlugin();
  bool _inited = false;
  int _id = 1000;

  bool get supported =>
      Platform.isAndroid || Platform.isIOS || Platform.isMacOS;

  Future<void> init() async {
    if (_inited || !supported) return;
    const android = AndroidInitializationSettings('@mipmap/launcher_icon');
    // Permissions are requested explicitly from the settings toggle, not here.
    const darwin = DarwinInitializationSettings(
      requestAlertPermission: false,
      requestBadgePermission: false,
      requestSoundPermission: false,
    );
    const settings = InitializationSettings(
      android: android,
      iOS: darwin,
      macOS: darwin,
    );
    try {
      await _plugin.initialize(settings);
      _inited = true;
    } catch (e) {
      Log.w('[Notif] init failed: $e');
    }
  }

  /// Prompt for notification permission (Android 13+, iOS, macOS). Returns
  /// whether it was granted.
  Future<bool> requestPermission() async {
    if (!supported) return false;
    await init();
    try {
      if (Platform.isAndroid) {
        final impl = _plugin.resolvePlatformSpecificImplementation<
            AndroidFlutterLocalNotificationsPlugin>();
        return (await impl?.requestNotificationsPermission()) ?? false;
      }
      if (Platform.isIOS) {
        final impl = _plugin.resolvePlatformSpecificImplementation<
            IOSFlutterLocalNotificationsPlugin>();
        return (await impl?.requestPermissions(
                alert: true, badge: true, sound: true)) ??
            false;
      }
      if (Platform.isMacOS) {
        final impl = _plugin.resolvePlatformSpecificImplementation<
            MacOSFlutterLocalNotificationsPlugin>();
        return (await impl?.requestPermissions(
                alert: true, badge: true, sound: true)) ??
            false;
      }
    } catch (e) {
      Log.w('[Notif] permission request failed: $e');
    }
    return false;
  }

  Future<void> show(String title, String body) async {
    if (!supported) return;
    await init();
    const android = AndroidNotificationDetails(
      'senclaw_messages',
      'Messages',
      channelDescription: 'New agent messages',
      importance: Importance.high,
      priority: Priority.high,
    );
    const darwin = DarwinNotificationDetails();
    const details = NotificationDetails(
      android: android,
      iOS: darwin,
      macOS: darwin,
    );
    try {
      await _plugin.show(_id++, title, body, details);
    } catch (e) {
      Log.w('[Notif] show failed: $e');
    }
  }
}
