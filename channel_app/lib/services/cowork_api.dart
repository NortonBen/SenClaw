import '../models/cowork_models.dart';
import 'api_client.dart';
import 'local_cache.dart';

/// Typed wrapper over the rebuilt Cowork (DAG teams) API at `/api/cowork/*`,
/// tunnelled through the relay. Replaces the old workspace-based endpoints,
/// which the daemon no longer serves. List fetches feed the [LocalCache]
/// domain tables for instant cache-first rendering.
class CoworkApi {
  final _api = ApiClient();

  // ── Teams ──────────────────────────────────────────────────────────────
  Future<List<CoworkTeam>> listTeams() async {
    final maps = jsonMaps(await _api.get('/api/cowork/teams'));
    LocalCache().putDomainList('cowork_teams', maps);
    return maps.map(CoworkTeam.fromJson).toList();
  }

  Future<List<CoworkTeam>> listTeamsCached() async =>
      (await LocalCache().getDomainList('cowork_teams'))
          .map(CoworkTeam.fromJson)
          .toList();

  Future<CoworkTeam> createTeam({
    required String name,
    required String managerFolder,
    List<String> members = const [],
    String? workspaceDir,
  }) async {
    final r = await _api.post('/api/cowork/teams', body: {
      'name': name,
      'manager_folder': managerFolder,
      'members': members,
      'workspace_dir': ?workspaceDir,
    });
    return CoworkTeam.fromJson((r as Map).cast<String, dynamic>());
  }

  Future<CoworkTeam> updateTeam(
    String id, {
    String? name,
    String? managerFolder,
    String? workspaceDir,
    CoworkTeamSettings? settings,
  }) async {
    final r = await _api.patch('/api/cowork/teams/$id', body: {
      'name': ?name,
      'manager_folder': ?managerFolder,
      'workspace_dir': ?workspaceDir,
      'settings': ?settings?.toJson(),
    });
    return CoworkTeam.fromJson((r as Map).cast<String, dynamic>());
  }

  Future<void> deleteTeam(String id) => _api.delete('/api/cowork/teams/$id');

  Future<void> saveAsTemplate(String id,
          {String? name, String? description, String? icon}) =>
      _api.post('/api/cowork/teams/$id/save-as-template', body: {
        'name': ?name,
        'description': ?description,
        'icon': ?icon,
      });

  // ── Members ────────────────────────────────────────────────────────────
  Future<CoworkTeam> upsertMember(String id, TeamMember member) async {
    final r = await _api.put('/api/cowork/teams/$id/members', body: member.toJson());
    return CoworkTeam.fromJson((r as Map).cast<String, dynamic>());
  }

  Future<CoworkTeam> removeMember(String id, String folder) async {
    final r = await _api.delete('/api/cowork/teams/$id/members/$folder');
    return CoworkTeam.fromJson((r as Map).cast<String, dynamic>());
  }

  // ── Templates ──────────────────────────────────────────────────────────
  Future<List<CoworkTemplate>> listTemplates() async {
    final maps = jsonMaps(await _api.get('/api/cowork/templates'));
    LocalCache().putDomainList('cowork_templates', maps);
    return maps.map(CoworkTemplate.fromJson).toList();
  }

  Future<List<CoworkTemplate>> listTemplatesCached() async =>
      (await LocalCache().getDomainList('cowork_templates'))
          .map(CoworkTemplate.fromJson)
          .toList();

  Future<CoworkTeam> createFromTemplate(String templateId,
      {String? name, String? workspaceDir}) async {
    final r = await _api.post('/api/cowork/teams/from-template', body: {
      'template_id': templateId,
      'name': ?name,
      'workspace_dir': ?workspaceDir,
    });
    return CoworkTeam.fromJson((r as Map).cast<String, dynamic>());
  }

  Future<void> deleteTemplate(String id) =>
      _api.delete('/api/cowork/templates/$id');

  // ── Personas (member picker) ───────────────────────────────────────────
  Future<List<CoworkPersona>> listPersonas() async {
    final maps = jsonMaps(await _api.get('/api/cowork/personas'));
    LocalCache().putDomainList('cowork_personas', maps);
    return maps.map(CoworkPersona.fromJson).toList();
  }

  Future<List<CoworkPersona>> listPersonasCached() async =>
      (await LocalCache().getDomainList('cowork_personas'))
          .map(CoworkPersona.fromJson)
          .toList();

  // ── Kanban tasks ───────────────────────────────────────────────────────
  Future<List<CoworkTeamTask>> listTasks(String teamId) async {
    final r = await _api.get('/api/cowork/teams/$teamId/tasks');
    final list = r is List ? r : const [];
    return list
        .map((e) => CoworkTeamTask.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<void> createTask(
    String teamId, {
    required String title,
    String? description,
    String? assignee,
    String? priority,
    String? status,
  }) =>
      _api.post('/api/cowork/teams/$teamId/tasks', body: {
        'title': title,
        'description': ?description,
        'assignee': ?assignee,
        'priority': ?priority,
        'status': ?status,
      });

  Future<void> updateTask(
    String teamId,
    String taskId, {
    String? status,
    String? assignee,
    String? priority,
    String? title,
  }) =>
      _api.patch('/api/cowork/teams/$teamId/tasks/$taskId', body: {
        'status': ?status,
        'assignee': ?assignee,
        'priority': ?priority,
        'title': ?title,
      });

  Future<void> deleteTask(String teamId, String taskId) =>
      _api.delete('/api/cowork/teams/$teamId/tasks/$taskId');
}
