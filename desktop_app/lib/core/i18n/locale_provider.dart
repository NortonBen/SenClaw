import 'dart:ui' show PlatformDispatcher;

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart' show Intl;

import '../prefs.dart';
import 'l10n.dart';

const _kLanguageKey = 'senclaw:app-language';

/// App display language. `system` follows the OS locale (Vietnamese OS → vi,
/// anything else → en), mirroring how the theme's System card behaves.
enum AppLanguage { system, en, vi }

class AppLanguageNotifier extends StateNotifier<AppLanguage> {
  AppLanguageNotifier(Ref ref) : _ref = ref, super(_load(ref));
  final Ref _ref;

  static AppLanguage _load(Ref ref) {
    switch (ref.read(prefsHelperProvider).string(_kLanguageKey, 'system')) {
      case 'en':
        return AppLanguage.en;
      case 'vi':
        return AppLanguage.vi;
      default:
        return AppLanguage.system;
    }
  }

  void set(AppLanguage lang) {
    state = lang;
    _ref.read(prefsHelperProvider).setString(_kLanguageKey, lang.name);
  }
}

final appLanguageProvider =
    StateNotifierProvider<AppLanguageNotifier, AppLanguage>(
      (ref) => AppLanguageNotifier(ref),
    );

/// Resolved 2-letter language code ('en' | 'vi') the UI should render in.
/// Also keeps [L10n.global] in sync for code without a BuildContext, and
/// [Intl.defaultLocale] in sync so every bare `DateFormat(...)` renders its
/// month and weekday names in the selected language. Only dates are affected —
/// the app formats numbers and money by hand, not through `NumberFormat`.
final localeCodeProvider = Provider<String>((ref) {
  final lang = ref.watch(appLanguageProvider);
  final code = switch (lang) {
    AppLanguage.en => 'en',
    AppLanguage.vi => 'vi',
    AppLanguage.system =>
      PlatformDispatcher.instance.locale.languageCode == 'vi' ? 'vi' : 'en',
  };
  L10n.global = L10n(code);
  Intl.defaultLocale = code;
  return code;
});
