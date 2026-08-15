import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';

Map<String, dynamic> _asMap(dynamic r) =>
    r is Map ? r.cast<String, dynamic>() : {};

final adminPermsProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/admin-permissions')));

final agentBehaviorProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/agent-behavior')));

/// Autonomous Kanban/MCP dispatcher toggle ({enabled}).
final dispatchConfigProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/dispatch-config')));

final embeddingConfigProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/embedding-config')));

final cognitiveConfigProvider = FutureProvider<Map<String, dynamic>>((ref) async {
  final r = _asMap(await ref.read(apiClientProvider).get('/api/cognitive-config'));
  // The handler returns {effective, saved}; prefer saved for editing.
  return _asMap(r['saved'] ?? r['effective'] ?? r);
});

class LocalModel {
  final String id;
  final String label;
  final double sizeGb;
  final bool installed;
  final bool loaded;
  final String? downloadStatus; // downloading | error | cancelled | done | null
  final double? downloadProgress; // 0..1 when downloading
  const LocalModel(this.id, this.label, this.sizeGb, this.installed,
      this.loaded, this.downloadStatus, this.downloadProgress);

  bool get downloading => downloadStatus == 'downloading';

  factory LocalModel.fromJson(Map<String, dynamic> j) {
    final d = (j['download'] as Map?)?.cast<String, dynamic>();
    double? prog;
    if (d != null) {
      final got = (d['downloaded_bytes'] as num?)?.toDouble();
      final tot = (d['total_bytes'] as num?)?.toDouble();
      if (got != null && tot != null && tot > 0) prog = (got / tot).clamp(0, 1);
    }
    return LocalModel(
      '${j['id'] ?? ''}',
      '${j['label'] ?? j['id'] ?? ''}',
      (j['approx_size_gb'] as num?)?.toDouble() ?? 0,
      j['installed'] == true,
      j['loaded'] == true,
      d?['status'] as String?,
      prog,
    );
  }
}

final localModelsProvider = FutureProvider<List<LocalModel>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/local-models');
  final list = (r is Map ? r['models'] : r) as List? ?? const [];
  return list
      .whereType<Map>()
      .map((m) => LocalModel.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Local inference runtime info: {platform, local_models_dir, feature_metal…}.
final localModelsRuntimeProvider =
    FutureProvider<Map<String, dynamic>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/local-models/runtime');
  return r is Map ? r.cast<String, dynamic>() : <String, dynamic>{};
});

/// A downloadable media model (whisper / tts / ocr — same JSON shape).
class MediaModel {
  final String id;
  final String label;
  final String description;
  final double sizeGb;
  final bool installed;
  final String? downloadStatus; // queued | downloading | error | done | null
  final double? downloadProgress; // 0..1 when downloading
  final List<String> languages; // supported language codes (may be empty)
  final String? defaultLanguage;
  /// Selectable voices: [{name, description, gender}] (empty = free-form).
  final List<Map<String, String>> voices;
  final String? defaultVoice;
  const MediaModel(this.id, this.label, this.description, this.sizeGb,
      this.installed, this.downloadStatus, this.downloadProgress,
      [this.languages = const [],
      this.defaultLanguage,
      this.voices = const [],
      this.defaultVoice]);

  /// True while a download is queued or in flight — used to disable the
  /// Download button and drive polling (matches the web `['queued',
  /// 'downloading'].includes(status)` check).
  bool get downloading =>
      downloadStatus == 'downloading' || downloadStatus == 'queued';

  factory MediaModel.fromJson(Map<String, dynamic> j) {
    final gb = (j['approx_size_gb'] as num?)?.toDouble();
    final mb = (j['approx_size_mb'] as num?)?.toDouble();
    final d = (j['download'] as Map?)?.cast<String, dynamic>();
    double? prog;
    if (d != null) {
      final got = (d['downloaded_bytes'] as num?)?.toDouble();
      final tot = (d['total_bytes'] as num?)?.toDouble();
      if (got != null && tot != null && tot > 0) prog = (got / tot).clamp(0, 1);
    }
    return MediaModel(
      '${j['id'] ?? ''}',
      '${j['label'] ?? j['id'] ?? ''}',
      '${j['description'] ?? ''}',
      gb ?? (mb != null ? mb / 1024 : 0),
      j['installed'] == true,
      d?['status'] as String?,
      prog,
      (j['languages'] as List?)?.map((e) => '$e').toList() ?? const [],
      j['default_language'] as String?,
      (j['voices'] as List?)
              ?.whereType<Map>()
              .map((v) => {
                    'name': '${v['name'] ?? ''}',
                    'description': '${v['description'] ?? ''}',
                    'gender': '${v['gender'] ?? ''}',
                  })
              .toList() ??
          const [],
      j['default_voice'] as String?,
    );
  }
}

/// Models for a media domain (`whisper` | `tts` | `ocr`).
final mediaModelsProvider =
    FutureProvider.family<List<MediaModel>, String>((ref, domain) async {
  final r = await ref.read(apiClientProvider).get('/api/$domain/models');
  final list = (r is Map ? r['models'] : r) as List? ?? const [];
  return list
      .whereType<Map>()
      .map((m) => MediaModel.fromJson(m.cast<String, dynamic>()))
      .toList();
});

/// Generic POST helper used by section toggles/actions; invalidates the
/// matching provider so the UI reflects the new server state.
class SettingsApi {
  SettingsApi(this._ref);
  final Ref _ref;

  Future<void> post(String path, Object body, ProviderOrFamily refresh) async {
    await _ref.read(apiClientProvider).post(path, body: body);
    _ref.invalidate(refresh);
  }

  Future<void> raw(String path, {Object? body}) =>
      _ref.read(apiClientProvider).post(path, body: body);
}

final settingsApiProvider = Provider<SettingsApi>((ref) => SettingsApi(ref));

// ===== Soul Core (USER.md / TOOLS.md / AGENTS.md) =====

/// The user profile: who the *human* is. Distinct from a Profile's SOUL.md,
/// which is who the *agent* is — see `src/user_profile/` on the daemon side.
final userProfileProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/user-profile')));

/// Machine-local environment notes. Private tier by definition (SSH hosts,
/// internal IPs), so the daemon withholds it from group chats.
final toolsNotesProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/tools-notes')));

/// User-editable operating rules appended to the system prompt.
final agentsRulesProvider = FutureProvider<Map<String, dynamic>>((ref) async =>
    _asMap(await ref.read(apiClientProvider).get('/api/agents-rules')));
