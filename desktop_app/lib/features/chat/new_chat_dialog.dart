import 'dart:convert';
import 'dart:io' show File, Platform;

import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:http/http.dart' as http;
import 'package:path_provider/path_provider.dart';
import 'package:record/record.dart';
import '../../core/i18n/l10n.dart';
import '../../core/prefs.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import '../cowork/cowork_providers.dart';
import '../workflow/workflow_quick_start.dart';
import 'agents_provider.dart';
import 'audio_service.dart';
import 'conversation_provider.dart';
import 'groups_provider.dart';
import 'mini_chat_screen.dart' show subWindowIdProvider;
import 'widgets/slash_mention_input.dart';

class LlmConfig {
  final String id;
  final String label;
  // Full endpoint fields (GET /api/llm-config returns these, apiKey included)
  // so the editor can pre-fill when editing an existing endpoint.
  final String provider;
  final String baseUrl;
  final String apiKey;
  final String modelName;
  final String adapt;
  final int maxTokens;
  final int contextLength;

  /// Explicit vision-support override; null = daemon auto-infers from the
  /// model name (mirrors the web LLMSettings tri-state).
  final bool? vision;
  const LlmConfig(
    this.id,
    this.label, {
    this.provider = '',
    this.baseUrl = '',
    this.apiKey = '',
    this.modelName = '',
    this.adapt = '',
    this.maxTokens = 0,
    this.contextLength = 0,
    this.vision,
  });
}

/// LLM config list + the active model id per role (main/quick/cognitive).
/// A named class (not a record) so its provider type is stable across changes.
class LlmConfigData {
  final List<LlmConfig> configs;
  final String? activeId;
  final String? activeQuickId;
  final String? activeCognitiveId;
  const LlmConfigData({
    this.configs = const [],
    this.activeId,
    this.activeQuickId,
    this.activeCognitiveId,
  });
}

/// LLM configs for the model picker (`GET /api/llm-config`).
final llmConfigsProvider = FutureProvider<LlmConfigData>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/llm-config');
  if (r is! Map) return const LlmConfigData();
  final configs = ((r['configs'] as List?) ?? const [])
      .whereType<Map>()
      .map((m) => LlmConfig(
            '${m['id']}',
            '${m['label'] ?? m['id']}',
            provider: '${m['provider'] ?? ''}',
            baseUrl: '${m['baseURL'] ?? ''}',
            apiKey: '${m['apiKey'] ?? ''}',
            modelName: '${m['modelName'] ?? ''}',
            adapt: '${m['adapt'] ?? ''}',
            maxTokens: (m['maxTokens'] as num?)?.toInt() ?? 0,
            contextLength: (m['contextLength'] as num?)?.toInt() ?? 0,
            vision: m['vision'] is bool ? m['vision'] as bool : null,
          ))
      .toList();
  return LlmConfigData(
    configs: configs,
    activeId: r['activeId'] as String?,
    activeQuickId: r['activeQuickId'] as String?,
    activeCognitiveId: r['activeCognitiveId'] as String?,
  );
});

/// Whether the New Chat page is shown in the conversation area (replaces the
/// old modal dialog — the form now lives inline as a full page).
final showNewChatProvider = StateProvider<bool>((ref) => false);

/// Full-page New Chat form, rendered inside the conversation area (web
/// NewChatScreen). Replaces the previous modal dialog.
class NewChatScreen extends ConsumerStatefulWidget {
  const NewChatScreen({super.key});
  @override
  ConsumerState<NewChatScreen> createState() => _NewChatScreenState();
}

class _NewChatScreenState extends ConsumerState<NewChatScreen> {
  final _msg = TextEditingController();
  String? _agentFolder;
  String? _modelId; // null = active default
  String _chatType = 'Agent'; // Agent | Plan | Dag (web Segmented)
  String _kind = 'chat'; // chat | code | cowork | schedule
  String? _workDir;
  String? _templateId; // selected cowork template
  String _freq = 'daily'; // schedule frequency (daily|weekly|monthly|once|advanced)
  final _schedTime = TextEditingController(text: '09:00');
  final _cron = TextEditingController(text: '0 9 * * *');
  bool _creating = false;

  // First-message attachments + mic dictation (parity with the main chat
  // composer in conversation_pane).
  final List<Map<String, String>> _attachments = [];
  final _recorder = AudioRecorder();
  bool _recording = false;
  bool _transcribing = false;

  bool get _isCode => _kind == 'code';
  bool get _isCowork => _kind == 'cowork';
  bool get _isSchedule => _kind == 'schedule';
  bool get _isWorkflow => _kind == 'workflow';

  @override
  void initState() {
    super.initState();
    // The composer no longer owns an onChanged callback, so keep the Start
    // button's enabled state in step with the draft here.
    _msg.addListener(_onDraftChanged);
  }

  void _onDraftChanged() => setState(() {});

  @override
  void dispose() {
    _msg.removeListener(_onDraftChanged);
    _msg.dispose();
    _schedTime.dispose();
    _cron.dispose();
    _recorder.dispose();
    super.dispose();
  }

  Future<void> _attach() async {
    final res = await FilePicker.platform.pickFiles(
      type: FileType.image,
      allowMultiple: true,
      withData: true,
    );
    if (res == null) return;
    for (final f in res.files) {
      final bytes = f.bytes;
      if (bytes == null) continue;
      final ext = (f.extension ?? 'png').toLowerCase();
      final mime = ext == 'jpg' || ext == 'jpeg' ? 'image/jpeg' : 'image/$ext';
      setState(() => _attachments.add({
            'mimeType': mime,
            'dataUrl': 'data:$mime;base64,${base64Encode(bytes)}',
          }));
    }
  }

  /// Toggle mic recording. On stop, send the audio to Whisper and append the
  /// recognized text to the composer.
  Future<void> _toggleMic() async {
    if (_recording) {
      setState(() {
        _recording = false;
        _transcribing = true;
      });
      try {
        final out = await _recorder.stop();
        if (out == null) return;
        Uint8List bytes;
        String filename;
        if (kIsWeb) {
          bytes = (await http.get(Uri.parse(out))).bodyBytes;
          filename = 'recording.webm';
        } else {
          bytes = await File(out).readAsBytes();
          filename = out.split(Platform.pathSeparator).last;
        }
        final text =
            await ref.read(audioServiceProvider).transcribe(bytes, filename);
        if (text.isNotEmpty) {
          final prefix = _msg.text.trimRight();
          setState(
              () => _msg.text = prefix.isEmpty ? text : '$prefix $text');
        }
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
                content: Text(context
                    .trArgs('Transcription failed: {e}', {'e': e}))),
          );
        }
      } finally {
        if (mounted) setState(() => _transcribing = false);
      }
      return;
    }
    if (!await _recorder.hasPermission()) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
              content: Text(context.tr('Microphone permission denied'))),
        );
      }
      return;
    }
    String path = '';
    if (!kIsWeb) {
      final dir = await getTemporaryDirectory();
      path =
          '${dir.path}${Platform.pathSeparator}senclaw_rec_${DateTime.now().millisecondsSinceEpoch}.m4a';
    }
    await _recorder.start(const RecordConfig(), path: path);
    if (mounted) setState(() => _recording = true);
  }

  /// Attach + mic buttons shared by the full and mini composer toolbars.
  List<Widget> _micAttachButtons(AppColors c) => [
        IconButton(
          tooltip: context.tr('Attach images'),
          visualDensity: VisualDensity.compact,
          icon: Icon(Icons.attach_file, size: 18, color: c.textSecondary),
          onPressed: _attach,
        ),
        IconButton(
          tooltip: _recording
              ? context.tr('Stop recording')
              : context.tr('Dictate (Whisper)'),
          visualDensity: VisualDensity.compact,
          icon: _transcribing
              ? const SizedBox(
                  width: 16,
                  height: 16,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : Icon(_recording ? Icons.stop_circle : Icons.mic_none,
                  size: 18,
                  color: _recording ? AppTokens.danger : c.textSecondary),
          onPressed: _transcribing ? null : _toggleMic,
        ),
      ];

  static const _kRecentDirs = 'senclaw:recent-workdirs';

  Future<void> _pickFolder() async {
    final p = await _pickProject();
    if (p != null) setState(() => _workDir = p.isEmpty ? null : p);
  }

  Future<void> _pickTime() async {
    final parts = _schedTime.text.split(':');
    final initial = TimeOfDay(
      hour: int.tryParse(parts.firstOrNull ?? '') ?? 9,
      minute: int.tryParse(parts.length > 1 ? parts[1] : '') ?? 0,
    );
    final picked = await showTimePicker(
        context: context,
        initialTime: initial,
        builder: (ctx, child) => MediaQuery(
            data: MediaQuery.of(ctx)
                .copyWith(alwaysUse24HourFormat: true),
            child: child!));
    if (picked != null) {
      setState(() => _schedTime.text =
          '${picked.hour.toString().padLeft(2, '0')}:${picked.minute.toString().padLeft(2, '0')}');
    }
  }

  /// Web-style project picker: search + recent projects + add new + "don't
  /// work in a project". Returns null=cancel, ''=clear, or the chosen path.
  Future<String?> _pickProject() async {
    final prefs = ref.read(prefsHelperProvider);
    final recents = prefs.stringSet(_kRecentDirs).toList()..sort();
    var query = '';
    return showDialog<String>(
      context: context,
      builder: (dctx) {
        final c = dctx.colors;
        return StatefulBuilder(builder: (dctx, setLocal) {
          final filtered = recents
              .where((p) => p.toLowerCase().contains(query.toLowerCase()))
              .toList();
          return Dialog(
            backgroundColor: c.surface,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 420, maxHeight: 480),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Padding(
                    padding: const EdgeInsets.all(AppTokens.s12),
                    child: TextField(
                      autofocus: true,
                      onChanged: (v) => setLocal(() => query = v),
                      decoration: InputDecoration(
                        prefixIcon: const Icon(Icons.search, size: 16),
                        hintText: dctx.tr('Search projects'),
                        isDense: true,
                        border: const OutlineInputBorder(),
                      ),
                    ),
                  ),
                  Flexible(
                    child: ListView(
                      shrinkWrap: true,
                      padding: EdgeInsets.zero,
                      children: [
                        for (final p in filtered)
                          ListTile(
                            dense: true,
                            leading: Icon(Icons.folder_outlined,
                                size: 18, color: c.accent),
                            title: Text(
                                p.split('/').where((s) => s.isNotEmpty).last,
                                maxLines: 1, overflow: TextOverflow.ellipsis),
                            subtitle: Text(p,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 11)),
                            onTap: () => Navigator.of(dctx).pop(p),
                          ),
                      ],
                    ),
                  ),
                  const Divider(height: 1),
                  Padding(
                    padding: const EdgeInsets.fromLTRB(AppTokens.s16,
                        AppTokens.s8, AppTokens.s16, 2),
                    child: Text(dctx.tr('ADD NEW PROJECT'),
                        style: TextStyle(
                            color: c.textMuted,
                            fontSize: 10,
                            fontWeight: FontWeight.w700,
                            letterSpacing: 0.5)),
                  ),
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.add_box_outlined, size: 18),
                    title: Text(dctx.tr('Start from scratch')),
                    subtitle: Text(dctx.tr('Create a new folder'),
                        style: TextStyle(color: c.textMuted, fontSize: 11)),
                    onTap: () async {
                      final created = await _createProjectFolder(dctx);
                      if (created != null && dctx.mounted) {
                        prefs.setStringSet(_kRecentDirs,
                            {...prefs.stringSet(_kRecentDirs), created});
                        Navigator.of(dctx).pop(created);
                      }
                    },
                  ),
                  ListTile(
                    dense: true,
                    leading: const Icon(Icons.folder_open_outlined, size: 18),
                    title: Text(dctx.tr('Use an existing folder')),
                    onTap: () async {
                      final dir =
                          await FilePicker.platform.getDirectoryPath();
                      if (dir != null) {
                        prefs.setStringSet(
                            _kRecentDirs, {...prefs.stringSet(_kRecentDirs), dir});
                      }
                      if (dctx.mounted) Navigator.of(dctx).pop(dir);
                    },
                  ),
                  const Divider(height: 1),
                  ListTile(
                    dense: true,
                    leading: Icon(Icons.do_not_disturb_alt_outlined,
                        size: 18, color: c.textMuted),
                    title: Text(dctx.tr("Don't work in a project"),
                        style: TextStyle(color: c.textMuted)),
                    onTap: () => Navigator.of(dctx).pop(''),
                  ),
                ],
              ),
            ),
          );
        });
      },
    );
  }

  Future<void> _start() async {
    final text = _msg.text.trim();
    if (text.isEmpty) return;

    // Schedule: create a scheduled task (not a chat).
    if (_isSchedule) {
      setState(() => _creating = true);
      try {
        await ref.read(apiClientProvider).post('/api/space/schedules', body: {
          'prompt': text,
          'label': text.length > 40 ? '${text.substring(0, 40)}…' : text,
          if (_freq == 'advanced')
            'cron_advanced': _cron.text.trim()
          else ...{
            'frequency': _freq,
            'time_local': _schedTime.text.trim(),
          },
          'agent_mode': _chatType,
          if (_modelId != null && _modelId!.isNotEmpty) 'model_id': _modelId,
          if (_agentFolder != null && _agentFolder!.isNotEmpty)
            'agent_folder': _agentFolder,
        });
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
              SnackBar(content: Text(context.tr('Schedule created'))));
        }
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
              content: Text(context.trArgs('Failed: {e}', {'e': e}))));
        }
      }
      if (mounted) {
        setState(() => _creating = false);
        ref.read(showNewChatProvider.notifier).state = false;
      }
      return;
    }

    // Cowork: spin up a team from the chosen template, then open its chat and
    // hand it the goal as the first message.
    if (_isCowork) {
      final tmplId = _templateId ??
          ref.read(coworkTemplatesProvider).valueOrNull?.firstOrNull?.id;
      if (tmplId == null) return;
      setState(() => _creating = true);
      final teamId =
          await createTeamFromTemplate(ref, tmplId, workspaceDir: _workDir);
      if (!mounted) return;
      setState(() => _creating = false);
      if (teamId == null) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text(context.tr('Failed to create team'))));
        return;
      }
      final jid = 'cowork:$teamId';
      ref.read(selectedJidProvider.notifier).state = jid;
      final convo = ref.read(conversationProvider(jid).notifier);
      convo.setAgentMode('Dag');
      convo.sendText(text, attachments: List.of(_attachments));
      ref.read(showNewChatProvider.notifier).state = false;
      return;
    }

    final agents = ref.read(agentsProvider);
    final folder = _agentFolder ??
        ref.read(agentsProvider.notifier).defaultAgent?.folder ??
        'main';
    final agentName =
        agents.where((a) => a.folder == folder).map((a) => a.name).firstOrNull ??
            folder;
    final name = text.length > 40 ? '${text.substring(0, 40)}…' : text;

    final jid = ref.read(groupsProvider.notifier).createChat(
          folder: folder,
          name: name.isEmpty
              ? context.trArgs('New chat with {agent}', {'agent': agentName})
              : name,
          isCode: _isCode,
          workDir: _workDir,
          modelId: _modelId,
        );
    ref.read(selectedJidProvider.notifier).state = jid;
    // Instantiates the conversation notifier (subscribes) and queues the
    // first message right after the register:group frame on the same socket.
    final convo = ref.read(conversationProvider(jid).notifier);
    convo.setAgentMode(_chatType); // Agent / Plan / Dag
    convo.sendText(text, attachments: List.of(_attachments));
    ref.read(showNewChatProvider.notifier).state = false;
  }

  /// "Start from scratch": prompt for a path, POST /api/workspace/mkdir, and
  /// return the canonical created path (null on cancel/error).
  Future<String?> _createProjectFolder(BuildContext ctx) async {
    final ctrl = TextEditingController(text: '~/');
    final ok = await showDialog<bool>(
      context: ctx,
      builder: (dctx) => AlertDialog(
        title: Text(dctx.tr('Start a new project folder')),
        content: SizedBox(
          width: 420,
          child: TextField(
            controller: ctrl,
            autofocus: true,
            decoration: InputDecoration(
              labelText: dctx.tr('Folder path'),
              hintText: '~/projects/my-app',
              border: const OutlineInputBorder(),
            ),
            onSubmitted: (_) => Navigator.of(dctx).pop(true),
          ),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(dctx).pop(false),
              child: Text(dctx.tr('Cancel'))),
          FilledButton(
              onPressed: () => Navigator.of(dctx).pop(true),
              child: Text(dctx.tr('Create'))),
        ],
      ),
    );
    if (ok != true || ctrl.text.trim().isEmpty) return null;
    try {
      final r = await ref.read(apiClientProvider).post('/api/workspace/mkdir',
          body: {'path': ctrl.text.trim(), 'recursive': true});
      return (r is Map ? r['path'] as String? : null) ?? ctrl.text.trim();
    } catch (e) {
      if (ctx.mounted) {
        ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(
            content: Text(ctx.trArgs('Create failed: {e}', {'e': e}))));
      }
      return null;
    }
  }

  ({String heading, String sub}) _greeting(String? agentName) {
    if (_isWorkflow) {
      return (
        heading: context.tr('Run a workflow'),
        sub: context.tr(
            'Pick a saved routine, or describe a new one and let the agent build it.')
      );
    }
    if (_isSchedule) {
      return (
        heading: context.tr('Create a schedule'),
        sub: context.tr('Describe the task and when it should run.')
      );
    }
    if (_isCowork) {
      return (
        heading: context.tr('Start a Cowork team'),
        sub: context.tr('Pick a template, then describe the goal.')
      );
    }
    if (_isCode) {
      final base =
          _workDir?.split('/').where((s) => s.isNotEmpty).lastOrNull;
      return base != null
          ? (
              heading: context
                  .trArgs('What should we build in {folder}?', {'folder': base}),
              sub: ''
            )
          : (
              heading: context.tr('Pick a workspace folder to start'),
              sub: context.tr('Choose your project root below.')
            );
    }
    return agentName != null
        ? (
            heading: context.trArgs('Chat with {agent}', {'agent': agentName}),
            sub: context.tr('No workspace needed — just a conversation.')
          )
        : (
            heading: context.tr('How can I help today?'),
            sub: context.tr('No workspace needed — just a conversation.')
          );
  }

  static const _suggestions = [
    'Summarize my unread messages',
    'Plan a project roadmap',
    'Research a topic and cite sources',
    'Help me debug an error',
  ];

  /// Profile (agent) dropdown — shared by the full + mini toolbars.
  Widget _profileDropdownInner(AppColors c, List<AgentInfo> agents,
      {String hint = 'Default'}) {
    return DropdownButtonHideUnderline(
      child: DropdownButton<String>(
        value: _agentFolder,
        isExpanded: true,
        isDense: true,
        hint: Row(children: [
          Icon(Icons.person_outline, size: 13, color: c.textMuted),
          const SizedBox(width: 4),
          Text(context.tr(hint),
              style: TextStyle(color: c.textSecondary, fontSize: 13)),
        ]),
        style: TextStyle(color: c.textPrimary, fontSize: 13),
        items: [
          for (final a in agents)
            DropdownMenuItem(
                value: a.folder,
                child: Text(a.name, overflow: TextOverflow.ellipsis)),
        ],
        onChanged: (v) => setState(() => _agentFolder = v),
      ),
    );
  }

  /// Model dropdown — shared by the full + mini toolbars.
  Widget _modelDropdownInner(AppColors c, AsyncValue<LlmConfigData> llm) {
    return llm.maybeWhen(
      data: (d) => DropdownButtonHideUnderline(
        child: DropdownButton<String?>(
          value: _modelId,
          isExpanded: true,
          isDense: true,
          hint: Text(context.tr('Model'),
              style: TextStyle(color: c.textSecondary, fontSize: 13)),
          style: TextStyle(color: c.textPrimary, fontSize: 13),
          items: [
            DropdownMenuItem(
                value: null,
                child: Text(context.tr('Active default'),
                    style: TextStyle(color: c.textSecondary, fontSize: 13))),
            for (final m in d.configs)
              DropdownMenuItem(
                  value: m.id,
                  child: Text(m.label, overflow: TextOverflow.ellipsis)),
          ],
          onChanged: (v) => setState(() => _modelId = v),
        ),
      ),
      orElse: () => const SizedBox.shrink(),
    );
  }

  /// Compact two-row toolbar for the narrow mini-chat window: profile + model
  /// get a full-width row of their own (so "Active default" isn't squeezed to a
  /// wrap), with mode icons + send underneath.
  Widget _miniChatToolbar(BuildContext context, AppColors c,
      List<AgentInfo> agents, AsyncValue<LlmConfigData> llm, bool canStart) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(children: [
          if (_isCode) ...[
            _FolderPill(workDir: _workDir, onTap: _pickFolder),
            Container(
                width: 1,
                height: 16,
                color: c.border,
                margin: const EdgeInsets.symmetric(horizontal: AppTokens.s4)),
          ],
          Expanded(child: _profileDropdownInner(c, agents)),
          const SizedBox(width: AppTokens.s8),
          Expanded(child: _modelDropdownInner(c, llm)),
        ]),
        const SizedBox(height: AppTokens.s8),
        Row(children: [
          _ModeIcons(
            value: _chatType,
            onChanged: (t) => setState(() => _chatType = t),
          ),
          const Spacer(),
          ..._micAttachButtons(c),
          _SendButton(enabled: canStart && !_creating, onTap: _start),
        ]),
      ],
    );
  }

  /// Compact two-row schedule toolbar for the narrow mini-chat window: profile +
  /// model on top, then frequency + time/cron + send underneath.
  Widget _miniScheduleToolbar(BuildContext context, AppColors c,
      List<AgentInfo> agents, AsyncValue<LlmConfigData> llm, bool canStart) {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(children: [
          Expanded(child: _profileDropdownInner(c, agents, hint: 'Profile')),
          const SizedBox(width: AppTokens.s8),
          Expanded(child: _modelDropdownInner(c, llm)),
        ]),
        const SizedBox(height: AppTokens.s8),
        Row(children: [
          Expanded(
            child: DropdownButtonHideUnderline(
              child: DropdownButton<String>(
                value: _freq,
                isExpanded: true,
                isDense: true,
                style: TextStyle(color: c.textPrimary, fontSize: 13),
                items: [
                  DropdownMenuItem(
                      value: 'daily', child: Text(context.tr('Daily'))),
                  DropdownMenuItem(
                      value: 'weekly', child: Text(context.tr('Weekly'))),
                  DropdownMenuItem(
                      value: 'monthly', child: Text(context.tr('Monthly'))),
                  DropdownMenuItem(
                      value: 'once', child: Text(context.tr('Once'))),
                  DropdownMenuItem(
                      value: 'advanced', child: Text(context.tr('Advanced'))),
                ],
                onChanged: (v) => setState(() => _freq = v ?? 'daily'),
              ),
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          if (_freq == 'advanced')
            Expanded(
              child: TextField(
                controller: _cron,
                decoration: const InputDecoration(
                    hintText: '0 9 * * *',
                    isDense: true,
                    isCollapsed: true,
                    border: InputBorder.none),
                style: const TextStyle(
                    fontSize: 12, fontFamily: AppTokens.fontMono),
              ),
            )
          else
            GestureDetector(
              onTap: _pickTime,
              child: Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                decoration: BoxDecoration(
                  color: c.surfaceAlt,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                  border: Border.all(color: c.border),
                ),
                child: Row(mainAxisSize: MainAxisSize.min, children: [
                  Icon(Icons.schedule, size: 13, color: c.textMuted),
                  const SizedBox(width: 4),
                  Text(_schedTime.text,
                      style:
                          TextStyle(color: c.textPrimary, fontSize: 13)),
                ]),
              ),
            ),
          const SizedBox(width: AppTokens.s8),
          _SendButton(enabled: canStart && !_creating, onTap: _start),
        ]),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final agents = ref.watch(agentsProvider).where((a) => !a.isSchedule).toList();
    final llm = ref.watch(llmConfigsProvider);
    final agentName = _agentFolder == null
        ? null
        : agents
            .where((a) => a.folder == _agentFolder)
            .map((a) => a.name)
            .firstOrNull;
    final g = _greeting(agentName);
    final canStart = _msg.text.trim().isNotEmpty;
    // The compact mini-chat window hides the quick-suggestion chips (too little
    // room); the full New Chat page still shows them.
    final isMini = ref.watch(subWindowIdProvider) != null;

    return Center(
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(AppTokens.s24),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 680),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // Chat / Code / Cowork kind selector (web top segmented).
              // FittedBox lets it shrink to fit on narrow windows instead of
              // overflowing the right edge.
              Center(
                child: FittedBox(
                  fit: BoxFit.scaleDown,
                  child: _KindSegmented(
                    kind: _kind,
                    onChanged: (k) => setState(() {
                      _kind = k;
                      if (k != 'code') _workDir = null;
                    }),
                  ),
                ),
              ),
              const SizedBox(height: AppTokens.s20),
              // Centered greeting.
              Column(
                children: [
                  Container(
                    width: 56,
                    height: 56,
                    decoration: BoxDecoration(
                      color: c.accent.withValues(alpha: 0.12),
                      shape: BoxShape.circle,
                    ),
                    child: Icon(Icons.auto_awesome, color: c.accent, size: 26),
                  ),
                  const SizedBox(height: AppTokens.s12),
                  Text(g.heading,
                      textAlign: TextAlign.center,
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 20,
                          fontWeight: FontWeight.w700)),
                  if (g.sub.isNotEmpty) ...[
                    const SizedBox(height: AppTokens.s4),
                    Text(g.sub,
                        textAlign: TextAlign.center,
                        style: TextStyle(color: c.textMuted, fontSize: 13)),
                  ],
                ],
              ),
              const SizedBox(height: AppTokens.s20),
              // Workflow kind swaps the composer for the quick-start panel
              // (pick & run a saved workflow, or create one with the agent).
              if (_isWorkflow)
                const WorkflowQuickStart()
              else ...[
              // Unified input card: textarea + toolbar in one rounded surface.
              Container(
                decoration: BoxDecoration(
                  color: c.surface,
                  borderRadius: BorderRadius.circular(AppTokens.rXl),
                  border: Border.all(color: c.border),
                  boxShadow: [
                    BoxShadow(
                        color: Colors.black.withValues(alpha: 0.08),
                        blurRadius: 24,
                        offset: const Offset(0, 8)),
                  ],
                ),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(
                          AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
                      // Same `/ # @` affordances as an open conversation. Files
                      // are only offered once a workspace is picked.
                      child: SlashMentionField(
                        controller: _msg,
                        onSend: canStart ? _start : () {},
                        fileScope: mentionScopeForPath(_workDir),
                        autofocus: true,
                        minLines: 3,
                        decoration: InputDecoration(
                          hintText: context.tr(
                              'Ask anything, or describe a task…   / # skill · @ file'),
                          border: InputBorder.none,
                          isCollapsed: true,
                        ),
                      ),
                    ),
                    if (_attachments.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.fromLTRB(
                            AppTokens.s16, AppTokens.s8, AppTokens.s16, 0),
                        child: Wrap(
                          spacing: AppTokens.s8,
                          runSpacing: AppTokens.s8,
                          children: [
                            for (var i = 0; i < _attachments.length; i++)
                              Chip(
                                label: Text(
                                    context.trArgs('image {n}', {'n': i + 1}),
                                    style: const TextStyle(fontSize: 12)),
                                avatar:
                                    const Icon(Icons.image_outlined, size: 14),
                                onDeleted: () => setState(
                                    () => _attachments.removeAt(i)),
                              ),
                          ],
                        ),
                    ),
                    Divider(height: AppTokens.s16, color: c.border),
                    // Toolbar row.
                    Padding(
                      padding: const EdgeInsets.fromLTRB(
                          AppTokens.s8, 0, AppTokens.s8, AppTokens.s8),
                      child: LayoutBuilder(builder: (context, constraints) {
                        // Adapt to available width, not just the mini window:
                        // narrow → compact two-row toolbar; wide → single row.
                        final compact = constraints.maxWidth < 460;
                        return compact && !_isCowork
                          ? (_isSchedule
                              ? _miniScheduleToolbar(
                                  context, c, agents, llm, canStart)
                              : _miniChatToolbar(
                                  context, c, agents, llm, canStart))
                          : Row(
                        children: [
                          if (_isCowork) ...[
                            _FolderPill(
                                workDir: _workDir, onTap: _pickFolder),
                            Container(
                                width: 1,
                                height: 16,
                                color: c.border,
                                margin: const EdgeInsets.symmetric(
                                    horizontal: AppTokens.s4)),
                            Expanded(
                              child: DropdownButtonHideUnderline(
                                child: Builder(builder: (_) {
                                  final tmpls = ref
                                          .watch(coworkTemplatesProvider)
                                          .valueOrNull ??
                                      const [];
                                  final sel =
                                      _templateId ?? tmpls.firstOrNull?.id;
                                  return DropdownButton<String>(
                                    value: tmpls.any((t) => t.id == sel)
                                        ? sel
                                        : null,
                                    isExpanded: true,
                                    isDense: true,
                                    hint: Row(children: [
                                      Icon(Icons.groups_outlined,
                                          size: 13, color: c.textMuted),
                                      const SizedBox(width: 4),
                                      Text(context.tr('Pick a team template'),
                                          style: TextStyle(
                                              color: c.textSecondary,
                                              fontSize: 13)),
                                    ]),
                                    style: TextStyle(
                                        color: c.textPrimary, fontSize: 13),
                                    items: [
                                      for (final t in tmpls)
                                        DropdownMenuItem(
                                            value: t.id,
                                            child: Text('${t.icon} ${t.name}',
                                                overflow:
                                                    TextOverflow.ellipsis)),
                                    ],
                                    onChanged: (v) =>
                                        setState(() => _templateId = v),
                                  );
                                }),
                              ),
                            ),
                          ] else if (_isSchedule) ...[
                            // Order: Profile · Model · schedule type · time.
                            // Profile (agent) to run the schedule.
                            Expanded(
                              child: DropdownButtonHideUnderline(
                                child: DropdownButton<String>(
                                  value: _agentFolder,
                                  isExpanded: true,
                                  isDense: true,
                                  hint: Row(children: [
                                    Icon(Icons.person_outline,
                                        size: 13, color: c.textMuted),
                                    const SizedBox(width: 4),
                                    Text(context.tr('Profile'),
                                        style: TextStyle(
                                            color: c.textSecondary,
                                            fontSize: 13)),
                                  ]),
                                  style: TextStyle(
                                      color: c.textPrimary, fontSize: 13),
                                  items: [
                                    for (final a in agents)
                                      DropdownMenuItem(
                                          value: a.folder,
                                          child: Text(a.name,
                                              overflow:
                                                  TextOverflow.ellipsis)),
                                  ],
                                  onChanged: (v) =>
                                      setState(() => _agentFolder = v),
                                ),
                              ),
                            ),
                            const SizedBox(width: AppTokens.s8),
                            // Model.
                            Expanded(
                              child: llm.maybeWhen(
                                data: (d) => DropdownButtonHideUnderline(
                                  child: DropdownButton<String?>(
                                    value: _modelId,
                                    isExpanded: true,
                                    isDense: true,
                                    hint: Text(context.tr('Model'),
                                        style: TextStyle(
                                            color: c.textSecondary,
                                            fontSize: 13)),
                                    style: TextStyle(
                                        color: c.textPrimary, fontSize: 13),
                                    items: [
                                      DropdownMenuItem(
                                          value: null,
                                          child: Text(context.tr('Default'),
                                              style: TextStyle(
                                                  color: c.textSecondary,
                                                  fontSize: 13))),
                                      for (final m in d.configs)
                                        DropdownMenuItem(
                                            value: m.id,
                                            child: Text(m.label,
                                                overflow:
                                                    TextOverflow.ellipsis)),
                                    ],
                                    onChanged: (v) =>
                                        setState(() => _modelId = v),
                                  ),
                                ),
                                orElse: () => const SizedBox.shrink(),
                              ),
                            ),
                            const SizedBox(width: AppTokens.s8),
                            // Schedule type (loại lịch).
                            SizedBox(
                              width: 100,
                              child: DropdownButtonHideUnderline(
                                child: DropdownButton<String>(
                                  value: _freq,
                                  isExpanded: true,
                                  isDense: true,
                                  style: TextStyle(
                                      color: c.textPrimary, fontSize: 13),
                                  items: [
                                    DropdownMenuItem(
                                        value: 'daily',
                                        child: Text(context.tr('Daily'))),
                                    DropdownMenuItem(
                                        value: 'weekly',
                                        child: Text(context.tr('Weekly'))),
                                    DropdownMenuItem(
                                        value: 'monthly',
                                        child: Text(context.tr('Monthly'))),
                                    DropdownMenuItem(
                                        value: 'once',
                                        child: Text(context.tr('Once'))),
                                    DropdownMenuItem(
                                        value: 'advanced',
                                        child: Text(context.tr('Advanced'))),
                                  ],
                                  onChanged: (v) =>
                                      setState(() => _freq = v ?? 'daily'),
                                ),
                              ),
                            ),
                            const SizedBox(width: AppTokens.s8),
                            if (_freq == 'advanced')
                              SizedBox(
                                width: 120,
                                child: TextField(
                                  controller: _cron,
                                  decoration: const InputDecoration(
                                      hintText: '0 9 * * *',
                                      isDense: true,
                                      isCollapsed: true,
                                      border: InputBorder.none),
                                  style: const TextStyle(
                                      fontSize: 12,
                                      fontFamily: AppTokens.fontMono),
                                ),
                              )
                            else
                              GestureDetector(
                                onTap: _pickTime,
                                child: Container(
                                  padding: const EdgeInsets.symmetric(
                                      horizontal: 10, vertical: 6),
                                  decoration: BoxDecoration(
                                    color: c.surfaceAlt,
                                    borderRadius:
                                        BorderRadius.circular(AppTokens.rMd),
                                    border: Border.all(color: c.border),
                                  ),
                                  child: Row(
                                      mainAxisSize: MainAxisSize.min,
                                      children: [
                                        Icon(Icons.schedule,
                                            size: 13, color: c.textMuted),
                                        const SizedBox(width: 4),
                                        Text(_schedTime.text,
                                            style: TextStyle(
                                                color: c.textPrimary,
                                                fontSize: 13)),
                                      ]),
                                ),
                              ),
                          ] else ...[
                          if (_isCode) ...[
                            _FolderPill(
                                workDir: _workDir, onTap: _pickFolder),
                            Container(
                                width: 1,
                                height: 16,
                                color: c.border,
                                margin: const EdgeInsets.symmetric(
                                    horizontal: AppTokens.s4)),
                          ],
                          // Agent (borderless).
                          Expanded(
                              flex: 3,
                              child: _profileDropdownInner(c, agents)),
                          const SizedBox(width: AppTokens.s8),
                          // Model (borderless).
                          Expanded(
                              flex: 2, child: _modelDropdownInner(c, llm)),
                          const SizedBox(width: AppTokens.s8),
                          _ModeIcons(
                            value: _chatType,
                            onChanged: (t) => setState(() => _chatType = t),
                          ),
                          ..._micAttachButtons(c),
                          ],
                          const SizedBox(width: AppTokens.s8),
                          _SendButton(
                              enabled: canStart && !_creating, onTap: _start),
                        ],
                      );
                      }),
                    ),
                  ],
                ),
              ),
              // Suggestion chips — full window only (hidden in the mini chat).
              if (!isMini) ...[
                const SizedBox(height: AppTokens.s16),
                Wrap(
                  alignment: WrapAlignment.center,
                  spacing: AppTokens.s8,
                  runSpacing: AppTokens.s8,
                  children: [
                    for (final s in _suggestions)
                      _SuggestionChip(
                          text: context.tr(s),
                          onTap: () =>
                              setState(() => _msg.text = context.tr(s))),
                  ],
                ),
              ],
              ],
            ],
          ),
        ),
      ),
    );
  }
}

/// 💬 Chat / ⌨️ Code / 👥 Cowork kind selector (web top segmented + Cowork).
class _KindSegmented extends StatelessWidget {
  const _KindSegmented({required this.kind, required this.onChanged});
  final String kind;
  final void Function(String) onChanged;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    Widget seg(String label, String value) => GestureDetector(
          onTap: () => onChanged(value),
          child: AnimatedContainer(
            duration: const Duration(milliseconds: 120),
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s16, vertical: AppTokens.s6),
            decoration: BoxDecoration(
              color: kind == value ? c.accent : Colors.transparent,
              borderRadius: BorderRadius.circular(AppTokens.rXl),
            ),
            child: Text(label,
                style: TextStyle(
                    color: kind == value ? Colors.white : c.textSecondary,
                    fontSize: 13,
                    fontWeight:
                        kind == value ? FontWeight.w600 : FontWeight.w400)),
          ),
        );
    return Container(
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rXl),
        border: Border.all(color: c.border),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        seg(context.tr('💬 Chat'), 'chat'),
        seg(context.tr('⌨️ Code'), 'code'),
        seg(context.tr('👥 Cowork'), 'cowork'),
        seg(context.tr('🕐 Schedule'), 'schedule'),
        seg(context.tr('🔁 Workflow'), 'workflow'),
      ]),
    );
  }
}

/// Agent / Plan / Dag icon segmented (web ⚡/💡/🔀).
class _ModeIcons extends StatelessWidget {
  const _ModeIcons({required this.value, required this.onChanged});
  final String value;
  final void Function(String) onChanged;
  static const _opts = [
    ('Agent', Icons.bolt, 'Agent — full tool access'),
    ('Plan', Icons.lightbulb_outline, 'Plan — research then propose'),
    ('Dag', Icons.account_tree_outlined, 'DAG — multi-agent dispatch'),
  ];
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rLg),
        border: Border.all(color: c.border),
      ),
      child: Row(mainAxisSize: MainAxisSize.min, children: [
        for (final (val, icon, tip) in _opts)
          Tooltip(
            message: context.tr(tip),
            child: GestureDetector(
              onTap: () => onChanged(val),
              child: Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s8, vertical: AppTokens.s4),
                decoration: BoxDecoration(
                  color: value == val ? c.accent : Colors.transparent,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                ),
                child: Icon(icon,
                    size: 16,
                    color: value == val ? Colors.white : c.textSecondary),
              ),
            ),
          ),
      ]),
    );
  }
}

/// Round send button (web Start arrow).
class _SendButton extends StatelessWidget {
  const _SendButton({required this.enabled, required this.onTap});
  final bool enabled;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Tooltip(
      message: context.tr('Start (Enter)'),
      child: GestureDetector(
        onTap: enabled ? onTap : null,
        child: Container(
          width: 32,
          height: 32,
          decoration: BoxDecoration(
            color: enabled ? c.accent : c.surfaceAlt,
            shape: BoxShape.circle,
          ),
          child: Icon(Icons.arrow_upward,
              size: 16, color: enabled ? Colors.white : c.textMuted),
        ),
      ),
    );
  }
}

/// Folder pill (Code chats) — picks a workspace folder.
class _FolderPill extends StatelessWidget {
  const _FolderPill({required this.workDir, required this.onTap});
  final String? workDir;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final has = workDir != null;
    final name = has
        ? (workDir!.split('/').where((s) => s.isNotEmpty).lastOrNull ??
            context.tr('Folder'))
        : context.tr('Folder');
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 5),
        decoration: BoxDecoration(
          color: has ? c.accentSoft : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Icon(Icons.folder_open_outlined,
              size: 13, color: has ? c.accent : c.textMuted),
          const SizedBox(width: 4),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 120),
            child: Text(name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                    color: has ? c.accent : c.textSecondary, fontSize: 12)),
          ),
        ]),
      ),
    );
  }
}

/// A suggestion chip that fills the input when tapped.
class _SuggestionChip extends StatelessWidget {
  const _SuggestionChip({required this.text, required this.onTap});
  final String text;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding:
            const EdgeInsets.symmetric(horizontal: AppTokens.s12, vertical: 6),
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(AppTokens.rXl),
          border: Border.all(color: c.border),
        ),
        child: Text(text,
            style: TextStyle(color: c.textSecondary, fontSize: 12)),
      ),
    );
  }
}
