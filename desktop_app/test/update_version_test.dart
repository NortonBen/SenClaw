import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/update/version.dart';

Version v(String s) => Version.tryParse(s)!;

void main() {
  group('parse', () {
    test('accepts bare and v-prefixed releases', () {
      expect(v('0.3.0').toString(), '0.3.0');
      expect(v('v0.3.0').toString(), '0.3.0');
      expect(v('1.2.3-beta.1').toString(), '1.2.3-beta.1');
      // Build metadata parses but is ignored for precedence (semver §10).
      expect(v('1.2.3+abc').toString(), '1.2.3');
    });

    test('rejects junk rather than guessing', () {
      for (final s in ['dev', '', 'v', '1.2', '1.2.3.4', 'latest', 'x.y.z', '1.2.-3']) {
        expect(Version.tryParse(s), isNull, reason: 'should reject "$s"');
      }
    });

    // 'dev' parsing as 0.0.0 would make every dev build look ancient and get
    // prompted to overwrite itself with a release.
    test('a dev build has no version, and is not zero', () {
      expect(Version.tryParse('dev'), isNull);
    });
  });

  group('precedence', () {
    // The bug this class exists for: string compare puts 0.10.0 BELOW 0.9.0.
    test('compares numerically, not lexically', () {
      expect(v('0.10.0') > v('0.9.0'), isTrue);
      expect('0.10.0'.compareTo('0.9.0') < 0, isTrue, reason: 'the trap is real');
      expect(v('0.2.0') > v('0.10.0'), isFalse);
      expect(v('1.0.0') > v('0.99.99'), isTrue);
      expect(v('0.3.1') > v('0.3.0'), isTrue);
    });

    test('equal versions are neither newer nor older', () {
      expect(v('0.3.0') > v('0.3.0'), isFalse);
      expect(v('0.3.0') < v('0.3.0'), isFalse);
      expect(v('v0.3.0') == v('0.3.0'), isTrue);
    });

    test('a prerelease ranks below its own stable release', () {
      expect(v('1.0.0') > v('1.0.0-beta.1'), isTrue);
      expect(v('1.0.0-beta.1') > v('0.9.9'), isTrue);
    });

    // Semver §11.4 — the ordering a beta channel depends on.
    test('orders prerelease identifiers per spec', () {
      final ordered = [
        '1.0.0-alpha',
        '1.0.0-alpha.1',
        '1.0.0-alpha.beta',
        '1.0.0-beta',
        '1.0.0-beta.2',
        '1.0.0-beta.11',
        '1.0.0-rc.1',
        '1.0.0',
      ];
      for (var i = 0; i + 1 < ordered.length; i++) {
        expect(v(ordered[i]) < v(ordered[i + 1]), isTrue,
            reason: '${ordered[i]} should sort below ${ordered[i + 1]}');
      }
    });

    test('numeric prerelease fields compare as numbers', () {
      // beta.11 > beta.2 lexically fails ('1' < '2').
      expect(v('1.0.0-beta.11') > v('1.0.0-beta.2'), isTrue);
    });
  });
}
