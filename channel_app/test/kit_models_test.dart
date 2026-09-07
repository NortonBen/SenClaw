import 'package:channel_app/models/kit_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('AvailableKit', () {
    test('is not installed when installedVersion is absent', () {
      final k = AvailableKit.fromJson({
        'sourceId': 'builtin',
        'name': 'Fabric Patterns',
        'version': '1.4.470',
      });
      expect(k.installed, isFalse);
      expect(k.hasUpdate, isFalse);
      expect(k.installable, isTrue);
    });

    test('reports an update only when both versions are known and differ', () {
      AvailableKit at(String? installed, String offered) =>
          AvailableKit.fromJson({
            'sourceId': 'builtin',
            'name': 'k',
            'version': offered,
            'installedVersion': installed,
          });
      expect(at('1.0.0', '1.1.0').hasUpdate, isTrue);
      expect(at('1.1.0', '1.1.0').hasUpdate, isFalse);
      // An unknown offered version must not read as "update available".
      expect(at('1.0.0', '').hasUpdate, isFalse);
      expect(at(null, '1.1.0').hasUpdate, isFalse);
    });

    test('installable is false only when the daemon says so', () {
      expect(
        AvailableKit.fromJson({'sourceId': 's', 'name': 'k', 'installable': false})
            .installable,
        isFalse,
      );
    });
  });

  group('KitParam', () {
    test('reads `type` and falls back label → key', () {
      final p = KitParam.fromJson({'key': 'apiKey', 'type': 'string', 'secret': true});
      expect(p.type, 'string');
      expect(p.title, 'apiKey');
      expect(p.secret, isTrue);
    });

    test('select options fall back to value when unlabelled', () {
      final p = KitParam.fromJson({
        'key': 'mode',
        'type': 'select',
        'options': [
          {'value': 'a', 'label': 'Alpha'},
          {'value': 'b'},
        ],
      });
      expect(p.options.map((e) => e.key).toList(), ['a', 'b']);
      expect(p.options.map((e) => e.value).toList(), ['Alpha', 'b']);
    });
  });

  group('KitReport', () {
    test('reads the item kind from `type` — the Rust field is renamed', () {
      final r = KitReport.fromJson({
        'ok': true,
        'report': {
          'items': [
            {'type': 'skill', 'name': 'a', 'status': 'created'},
            {'type': 'job', 'name': 'b', 'status': 'failed', 'detail': 'boom'},
          ],
        },
      });
      expect(r.items.first.kind, 'skill');
      expect(r.failed, 1);
      expect(r.changed, 1);
      expect(r.items.last.detail, 'boom');
    });

    test('counts `removed` as changed so uninstall summaries are right', () {
      final r = KitReport.fromJson({
        'ok': true,
        'report': {
          'items': [
            {'type': 'skill', 'name': 'a', 'status': 'removed'},
            {'type': 'skill', 'name': 'b', 'status': 'missing'},
          ],
        },
      });
      expect(r.changed, 1);
      expect(r.failed, 0);
    });

    test('flattens a warning into subject: detail', () {
      final r = KitReport.fromJson({
        'ok': true,
        'report': {
          'items': <dynamic>[],
          'warnings': [
            {'kind': 'unsupported', 'subject': 'mcpServers', 'detail': 'not installed'},
          ],
        },
      });
      expect(r.warnings.single, 'mcpServers: not installed');
    });
  });

  test('KitPreview surfaces paramError and the installed version', () {
    final p = KitPreview.fromJson({
      'id': 'fabric',
      'name': 'Fabric Patterns',
      'version': '1.4.470',
      'params': [
        {'key': 'dir', 'type': 'folder', 'required': true},
      ],
      'paramError': 'missing required param "dir"',
      'items': [
        {'type': 'job', 'name': 'nightly', 'cron': '0 3 * * *', 'enabled': false},
      ],
      'installed': {'version': '1.4.0'},
    });
    expect(p.paramError, contains('dir'));
    expect(p.installedVersion, '1.4.0');
    expect(p.items.single.cron, '0 3 * * *');
    // enabled:false means the job installs paused — the UI says so.
    expect(p.items.single.enabled, isFalse);
  });
}
