import 'package:channel_app/models/workbench_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('WorkbenchFile', () {
    test('infers the extension from the path when the daemon omits it', () {
      expect(WorkbenchFile.fromJson({'path': 'a/b/report.MD'}).extension, 'md');
      expect(WorkbenchFile.fromJson({'path': 'index.html'}).extension, 'html');
      expect(WorkbenchFile.fromJson({'path': 'Makefile'}).extension, '');
    });

    test('an explicit extension wins over the path', () {
      expect(
        WorkbenchFile.fromJson({'path': 'page.txt', 'extension': 'html'}).extension,
        'html',
      );
    });
  });

  group('WorkbenchArtifact', () {
    test('prefers index.html as the entry file', () {
      final a = WorkbenchArtifact.fromJson({
        'id': 'a1',
        'mode': 'static',
        'files': [
          {'path': 'notes.md'},
          {'path': 'site/index.html'},
        ],
      });
      expect(a.primaryFile!.path, 'site/index.html');
    });

    test('falls back to the first renderable file, then to the first file', () {
      final md = WorkbenchArtifact.fromJson({
        'id': 'a2',
        'files': [
          {'path': 'data.json'},
          {'path': 'readme.md'},
        ],
      });
      expect(md.primaryFile!.path, 'readme.md');

      final none = WorkbenchArtifact.fromJson({
        'id': 'a3',
        'files': [
          {'path': 'data.json'},
        ],
      });
      expect(none.primaryFile!.path, 'data.json');

      expect(WorkbenchArtifact.fromJson({'id': 'a4'}).primaryFile, isNull);
    });

    test('survives a round trip through the cache encoding', () {
      final original = WorkbenchArtifact.fromJson({
        'id': 'a5',
        'title': 'Dashboard',
        'mode': 'backend',
        'url': 'http://127.0.0.1:4900',
        'process': {'status': 'ready', 'logPath': '/tmp/a5.log'},
        'createdAt': 1755000000000,
        'files': [
          {'path': 'main.py', 'content': 'print(1)'},
        ],
      });
      final restored = WorkbenchArtifact.fromJson(original.toJson());
      expect(restored.id, 'a5');
      expect(restored.title, 'Dashboard');
      expect(restored.mode, 'backend');
      expect(restored.url, 'http://127.0.0.1:4900');
      expect(restored.process!.status, 'ready');
      expect(restored.process!.logPath, '/tmp/a5.log');
      expect(restored.createdAt, 1755000000000);
      expect(restored.files.single.content, 'print(1)');
    });

    test('withProcessStatus replaces only the status', () {
      final a = WorkbenchArtifact.fromJson({
        'id': 'a6',
        'process': {'status': 'starting', 'logPath': '/tmp/a6.log'},
      });
      final crashed = a.withProcessStatus('crashed');
      expect(crashed.process!.status, 'crashed');
      expect(crashed.process!.logPath, '/tmp/a6.log');
      expect(crashed.id, a.id);
    });

    test('an artifact with no process stays without one', () {
      final a = WorkbenchArtifact.fromJson({'id': 'a7'});
      expect(a.withProcessStatus('ready').process, isNull);
    });
  });
}
