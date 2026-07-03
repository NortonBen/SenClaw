// Workflow models — mirror the daemon's `/api/workflows*` payloads
// (src/workflow/types.rs + service.rs, camelCase wire format).

class WorkflowInputDef {
  final String name;
  final bool required;
  final String? defaultValue;
  final String? description;
  const WorkflowInputDef(
      this.name, this.required, this.defaultValue, this.description);

  factory WorkflowInputDef.fromJson(Map<String, dynamic> j) => WorkflowInputDef(
        '${j['name'] ?? ''}',
        j['required'] == true,
        j['default'] as String?,
        j['description'] as String?,
      );
}

class WorkflowStepDef {
  final String id;
  final String kind; // agent | script
  final List<String> dependsOn;
  final String? persona;
  final String? guidance;
  final int? timeout;
  const WorkflowStepDef(this.id, this.kind, this.dependsOn, this.persona,
      this.guidance, this.timeout);

  factory WorkflowStepDef.fromJson(Map<String, dynamic> j) => WorkflowStepDef(
        '${j['id'] ?? ''}',
        '${j['kind'] ?? 'script'}',
        ((j['dependsOn'] as List?) ?? const []).map((e) => '$e').toList(),
        j['persona'] as String?,
        j['guidance'] as String?,
        (j['timeout'] as num?)?.toInt(),
      );
}

class WorkflowDefSummary {
  final String name;
  final String? description;
  final int stepCount;
  final List<WorkflowInputDef> inputs;
  final String? guidance;
  final String? workspace;
  final List<WorkflowStepDef> steps;
  const WorkflowDefSummary(this.name, this.description, this.stepCount,
      this.inputs, this.guidance, this.workspace, this.steps);

  factory WorkflowDefSummary.fromJson(Map<String, dynamic> j) =>
      WorkflowDefSummary(
        '${j['name'] ?? ''}',
        j['description'] as String?,
        (j['stepCount'] as num?)?.toInt() ?? 0,
        ((j['inputs'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => WorkflowInputDef.fromJson(m.cast<String, dynamic>()))
            .toList(),
        j['guidance'] as String?,
        j['workspace'] as String?,
        ((j['steps'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => WorkflowStepDef.fromJson(m.cast<String, dynamic>()))
            .toList(),
      );
}

class WorkflowStepRun {
  final String id;
  final String kind;
  final String status; // pending | running | done | failed | skipped
  final String result;
  final String? error;
  final String? observeLabel;
  final String? observeContent;
  final String? observeArtifactPath;
  const WorkflowStepRun(this.id, this.kind, this.status, this.result,
      this.error, this.observeLabel, this.observeContent,
      this.observeArtifactPath);

  factory WorkflowStepRun.fromJson(Map<String, dynamic> j) {
    final obs = j['observe'] as Map?;
    return WorkflowStepRun(
      '${j['id'] ?? ''}',
      '${j['kind'] ?? 'script'}',
      '${j['status'] ?? 'pending'}',
      '${j['result'] ?? ''}',
      j['error'] as String?,
      obs?['label'] as String?,
      obs?['content'] as String?,
      obs?['artifactPath'] as String?,
    );
  }
}

class WorkflowRun {
  final String id;
  final String workflowName;
  /// Optional user-given display name (rename). Falls back to [id].
  final String? label;
  final Map<String, String> inputs;
  final String status; // running | done | partial-failed | cancelled | interrupted
  final String runDir;
  final List<WorkflowStepRun> steps;
  final String? trigger;
  final String createdAt;
  final String? completedAt;
  const WorkflowRun(this.id, this.workflowName, this.label, this.inputs,
      this.status, this.runDir, this.steps, this.trigger, this.createdAt,
      this.completedAt);

  factory WorkflowRun.fromJson(Map<String, dynamic> j) => WorkflowRun(
        '${j['id'] ?? ''}',
        '${j['workflowName'] ?? ''}',
        j['label'] as String?,
        ((j['inputs'] as Map?) ?? const {})
            .map((k, v) => MapEntry('$k', '$v')),
        '${j['status'] ?? 'running'}',
        '${j['runDir'] ?? ''}',
        ((j['steps'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => WorkflowStepRun.fromJson(m.cast<String, dynamic>()))
            .toList(),
        j['trigger'] as String?,
        '${j['createdAt'] ?? ''}',
        j['completedAt'] as String?,
      );

  bool get isActive => status == 'running';

  /// Display name: user label when set, else the run id.
  String get title => (label ?? '').trim().isNotEmpty ? label!.trim() : id;

  /// One markdown document for the whole run (download / wiki).
  String toMarkdown() {
    final b = StringBuffer()
      ..writeln('# $title')
      ..writeln()
      ..writeln('- Workflow: `$workflowName`')
      ..writeln('- Run: `$id` — **$status**')
      ..writeln(
          '- Started: $createdAt${completedAt != null ? ' · Finished: $completedAt' : ''}');
    if (inputs.isNotEmpty) {
      b.writeln(
          '- Inputs: ${inputs.entries.map((e) => '`${e.key}=${e.value}`').join(', ')}');
    }
    for (final s in steps) {
      b
        ..writeln()
        ..writeln('## ${s.id} (${s.kind}) — ${s.status}')
        ..writeln();
      if (s.error != null) b.writeln('> ⚠️ ${s.error}\n');
      if ((s.observeContent ?? '').isNotEmpty) {
        b
          ..writeln('### ${s.observeLabel ?? 'observe'}')
          ..writeln()
          ..writeln(s.observeContent)
          ..writeln();
      }
      if (s.result.isNotEmpty) b.writeln(s.result);
    }
    return b.toString();
  }
}
