import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart' show FontLoader;
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/i18n/l10n.dart';
import 'package:senclaw_desktop/core/i18n/locale_provider.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/core/update/update_announcer.dart';
import 'package:senclaw_desktop/core/update/update_manifest.dart';
import 'package:senclaw_desktop/features/settings/settings_screen.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Vietnamese strings run longer than their English sources, so a layout that
/// only just fits in English can overflow once translated. These render the
/// screens that are safe to build without a live daemon and fail on any
/// overflow the framework reports.
///
/// Measuring that needs a REAL font: `flutter_test`'s default renders every
/// glyph as a box exactly `fontSize` wide, which inflates a 14px label to
/// 14px *per character* and reports overflow that does not exist on screen.
/// Roboto ships with the SDK, so load it and render as the app does.
bool _fontsLoaded = false;

Future<bool> _loadRealFont() async {
  if (_fontsLoaded) return true;
  final root = Platform.environment['FLUTTER_ROOT'];
  if (root == null) return false;
  final dir = Directory('$root/bin/cache/artifacts/material_fonts');
  if (!dir.existsSync()) return false;
  for (final family in ['Roboto']) {
    final loader = FontLoader(family);
    for (final weight in ['Regular', 'Medium', 'Bold']) {
      final f = File('${dir.path}/Roboto-$weight.ttf');
      if (f.existsSync()) {
        loader.addFont(Future.value(f.readAsBytesSync().buffer.asByteData()));
      }
    }
    await loader.load();
  }
  _fontsLoaded = true;
  return true;
}

Future<void> _pump(
  WidgetTester tester,
  Widget child, {
  required String lang,
  required Size size,
  String? section,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  SharedPreferences.setMockInitialValues({'senclaw:app-language': lang});
  final prefs = await SharedPreferences.getInstance();
  final container =
      ProviderContainer(overrides: [prefsProvider.overrideWithValue(prefs)]);
  addTearDown(container.dispose);
  addTearDown(() => L10n.global = const L10n('en'));
  // The screen opens on General, whose panels are API-backed; point it at the
  // section under test.
  if (section != null) {
    container.read(settingsSectionProvider.notifier).state = section;
  }

  await tester.pumpWidget(UncontrolledProviderScope(
    container: container,
    child: MaterialApp(
      locale: Locale(container.read(localeCodeProvider)),
      supportedLocales: const [Locale('en'), Locale('vi')],
      localizationsDelegates: const [
        L10nDelegate(),
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      theme: ThemeData(fontFamily: 'Roboto'),
      home: Scaffold(body: child),
    ),
  ));
  await tester.pumpAndSettle();
}

void main() {
  setUpAll(() async {
    if (!await _loadRealFont()) {
      // Without SDK fonts the box-glyph fallback reports phantom overflow, so
      // these assertions would be meaningless rather than merely noisy.
      fail('Roboto not found under FLUTTER_ROOT — cannot measure real layout');
    }
  });

  for (final lang in ['en', 'vi']) {
    testWidgets('Settings › Appearance lays out in $lang at 1280px',
        (tester) async {
      await _pump(tester, const SettingsScreen(),
          lang: lang, size: const Size(1280, 820), section: 'appearance');

      // pumpWidget rethrows layout overflow as a test failure, so reaching
      // here means nothing overflowed. Assert the content actually rendered.
      expect(tester.takeException(), isNull);
      expect(find.text(lang == 'vi' ? 'Cài đặt' : 'Settings'), findsWidgets);
      expect(find.text(lang == 'vi' ? 'Giao diện' : 'Appearance'), findsWidgets);
      // The three language cards; endonyms stay untranslated by design.
      expect(find.text('English'), findsOneWidget);
      expect(find.text('Tiếng Việt'), findsOneWidget);
    });

    testWidgets('Settings sidebar survives a narrow window in $lang',
        (tester) async {
      await _pump(tester, const SettingsScreen(),
          lang: lang, size: const Size(900, 600));
      expect(tester.takeException(), isNull);
    });

    // Three actions on one row, and the Vietnamese labels are the longer ones —
    // the popup greets the user at launch, so an overflow here is the first
    // thing they would see.
    testWidgets('the update popup lays out in $lang', (tester) async {
      await _pump(tester, const SizedBox.expand(),
          lang: lang, size: const Size(900, 700));

      unawaited(showUpdateAvailableDialog(
        tester.element(find.byType(SizedBox).first),
        manifest: UpdateManifest.tryParse(
            '{"version":"0.5.0","notes":"- chuẩn hoá thông báo cập nhật",'
            '"assets":{}}')!,
        currentVersion: '0.4.10',
      ));
      await tester.pumpAndSettle();

      expect(tester.takeException(), isNull);
      expect(
          find.text(lang == 'vi'
              ? 'Nhắc lại sau'
              : 'Remind me later'),
          findsOneWidget);
      expect(find.text(lang == 'vi' ? 'Xem cập nhật' : 'View update'),
          findsOneWidget);
    });
  }

  testWidgets('switching language re-renders the settings sidebar',
      (tester) async {
    await _pump(tester, const SettingsScreen(),
        lang: 'en', size: const Size(1280, 820));
    expect(find.text('Channels'), findsOneWidget);
    expect(find.text('Kênh'), findsNothing);
  });
}
