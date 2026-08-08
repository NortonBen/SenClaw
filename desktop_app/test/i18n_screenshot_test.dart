@Tags(['screenshot'])
library;

import 'dart:async';
import 'dart:convert';
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
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Renders Settings › Appearance in each language to a PNG. Run manually:
///   flutter test test/i18n_screenshot_test.dart --update-goldens
/// Not part of the normal suite (see `tags` in dart_test.yaml) — golden bytes
/// differ across platforms and font versions, so this is a review aid, not a
/// regression gate.
Future<void> _loadFont() async {
  final root = Platform.environment['FLUTTER_ROOT'];
  final dir = Directory('$root/bin/cache/artifacts/material_fonts');
  final loader = FontLoader('Roboto');
  for (final w in ['Regular', 'Medium', 'Bold']) {
    final f = File('${dir.path}/Roboto-$w.ttf');
    if (f.existsSync()) {
      loader.addFont(Future.value(f.readAsBytesSync().buffer.asByteData()));
    }
  }
  await loader.load();
}

void main() {
  setUpAll(_loadFont);

  for (final lang in ['en', 'vi']) {
    testWidgets('appearance-$lang', (tester) async {
      tester.view.physicalSize = const Size(1180, 560);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);

      SharedPreferences.setMockInitialValues({'senclaw:app-language': lang});
      final prefs = await SharedPreferences.getInstance();
      final container = ProviderContainer(
          overrides: [prefsProvider.overrideWithValue(prefs)]);
      addTearDown(container.dispose);
      addTearDown(() => L10n.global = const L10n('en'));
      container.read(settingsSectionProvider.notifier).state = 'appearance';

      await tester.pumpWidget(UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          debugShowCheckedModeBanner: false,
          locale: Locale(container.read(localeCodeProvider)),
          supportedLocales: const [Locale('en'), Locale('vi')],
          localizationsDelegates: const [
            L10nDelegate(),
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          theme: AppTheme.light()
              .copyWith(textTheme: AppTheme.light().textTheme.apply(
                    fontFamily: 'Roboto',
                  )),
          home: const Scaffold(body: SettingsScreen()),
        ),
      ));
      await tester.pumpAndSettle();

      await expectLater(find.byType(SettingsScreen),
          matchesGoldenFile('goldens/appearance_$lang.png'));
    });

    // The startup "update available" popup — the first thing a user sees after
    // a release ships, so it is worth being able to look at it.
    testWidgets('update-popup-$lang', (tester) async {
      tester.view.physicalSize = const Size(760, 560);
      tester.view.devicePixelRatio = 1.0;
      addTearDown(tester.view.reset);

      SharedPreferences.setMockInitialValues({'senclaw:app-language': lang});
      final prefs = await SharedPreferences.getInstance();
      final container = ProviderContainer(
          overrides: [prefsProvider.overrideWithValue(prefs)]);
      addTearDown(container.dispose);
      addTearDown(() => L10n.global = const L10n('en'));

      await tester.pumpWidget(UncontrolledProviderScope(
        container: container,
        child: MaterialApp(
          debugShowCheckedModeBanner: false,
          locale: Locale(container.read(localeCodeProvider)),
          supportedLocales: const [Locale('en'), Locale('vi')],
          localizationsDelegates: const [
            L10nDelegate(),
            GlobalMaterialLocalizations.delegate,
            GlobalWidgetsLocalizations.delegate,
            GlobalCupertinoLocalizations.delegate,
          ],
          theme: AppTheme.light()
              .copyWith(textTheme: AppTheme.light().textTheme.apply(
                    fontFamily: 'Roboto',
                  )),
          home: const Scaffold(body: SizedBox.expand()),
        ),
      ));
      await tester.pumpAndSettle();

      unawaited(showUpdateAvailableDialog(
        tester.element(find.byType(SizedBox).first),
        manifest: UpdateManifest.tryParse(jsonEncode({
          'version': '0.5.0',
          'notes': '- Tự động kiểm tra cập nhật ngay khi khởi động\n'
              '- Nhắc lại sau / bỏ qua một phiên bản\n'
              '- Sửa lỗi treo khi thay bundle trên Windows',
          'assets': <String, Object>{},
        }))!,
        currentVersion: '0.4.10',
      ));
      await tester.pumpAndSettle();

      await expectLater(find.byType(AlertDialog),
          matchesGoldenFile('goldens/update_popup_$lang.png'));
    });
  }
}
