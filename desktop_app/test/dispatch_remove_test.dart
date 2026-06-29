// Verifies the Console dispatch remove (✕) writes into state AND persists —
// a removed parent/task must not be resurrected by a later `dispatch:update`
// re-push from the server (the `_hiddenParents` / `_hiddenTasks` mechanism).

import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/ws_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/dock/dispatch_provider.dart';

/// A WsClient whose event stream we control — no real socket.
class FakeWs extends WsClient {
  FakeWs() : super(const AppConfig(host: 'x', uiPort: 0, wsPort: 0));
  final _ec = StreamController<WsEvent>.broadcast();
  @override
  Stream<WsEvent> get events => _ec.stream;
  @override
  Stream<WsStatus> get statusStream => Stream<WsStatus>.empty();
  @override
  void send(Map<String, dynamic> msg) {}
  @override
  void dispose() => _ec.close();
  void emit(WsEvent e) => _ec.add(e);
}

Map<String, dynamic> _parent(String id, String status, List tasks) =>
    {'id': id, 'goal': 'goal-$id', 'status': status, 'tasks': tasks};
Map<String, dynamic> _task(String id, String status) =>
    {'id': id, 'label': id, 'agentId': 'a', 'status': status};

void main() {
  late FakeWs ws;
  late ProviderContainer c;

  setUp(() {
    ws = FakeWs();
    c = ProviderContainer(
        overrides: [wsClientProvider.overrideWithValue(ws)]);
  });
  tearDown(() => c.dispose());

  Future<void> pump() => Future<void>.delayed(Duration.zero);

  test('removeParent updates state and survives a dispatch:update re-push',
      () async {
    final n = c.read(dispatchProvider.notifier);

    ws.emit({
      'type': 'dispatch:update',
      'parents': [
        _parent('p1', 'active', [_task('t1', 'done'), _task('t2', 'processing')]),
        _parent('p2', 'done', [_task('t3', 'done')]),
      ],
    });
    await pump();
    expect(c.read(dispatchProvider).parents.length, 2);

    // Click ✕ on p1.
    n.removeParent('p1');
    expect(c.read(dispatchProvider).parents.map((p) => p.id).toList(), ['p2']);

    // Server re-pushes the same parents — p1 must NOT come back.
    ws.emit({
      'type': 'dispatch:update',
      'parents': [
        _parent('p1', 'active', [_task('t1', 'done')]),
        _parent('p2', 'done', [_task('t3', 'done')]),
      ],
    });
    await pump();
    expect(c.read(dispatchProvider).parents.map((p) => p.id).toList(), ['p2'],
        reason: 'removed parent must not be resurrected by a re-push');
  });

  test('removeTask hides one task and survives a re-push', () async {
    final n = c.read(dispatchProvider.notifier);

    ws.emit({
      'type': 'dispatch:update',
      'parents': [
        _parent('p1', 'active', [_task('t1', 'done'), _task('t2', 'processing')]),
      ],
    });
    await pump();
    expect(c.read(dispatchProvider).parents.first.tasks.length, 2);

    n.removeTask('p1', 't1');
    expect(
        c.read(dispatchProvider).parents.first.tasks.map((t) => t.id).toList(),
        ['t2']);

    ws.emit({
      'type': 'dispatch:update',
      'parents': [
        _parent('p1', 'active', [_task('t1', 'done'), _task('t2', 'processing')]),
      ],
    });
    await pump();
    expect(
        c.read(dispatchProvider).parents.first.tasks.map((t) => t.id).toList(),
        ['t2'],
        reason: 'removed task must not be resurrected by a re-push');
  });
}
