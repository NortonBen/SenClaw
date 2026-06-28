import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Injected from `main()` after `SharedPreferences.getInstance()`.
final prefsProvider = Provider<SharedPreferences>(
  (_) => throw UnimplementedError('prefsProvider must be overridden in main()'),
);

/// Tiny typed helpers over SharedPreferences for the sidebar's persisted state
/// (replaces the React `localStorage` keys verbatim).
class Prefs {
  Prefs(this._p);
  final SharedPreferences _p;

  Set<String> stringSet(String key) {
    final raw = _p.getString(key);
    if (raw == null) return {};
    try {
      return (jsonDecode(raw) as List).map((e) => '$e').toSet();
    } catch (_) {
      return {};
    }
  }

  Future<void> setStringSet(String key, Set<String> value) =>
      _p.setString(key, jsonEncode(value.toList()));

  String string(String key, String fallback) => _p.getString(key) ?? fallback;
  Future<void> setString(String key, String value) => _p.setString(key, value);

  Map<String, int> intMap(String key) {
    final raw = _p.getString(key);
    if (raw == null) return {};
    try {
      return (jsonDecode(raw) as Map)
          .map((k, v) => MapEntry('$k', (v as num).toInt()));
    } catch (_) {
      return {};
    }
  }

  Future<void> setIntMap(String key, Map<String, int> value) =>
      _p.setString(key, jsonEncode(value));
}

final prefsHelperProvider = Provider<Prefs>((ref) => Prefs(ref.watch(prefsProvider)));

// localStorage keys ported 1:1 from the React sidebar.
const kPinnedKey = 'senclaw:pinned-jids';
const kOrganizeKey = 'senclaw:sessionlist-organize';
const kSortKey = 'senclaw:sessionlist-sort';
const kCollapsedKey = 'senclaw:collapsed-folders';
const kLastSeenKey = 'senclaw:chat-last-seen';
const kPinnedAppsKey = 'senclaw:pinned-apps';
