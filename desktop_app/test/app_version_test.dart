import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';

/// What CI injects (`--dart-define=SENCLAW_VERSION=${TAG#v}`), read here the
/// same way `app_config.dart` reads it. Both are const-folded at compile time,
/// so this test only means anything when run through the same build define —
/// which is exactly what it is checking.
const _injected = String.fromEnvironment(
  'SENCLAW_VERSION',
  defaultValue: 'dev',
);

void main() {
  test('kAppVersion tracks the SENCLAW_VERSION build define', () {
    expect(kAppVersion, _injected);
  });

  test('kIsDevBuild is true exactly when no release version was injected', () {
    expect(kIsDevBuild, _injected == 'dev');
  });

  // Guards the bug this whole wiring exists to prevent: a hard-coded literal
  // drifting ABOVE the real git tag makes the update check conclude the local
  // build is newer and report "up to date" forever, silently.
  test('a plain `flutter test` (no define) yields dev, not a literal', () {
    if (const bool.hasEnvironment('SENCLAW_VERSION')) return;
    expect(kAppVersion, 'dev');
    expect(kIsDevBuild, isTrue);
  });
}
