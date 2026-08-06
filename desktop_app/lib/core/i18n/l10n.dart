import 'package:flutter/foundation.dart' show SynchronousFuture;
import 'package:flutter/widgets.dart';

import 'vi/background.dart';
import 'vi/chat_main.dart';
import 'vi/chat_misc.dart';
import 'vi/chat_widgets.dart';
import 'vi/cognitive_wiki.dart';
import 'vi/common.dart';
import 'vi/dashboard_usage.dart';
import 'vi/dock_cowork.dart';
import 'vi/kanban.dart';
import 'vi/plugins_misc.dart';
import 'vi/plugins_screen.dart';
import 'vi/settings_misc.dart';
import 'vi/settings_screen.dart';
import 'vi/shell_misc.dart';
import 'vi/space.dart';
import 'vi/workflow.dart';

/// App localization. The English string IS the key: `t('Settings')` returns
/// the Vietnamese translation when the app language is `vi`, and the string
/// itself otherwise (or when no translation exists — English is the source of
/// truth, so missing entries degrade gracefully instead of showing raw keys).
///
/// Widgets use the [L10nX] extension (`context.tr(...)`) so they rebuild when
/// the language changes. Non-widget code (tray menu, notifications, providers)
/// uses [L10n.global], which the locale provider keeps in sync.
class L10n {
  const L10n(this.code);

  /// 'en' | 'vi'.
  final String code;

  bool get isVi => code == 'vi';

  /// Vietnamese dictionary, merged from per-area part files under `vi/` so
  /// parallel edits never collide on one giant map.
  static final Map<String, String> _vi = {
    ...viCommon,
    ...viShellMisc,
    ...viSettingsScreen,
    ...viSettingsMisc,
    ...viPluginsScreen,
    ...viPluginsMisc,
    ...viChatMain,
    ...viChatMisc,
    ...viChatWidgets,
    ...viSpace,
    ...viWorkflow,
    ...viBackground,
    ...viKanban,
    ...viDashboardUsage,
    ...viCognitiveWiki,
    ...viDockCowork,
  };

  /// Current language for code that has no BuildContext (tray menu labels,
  /// OS notifications, provider-side snackbar text). Updated by
  /// `localeCodeProvider`; defaults to English so widget tests that pump
  /// subtrees without our delegate keep asserting English strings.
  static L10n global = const L10n('en');

  /// Translate [en]. Falls back to the English string itself.
  String t(String en) => isVi ? (_vi[en] ?? en) : en;

  /// Translate a template and substitute `{name}` placeholders:
  /// `tArgs('Version {v}', {'v': '1.2'})`.
  String tArgs(String en, Map<String, Object?> args) {
    var s = t(en);
    args.forEach((k, v) => s = s.replaceAll('{$k}', '$v'));
    return s;
  }

  /// English-style plural pick with `{n}` substitution. Vietnamese does not
  /// inflect, so both keys usually map to the same translation.
  String plural(int n, String one, String many) =>
      tArgs(n == 1 ? one : many, {'n': n});

  static L10n of(BuildContext context) =>
      Localizations.of<L10n>(context, L10n) ?? global;
}

class L10nDelegate extends LocalizationsDelegate<L10n> {
  const L10nDelegate();

  @override
  bool isSupported(Locale locale) => true;

  @override
  Future<L10n> load(Locale locale) =>
      SynchronousFuture(L10n(locale.languageCode == 'vi' ? 'vi' : 'en'));

  @override
  bool shouldReload(L10nDelegate old) => false;
}

extension L10nX on BuildContext {
  /// Translate an English UI string for the active app language.
  String tr(String en) => L10n.of(this).t(en);

  /// Translate a `{placeholder}` template: `context.trArgs('Hi {name}', ...)`.
  String trArgs(String en, Map<String, Object?> args) =>
      L10n.of(this).tArgs(en, args);

  /// Plural helper: `context.trPlural(n, '{n} item', '{n} items')`.
  String trPlural(int n, String one, String many) =>
      L10n.of(this).plural(n, one, many);
}
