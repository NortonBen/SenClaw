import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../core/prefs.dart';
import '../models/agent_model.dart';
import '../models/cowork_models.dart';
import '../models/workflow_models.dart';
import '../services/code_api.dart';
import '../services/cowork_api.dart';
import '../services/language_service.dart';
import '../services/llm_api.dart';
import '../services/workflow_api.dart';
import '../theme/tokens.dart';
import 'code/code_session_screen.dart';
import 'code/folder_picker.dart';
import 'cowork/cowork_workspace_screen.dart';
import 'workflow/workflow_screen.dart';

/// Result of the New chat screen for the *chat* kind: which profile to talk to
/// and the first message. Code/Cowork kinds navigate to their own detail screen
/// and return null.
class NewChatResult {
  final String agentFolder;
  final String message;

  /// Agent mode for the conversation: 'Agent' | 'Plan' | 'Dag'.
  final String mode;
  const NewChatResult(this.agentFolder, this.message, this.mode);
}

/// Full-page New chat composer (desktop_app NewChatScreen layout adapted for the
/// mobile remote): a Chat / Code / Cowork kind selector, a centered greeting, a
/// unified input card with a profile / project / template selector + round send,
/// and quick-suggestion chips.
///
/// "Limited project" = the picker only offers folders saved in the app (prefs
/// `senclaw:projects`); browsing the filesystem is reachable only via "Add
/// project", which pins the chosen folder.
class NewChatScreen extends ConsumerStatefulWidget {
  const NewChatScreen({super.key, required this.agents});
  final List<AgentInfo> agents;

  @override
  ConsumerState<NewChatScreen> createState() => _NewChatScreenState();
}

class _NewChatScreenState extends ConsumerState<NewChatScreen> {
  static const _kProjects = 'senclaw:projects';
  static List<String> get _suggestions => [
        tr('Tóm tắt tin nhắn chưa đọc', 'Summarize unread messages'),
        tr('Lập kế hoạch cho một dự án', 'Plan a project'),
        tr('Nghiên cứu một chủ đề và trích nguồn',
            'Research a topic with sources'),
        tr('Giúp tôi debug một lỗi', 'Help me debug an error'),
      ];

  final _msg = TextEditingController();
  final _llmApi = LlmApi();
  String _kind = 'chat'; // chat | code | cowork | workflow
  String _chatType = 'Agent'; // Agent | Plan | Dag
  String? _agentFolder;
  String? _modelId; // null = keep the active default model
  String? _activeModelId;
  String? _workDir;
  String? _templateId;
  bool _creating = false;
  List<CoworkTemplate> _templates = [];
  List<LlmOption> _models = [];

  // ── Workflow quick-start state ──
  final _wfApi = WorkflowApi();
  final _wfDesc = TextEditingController();
  List<WorkflowDefSummary> _wfDefs = [];
  bool _wfLoaded = false;
  String? _wfSelected;
  final Map<String, TextEditingController> _wfInputCtrls = {};
  bool _wfStarting = false;
  bool _wfDrafting = false;

  @override
  void initState() {
    super.initState();
    _agentFolder =
        widget.agents.isNotEmpty ? widget.agents.first.folder : null;
    _loadTemplates();
    _loadModels();
  }

  Future<void> _loadTemplates() async {
    var fresh = false;
    // Local-DB paint races the relay fetch — the relay result wins.
    unawaited(CoworkApi().listTemplatesCached().then((cached) {
      if (fresh || cached.isEmpty || !mounted || _templates.isNotEmpty) return;
      setState(() => _templates = cached);
    }));
    try {
      final t = await CoworkApi().listTemplates();
      fresh = true;
      if (mounted) setState(() => _templates = t);
    } catch (_) {/* templates are optional */}
  }

  Future<void> _loadModels() async {
    var fresh = false;
    // Local-DB paint races the relay fetch — the relay result wins.
    unawaited(_llmApi.listCached().then((cached) {
      if (fresh || cached.configs.isEmpty || !mounted) return;
      if (_models.isNotEmpty) return;
      setState(() {
        _models = cached.configs;
        _activeModelId = cached.activeId;
      });
    }));
    try {
      final l = await _llmApi.list();
      fresh = true;
      if (mounted) {
        setState(() {
          _models = l.configs;
          _activeModelId = l.activeId;
        });
      }
    } catch (_) {/* model list optional */}
  }

  /// Apply the picked model. The relay has no per-chat model, so this sets the
  /// daemon's GLOBAL active model (no-op when the default is kept).
  Future<void> _applyModel() async {
    final id = _modelId;
    if (id == null || id == _activeModelId) return;
    try {
      await _llmApi.setActive(id);
    } catch (_) {/* keep going even if it fails */}
  }

  @override
  void dispose() {
    _msg.dispose();
    _wfDesc.dispose();
    for (final c in _wfInputCtrls.values) {
      c.dispose();
    }
    super.dispose();
  }

  Future<void> _loadWorkflows() async {
    var fresh = false;
    // Local-DB paint races the relay fetch — the relay result wins.
    unawaited(_wfApi.listDefsCached().then((cached) {
      if (fresh || cached.isEmpty || !mounted || _wfDefs.isNotEmpty) return;
      setState(() {
        _wfDefs = cached;
        _wfLoaded = true;
      });
    }));
    try {
      final defs = await _wfApi.listDefs();
      fresh = true;
      if (mounted) {
        setState(() {
          _wfDefs = defs;
          _wfLoaded = true;
        });
      }
    } catch (_) {
      if (mounted) setState(() => _wfLoaded = true);
    }
  }

  String? get _effectiveTemplate =>
      _templateId ?? (_templates.isNotEmpty ? _templates.first.id : null);

  bool get _canSubmit {
    switch (_kind) {
      case 'code':
        return _workDir != null && !_creating;
      case 'cowork':
        return _effectiveTemplate != null && !_creating;
      default:
        return _msg.text.trim().isNotEmpty &&
            widget.agents.isNotEmpty &&
            !_creating;
    }
  }

  // ── Workflow quick-start (pick & run, or create with AI) ──────────────────

  void _wfPick(String name) {
    WorkflowDefSummary? d;
    for (final x in _wfDefs) {
      if (x.name == name) {
        d = x;
        break;
      }
    }
    setState(() {
      _wfSelected = name;
      for (final c in _wfInputCtrls.values) {
        c.dispose();
      }
      _wfInputCtrls.clear();
      for (final i in d?.inputs ?? const <WorkflowInputDef>[]) {
        _wfInputCtrls[i.name] =
            TextEditingController(text: i.defaultValue ?? '');
      }
    });
  }

  Future<void> _wfRun(WorkflowDefSummary def) async {
    final missing = def.inputs
        .where((i) =>
            i.required && (_wfInputCtrls[i.name]?.text.trim() ?? '').isEmpty)
        .map((i) => i.name)
        .toList();
    if (missing.isNotEmpty) {
      _wfSnack(tr('Thiếu input bắt buộc: ${missing.join(', ')}',
          'Missing required inputs: ${missing.join(', ')}'));
      return;
    }
    setState(() => _wfStarting = true);
    try {
      final inputs = <String, String>{
        for (final e in _wfInputCtrls.entries)
          if (e.value.text.trim().isNotEmpty) e.key: e.value.text,
      };
      final runId = await _wfApi.startRun(def.name, inputs);
      if (!mounted) return;
      // Leave New Session and land on the live run detail.
      final nav = Navigator.of(context);
      nav.pop();
      nav.push(MaterialPageRoute(
          builder: (_) => WorkflowRunDetailScreen(runId: runId)));
    } catch (e) {
      _wfSnack(tr('Chạy thất bại: $e', 'Run failed: $e'));
      if (mounted) setState(() => _wfStarting = false);
    }
  }

  Future<void> _wfCreateWithAi() async {
    if (_wfDesc.text.trim().isEmpty) {
      _wfSnack(tr('Hãy mô tả quy trình trước', 'Describe the workflow first'));
      return;
    }
    setState(() => _wfDrafting = true);
    String content;
    try {
      // Agent authors a validated draft — nothing saved yet.
      content = await _wfApi.draft(_wfDesc.text);
    } catch (e) {
      _wfSnack(tr('Soạn thảo thất bại: $e', 'Drafting failed: $e'));
      if (mounted) setState(() => _wfDrafting = false);
      return;
    }
    if (!mounted) return;
    setState(() => _wfDrafting = false);
    await _wfReviewAndSave(content);
  }

  /// Review dialog: user edits the agent's draft, Save persists it (validated
  /// server-side, with an overwrite confirm on name clash), Cancel discards.
  /// Validation errors keep the editor open so the fix isn't lost.
  Future<void> _wfReviewAndSave(String content) async {
    final ctrl = TextEditingController(text: content);
    var busy = false;
    final c = context.colors;

    Future<void> save(BuildContext ctx, StateSetter setDlg,
        {bool overwrite = false}) async {
      setDlg(() => busy = true);
      try {
        final created = await _wfApi.create(ctrl.text, overwrite: overwrite);
        await _loadWorkflows();
        _wfPick(created);
        _wfDesc.clear();
        _wfSnack(tr('Đã lưu "$created" — điền input rồi bấm Chạy',
            'Saved "$created" — fill in the inputs and press Run'));
        if (ctx.mounted) Navigator.pop(ctx);
      } catch (e) {
        setDlg(() => busy = false);
        final msg = '$e';
        if (!overwrite && msg.contains('already exists') && ctx.mounted) {
          final ok = await showDialog<bool>(
            context: ctx,
            builder: (ctx2) => AlertDialog(
              title: Text(tr('Workflow đã tồn tại', 'Workflow already exists')),
              content: Text(tr('Ghi đè định nghĩa hiện có?',
                  'Overwrite the existing definition?')),
              actions: [
                TextButton(
                    onPressed: () => Navigator.pop(ctx2, false),
                    child: Text(tr('Huỷ', 'Cancel'))),
                FilledButton(
                    onPressed: () => Navigator.pop(ctx2, true),
                    child: Text(tr('Ghi đè', 'Overwrite'))),
              ],
            ),
          );
          if (ok == true && ctx.mounted) {
            await save(ctx, setDlg, overwrite: true);
            return;
          }
        } else {
          // Validation error — keep the editor open.
          _wfSnack(tr('Lưu thất bại: $msg', 'Save failed: $msg'));
        }
      }
    }

    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setDlg) => Dialog(
          insetPadding: const EdgeInsets.all(12),
          backgroundColor: c.surface,
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text(tr('Xem lại bản nháp', 'Review the draft'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 16,
                        fontWeight: FontWeight.w700)),
                const SizedBox(height: 4),
                Text(
                    tr('Sửa nếu cần rồi Lưu (được kiểm tra DAG/persona khi lưu). Huỷ sẽ bỏ bản nháp.',
                        'Edit if needed, then Save (DAG/persona validated on save). Cancel discards the draft.'),
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
                const SizedBox(height: 10),
                SizedBox(
                  height: MediaQuery.of(ctx).size.height * 0.55,
                  child: TextField(
                    controller: ctrl,
                    maxLines: null,
                    expands: true,
                    enabled: !busy,
                    textAlignVertical: TextAlignVertical.top,
                    style: TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12,
                        color: c.textPrimary),
                    decoration:
                        const InputDecoration(border: OutlineInputBorder()),
                  ),
                ),
                const SizedBox(height: 10),
                Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    TextButton(
                      onPressed: busy ? null : () => Navigator.pop(ctx),
                      child: Text(tr('Huỷ', 'Cancel')),
                    ),
                    const SizedBox(width: 8),
                    FilledButton(
                      onPressed: busy ? null : () => save(ctx, setDlg),
                      child: Text(busy
                          ? tr('Đang lưu…', 'Saving…')
                          : tr('Lưu workflow', 'Save workflow')),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  void _wfSnack(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  // ── Restricted project picker ──────────────────────────────────────────────

  Future<void> _addProject(String path) async {
    final prefs = ref.read(prefsHelperProvider);
    final list = prefs.stringList(_kProjects);
    if (!list.contains(path)) {
      list.insert(0, path);
      await prefs.setStringList(_kProjects, list);
    }
  }

  Future<void> _pickProject() async {
    final c = context.colors;
    final picked = await showModalBottomSheet<String>(
      context: context,
      backgroundColor: c.surface,
      shape: const RoundedRectangleBorder(
        borderRadius:
            BorderRadius.vertical(top: Radius.circular(AppTokens.rXl)),
      ),
      builder: (sheetCtx) => StatefulBuilder(
        builder: (sheetCtx, setSheet) {
          final prefs = ref.read(prefsHelperProvider);
          final projects = prefs.stringList(_kProjects);
          return SafeArea(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const SizedBox(height: 10),
                Container(
                  width: 40,
                  height: 4,
                  decoration: BoxDecoration(
                      color: c.borderStrong,
                      borderRadius: BorderRadius.circular(2)),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 12, 16, 4),
                  child: Row(children: [
                    Icon(Icons.folder_special_outlined,
                        color: c.accent, size: 18),
                    const SizedBox(width: 8),
                    Text(tr('Project cho phép', 'Allowed projects'),
                        style: TextStyle(
                            color: c.textPrimary, fontWeight: FontWeight.w600)),
                  ]),
                ),
                if (projects.isEmpty)
                  Padding(
                    padding:
                        const EdgeInsets.symmetric(vertical: 16, horizontal: 16),
                    child: Text(
                        tr('Chưa có project nào — thêm một thư mục để bắt đầu.',
                            'No projects yet — add a folder to get started.'),
                        style: TextStyle(color: c.textMuted, fontSize: 13)),
                  )
                else
                  Flexible(
                    child: ListView(
                      shrinkWrap: true,
                      children: [
                        for (final p in projects)
                          ListTile(
                            leading: Icon(Icons.folder_outlined,
                                color: AppTokens.warning),
                            title: Text(
                                p.split('/').where((s) => s.isNotEmpty).last,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(color: c.textPrimary)),
                            subtitle: Text(p,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style:
                                    TextStyle(color: c.textMuted, fontSize: 11)),
                            trailing: IconButton(
                              tooltip: tr('Bỏ ghim', 'Unpin'),
                              icon: Icon(Icons.close,
                                  size: 16, color: c.textMuted),
                              onPressed: () async {
                                final list = prefs.stringList(_kProjects)
                                  ..remove(p);
                                await prefs.setStringList(_kProjects, list);
                                setSheet(() {});
                              },
                            ),
                            onTap: () => Navigator.of(sheetCtx).pop(p),
                          ),
                      ],
                    ),
                  ),
                Divider(height: 1, color: c.border),
                ListTile(
                  leading: Icon(Icons.add, color: c.accent),
                  title: Text(tr('Thêm project…', 'Add project…'),
                      style: TextStyle(color: c.textPrimary)),
                  subtitle: Text(
                      tr('Duyệt thư mục một lần rồi ghim vào danh sách',
                          'Browse for a folder once, then pin it to the list'),
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                  onTap: () async {
                    final path = await FolderPicker.show(sheetCtx);
                    if (path != null && path.isNotEmpty) {
                      await _addProject(path);
                      if (sheetCtx.mounted) Navigator.of(sheetCtx).pop(path);
                    }
                  },
                ),
                const SizedBox(height: 8),
              ],
            ),
          );
        },
      ),
    );
    if (picked != null && picked.isNotEmpty) {
      setState(() => _workDir = picked);
    } else {
      setState(() {}); // a pin may have been removed
    }
  }

  // ── Submit ─────────────────────────────────────────────────────────────────

  Future<void> _submit() async {
    if (_kind == 'chat') {
      final folder = _agentFolder ??
          (widget.agents.isNotEmpty ? widget.agents.first.folder : null);
      if (folder == null) return;
      setState(() => _creating = true);
      await _applyModel();
      if (!mounted) return;
      Navigator.of(context).pop(NewChatResult(folder, _msg.text.trim(), _chatType));
      return;
    }

    if (_kind == 'code') {
      final ws = _workDir;
      if (ws == null) return;
      setState(() => _creating = true);
      try {
        await _applyModel();
        final name = _msg.text.trim().isNotEmpty
            ? _msg.text.trim()
            : ws.split('/').where((s) => s.isNotEmpty).last;
        final session = await CodeApi().createSession(
            name: name, workspace: ws, language: '', initGit: false);
        if (!mounted) return;
        final nav = Navigator.of(context);
        nav.pop();
        nav.push(MaterialPageRoute(
            builder: (_) => CodeSessionScreen(session: session)));
      } catch (e) {
        if (mounted) {
          setState(() => _creating = false);
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(
                  content: Text(tr('Lỗi tạo session: $e',
                      'Failed to create session: $e'))));
        }
      }
      return;
    }

    // cowork
    final tmpl = _effectiveTemplate;
    if (tmpl == null) return;
    setState(() => _creating = true);
    try {
      final name = _msg.text.trim().isNotEmpty ? _msg.text.trim() : null;
      final team = await CoworkApi()
          .createFromTemplate(tmpl, name: name, workspaceDir: _workDir);
      if (!mounted) return;
      final nav = Navigator.of(context);
      nav.pop();
      nav.push(MaterialPageRoute(builder: (_) => CoworkTeamScreen(team: team)));
    } catch (e) {
      if (mounted) {
        setState(() => _creating = false);
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(
                content:
                    Text(tr('Lỗi tạo team: $e', 'Failed to create team: $e'))));
      }
    }
  }

  // ── Build ──────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final g = _greeting();
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Text(tr('Tạo mới', 'New'),
            style: TextStyle(color: c.textPrimary)),
      ),
      body: Center(
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(20),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 640),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Center(
                  child: _KindSegmented(
                    kind: _kind,
                    onChanged: (k) => setState(() {
                      _kind = k;
                      if (k == 'chat') _workDir = null;
                      if (k == 'workflow' && !_wfLoaded) _loadWorkflows();
                    }),
                  ),
                ),
                const SizedBox(height: 20),
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
                    const SizedBox(height: 12),
                    Text(g.heading,
                        textAlign: TextAlign.center,
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 20,
                            fontWeight: FontWeight.w700)),
                    if (g.sub.isNotEmpty) ...[
                      const SizedBox(height: 4),
                      Text(g.sub,
                          textAlign: TextAlign.center,
                          style: TextStyle(color: c.textMuted, fontSize: 13)),
                    ],
                  ],
                ),
                const SizedBox(height: 20),
                // Workflow kind swaps the composer for the quick-start panel.
                if (_kind == 'workflow') ...[
                  _workflowQuickStart(c),
                ] else ...[
                // Unified input card.
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
                    children: [
                      Padding(
                        padding: const EdgeInsets.fromLTRB(16, 12, 16, 0),
                        child: TextField(
                          controller: _msg,
                          autofocus: _kind == 'chat',
                          minLines: 3,
                          maxLines: 8,
                          onChanged: (_) => setState(() {}),
                          style: TextStyle(color: c.textPrimary),
                          decoration: InputDecoration(
                            hintText: _hint,
                            hintStyle: TextStyle(color: c.textMuted),
                            border: InputBorder.none,
                            isCollapsed: true,
                          ),
                        ),
                      ),
                      Divider(height: 16, color: c.border),
                      Padding(
                        padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
                        child: _toolbar(c),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 16),
                Wrap(
                  alignment: WrapAlignment.center,
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    for (final s in _suggestions)
                      _SuggestionChip(
                          text: s,
                          onTap: () => setState(() => _msg.text = s)),
                  ],
                ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }

  /// Workflow quick-start panel (replaces the composer for the workflow kind):
  /// run a saved workflow, or describe one and let the AI agent author it.
  Widget _workflowQuickStart(AppColors c) {
    WorkflowDefSummary? selected;
    for (final d in _wfDefs) {
      if (d.name == _wfSelected) {
        selected = d;
        break;
      }
    }

    BoxDecoration card() => BoxDecoration(
          color: c.surface,
          borderRadius: BorderRadius.circular(AppTokens.rXl),
          border: Border.all(color: c.border),
        );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        // ── Pick & run an existing workflow ──
        Container(
          padding: const EdgeInsets.all(16),
          decoration: card(),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Icon(Icons.account_tree_outlined, size: 16, color: c.accent),
                const SizedBox(width: 6),
                Text(tr('Chạy workflow có sẵn', 'Run a saved workflow'),
                    style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w600,
                        fontSize: 14)),
              ]),
              const SizedBox(height: 8),
              if (!_wfLoaded)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 8),
                  child: Text(tr('Đang tải danh sách…', 'Loading list…'),
                      style: TextStyle(color: c.textMuted, fontSize: 13)),
                )
              else
                DropdownButtonHideUnderline(
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 8),
                    decoration: BoxDecoration(
                      color: c.surfaceAlt,
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                      border: Border.all(color: c.border),
                    ),
                    child: DropdownButton<String>(
                      value: _wfDefs.any((d) => d.name == _wfSelected)
                          ? _wfSelected
                          : null,
                      isExpanded: true,
                      hint: Text(
                          _wfDefs.isEmpty
                              ? tr('Chưa có workflow — tạo mới bên dưới',
                                  'No workflows yet — create one below')
                              : tr('Chọn workflow…', 'Select a workflow…'),
                          style: TextStyle(color: c.textMuted, fontSize: 13)),
                      items: [
                        for (final d in _wfDefs)
                          DropdownMenuItem(
                            value: d.name,
                            child: Text(
                              tr('${d.name} · ${d.stepCount} bước${(d.description ?? '').isEmpty ? '' : ' — ${d.description}'}',
                                  '${d.name} · ${d.stepCount} steps${(d.description ?? '').isEmpty ? '' : ' — ${d.description}'}'),
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                  color: c.textPrimary, fontSize: 13),
                            ),
                          ),
                      ],
                      onChanged: (v) {
                        if (v != null) _wfPick(v);
                      },
                    ),
                  ),
                ),
              if (selected != null) ...[
                const SizedBox(height: 12),
                for (final i in selected.inputs)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8),
                    child: TextField(
                      controller: _wfInputCtrls[i.name],
                      style: TextStyle(color: c.textPrimary, fontSize: 14),
                      decoration: InputDecoration(
                        labelText: i.required ? '${i.name} *' : i.name,
                        helperText: i.description,
                        border: const OutlineInputBorder(),
                        isDense: true,
                      ),
                    ),
                  ),
                FilledButton.icon(
                  onPressed: _wfStarting ? null : () => _wfRun(selected!),
                  icon: _wfStarting
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.play_arrow_rounded, size: 18),
                  label: Text(_wfStarting
                      ? tr('Đang khởi chạy…', 'Starting…')
                      : tr('Chạy workflow', 'Run workflow')),
                ),
              ],
            ],
          ),
        ),
        const SizedBox(height: 8),
        Center(
            child:
                Text(tr('hoặc', 'or'),
                    style: TextStyle(color: c.textMuted, fontSize: 12))),
        const SizedBox(height: 8),
        // ── Create a new one with the AI agent ──
        Container(
          padding: const EdgeInsets.all(16),
          decoration: card(),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(children: [
                Icon(Icons.auto_awesome, size: 16, color: c.accent),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                      tr('Tạo workflow mới bằng AI agent',
                          'Create a new workflow with an AI agent'),
                      style: TextStyle(
                          color: c.textPrimary,
                          fontWeight: FontWeight.w600,
                          fontSize: 14)),
                ),
              ]),
              const SizedBox(height: 8),
              TextField(
                controller: _wfDesc,
                maxLines: 4,
                enabled: !_wfDrafting,
                style: TextStyle(color: c.textPrimary, fontSize: 14),
                decoration: InputDecoration(
                  border: const OutlineInputBorder(),
                  hintText: tr(
                      'Mô tả quy trình… vd: Hàng tuần nghiên cứu một chủ đề từ 3 góc nhìn song song, rồi tổng hợp thành một báo cáo.',
                      'Describe the workflow… e.g.: Every week research a topic from 3 angles in parallel, then synthesize into one report.'),
                  hintStyle: TextStyle(color: c.textMuted, fontSize: 13),
                ),
              ),
              const SizedBox(height: 8),
              OutlinedButton.icon(
                onPressed: _wfDrafting ? null : _wfCreateWithAi,
                icon: _wfDrafting
                    ? const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.auto_awesome, size: 16),
                label: Text(_wfDrafting
                    ? tr('Agent đang soạn (30–120s)…',
                        'Agent is drafting (30–120s)…')
                    : tr('Tạo workflow', 'Create workflow')),
              ),
              const SizedBox(height: 6),
              Text(
                tr('Bản nháp sẽ mở trong trình soạn để xem lại — Lưu để giữ, Huỷ để bỏ.',
                    'The draft opens in the editor for review — Save to keep, Cancel to discard.'),
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textMuted, fontSize: 11),
              ),
            ],
          ),
        ),
      ],
    );
  }

  ({String heading, String sub}) _greeting() {
    if (_kind == 'code') {
      String? base;
      if (_workDir != null) {
        final parts = _workDir!.split('/').where((s) => s.isNotEmpty).toList();
        base = parts.isEmpty ? null : parts.last;
      }
      return base != null
          ? (heading: tr('Xây gì trong $base?', 'What to build in $base?'), sub: '')
          : (
              heading: tr('Chọn project để bắt đầu', 'Pick a project to start'),
              sub: tr('Chọn thư mục dự án bên dưới.',
                  'Pick a project folder below.')
            );
    }
    if (_kind == 'cowork') {
      return (
        heading: tr('Tạo nhóm Cowork', 'Create a Cowork team'),
        sub: tr('Chọn mẫu, rồi mô tả mục tiêu.',
            'Pick a template, then describe the goal.')
      );
    }
    if (_kind == 'workflow') {
      return (
        heading: tr('Chạy một workflow', 'Run a workflow'),
        sub: tr('Chọn workflow có sẵn, hoặc để AI agent soạn quy trình mới.',
            'Pick a saved workflow, or let an AI agent draft a new one.')
      );
    }
    final name = _agentFolder == null
        ? null
        : widget.agents
            .where((a) => a.folder == _agentFolder)
            .map((a) => a.name)
            .firstOrNull;
    return name != null
        ? (
            heading: tr('Chat với $name', 'Chat with $name'),
            sub: tr('Không cần workspace — chỉ trò chuyện.',
                'No workspace needed — just chat.')
          )
        : (heading: tr('Mình giúp gì hôm nay?', 'How can I help today?'), sub: '');
  }

  String get _hint => switch (_kind) {
        'code' => tr('Tên session (tuỳ chọn)…', 'Session name (optional)…'),
        'cowork' => tr('Mục tiêu nhóm (tuỳ chọn)…', 'Team goal (optional)…'),
        _ => tr('Hỏi bất cứ điều gì, hoặc mô tả một tác vụ…',
            'Ask anything, or describe a task…'),
      };

  Widget _toolbar(AppColors c) {
    final send =
        _SendButton(enabled: _canSubmit, creating: _creating, onTap: _submit);
    switch (_kind) {
      case 'cowork':
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            _templateDropdown(c),
            const SizedBox(height: 8),
            Row(children: [
              _ProjectPill(workDir: _workDir, onTap: _pickProject),
              const Spacer(),
              send,
            ]),
          ],
        );
      case 'code':
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(children: [
              _ProjectPill(workDir: _workDir, onTap: _pickProject),
              const Spacer(),
            ]),
            const SizedBox(height: 8),
            Row(children: [
              Expanded(child: _modelDropdown(c)),
              const SizedBox(width: 8),
              send,
            ]),
          ],
        );
      default: // chat — profile + model (like desktop)
        return Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(children: [
              Expanded(child: _agentDropdown(c)),
              const SizedBox(width: 8),
              Expanded(child: _modelDropdown(c)),
            ]),
            const SizedBox(height: 8),
            Row(children: [
              _ModeIcons(
                  value: _chatType,
                  onChanged: (m) => setState(() => _chatType = m)),
              const Spacer(),
              send,
            ]),
          ],
        );
    }
  }

  Widget _modelDropdown(AppColors c) {
    final activeLabel = _models
        .where((m) => m.id == _activeModelId)
        .map((m) => m.label)
        .firstOrNull;
    return DropdownButtonHideUnderline(
      child: DropdownButton<String?>(
        value: _modelId,
        isExpanded: true,
        isDense: true,
        icon: Icon(Icons.expand_more, color: c.textMuted, size: 18),
        style: TextStyle(color: c.textPrimary, fontSize: 14),
        items: [
          DropdownMenuItem<String?>(
            value: null,
            child: Row(children: [
              Icon(Icons.memory, size: 15, color: c.accent),
              const SizedBox(width: 8),
              Flexible(
                child: Text(
                    activeLabel != null
                        ? tr('Mặc định · $activeLabel',
                            'Default · $activeLabel')
                        : tr('Model mặc định', 'Default model'),
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textSecondary)),
              ),
            ]),
          ),
          for (final m in _models)
            DropdownMenuItem<String?>(
              value: m.id,
              child: Row(children: [
                Icon(Icons.memory, size: 15, color: c.accent),
                const SizedBox(width: 8),
                Flexible(
                    child: Text(m.label, overflow: TextOverflow.ellipsis)),
              ]),
            ),
        ],
        onChanged: (v) => setState(() => _modelId = v),
      ),
    );
  }

  Widget _agentDropdown(AppColors c) {
    if (widget.agents.isEmpty) {
      return Padding(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 8),
        child: Text(
            tr('Chưa có profile nào trên kênh này',
                'No profiles on this channel yet'),
            style: TextStyle(color: c.textMuted, fontSize: 13)),
      );
    }
    return DropdownButtonHideUnderline(
      child: DropdownButton<String>(
        value: _agentFolder,
        isExpanded: true,
        isDense: true,
        icon: Icon(Icons.expand_more, color: c.textMuted, size: 18),
        style: TextStyle(color: c.textPrimary, fontSize: 14),
        items: [
          for (final a in widget.agents)
            DropdownMenuItem(
                value: a.folder,
                child: Row(children: [
                  Icon(Icons.person_outline, size: 15, color: c.accent),
                  const SizedBox(width: 8),
                  Flexible(
                      child: Text(a.name, overflow: TextOverflow.ellipsis)),
                ])),
        ],
        onChanged: (v) => setState(() => _agentFolder = v),
      ),
    );
  }

  Widget _templateDropdown(AppColors c) {
    if (_templates.isEmpty) {
      return Text(tr('Đang tải mẫu…', 'Loading templates…'),
          style: TextStyle(color: c.textMuted, fontSize: 13));
    }
    return DropdownButtonHideUnderline(
      child: DropdownButton<String>(
        value: _effectiveTemplate,
        isExpanded: true,
        isDense: true,
        icon: Icon(Icons.expand_more, color: c.textMuted, size: 18),
        style: TextStyle(color: c.textPrimary, fontSize: 14),
        items: [
          for (final t in _templates)
            DropdownMenuItem(
                value: t.id,
                child: Text('${t.icon} ${t.name}',
                    overflow: TextOverflow.ellipsis)),
        ],
        onChanged: (v) => setState(() => _templateId = v),
      ),
    );
  }
}

/// 💬 Chat / ⌨️ Code / 👥 Cowork kind selector.
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
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 7),
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
    // FittedBox: four segments can exceed narrow phone widths — scale down
    // instead of overflowing.
    return FittedBox(
      fit: BoxFit.scaleDown,
      child: Container(
        padding: const EdgeInsets.all(3),
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(AppTokens.rXl),
          border: Border.all(color: c.border),
        ),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          seg(tr('💬 Chat', '💬 Chat'), 'chat'),
          seg(tr('⌨️ Code', '⌨️ Code'), 'code'),
          seg(tr('👥 Cowork', '👥 Cowork'), 'cowork'),
          seg(tr('🔁 Flow', '🔁 Flow'), 'workflow'),
        ]),
      ),
    );
  }
}

/// Agent / Plan / DAG mode selector (desktop ⚡/💡/🔀 icons).
class _ModeIcons extends StatelessWidget {
  const _ModeIcons({required this.value, required this.onChanged});
  final String value;
  final void Function(String) onChanged;

  static List<(String, IconData, String)> get _opts => [
        (
          'Agent',
          Icons.bolt,
          tr('Agent — toàn quyền dùng công cụ', 'Agent — full tool access')
        ),
        (
          'Plan',
          Icons.lightbulb_outline,
          tr('Plan — nghiên cứu rồi đề xuất', 'Plan — research then propose')
        ),
        (
          'Dag',
          Icons.account_tree_outlined,
          tr('DAG — điều phối đa agent', 'DAG — multi-agent orchestration')
        ),
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
            message: tip,
            child: GestureDetector(
              onTap: () => onChanged(val),
              child: Container(
                padding:
                    const EdgeInsets.symmetric(horizontal: 8, vertical: 5),
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

/// Round send button (desktop Start arrow).
class _SendButton extends StatelessWidget {
  const _SendButton(
      {required this.enabled, required this.creating, required this.onTap});
  final bool enabled;
  final bool creating;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return GestureDetector(
      onTap: enabled ? onTap : null,
      child: Container(
        width: 36,
        height: 36,
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: enabled ? c.accent : c.surfaceAlt,
          shape: BoxShape.circle,
        ),
        child: creating
            ? const SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(
                    strokeWidth: 2, color: Colors.white))
            : Icon(Icons.arrow_upward,
                size: 18, color: enabled ? Colors.white : c.textMuted),
      ),
    );
  }
}

/// Pill that shows the chosen project folder (or prompts to pick one).
class _ProjectPill extends StatelessWidget {
  const _ProjectPill({required this.workDir, required this.onTap});
  final String? workDir;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final has = workDir != null;
    final name = has
        ? (workDir!.split('/').where((s) => s.isNotEmpty).isEmpty
            ? 'Project'
            : workDir!.split('/').where((s) => s.isNotEmpty).last)
        : tr('Chọn project', 'Select project');
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
        decoration: BoxDecoration(
          color: has ? c.accentSoft : Colors.transparent,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(color: has ? c.accent : c.border),
        ),
        child: Row(mainAxisSize: MainAxisSize.min, children: [
          Icon(Icons.folder_open_outlined,
              size: 15, color: has ? c.accent : c.textMuted),
          const SizedBox(width: 6),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 140),
            child: Text(name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                    color: has ? c.accent : c.textSecondary, fontSize: 13)),
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
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
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
