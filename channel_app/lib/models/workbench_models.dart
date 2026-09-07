/// Workbench artifacts — what an agent built during a turn: a rendered page, a
/// set of files, or a running local service.
///
/// **There is no list endpoint.** The daemon has `mark-viewed`, `close`,
/// `read-file` and `logs`, and nothing that enumerates artifacts — they only
/// ever arrive as `workbench:new` events. The web UI mirrors its state into
/// localStorage for exactly that reason, and [WorkbenchStore] does the same
/// here. An artifact this device never received an event for is not visible on
/// this device.
library;

class WorkbenchFile {
  final String path;
  final String? content;
  final String? mimeType;

  /// Resolved extension. Inferred from [path] when the daemon omits it.
  final String extension;

  const WorkbenchFile({
    required this.path,
    this.content,
    this.mimeType,
    this.extension = '',
  });

  factory WorkbenchFile.fromJson(Map<String, dynamic> j) {
    final path = (j['path'] ?? '').toString();
    var ext = (j['extension'] ?? '').toString();
    if (ext.isEmpty) {
      final dot = path.lastIndexOf('.');
      if (dot >= 0 && dot < path.length - 1) ext = path.substring(dot + 1);
    }
    return WorkbenchFile(
      path: path,
      content: j['content']?.toString(),
      mimeType: j['mimeType']?.toString(),
      extension: ext.toLowerCase(),
    );
  }

  Map<String, dynamic> toJson() => {
        'path': path,
        if (content != null) 'content': content,
        if (mimeType != null) 'mimeType': mimeType,
        'extension': extension,
      };
}

class WorkbenchProcess {
  /// `starting` | `ready` | `crashed` | `stopped`.
  final String status;
  final String? logPath;

  const WorkbenchProcess({this.status = 'starting', this.logPath});

  factory WorkbenchProcess.fromJson(Map<String, dynamic> j) => WorkbenchProcess(
        status: (j['status'] ?? 'starting').toString(),
        logPath: j['logPath']?.toString(),
      );

  Map<String, dynamic> toJson() => {
        'status': status,
        if (logPath != null) 'logPath': logPath,
      };

  WorkbenchProcess withStatus(String s) =>
      WorkbenchProcess(status: s, logPath: logPath);
}

class WorkbenchArtifact {
  final String id;
  final String title;

  /// `static` (files only) | `web` (has a URL) | `backend` (a running process).
  final String mode;
  final List<WorkbenchFile> files;
  final String? url;
  final WorkbenchProcess? process;
  final String? usage;
  final int createdAt;

  const WorkbenchArtifact({
    required this.id,
    this.title = '',
    this.mode = 'static',
    this.files = const [],
    this.url,
    this.process,
    this.usage,
    this.createdAt = 0,
  });

  /// The file a viewer should open first: an `index.html`, else the first
  /// html/markdown, else whatever came first.
  WorkbenchFile? get primaryFile {
    if (files.isEmpty) return null;
    for (final f in files) {
      if (f.path.endsWith('index.html')) return f;
    }
    for (final f in files) {
      if (f.extension == 'html' || f.extension == 'md') return f;
    }
    return files.first;
  }

  factory WorkbenchArtifact.fromJson(Map<String, dynamic> j) =>
      WorkbenchArtifact(
        id: (j['id'] ?? '').toString(),
        title: (j['title'] ?? '').toString(),
        mode: (j['mode'] ?? 'static').toString(),
        files: ((j['files'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => WorkbenchFile.fromJson(m.cast<String, dynamic>()))
            .toList(),
        url: j['url']?.toString(),
        process: j['process'] is Map
            ? WorkbenchProcess.fromJson(
                (j['process'] as Map).cast<String, dynamic>())
            : null,
        usage: j['usage']?.toString(),
        createdAt: (j['createdAt'] as num?)?.toInt() ?? 0,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'title': title,
        'mode': mode,
        'files': files.map((f) => f.toJson()).toList(),
        if (url != null) 'url': url,
        if (process != null) 'process': process!.toJson(),
        if (usage != null) 'usage': usage,
        'createdAt': createdAt,
      };

  WorkbenchArtifact withProcessStatus(String status) => WorkbenchArtifact(
        id: id,
        title: title,
        mode: mode,
        files: files,
        url: url,
        process: process?.withStatus(status),
        usage: usage,
        createdAt: createdAt,
      );
}
