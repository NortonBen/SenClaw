import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../models/cowork_models.dart';

final teamsProvider = FutureProvider<List<CoworkTeam>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cowork/teams');
  return (r is List ? r : const [])
      .whereType<Map>()
      .map((m) => CoworkTeam.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Tasks for one team (`/api/cowork/teams/:id/tasks`).
final teamTasksProvider =
    FutureProvider.family<List<CoworkTask>, String>((ref, teamId) async {
  final r =
      await ref.read(apiClientProvider).get('/api/cowork/teams/$teamId/tasks');
  return (r is List ? r : const [])
      .whereType<Map>()
      .map((m) => CoworkTask.fromJson(m.cast<String, dynamic>()))
      .toList();
});

final coworkTemplatesProvider =
    FutureProvider<List<CoworkTemplate>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/cowork/templates');
  return (r is List ? r : const [])
      .whereType<Map>()
      .map((m) => CoworkTemplate.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Spin up a team from a template; returns the new team id (or null).
Future<String?> createTeamFromTemplate(WidgetRef ref, String templateId,
    {String? workspaceDir}) async {
  final r = await ref.read(apiClientProvider).post(
      '/api/cowork/teams/from-template',
      body: {
        'template_id': templateId,
        if (workspaceDir != null && workspaceDir.isNotEmpty)
          'workspace_dir': workspaceDir,
      });
  ref.invalidate(teamsProvider);
  return r is Map ? r['id'] as String? : null;
}

/// The team currently open in the detail pane (null = list view).
final openTeamProvider = StateProvider<String?>((ref) => null);
