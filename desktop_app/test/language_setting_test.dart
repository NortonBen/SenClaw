import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:intl/date_symbol_data_local.dart' show initializeDateFormatting;
import 'package:intl/intl.dart' show DateFormat, Intl;
import 'package:senclaw_desktop/core/i18n/l10n.dart';
import 'package:senclaw_desktop/core/i18n/locale_provider.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Minimal host that renders `context.tr(...)` under the app's real delegate
/// stack, driven by the same provider the Settings screen writes to.
class _Host extends ConsumerWidget {
  const _Host(this.key1);
  final String key1;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return MaterialApp(
      locale: Locale(ref.watch(localeCodeProvider)),
      supportedLocales: const [Locale('en'), Locale('vi')],
      localizationsDelegates: const [
        L10nDelegate(),
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      home: Builder(builder: (c) => Text(c.tr(key1))),
    );
  }
}

Future<ProviderContainer> _container() async {
  SharedPreferences.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  return ProviderContainer(overrides: [prefsProvider.overrideWithValue(prefs)]);
}

void main() {
  testWidgets('defaults to English and translates after switching to vi',
      (tester) async {
    final container = await _container();
    addTearDown(container.dispose);

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const _Host('Settings'),
    ));
    expect(find.text('Settings'), findsOneWidget);

    container.read(appLanguageProvider.notifier).set(AppLanguage.vi);
    await tester.pumpAndSettle();
    expect(find.text('Cài đặt'), findsOneWidget);

    container.read(appLanguageProvider.notifier).set(AppLanguage.en);
    await tester.pumpAndSettle();
    expect(find.text('Settings'), findsOneWidget);
  });

  testWidgets('untranslated keys fall back to the English source string',
      (tester) async {
    final container = await _container();
    addTearDown(container.dispose);

    await tester.pumpWidget(UncontrolledProviderScope(
      container: container,
      child: const _Host('A string nobody has translated yet'),
    ));
    container.read(appLanguageProvider.notifier).set(AppLanguage.vi);
    await tester.pumpAndSettle();
    expect(find.text('A string nobody has translated yet'), findsOneWidget);
  });

  test('language choice persists across restarts', () async {
    SharedPreferences.setMockInitialValues({});
    final prefs = await SharedPreferences.getInstance();

    final first =
        ProviderContainer(overrides: [prefsProvider.overrideWithValue(prefs)]);
    first.read(appLanguageProvider.notifier).set(AppLanguage.vi);
    first.dispose();

    final second =
        ProviderContainer(overrides: [prefsProvider.overrideWithValue(prefs)]);
    addTearDown(second.dispose);
    expect(second.read(appLanguageProvider), AppLanguage.vi);
    expect(second.read(localeCodeProvider), 'vi');
  });

  test('L10n.global tracks the selected language for context-free callers',
      () async {
    final container = await _container();
    addTearDown(container.dispose);
    addTearDown(() => L10n.global = const L10n('en'));

    container.read(appLanguageProvider.notifier).set(AppLanguage.vi);
    container.read(localeCodeProvider); // recompute — this is what syncs global
    expect(L10n.global.t('Cancel'), 'Huỷ');

    container.read(appLanguageProvider.notifier).set(AppLanguage.en);
    container.read(localeCodeProvider);
    expect(L10n.global.t('Cancel'), 'Cancel');
  });

  test('DateFormat month/weekday names follow the selected language',
      () async {
    await initializeDateFormatting();
    final container = await _container();
    addTearDown(container.dispose);
    addTearDown(() => Intl.defaultLocale = null);

    final march = DateTime(2026, 3, 9); // a Monday

    container.read(appLanguageProvider.notifier).set(AppLanguage.en);
    container.read(localeCodeProvider);
    expect(DateFormat('MMMM').format(march), 'March');
    expect(DateFormat('EEE').format(march), 'Mon');

    container.read(appLanguageProvider.notifier).set(AppLanguage.vi);
    container.read(localeCodeProvider);
    expect(DateFormat('MMMM').format(march), contains('3'));
    expect(DateFormat('EEE').format(march), isNot('Mon'));

    // Numeric-only patterns must not shift — the app relies on these for
    // timestamps and they are language-neutral by design.
    expect(DateFormat('yyyy-MM-dd HH:mm').format(DateTime(2026, 3, 9, 14, 5)),
        '2026-03-09 14:05');
  });

  test('placeholder and plural helpers substitute correctly', () {
    const vi = L10n('vi');
    expect(vi.tArgs('Version {v}', {'v': '1.2.3'}), contains('1.2.3'));
    expect(const L10n('en').plural(1, '{n} channel', '{n} channels'),
        '1 channel');
    expect(const L10n('en').plural(3, '{n} channel', '{n} channels'),
        '3 channels');
  });
}
