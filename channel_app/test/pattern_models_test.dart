import 'package:channel_app/models/pattern_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('PatternEntry', () {
    test('parses a row and defaults the optional shadow list', () {
      final e = PatternEntry.fromJson({
        'name': 'summarize',
        'source': 'fabric',
        'description': 'Summarise a transcript.',
        'writable': false,
      });
      expect(e.name, 'summarize');
      expect(e.source, 'fabric');
      expect(e.shadowedIn, isEmpty);
      expect(e.writable, isFalse);
    });

    test('keeps shadowedIn — it is why an edit appears to do nothing', () {
      final e = PatternEntry.fromJson({
        'name': 'summarize',
        'source': 'user',
        'shadowedIn': ['fabric', 'kit-x'],
      });
      expect(e.shadowedIn, ['fabric', 'kit-x']);
    });
  });

  group('PatternSource', () {
    test('accepts the daemon camelCase and the `ref` alias', () {
      final a = PatternSource.fromJson({'id': 'x', 'kind': 'git', 'gitRef': 'v1.4.470'});
      final b = PatternSource.fromJson({'id': 'x', 'kind': 'git', 'ref': 'v1.4.470'});
      expect(a.gitRef, 'v1.4.470');
      expect(b.gitRef, 'v1.4.470');
    });

    test('defaults enabled to true when the field is absent', () {
      expect(PatternSource.fromJson({'id': 'x', 'kind': 'local'}).enabled, isTrue);
      expect(
        PatternSource.fromJson({'id': 'x', 'kind': 'local', 'enabled': false}).enabled,
        isFalse,
      );
    });

    test('a tag or sha is pinned, a branch is not', () {
      PatternSource git(String ref) =>
          PatternSource.fromJson({'id': 'x', 'kind': 'git', 'gitRef': ref});
      expect(git('v1.4.470').pinned, isTrue);
      expect(git('a1b2c3d4e5f6789').pinned, isTrue);
      expect(git('main').pinned, isFalse);
      expect(git('master').pinned, isFalse);
      expect(git('').pinned, isFalse);
    });

    test('a local source is always pinned — nothing upstream can rewrite it', () {
      expect(
        PatternSource.fromJson({'id': 'user', 'kind': 'local', 'gitRef': 'main'}).pinned,
        isTrue,
      );
    });
  });

  group('PatternRunResult', () {
    test('parses a real run', () {
      final r = PatternRunResult.fromJson({
        'ok': true,
        'pattern': 'summarize',
        'source': 'fabric',
        'text': '# Summary',
        'model': 'claude-opus-5',
        'finish': 'stop',
        'latencyMs': 1234,
        'unresolved': ['author'],
      });
      expect(r.text, '# Summary');
      expect(r.latencyMs, 1234);
      expect(r.unresolved, ['author']);
      expect(r.dryRun, isFalse);
    });

    test('a dry run carries the rendered prompt instead of text', () {
      final r = PatternRunResult.fromJson({
        'ok': true,
        'dryRun': true,
        'pattern': 'summarize',
        'rendered': {'system': 'SYS', 'user': 'USR', 'unresolved': <String>[]},
      });
      expect(r.dryRun, isTrue);
      expect(r.renderedSystem, 'SYS');
      expect(r.renderedUser, 'USR');
      expect(r.text, isEmpty);
    });
  });

  test('PatternCatalogEntry marks the bundled offer as installable offline', () {
    final e = PatternCatalogEntry.fromJson({
      'id': 'starter',
      'name': 'Thư viện đi kèm',
      'kind': 'bundled',
      'count': 261,
      'license': 'MIT',
      'installed': false,
      'pinned': true,
    });
    expect(e.kind, 'bundled');
    expect(e.count, 261);
    expect(e.url, isNull);
    expect(e.pinned, isTrue);
  });
}
