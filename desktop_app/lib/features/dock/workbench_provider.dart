import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';

class WorkbenchArtifact {
  final String id;
  final String title;
  final String mode; // static | web | backend
  final List<String> files;
  final String? url;
  const WorkbenchArtifact({
    required this.id,
    required this.title,
    required this.mode,
    required this.files,
    this.url,
  });

  factory WorkbenchArtifact.fromJson(Map<String, dynamic> j) =>
      WorkbenchArtifact(
        id: '${j['id'] ?? ''}',
        title: '${j['title'] ?? 'Artifact'}',
        mode: '${j['mode'] ?? 'static'}',
        files: ((j['files'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => '${m['path'] ?? ''}')
            .where((p) => p.isNotEmpty)
            .toList(),
        url: j['url'] as String?,
      );
}

/// Holds the current workbench artifact per groupJid (`workbench:new`).
class WorkbenchNotifier extends StateNotifier<Map<String, WorkbenchArtifact>> {
  WorkbenchNotifier(this._ref) : super(const {}) {
    _sub = _ref.read(wsClientProvider).events.listen((e) {
      if (e['type'] == 'workbench:new' && e['artifact'] is Map) {
        final jid = '${e['groupJid'] ?? '_'}';
        final art = WorkbenchArtifact.fromJson(
            (e['artifact'] as Map).cast<String, dynamic>());
        state = {...state, jid: art};
      }
    });
  }
  final Ref _ref;
  late final dynamic _sub;

  /// Read one artifact file via REST (`/api/workbench/:jid/:id/read-file`).
  Future<String> readFile(String jid, String artifactId, String path) async {
    final r = await _ref.read(apiClientProvider).get(
      '/api/workbench/$jid/$artifactId/read-file',
      query: {'path': path},
    );
    if (r is Map && r['content'] != null) return '${r['content']}';
    return r is String ? r : '';
  }

  /// Close an artifact (REST) and drop it from the dock.
  Future<void> close(String jid, String artifactId) async {
    try {
      await _ref
          .read(apiClientProvider)
          .post('/api/workbench/$jid/$artifactId/close');
    } catch (_) {}
    final next = {...state}..remove(jid);
    state = next;
  }

  /// Mark an artifact as viewed (REST).
  void markViewed(String jid, String artifactId) => _ref
      .read(apiClientProvider)
      .post('/api/workbench/$jid/$artifactId/mark-viewed')
      .ignore();

  @override
  void dispose() {
    _sub.cancel();
    super.dispose();
  }
}

final workbenchProvider =
    StateNotifierProvider<WorkbenchNotifier, Map<String, WorkbenchArtifact>>(
      (ref) => WorkbenchNotifier(ref),
    );
