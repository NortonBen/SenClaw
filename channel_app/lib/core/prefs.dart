import 'dart:convert';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Injected from `main()` after `SharedPreferences.getInstance()`.
final prefsProvider = Provider<SharedPreferences>(
  (_) => throw UnimplementedError('prefsProvider must be overridden in main()'),
);

/// Tiny typed helpers over SharedPreferences for persisted UI state.
class Prefs {
  Prefs(this._p);
  final SharedPreferences _p;

  String string(String key, String fallback) => _p.getString(key) ?? fallback;
  Future<void> setString(String key, String value) => _p.setString(key, value);

  bool boolean(String key, bool fallback) => _p.getBool(key) ?? fallback;
  Future<void> setBool(String key, bool value) => _p.setBool(key, value);

  /// An ordered, de-duplicated string list stored as a JSON array.
  List<String> stringList(String key) {
    final raw = _p.getString(key);
    if (raw == null) return [];
    try {
      return (jsonDecode(raw) as List).map((e) => '$e').toList();
    } catch (_) {
      return [];
    }
  }

  Future<void> setStringList(String key, List<String> value) =>
      _p.setString(key, jsonEncode(value));
}

final prefsHelperProvider = Provider<Prefs>((ref) => Prefs(ref.watch(prefsProvider)));
