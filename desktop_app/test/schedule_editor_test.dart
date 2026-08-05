import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'package:senclaw_desktop/widgets/schedule_editor.dart';
import 'package:senclaw_desktop/models/space_models.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/features/chat/agents_provider.dart';
import 'package:senclaw_desktop/features/chat/new_chat_dialog.dart' show llmConfigsProvider, LlmConfig, LlmConfigData;

/// Records the REST calls the dialog makes instead of hitting the network.
class _RecordApi extends ApiClient {
  _RecordApi() : super(const AppConfig(host: '127.0.0.1', uiPort: 1, wsPort: 2));
  String? lastMethod;
  String? lastPath;
  Object? lastBody;
  @override
  Future<dynamic> patch(String path, {Object? body}) async {
    lastMethod = 'PATCH';
    lastPath = path;
    lastBody = body;
    return <String, dynamic>{'id': 'sched-1'};
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    lastMethod = 'POST';
    lastPath = path;
    lastBody = body;
    return <String, dynamic>{'id': 'sched-new'};
  }
}

/// Seeds the agents list without touching the WebSocket.
class _SeededAgents extends AgentsNotifier {
  _SeededAgents(super.ref, List<AgentInfo> seed) {
    state = seed;
  }
}

AgentInfo _agent(int id, String folder, String name) =>
    AgentInfo(id: id, folder: folder, name: name);

void main() {
  final agents = [
    _agent(1, 'main', 'main (web)'),
    _agent(5, 'research-assistant', 'research-assistant'),
    _agent(16, 'ssh', 'SSH'),
  ];

  final existing = SpaceSchedule(
    id: 'sched-1',
    label: 'Kết nối ssh',
    prompt: 'Kết nối ssh và report thông tin hệ thống',
    agentMode: 'agent',
    agentFolder: null, // "Default"
    modelId: null,
    scheduleType: 'cron',
    scheduleValue: '10 9 * * *', // daily 09:10
    status: 'active',
  );

  // Ignore the cosmetic "RenderFlex overflowed" that the 460px dialog produces
  // in the test viewport — it is not the behaviour under test. Rethrow anything
  // else so a real assertion/exception still fails the test.
  Object? nonOverflow(WidgetTester tester) {
    final e = tester.takeException();
    if (e == null) return null;
    if (e.toString().contains('overflowed')) return null;
    return e;
  }

  Future<_RecordApi> pump(WidgetTester tester, {SpaceSchedule? sched}) async {
    tester.view.physicalSize = const Size(1400, 1600);
    tester.view.devicePixelRatio = 1.0;
    addTearDown(tester.view.reset);
    final api = _RecordApi();
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          apiClientProvider.overrideWithValue(api),
          agentsProvider.overrideWith((ref) => _SeededAgents(ref, agents)),
          llmConfigsProvider.overrideWith((ref) async => const LlmConfigData(
                configs: [LlmConfig('gpt', 'GPT-4o'), LlmConfig('claude', 'Claude')],
              )),
        ],
        child: MaterialApp(
          theme: AppTheme.dark(),
          home: Scaffold(body: Center(child: ScheduleEditorDialog(existing: sched))),
        ),
      ),
    );
    await tester.pumpAndSettle();
    return api;
  }

  // Profile is the 3rd DropdownButtonFormField<String> (Frequency, Agent mode, Profile).
  Finder profileDropdown() => find.byType(DropdownButtonFormField<String>).at(2);

  testWidgets('renders an existing schedule without any exception (no overflow)',
      (tester) async {
    await pump(tester, sched: existing);
    // Strict: after the isExpanded fix the Agent-mode/Profile row must not
    // overflow, so there should be no exception at all.
    expect(tester.takeException(), isNull);
    expect(find.text('Profile (agent)'), findsOneWidget);
    expect(find.text('Save'), findsOneWidget);
  });

  testWidgets('Profile dropdown lists the loaded agent profiles', (tester) async {
    await pump(tester, sched: existing);
    await tester.tap(profileDropdown());
    await tester.pumpAndSettle();
    expect(nonOverflow(tester), isNull);
    // The seeded profiles must appear as menu entries.
    expect(find.text('SSH'), findsWidgets);
    expect(find.text('research-assistant'), findsWidgets);
  });

  testWidgets('picking a profile then Save PATCHes with agent_folder', (tester) async {
    final api = await pump(tester, sched: existing);
    await tester.tap(profileDropdown());
    await tester.pumpAndSettle();
    await tester.tap(find.text('SSH').last);
    await tester.pumpAndSettle();
    expect(nonOverflow(tester), isNull);

    await tester.tap(find.text('Save'));
    await tester.pumpAndSettle();

    expect(nonOverflow(tester), isNull);
    expect(api.lastMethod, 'PATCH');
    expect(api.lastPath, '/api/space/schedules/sched-1');
    final body = api.lastBody as Map<String, dynamic>;
    expect(body['agent_folder'], 'ssh');
    expect(body['frequency'], 'daily');
    expect(body['time_local'], '09:10');
  });
}
