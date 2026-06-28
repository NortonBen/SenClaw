class CoworkMember {
  final String folder;
  final String role;
  final String? responsibilities;
  final String? triggers;
  final String? handoffRules;
  final String? acceptanceCriteria;
  final String? outputFormat;
  final String? sla;
  final String? limits;

  const CoworkMember({
    required this.folder,
    required this.role,
    this.responsibilities,
    this.triggers,
    this.handoffRules,
    this.acceptanceCriteria,
    this.outputFormat,
    this.sla,
    this.limits,
  });

  factory CoworkMember.fromJson(Map<String, dynamic> j) => CoworkMember(
    folder: '${j['folder'] ?? ''}',
    role: '${j['role'] ?? ''}',
    responsibilities: j['responsibilities'] as String?,
    triggers: j['triggers'] as String?,
    handoffRules: j['handoff_rules'] as String?,
    acceptanceCriteria: j['acceptance_criteria'] as String?,
    outputFormat: j['output_format'] as String?,
    sla: j['sla'] as String?,
    limits: j['limits'] as String?,
  );
}

class CoworkTeam {
  final String id;
  final String name;
  final String managerFolder;
  final List<CoworkMember> members;
  final Map<String, dynamic> settings;

  const CoworkTeam({
    required this.id,
    required this.name,
    required this.managerFolder,
    this.members = const [],
    this.settings = const {},
  });

  factory CoworkTeam.fromJson(Map<String, dynamic> j) => CoworkTeam(
    id: '${j['id'] ?? ''}',
    name: '${j['name'] ?? ''}',
    managerFolder: '${j['manager_folder'] ?? ''}',
    members: ((j['members'] as List?) ?? const [])
        .whereType<Map>()
        .map((m) => CoworkMember.fromJson(m.cast<String, dynamic>()))
        .toList(),
    settings: (j['settings'] as Map?)?.cast<String, dynamic>() ?? const {},
  );
}

class CoworkTask {
  final String id;
  final String title;
  final String? description;
  final String status; // backlog|todo|in_progress|review|done|blocked
  final String? assignee;
  final String priority;
  final String? resultOutput;

  const CoworkTask({
    required this.id,
    required this.title,
    this.description,
    this.status = 'todo',
    this.assignee,
    this.priority = 'medium',
    this.resultOutput,
  });

  factory CoworkTask.fromJson(Map<String, dynamic> j) => CoworkTask(
    id: '${j['id'] ?? ''}',
    title: '${j['title'] ?? ''}',
    description: j['description'] as String?,
    status: '${j['status'] ?? 'todo'}',
    assignee: j['assignee'] as String?,
    priority: '${j['priority'] ?? 'medium'}',
    resultOutput: j['result_output'] as String?,
  );
}

class CoworkTemplate {
  final String id;
  final String name;
  final String description;
  final String icon;
  final String manager;
  final int memberCount;
  final bool builtin;
  final Map<String, dynamic> raw; // full template json, for the editor
  const CoworkTemplate({
    required this.id,
    required this.name,
    required this.description,
    required this.icon,
    this.manager = '',
    required this.memberCount,
    this.builtin = false,
    this.raw = const {},
  });

  factory CoworkTemplate.fromJson(Map<String, dynamic> j) => CoworkTemplate(
    id: '${j['id'] ?? ''}',
    name: '${j['name'] ?? ''}',
    description: '${j['description'] ?? ''}',
    icon: '${j['icon'] ?? '👥'}',
    manager: '${j['manager'] ?? j['manager_folder'] ?? ''}',
    memberCount: ((j['members'] as List?) ?? const []).length,
    builtin: j['builtin'] == true,
    raw: j,
  );
}

/// Canonical Kanban columns (matches the React board).
const kCoworkColumns = [
  ('todo', 'To do'),
  ('in_progress', 'In progress'),
  ('review', 'Review'),
  ('done', 'Done'),
  ('blocked', 'Blocked'),
];
