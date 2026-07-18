import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/update/update_manifest.dart';
import 'package:senclaw_desktop/core/update/update_service.dart';

/// Byte-for-byte shape of what the `Generate update manifest` step in
/// .github/workflows/desktop.yml writes. If this literal and that step drift
/// apart, the app stops seeing updates — so keep them in sync.
const _real = '''
{
  "version": "0.3.0",
  "publishedAt": "2026-07-17T08:06:30Z",
  "notes": "- feat: something\\n- fix: quote's \\"danger\\" \\\\ backslash",
  "minVersion": "0.0.0",
  "assets": {
    "aarch64-apple-darwin": {
      "name": "SenClaw-aarch64-apple-darwin.app.zip",
      "size": 84213760,
      "sha256": "5c655d67304116cb4a55f3936fe1f5cb33cf752f4e71ca46c1e07dd9161590eb"
    },
    "x86_64-pc-windows-msvc": {
      "name": "SenClaw-x86_64-pc-windows-msvc.zip",
      "size": 61234567,
      "sha256": "d2e930b19d7f6fab5e9492e905e3b14e1003eddcb7aeb24e2f113667f5f06a4a"
    }
  }
}
''';

void main() {
  group('parse', () {
    test('reads the manifest the release job actually produces', () {
      final m = UpdateManifest.tryParse(_real)!;
      expect(m.version.toString(), '0.3.0');
      expect(m.publishedAt, DateTime.utc(2026, 7, 17, 8, 6, 30));
      expect(m.minVersion.toString(), '0.0.0');
      expect(m.notes, contains('feat: something'));
      expect(m.notes, contains(r'quote'), reason: 'jq-escaped text must survive');

      final a = m.assetFor('aarch64-apple-darwin')!;
      expect(a.name, 'SenClaw-aarch64-apple-darwin.app.zip');
      expect(a.size, 84213760);
      expect(a.sha256.length, 64);
      expect(m.assetFor('x86_64-unknown-linux-gnu'), isNull,
          reason: 'a target absent from the manifest must not be invented');
    });

    // A background check nobody asked for must go quiet, not throw.
    test('returns null for junk instead of throwing', () {
      for (final s in ['', 'not json', '[]', '{}', '{"version": 3}', '{"version": "dev"}']) {
        expect(UpdateManifest.tryParse(s), isNull, reason: 'should reject: $s');
      }
    });

    test('tolerates unknown fields and missing optionals', () {
      final m = UpdateManifest.tryParse('''
        {"version":"9.9.9","futureField":{"x":1},"assets":{}}
      ''')!;
      expect(m.version.toString(), '9.9.9');
      expect(m.notes, isNull);
      expect(m.publishedAt, isNull);
      expect(m.minVersion, isNull);
      expect(m.assets, isEmpty);
    });

    test('drops malformed asset entries but keeps the good ones', () {
      final m = UpdateManifest.tryParse('''
        {"version":"1.0.0","assets":{
          "good":{"name":"a.zip","size":1,"sha256":"ff"},
          "noName":{"size":2},
          "notAMap":"nope"
        }}
      ''')!;
      expect(m.assets.keys, ['good']);
    });

    test('an asset without a checksum still parses (sha becomes empty)', () {
      final m = UpdateManifest.tryParse('''
        {"version":"1.0.0","assets":{"t":{"name":"a.zip"}}}
      ''')!;
      expect(m.assetFor('t')!.sha256, isEmpty);
      expect(m.assetFor('t')!.size, 0);
    });
  });

  group('bundlePathFrom', () {
    // Off-by-one here points the updater at the PARENT of the bundle — i.e. at
    // /Applications itself.
    test('macOS resolves the .app, not its parent', () {
      expect(
        bundlePathFrom(
          '/Applications/SenClaw Desktop.app/Contents/MacOS/senclaw_desktop',
          isMacOS: true,
        ),
        '/Applications/SenClaw Desktop.app',
      );
    });

    test('macOS handles an app installed outside /Applications', () {
      expect(
        bundlePathFrom(
          '/Users/me/Downloads/SenClaw Desktop.app/Contents/MacOS/senclaw_desktop',
          isMacOS: true,
        ),
        '/Users/me/Downloads/SenClaw Desktop.app',
      );
    });

    test('non-macOS uses the directory holding the executable', () {
      expect(
        bundlePathFrom('/opt/senclaw/desktop/senclaw_desktop', isMacOS: false),
        '/opt/senclaw/desktop',
      );
    });
  });

  group('assetUrl', () {
    // latest/download avoids api.github.com's 60/hour/IP rate limit and
    // resolves to the newest non-prerelease.
    test('points at the latest release asset', () {
      expect(
        assetUrl('SenClaw-aarch64-apple-darwin.app.zip'),
        'https://github.com/NortonBen/SenClaw/releases/latest/download/SenClaw-aarch64-apple-darwin.app.zip',
      );
    });
  });
}
