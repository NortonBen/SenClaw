/// Web / unsupported-platform no-op implementation of [NotificationService].
class NotificationService {
  static final NotificationService _i = NotificationService._();
  factory NotificationService() => _i;
  NotificationService._();

  /// Whether OS notifications are available on this platform (never on web).
  bool get supported => false;

  Future<void> init() async {}

  Future<bool> requestPermission() async => false;

  Future<void> show(String title, String body) async {}
}
