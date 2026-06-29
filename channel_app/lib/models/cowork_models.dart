/// Models mirroring the rebuilt Cowork (DAG teams) API at `/api/cowork/*`.
/// NOTE: these structs serialise with snake_case keys (manager_folder,
/// workspace_dir, created_at, handoff_rules, team_id, …) — unlike most other
/// app models. Timestamps are ISO-8601 strings.
library;

class TeamMember {
  final String folder;
  final String? role;
  final String? responsibilities;
  final String? triggers;
  final String? handoffRules;
  final String? acceptanceCriteria;
  final String? outputFormat;
  final String? sla;
  final String? limits;

  const TeamMember({
    required this.folder,
    this.role,
    this.responsibilities,
    this.triggers,
    this.handoffRules,
    this.acceptanceCriteria,
    this.outputFormat,
    this.sla,
    this.limits,
  });

  factory TeamMember.fromJson(Map<String, dynamic> j) => TeamMember(
        folder: (j['folder'] ?? '').toString(),
        role: j['role'] as String?,
        responsibilities: j['responsibilities'] as String?,
        triggers: j['triggers'] as String?,
        handoffRules: j['handoff_rules'] as String?,
        acceptanceCriteria: j['acceptance_criteria'] as String?,
        outputFormat: j['output_format'] as String?,
        sla: j['sla'] as String?,
        limits: j['limits'] as String?,
      );

  Map<String, dynamic> toJson() => {
        'folder': folder,
        'role': ?role,
        'responsibilities': ?responsibilities,
        'triggers': ?triggers,
        'handoff_rules': ?handoffRules,
        'acceptance_criteria': ?acceptanceCriteria,
        'output_format': ?outputFormat,
        'sla': ?sla,
        'limits': ?limits,
      };
}

class CoworkTeamSettings {
  final String? managerPreamble;
  final List<String>? managerTools;
  final bool? autoCreateTasks;

  const CoworkTeamSettings({
    this.managerPreamble,
    this.managerTools,
    this.autoCreateTasks,
  });

  factory CoworkTeamSettings.fromJson(Map<String, dynamic> j) =>
      CoworkTeamSettings(
        managerPreamble: j['manager_preamble'] as String?,
        managerTools:
            (j['manager_tools'] as List?)?.map((e) => e.toString()).toList(),
        autoCreateTasks: j['auto_create_tasks'] as bool?,
      );

  Map<String, dynamic> toJson() => {
        'manager_preamble': ?managerPreamble,
        'manager_tools': ?managerTools,
        'auto_create_tasks': ?autoCreateTasks,
      };
}

class CoworkTeam {
  final String id;
  final String name;
  final String managerFolder;
  final List<TeamMember> members;
  final String? workspaceDir;
  final String createdAt;
  final String jid;
  final CoworkTeamSettings settings;

  const CoworkTeam({
    required this.id,
    required this.name,
    required this.managerFolder,
    this.members = const [],
    this.workspaceDir,
    this.createdAt = '',
    this.jid = '',
    this.settings = const CoworkTeamSettings(),
  });

  factory CoworkTeam.fromJson(Map<String, dynamic> j) => CoworkTeam(
        id: (j['id'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        managerFolder: (j['manager_folder'] ?? '').toString(),
        members: ((j['members'] as List?) ?? const [])
            .map((e) => TeamMember.fromJson(e as Map<String, dynamic>))
            .toList(),
        workspaceDir: j['workspace_dir'] as String?,
        createdAt: (j['created_at'] ?? '').toString(),
        jid: (j['jid'] ?? '').toString(),
        settings: j['settings'] is Map
            ? CoworkTeamSettings.fromJson(
                (j['settings'] as Map).cast<String, dynamic>())
            : const CoworkTeamSettings(),
      );
}

class CoworkTemplate {
  final String id;
  final String name;
  final String description;
  final String icon;
  final String manager;
  final String managerRole;
  final List<TeamMember> members;
  final CoworkTeamSettings settings;
  final bool builtin;

  const CoworkTemplate({
    required this.id,
    required this.name,
    this.description = '',
    this.icon = '🧩',
    this.manager = '',
    this.managerRole = 'lead',
    this.members = const [],
    this.settings = const CoworkTeamSettings(),
    this.builtin = false,
  });

  factory CoworkTemplate.fromJson(Map<String, dynamic> j) => CoworkTemplate(
        id: (j['id'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        description: (j['description'] ?? '').toString(),
        icon: (j['icon'] ?? '🧩').toString(),
        manager: (j['manager'] ?? '').toString(),
        managerRole: (j['manager_role'] ?? 'lead').toString(),
        members: ((j['members'] as List?) ?? const [])
            .map((e) => TeamMember.fromJson(e as Map<String, dynamic>))
            .toList(),
        settings: j['settings'] is Map
            ? CoworkTeamSettings.fromJson(
                (j['settings'] as Map).cast<String, dynamic>())
            : const CoworkTeamSettings(),
        builtin: j['builtin'] as bool? ?? false,
      );
}

class CoworkPersona {
  final String name;
  final String description;
  const CoworkPersona({required this.name, this.description = ''});
  factory CoworkPersona.fromJson(Map<String, dynamic> j) => CoworkPersona(
        name: (j['name'] ?? '').toString(),
        description: (j['description'] ?? '').toString(),
      );
}

class CoworkTeamTask {
  final String id;
  final String teamId;
  final String title;
  final String? description;
  final String status; // backlog|todo|in_progress|review|done|blocked
  final String? assignee;
  final String? reviewer;
  final String priority; // low|medium|high|critical
  final List<String> dependsOn;
  final String? resultOutput;
  final String createdAt;
  final String updatedAt;

  const CoworkTeamTask({
    required this.id,
    required this.teamId,
    required this.title,
    this.description,
    this.status = 'todo',
    this.assignee,
    this.reviewer,
    this.priority = 'medium',
    this.dependsOn = const [],
    this.resultOutput,
    this.createdAt = '',
    this.updatedAt = '',
  });

  factory CoworkTeamTask.fromJson(Map<String, dynamic> j) => CoworkTeamTask(
        id: (j['id'] ?? '').toString(),
        teamId: (j['team_id'] ?? '').toString(),
        title: (j['title'] ?? '').toString(),
        description: j['description'] as String?,
        status: (j['status'] ?? 'todo').toString(),
        assignee: j['assignee'] as String?,
        reviewer: j['reviewer'] as String?,
        priority: (j['priority'] ?? 'medium').toString(),
        dependsOn: ((j['depends_on'] as List?) ?? const [])
            .map((e) => e.toString())
            .toList(),
        resultOutput: j['result_output'] as String?,
        createdAt: (j['created_at'] ?? '').toString(),
        updatedAt: (j['updated_at'] ?? '').toString(),
      );
}
