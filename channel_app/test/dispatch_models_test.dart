import 'package:channel_app/models/dispatch_models.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('DispatchTask', () {
    test('parses camelCase and defaults the optional fields', () {
      final t = DispatchTask.fromJson({
        'id': 'd-1',
        'label': 'write tests',
        'agentId': 'dev',
        'agentJid': 'tg:1:user:2',
        'dependsOn': ['d-0'],
        'status': 'processing',
        'createdAt': '2026-08-21T00:00:00Z',
      });
      expect(t.agentId, 'dev');
      expect(t.dependsOn, ['d-0']);
      expect(t.isVirtual, isFalse);
      expect(t.retryCount, 0);
      expect(t.isTerminal, isFalse);
    });

    test('terminal covers done, error and timeout', () {
      bool terminal(String s) =>
          DispatchTask.fromJson({'id': 'x', 'status': s}).isTerminal;
      expect(terminal('done'), isTrue);
      expect(terminal('error'), isTrue);
      expect(terminal('timeout'), isTrue);
      expect(terminal('registered'), isFalse);
      expect(terminal('processing'), isFalse);
    });

    test('a virtual task shows its persona as the worker', () {
      final t = DispatchTask.fromJson({
        'id': 'd-2',
        'agentId': 'persona:Reviewer',
        'agentJid': '',
        'isVirtual': true,
        'personaName': 'Reviewer',
      });
      expect(t.worker, 'Reviewer');

      final plain = DispatchTask.fromJson({'id': 'd-3', 'agentId': 'dev'});
      expect(plain.worker, 'dev');
    });

    test('checklistAuto is carried — an inferred checklist is advisory only', () {
      final t = DispatchTask.fromJson({
        'id': 'd-4',
        'checklistAuto': true,
        'checklist': [
          {'text': 'tests pass', 'done': false},
        ],
      });
      expect(t.checklistAuto, isTrue);
      expect(t.checklist.single.text, 'tests pass');
      expect(t.checklist.single.done, isFalse);
    });
  });

  group('DispatchParent', () {
    DispatchParent parent(List<String> statuses) => DispatchParent.fromJson({
          'id': 'p-1',
          'goal': 'ship it',
          'status': 'active',
          'tasks': [
            for (var i = 0; i < statuses.length; i++)
              {'id': 'd-$i', 'status': statuses[i]},
          ],
        });

    test('counts done, failed and running separately', () {
      final p = parent(['done', 'done', 'error', 'timeout', 'processing', 'registered']);
      expect(p.doneCount, 2);
      expect(p.failedCount, 2);
      expect(p.runningCount, 1);
      expect(p.tasks.length, 6);
    });

    test('an empty parent counts zero rather than throwing', () {
      final p = DispatchParent.fromJson({'id': 'p-2'});
      expect(p.tasks, isEmpty);
      expect(p.doneCount, 0);
      expect(p.status, 'queued');
    });
  });
}
