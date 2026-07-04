/// Local (OS-tray) notifications for new agent messages.
///
/// flutter_local_notifications (and dart:io) are unavailable on Flutter web, so
/// the real implementation is conditionally imported; the web build gets a
/// no-op stub. Both expose the same [NotificationService] singleton.
library;

export 'notification/notification_impl_stub.dart'
    if (dart.library.io) 'notification/notification_impl_io.dart';
