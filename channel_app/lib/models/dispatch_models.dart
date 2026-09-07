/// DAG sub-agent dispatch — one `dispatch_task` call fans out into N subtasks
/// with dependencies between them.
///
/// Read-only on mobile. The live `dispatch:update` event reaches WebSocket
/// admin clients only, so this app polls `GET /api/dispatch` while the screen
/// is open — the same shape the event carries in `parents`.
library;

class DispatchTask {
  final String id;
  final String label;

  /// Persisted agents: the folder. Virtual agents: `persona:<name>`.
  final String agentId;

  /// Persisted agents: the jid. Virtual agents: empty.
  final String agentJid;
  final List<String> dependsOn;
  final String prompt;

  /// `registered` | `processing` | `done` | `error` | `timeout`.
  final String status;
  final String? result;
  final String createdAt;
  final String? startedAt;
  final String? completedAt;
  final int timeoutSeconds;
  final bool isVirtual;
  final String? personaName;
  final int retryCount;
  final List<ChecklistItem> checklist;

  /// True when the checklist was inferred from the prompt rather than supplied.
  /// An auto checklist is advisory — a failed check downgrades to a warning
  /// instead of failing the task — so the UI must not present it as a verdict.
  final bool checklistAuto;

  const DispatchTask({
    required this.id,
    this.label = '',
    this.agentId = '',
    this.agentJid = '',
    this.dependsOn = const [],
    this.prompt = '',
    this.status = 'registered',
    this.result,
    this.createdAt = '',
    this.startedAt,
    this.completedAt,
    this.timeoutSeconds = 0,
    this.isVirtual = false,
    this.personaName,
    this.retryCount = 0,
    this.checklist = const [],
    this.checklistAuto = false,
  });

  bool get isTerminal =>
      status == 'done' || status == 'error' || status == 'timeout';

  /// What to show as the worker: the persona for a virtual task, else the
  /// agent folder.
  String get worker => isVirtual ? (personaName ?? agentId) : agentId;

  factory DispatchTask.fromJson(Map<String, dynamic> j) => DispatchTask(
        id: (j['id'] ?? '').toString(),
        label: (j['label'] ?? '').toString(),
        agentId: (j['agentId'] ?? '').toString(),
        agentJid: (j['agentJid'] ?? '').toString(),
        dependsOn: ((j['dependsOn'] as List?) ?? const [])
            .map((e) => e.toString())
            .toList(),
        prompt: (j['prompt'] ?? '').toString(),
        status: (j['status'] ?? 'registered').toString(),
        result: j['result']?.toString(),
        createdAt: (j['createdAt'] ?? '').toString(),
        startedAt: j['startedAt']?.toString(),
        completedAt: j['completedAt']?.toString(),
        timeoutSeconds: (j['timeoutSeconds'] as num?)?.toInt() ?? 0,
        isVirtual: j['isVirtual'] == true,
        personaName: j['personaName']?.toString(),
        retryCount: (j['retryCount'] as num?)?.toInt() ?? 0,
        checklist: ((j['checklist'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => ChecklistItem.fromJson(m.cast<String, dynamic>()))
            .toList(),
        checklistAuto: j['checklistAuto'] == true,
      );
}

class ChecklistItem {
  final String text;
  final bool done;

  const ChecklistItem({required this.text, this.done = false});

  factory ChecklistItem.fromJson(Map<String, dynamic> j) => ChecklistItem(
        text: (j['text'] ?? j['item'] ?? j['description'] ?? '').toString(),
        done: j['done'] == true || j['checked'] == true || j['passed'] == true,
      );
}

/// One `dispatch_task` call and everything it fanned out into.
class DispatchParent {
  final String id;
  final String goal;
  final String adminFolder;
  final String? sharedWorkspace;

  /// `queued` | `active` | `done`.
  final String status;
  final String createdAt;
  final String? completedAt;
  final List<DispatchTask> tasks;

  const DispatchParent({
    required this.id,
    this.goal = '',
    this.adminFolder = '',
    this.sharedWorkspace,
    this.status = 'queued',
    this.createdAt = '',
    this.completedAt,
    this.tasks = const [],
  });

  int get doneCount => tasks.where((t) => t.status == 'done').length;
  int get failedCount =>
      tasks.where((t) => t.status == 'error' || t.status == 'timeout').length;
  int get runningCount => tasks.where((t) => t.status == 'processing').length;

  factory DispatchParent.fromJson(Map<String, dynamic> j) => DispatchParent(
        id: (j['id'] ?? '').toString(),
        goal: (j['goal'] ?? '').toString(),
        adminFolder: (j['adminFolder'] ?? '').toString(),
        sharedWorkspace: j['sharedWorkspace']?.toString(),
        status: (j['status'] ?? 'queued').toString(),
        createdAt: (j['createdAt'] ?? '').toString(),
        completedAt: j['completedAt']?.toString(),
        tasks: ((j['tasks'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => DispatchTask.fromJson(m.cast<String, dynamic>()))
            .toList(),
      );
}
