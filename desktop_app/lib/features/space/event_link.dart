import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/i18n/l10n.dart';
import 'space_providers.dart';

/// Opening a calendar event's linked screen.
///
/// A calendar event can carry an internal Space-App route (`/space/app/study?
/// session=…`) so tapping the event — or its reminder — lands on the thing the
/// event is about, not just on a description of it.
///
/// The daemon already refuses to store a link that is not a `/space/app/…`
/// path. This re-checks anyway: the value arrives here through a WS
/// notification payload as well as through the REST list, and a UI that opens
/// whatever a payload says is a UI that can be pointed anywhere.
bool isInternalAppLink(String? link) {
  if (link == null) return false;
  final l = link.trim();
  if (!l.startsWith('/space/app/')) return false;
  if (l.contains('..') || l.contains('\\') || l.startsWith('//')) return false;
  return appIdOfLink(l) != null;
}

/// App id inside `/space/app/<id>[/…][?…]`, or null when the route is malformed.
String? appIdOfLink(String link) {
  if (!link.startsWith('/space/app/')) return null;
  final rest = link.substring('/space/app/'.length);
  final id = rest.split(RegExp(r'[/?#]')).first;
  if (id.isEmpty) return null;
  final ok = RegExp(r'^[A-Za-z0-9_-]+$').hasMatch(id);
  return ok ? id : null;
}

/// Launch the app a calendar event points at and navigate to the Apps screen.
///
/// Returns a human-readable reason on failure instead of doing nothing —
/// a button that silently no-ops is the failure mode this whole feature
/// exists to remove.
Future<String?> openEventLink(
  BuildContext context,
  WidgetRef ref,
  String? link,
) async {
  if (!isInternalAppLink(link)) {
    return L10n.global.t('This event has no valid link to open.');
  }
  final id = appIdOfLink(link!.trim())!;
  final apps = await ref.read(spaceAppsProvider.future);
  final app = apps.where((a) => a.id == id).firstOrNull;
  if (app == null) {
    return L10n.global.tArgs(
        'App `{id}` is not installed — install it, then open this event again.',
        {'id': id});
  }
  if (!app.enabled) {
    return L10n.global.tArgs('App `{id}` is disabled.', {'id': id});
  }
  ref.read(runningAppsProvider.notifier).openAt(app, link.trim());
  if (context.mounted) context.go('/apps');
  return null;
}
