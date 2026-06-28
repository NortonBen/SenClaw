import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/prefs.dart';

const _kThemeModeKey = 'senclaw:theme-mode';

/// App theme mode, persisted. Defaults to following the OS (`system`).
class ThemeModeNotifier extends StateNotifier<ThemeMode> {
  ThemeModeNotifier(Ref ref) : _ref = ref, super(_load(ref));
  final Ref _ref;

  static ThemeMode _load(Ref ref) {
    switch (ref.read(prefsHelperProvider).string(_kThemeModeKey, 'system')) {
      case 'light':
        return ThemeMode.light;
      case 'dark':
        return ThemeMode.dark;
      default:
        return ThemeMode.system;
    }
  }

  void set(ThemeMode mode) {
    state = mode;
    _ref.read(prefsHelperProvider).setString(_kThemeModeKey, mode.name);
  }
}

final themeModeProvider =
    StateNotifierProvider<ThemeModeNotifier, ThemeMode>(
      (ref) => ThemeModeNotifier(ref),
    );
