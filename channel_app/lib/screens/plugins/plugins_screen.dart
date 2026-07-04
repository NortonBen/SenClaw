import 'dart:async';
import 'package:flutter/material.dart';
import '../../models/plugin_models.dart';
import '../../services/language_service.dart';
import '../../services/plugins_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../widgets/states.dart';

/// Plugins hub mirroring the web Plugins panels: Skills, Subagents, MCP,
/// Marketplace, Hooks.
class PluginsScreen extends StatefulWidget {
  const PluginsScreen({super.key});

  @override
  State<PluginsScreen> createState() => _PluginsScreenState();
}

class _PluginsScreenState extends State<PluginsScreen>
    with SingleTickerProviderStateMixin {
  final _api = PluginsApi();
  late final TabController _tabs = TabController(length: 6, vsync: this);

  @override
  void dispose() {
    _tabs.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        title: Row(
          children: [
            Text('Plugins', style: TextStyle(color: c.textPrimary)),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        bottom: TabBar(
          controller: _tabs,
          isScrollable: true,
          tabAlignment: TabAlignment.start,
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: [
            const Tab(text: 'Skills'),
            const Tab(text: 'Subagents'),
            const Tab(text: 'Plugins'),
            const Tab(text: 'MCP'),
            Tab(text: tr('Chợ', 'Marketplace')),
            const Tab(text: 'Hooks'),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: [
            _SkillsTab(api: _api),
            _SubagentsTab(api: _api),
            _PluginsTab(api: _api),
            _McpTab(api: _api),
            _MarketplaceTab(api: _api),
            _HooksTab(api: _api),
          ],
        ),
      ),
    );
  }
}

void _toast(BuildContext ctx, String m) {
  ScaffoldMessenger.of(ctx).showSnackBar(SnackBar(content: Text(m)));
}

// ─── Skills ──────────────────────────────────────────────────────────────────

class _SkillsTab extends StatefulWidget {
  final PluginsApi api;
  const _SkillsTab({required this.api});

  @override
  State<_SkillsTab> createState() => _SkillsTabState();
}

class _SkillsTabState extends State<_SkillsTab>
    with AutomaticKeepAliveClientMixin {
  List<LocalSkill> _skills = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = _skills.isEmpty;
      _error = null;
    });
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_skills.isEmpty) {
      unawaited(widget.api.listSkillsCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted || _skills.isNotEmpty) return;
        setState(() {
          _skills = cached;
          _loading = false;
          _error = null;
        });
      }));
    }
    try {
      final s = await widget.api.listSkills();
      fresh = true;
      if (!mounted) return;
      setState(() {
        _skills = s;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _skills.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  Future<void> _toggle(LocalSkill s) async {
    try {
      await widget.api.toggleSkill(s.name, s.disabled);
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _delete(LocalSkill s) async {
    try {
      await widget.api.deleteSkill(s.name);
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _openSearch() async {
    await showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _ClawHubSearchSheet(api: widget.api),
    );
    _load();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _openSearch,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        icon: const Icon(Icons.search),
        label: const Text('ClawHub'),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_skills.isEmpty) {
      return EmptyState(
        icon: Icons.extension_outlined,
        message: tr('Chưa có skill', 'No skills yet'),
        hint: tr('Cài đặt skill từ ClawHub', 'Install skills from ClawHub'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _skills.length,
        itemBuilder: (ctx, i) {
          final s = _skills[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              title: Text(s.name,
                  style: TextStyle(
                      color: s.disabled ? c.textMuted : c.textPrimary,
                      fontWeight: FontWeight.w600)),
              subtitle: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (s.description.isNotEmpty)
                    Text(s.description,
                        style: TextStyle(
                            color: c.textSecondary, fontSize: 12),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis),
                  Text('${s.source}${s.version.isNotEmpty ? ' · v${s.version}' : ''}',
                      style: TextStyle(
                          color: c.textMuted, fontSize: 11)),
                ],
              ),
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Switch(
                    value: !s.disabled,
                    onChanged: (_) => _toggle(s),
                    activeThumbColor: c.accent,
                  ),
                  if (s.source.contains('clawhub'))
                    IconButton(
                      icon: Icon(Icons.delete_outline,
                          color: c.textMuted, size: 20),
                      onPressed: () => _delete(s),
                    ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}

class _ClawHubSearchSheet extends StatefulWidget {
  final PluginsApi api;
  const _ClawHubSearchSheet({required this.api});

  @override
  State<_ClawHubSearchSheet> createState() => _ClawHubSearchSheetState();
}

class _ClawHubSearchSheetState extends State<_ClawHubSearchSheet> {
  final _ctrl = TextEditingController();
  List<RemoteSkill> _results = [];
  bool _loading = false;
  final Set<String> _installing = {};

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _search() async {
    if (_ctrl.text.trim().isEmpty) return;
    setState(() => _loading = true);
    try {
      final r = await widget.api.searchSkills(_ctrl.text.trim());
      if (mounted) setState(() => _results = r);
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _install(RemoteSkill s) async {
    setState(() => _installing.add(s.slug));
    try {
      await widget.api.installSkill(s.slug);
      if (mounted) {
        _toast(context, tr('Đã cài ${s.slug}', 'Installed ${s.slug}'));
        setState(() => _results = _results
            .map((r) => r.slug == s.slug
                ? RemoteSkill(
                    slug: r.slug,
                    displayName: r.displayName,
                    summary: r.summary,
                    version: r.version,
                    score: r.score,
                    installed: true)
                : r)
            .toList());
      }
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    } finally {
      if (mounted) setState(() => _installing.remove(s.slug));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: EdgeInsets.fromLTRB(
          16, 16, 16, MediaQuery.of(context).viewInsets.bottom + 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(tr('Tìm skill trên ClawHub', 'Search skills on ClawHub'),
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 16,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 12),
          TextField(
            controller: _ctrl,
            style: TextStyle(color: c.textPrimary),
            textInputAction: TextInputAction.search,
            onSubmitted: (_) => _search(),
            decoration: InputDecoration(
              hintText: tr('Từ khoá…', 'Keywords…'),
              hintStyle: TextStyle(color: c.textMuted),
              suffixIcon: IconButton(
                icon: Icon(Icons.search, color: c.accent),
                onPressed: _search,
              ),
              filled: true,
              fillColor: c.surfaceAlt,
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(10),
                borderSide: BorderSide(color: c.border),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(10),
                borderSide: BorderSide(color: c.border),
              ),
            ),
          ),
          const SizedBox(height: 10),
          if (_loading)
            Padding(
              padding: const EdgeInsets.all(20),
              child: CircularProgressIndicator(color: c.accent),
            )
          else
            SizedBox(
              height: 320,
              child: ListView.builder(
                itemCount: _results.length,
                itemBuilder: (ctx, i) {
                  final s = _results[i];
                  final busy = _installing.contains(s.slug);
                  return ListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(
                        s.displayName.isEmpty ? s.slug : s.displayName,
                        style: TextStyle(color: c.textPrimary)),
                    subtitle: Text(s.summary,
                        style: TextStyle(
                            color: c.textSecondary, fontSize: 12),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis),
                    trailing: s.installed
                        ? Text(tr('Đã cài', 'Installed'),
                            style: TextStyle(
                                color: AppTokens.success, fontSize: 12))
                        : busy
                            ? SizedBox(
                                width: 18,
                                height: 18,
                                child: CircularProgressIndicator(
                                    strokeWidth: 2, color: c.accent))
                            : TextButton(
                                onPressed: () => _install(s),
                                child: Text(tr('Cài', 'Install'),
                                    style:
                                        TextStyle(color: c.accent))),
                  );
                },
              ),
            ),
        ],
      ),
    );
  }
}

// ─── Subagents ───────────────────────────────────────────────────────────────

class _SubagentsTab extends StatefulWidget {
  final PluginsApi api;
  const _SubagentsTab({required this.api});

  @override
  State<_SubagentsTab> createState() => _SubagentsTabState();
}

class _SubagentsTabState extends State<_SubagentsTab>
    with AutomaticKeepAliveClientMixin {
  List<Subagent> _subs = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = _subs.isEmpty;
      _error = null;
    });
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_subs.isEmpty) {
      unawaited(widget.api.listSubagentsCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted || _subs.isNotEmpty) return;
        setState(() {
          _subs = cached;
          _loading = false;
          _error = null;
        });
      }));
    }
    try {
      final s = await widget.api.listSubagents();
      fresh = true;
      if (!mounted) return;
      setState(() {
        _subs = s;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _subs.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  Future<void> _toggle(Subagent s) async {
    try {
      await widget.api.toggleSubagent(s.name, s.disabled);
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _create() async {
    final saved = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _SubagentEditor(api: widget.api),
    );
    if (saved == true) _load();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: _create,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_subs.isEmpty) {
      return EmptyState(
        icon: Icons.support_agent_outlined,
        message: tr('Chưa có subagent', 'No subagents yet'),
        hint: tr('Nhấn + để tạo persona mới', 'Tap + to create a new persona'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _subs.length,
        itemBuilder: (ctx, i) {
          final s = _subs[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              title: Text(s.name,
                  style: TextStyle(
                      color: s.disabled ? c.textMuted : c.textPrimary,
                      fontWeight: FontWeight.w600)),
              subtitle: Text(s.description,
                  style: TextStyle(color: c.textSecondary, fontSize: 12),
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis),
              trailing: Switch(
                value: !s.disabled,
                onChanged: (_) => _toggle(s),
                activeThumbColor: c.accent,
              ),
            ),
          );
        },
      ),
    );
  }
}

class _SubagentEditor extends StatefulWidget {
  final PluginsApi api;
  const _SubagentEditor({required this.api});

  @override
  State<_SubagentEditor> createState() => _SubagentEditorState();
}

class _SubagentEditorState extends State<_SubagentEditor> {
  final _name = TextEditingController();
  final _content = TextEditingController(
      text: '---\nname: \ndescription: \n---\n\n');
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _name.dispose();
    _content.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_name.text.trim().isEmpty) {
      setState(() => _error = tr('Cần tên', 'Name required'));
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await widget.api.createSubagent(_name.text.trim(), _content.text);
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _saving = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: EdgeInsets.fromLTRB(
          16, 16, 16, MediaQuery.of(context).viewInsets.bottom + 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(tr('Subagent mới', 'New subagent'),
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 16,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 12),
          TextField(
            controller: _name,
            style: TextStyle(color: c.textPrimary),
            decoration: _dec(context, tr('Tên (kebab-case)', 'Name (kebab-case)')),
          ),
          const SizedBox(height: 10),
          TextField(
            controller: _content,
            maxLines: 10,
            style: TextStyle(
                color: c.textPrimary, fontFamily: 'monospace', fontSize: 12),
            decoration: _dec(
                context,
                tr('Nội dung persona (markdown + frontmatter)',
                    'Persona content (markdown + frontmatter)')),
          ),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text(_error!,
                style: TextStyle(color: AppTokens.danger, fontSize: 12)),
          ],
          const SizedBox(height: 14),
          SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _save,
              style: ElevatedButton.styleFrom(
                backgroundColor: c.accent,
                foregroundColor: Colors.white,
                padding: const EdgeInsets.symmetric(vertical: 14),
              ),
              child: _saving
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.white))
                  : Text(tr('Tạo', 'Create')),
            ),
          ),
        ],
      ),
    );
  }

  InputDecoration _dec(BuildContext context, String hint) {
    final c = context.colors;
    return InputDecoration(
      hintText: hint,
      hintStyle: TextStyle(color: c.textMuted),
      filled: true,
      fillColor: c.surfaceAlt,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(10),
        borderSide: BorderSide(color: c.border),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(10),
        borderSide: BorderSide(color: c.border),
      ),
    );
  }
}

// ─── Plugins (packages) ──────────────────────────────────────────────────────

class _PluginsTab extends StatefulWidget {
  final PluginsApi api;
  const _PluginsTab({required this.api});

  @override
  State<_PluginsTab> createState() => _PluginsTabState();
}

class _PluginsTabState extends State<_PluginsTab>
    with AutomaticKeepAliveClientMixin {
  List<Plugin> _plugins = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = _plugins.isEmpty;
      _error = null;
    });
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_plugins.isEmpty) {
      unawaited(widget.api.listPluginsCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted) return;
        if (_plugins.isNotEmpty) return;
        setState(() {
          _plugins = cached;
          _loading = false;
          _error = null;
        });
      }));
    }
    try {
      final p = await widget.api.listPlugins();
      fresh = true;
      if (!mounted) return;
      setState(() {
        _plugins = p;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _plugins.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  Future<void> _toggle(Plugin p) async {
    try {
      await widget.api.togglePlugin(p.slug, !p.enabled);
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _delete(Plugin p) async {
    final c = context.colors;
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: c.surface,
        title: Text(tr('Gỡ plugin?', 'Remove plugin?'),
            style: TextStyle(color: c.textPrimary)),
        content: Text(p.displayName.isEmpty ? p.slug : p.displayName,
            style: TextStyle(color: c.textSecondary)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(tr('Huỷ', 'Cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(tr('Gỡ', 'Remove'),
                  style: TextStyle(color: AppTokens.danger))),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await widget.api.deletePlugin(p.slug);
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _openInstall() async {
    await showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _PluginInstallSheet(api: widget.api),
    );
    _load();
  }

  Color _statusColor(String s) {
    switch (s) {
      case 'running':
        return AppTokens.success;
      case 'error':
        return AppTokens.danger;
      default:
        return context.colors.textMuted;
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _openInstall,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        icon: const Icon(Icons.cloud_download_outlined),
        label: Text(tr('Cài plugin', 'Install plugin')),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_plugins.isEmpty) {
      return EmptyState(
        icon: Icons.widgets_outlined,
        message: tr('Chưa cài plugin', 'No plugins installed'),
        hint: tr('Cài plugin từ ClawHub', 'Install plugins from ClawHub'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _plugins.length,
        itemBuilder: (ctx, i) {
          final p = _plugins[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              leading: Container(
                width: 10,
                height: 10,
                margin: const EdgeInsets.only(top: 6),
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: _statusColor(p.status),
                ),
              ),
              title: Text(p.displayName.isEmpty ? p.slug : p.displayName,
                  style: TextStyle(
                      color: p.enabled ? c.textPrimary : c.textMuted,
                      fontWeight: FontWeight.w600)),
              subtitle: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (p.summary.isNotEmpty)
                    Text(p.summary,
                        style: TextStyle(
                            color: c.textSecondary, fontSize: 12),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis),
                  Text(
                      '${p.pluginType}${p.version.isNotEmpty ? ' · v${p.version}' : ''} · ${p.status}',
                      style: TextStyle(
                          color: c.textMuted, fontSize: 11)),
                  if (p.errorMsg != null && p.errorMsg!.isNotEmpty)
                    Text(p.errorMsg!,
                        style: TextStyle(
                            color: AppTokens.danger, fontSize: 11)),
                ],
              ),
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Switch(
                    value: p.enabled,
                    onChanged: (_) => _toggle(p),
                    activeThumbColor: c.accent,
                  ),
                  IconButton(
                    icon: Icon(Icons.delete_outline,
                        color: c.textMuted, size: 20),
                    onPressed: () => _delete(p),
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}

class _PluginInstallSheet extends StatefulWidget {
  final PluginsApi api;
  const _PluginInstallSheet({required this.api});

  @override
  State<_PluginInstallSheet> createState() => _PluginInstallSheetState();
}

class _PluginInstallSheetState extends State<_PluginInstallSheet> {
  final _ctrl = TextEditingController();
  List<RemoteSkill> _results = [];
  bool _loading = false;
  final Set<String> _installing = {};

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _search() async {
    if (_ctrl.text.trim().isEmpty) return;
    setState(() => _loading = true);
    try {
      final r = await widget.api.searchPlugins(_ctrl.text.trim());
      if (mounted) setState(() => _results = r);
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _install(RemoteSkill s) async {
    setState(() => _installing.add(s.slug));
    try {
      await widget.api.installPlugin(s.slug);
      if (mounted) {
        _toast(context, tr('Đã cài ${s.slug}', 'Installed ${s.slug}'));
        setState(() => _results = _results
            .map((r) => r.slug == s.slug
                ? RemoteSkill(
                    slug: r.slug,
                    displayName: r.displayName,
                    summary: r.summary,
                    version: r.version,
                    score: r.score,
                    installed: true)
                : r)
            .toList());
      }
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    } finally {
      if (mounted) setState(() => _installing.remove(s.slug));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: EdgeInsets.fromLTRB(
          16, 16, 16, MediaQuery.of(context).viewInsets.bottom + 16),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(tr('Cài plugin từ ClawHub', 'Install plugins from ClawHub'),
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 16,
                  fontWeight: FontWeight.bold)),
          const SizedBox(height: 12),
          TextField(
            controller: _ctrl,
            style: TextStyle(color: c.textPrimary),
            textInputAction: TextInputAction.search,
            onSubmitted: (_) => _search(),
            decoration: InputDecoration(
              hintText: tr('Từ khoá…', 'Keywords…'),
              hintStyle: TextStyle(color: c.textMuted),
              suffixIcon: IconButton(
                icon: Icon(Icons.search, color: c.accent),
                onPressed: _search,
              ),
              filled: true,
              fillColor: c.surfaceAlt,
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(10),
                borderSide: BorderSide(color: c.border),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(10),
                borderSide: BorderSide(color: c.border),
              ),
            ),
          ),
          const SizedBox(height: 10),
          if (_loading)
            Padding(
              padding: const EdgeInsets.all(20),
              child: CircularProgressIndicator(color: c.accent),
            )
          else
            SizedBox(
              height: 320,
              child: ListView.builder(
                itemCount: _results.length,
                itemBuilder: (ctx, i) {
                  final s = _results[i];
                  final busy = _installing.contains(s.slug);
                  return ListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(s.displayName.isEmpty ? s.slug : s.displayName,
                        style: TextStyle(color: c.textPrimary)),
                    subtitle: Text(s.summary,
                        style: TextStyle(
                            color: c.textSecondary, fontSize: 12),
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis),
                    trailing: s.installed
                        ? Text(tr('Đã cài', 'Installed'),
                            style: TextStyle(
                                color: AppTokens.success, fontSize: 12))
                        : busy
                            ? SizedBox(
                                width: 18,
                                height: 18,
                                child: CircularProgressIndicator(
                                    strokeWidth: 2, color: c.accent))
                            : TextButton(
                                onPressed: () => _install(s),
                                child: Text(tr('Cài', 'Install'),
                                    style:
                                        TextStyle(color: c.accent))),
                  );
                },
              ),
            ),
        ],
      ),
    );
  }
}

// ─── MCP ─────────────────────────────────────────────────────────────────────

class _McpTab extends StatefulWidget {
  final PluginsApi api;
  const _McpTab({required this.api});

  @override
  State<_McpTab> createState() => _McpTabState();
}

class _McpTabState extends State<_McpTab> with AutomaticKeepAliveClientMixin {
  List<McpServer> _servers = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = _servers.isEmpty;
      _error = null;
    });
    var fresh = false;
    // Local-DB paint races the relay fetch in parallel — the relay result
    // always wins once it arrives.
    if (_servers.isEmpty) {
      unawaited(widget.api.listMcpCached().then((cached) {
        if (fresh || cached.isEmpty || !mounted || _servers.isNotEmpty) return;
        setState(() {
          _servers = cached;
          _loading = false;
          _error = null;
        });
      }));
    }
    try {
      final s = await widget.api.listMcp();
      fresh = true;
      if (!mounted) return;
      setState(() {
        _servers = s;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        // Keep the cached view usable when the refresh fails.
        _error = _servers.isEmpty ? '$e' : null;
        _loading = false;
      });
    }
  }

  Color _statusColor(String s) {
    switch (s) {
      case 'connected':
        return AppTokens.success;
      case 'connecting':
        return AppTokens.warning;
      case 'error':
        return AppTokens.danger;
      default:
        return context.colors.textMuted;
    }
  }

  Future<void> _action(Future<void> Function() fn) async {
    try {
      await fn();
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _add() async {
    final saved = await showModalBottomSheet<bool>(
      context: context,
      isScrollControlled: true,
      backgroundColor: context.colors.surface,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(18)),
      ),
      builder: (_) => _McpEditor(api: widget.api),
    );
    if (saved == true) _load();
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: _add,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_servers.isEmpty) {
      return EmptyState(
        icon: Icons.cable_outlined,
        message: tr('Chưa có MCP server', 'No MCP servers yet'),
        hint: tr('Nhấn + để thêm server', 'Tap + to add a server'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _servers.length,
        itemBuilder: (ctx, i) {
          final s = _servers[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ExpansionTile(
              iconColor: c.textMuted,
              collapsedIconColor: c.textMuted,
              leading: Container(
                width: 10,
                height: 10,
                margin: const EdgeInsets.only(top: 6),
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: _statusColor(s.status),
                ),
              ),
              title: Text(s.name,
                  style: TextStyle(
                      color: c.textPrimary, fontWeight: FontWeight.w600)),
              subtitle: Text(
                  '${s.transport} · ${s.status}${s.tools.isNotEmpty ? ' · ${s.tools.length} tools' : ''}',
                  style:
                      TextStyle(color: c.textMuted, fontSize: 12)),
              trailing: Switch(
                value: s.enabled,
                onChanged: (v) =>
                    _action(() => widget.api.setMcpEnabled(s.name, v, scope: s.scope)),
                activeThumbColor: c.accent,
              ),
              childrenPadding:
                  const EdgeInsets.fromLTRB(16, 0, 16, 12),
              children: [
                if (s.description.isNotEmpty)
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Text(s.description,
                        style: TextStyle(
                            color: c.textSecondary, fontSize: 12)),
                  ),
                if (s.error != null)
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Text(s.error!,
                        style: TextStyle(
                            color: AppTokens.danger, fontSize: 12)),
                  ),
                if (s.tools.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: Wrap(
                      spacing: 6,
                      runSpacing: 6,
                      children: s.tools
                          .map((t) => Container(
                                padding: const EdgeInsets.symmetric(
                                    horizontal: 7, vertical: 3),
                                decoration: BoxDecoration(
                                  color: AppTokens.cyan.withValues(alpha: 0.1),
                                  borderRadius: BorderRadius.circular(6),
                                ),
                                child: Text(t.name,
                                    style: const TextStyle(
                                        color: AppTokens.cyan, fontSize: 10)),
                              ))
                          .toList(),
                    ),
                  ),
                const SizedBox(height: 8),
                Row(
                  children: [
                    if (s.status == 'connected')
                      TextButton.icon(
                        onPressed: () =>
                            _action(() => widget.api.disconnectMcp(s.name)),
                        icon: const Icon(Icons.link_off,
                            color: AppTokens.warning, size: 16),
                        label: Text(tr('Ngắt', 'Disconnect'),
                            style: const TextStyle(color: AppTokens.warning)),
                      )
                    else
                      TextButton.icon(
                        onPressed: () =>
                            _action(() => widget.api.connectMcp(s.name)),
                        icon: const Icon(Icons.link,
                            color: AppTokens.success, size: 16),
                        label: Text(tr('Kết nối', 'Connect'),
                            style: const TextStyle(color: AppTokens.success)),
                      ),
                    const Spacer(),
                    if (!s.builtin)
                      TextButton.icon(
                        onPressed: () => _action(() =>
                            widget.api.deleteMcp(s.name, scope: s.scope)),
                        icon: Icon(Icons.delete_outline,
                            color: c.textMuted, size: 16),
                        label: Text(tr('Xoá', 'Delete'),
                            style: TextStyle(color: c.textMuted)),
                      ),
                  ],
                ),
              ],
            ),
          );
        },
      ),
    );
  }
}

class _McpEditor extends StatefulWidget {
  final PluginsApi api;
  const _McpEditor({required this.api});

  @override
  State<_McpEditor> createState() => _McpEditorState();
}

class _McpEditorState extends State<_McpEditor> {
  final _name = TextEditingController();
  final _description = TextEditingController();
  final _command = TextEditingController();
  final _args = TextEditingController();
  final _url = TextEditingController();
  String _transport = 'stdio';
  String _scope = 'user';
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _command.dispose();
    _args.dispose();
    _url.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_name.text.trim().isEmpty) {
      setState(() => _error = tr('Cần tên', 'Name required'));
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      final body = <String, dynamic>{
        'name': _name.text.trim(),
        'transport': _transport,
        'description': _description.text.trim(),
        'enabled': true,
        'scope': _scope,
      };
      if (_transport == 'stdio') {
        body['command'] = _command.text.trim();
        body['args'] = _args.text
            .split(' ')
            .map((s) => s.trim())
            .where((s) => s.isNotEmpty)
            .toList();
      } else {
        body['url'] = _url.text.trim();
      }
      await widget.api.addMcp(body);
      if (mounted) Navigator.pop(context, true);
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _saving = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final stdio = _transport == 'stdio';
    return Padding(
      padding: EdgeInsets.fromLTRB(
          16, 16, 16, MediaQuery.of(context).viewInsets.bottom + 16),
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(tr('Thêm MCP server', 'Add MCP server'),
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.bold)),
            const SizedBox(height: 12),
            _f(_name, tr('Tên server', 'Server name')),
            const SizedBox(height: 10),
            Row(
              children: [
                Expanded(
                  child: _drop(tr('Giao thức', 'Transport'), _transport,
                      const ['stdio', 'sse', 'http'],
                      (v) => setState(() => _transport = v)),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: _drop(tr('Phạm vi', 'Scope'), _scope,
                      const ['user', 'project'],
                      (v) => setState(() => _scope = v)),
                ),
              ],
            ),
            const SizedBox(height: 10),
            _f(_description, tr('Mô tả (tuỳ chọn)', 'Description (optional)')),
            const SizedBox(height: 10),
            if (stdio) ...[
              _f(_command,
                  tr('Lệnh (vd: /path/to/binary)',
                      'Command (e.g. /path/to/binary)')),
              const SizedBox(height: 10),
              _f(_args,
                  tr('Tham số (cách nhau bằng dấu cách)',
                      'Arguments (space-separated)')),
            ] else
              _f(_url, 'URL'),
            if (_error != null) ...[
              const SizedBox(height: 8),
              Text(_error!,
                  style:
                      const TextStyle(color: AppTokens.danger, fontSize: 12)),
            ],
            const SizedBox(height: 14),
            SizedBox(
              width: double.infinity,
              child: ElevatedButton(
                onPressed: _saving ? null : _save,
                style: ElevatedButton.styleFrom(
                  backgroundColor: c.accent,
                  foregroundColor: Colors.white,
                  padding: const EdgeInsets.symmetric(vertical: 14),
                ),
                child: _saving
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(
                            strokeWidth: 2, color: Colors.white))
                    : Text(tr('Thêm', 'Add')),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _f(TextEditingController ctrl, String hint) {
    final c = context.colors;
    return TextField(
      controller: ctrl,
      style: TextStyle(color: c.textPrimary),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(color: c.textMuted),
        isDense: true,
        filled: true,
        fillColor: c.surfaceAlt,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: c.border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: c.border),
        ),
      ),
    );
  }

  Widget _drop(String label, String value, List<String> opts,
      ValueChanged<String> onChanged) {
    final c = context.colors;
    return InputDecorator(
      decoration: InputDecoration(
        labelText: label,
        labelStyle: TextStyle(color: c.textMuted),
        isDense: true,
        filled: true,
        fillColor: c.surfaceAlt,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: c.border),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(color: c.border),
        ),
      ),
      child: DropdownButtonHideUnderline(
        child: DropdownButton<String>(
          value: value,
          isExpanded: true,
          dropdownColor: c.surface,
          style: TextStyle(color: c.textPrimary),
          items: opts
              .map((o) => DropdownMenuItem(value: o, child: Text(o)))
              .toList(),
          onChanged: (v) => v == null ? null : onChanged(v),
        ),
      ),
    );
  }
}

// ─── Marketplace ─────────────────────────────────────────────────────────────

class _MarketplaceTab extends StatefulWidget {
  final PluginsApi api;
  const _MarketplaceTab({required this.api});

  @override
  State<_MarketplaceTab> createState() => _MarketplaceTabState();
}

class _MarketplaceTabState extends State<_MarketplaceTab>
    with AutomaticKeepAliveClientMixin {
  List<MarketplaceSource> _sources = [];
  bool _loading = true;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final s = await widget.api.listMarketplace();
      if (!mounted) return;
      setState(() {
        _sources = s;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _act(Future<void> Function() fn, String okMsg) async {
    try {
      await fn();
      if (mounted) _toast(context, okMsg);
      _load();
    } catch (e) {
      if (mounted) _toast(context, tr('Lỗi: $e', 'Error: $e'));
    }
  }

  Future<void> _add() async {
    final c = context.colors;
    final nameCtrl = TextEditingController();
    final urlCtrl = TextEditingController();
    String type = 'git';
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setLocal) => AlertDialog(
          backgroundColor: c.surface,
          title: Text(tr('Thêm nguồn', 'Add source'),
              style: TextStyle(color: c.textPrimary)),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: nameCtrl,
                style: TextStyle(color: c.textPrimary),
                decoration: InputDecoration(
                    labelText: tr('Tên', 'Name'),
                    labelStyle: TextStyle(color: c.textSecondary)),
              ),
              const SizedBox(height: 8),
              TextField(
                controller: urlCtrl,
                style: TextStyle(color: c.textPrimary),
                decoration: InputDecoration(
                    labelText: type == 'git'
                        ? 'Git URL'
                        : tr('Đường dẫn cục bộ', 'Local path'),
                    labelStyle: TextStyle(color: c.textSecondary)),
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  for (final t in const ['git', 'local'])
                    Padding(
                      padding: const EdgeInsets.only(right: 8),
                      child: ChoiceChip(
                        label: Text(t),
                        selected: type == t,
                        onSelected: (_) => setLocal(() => type = t),
                      ),
                    ),
                ],
              ),
            ],
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx, false),
                child: Text(tr('Huỷ', 'Cancel'))),
            TextButton(
                onPressed: () => Navigator.pop(ctx, true),
                child: Text(tr('Thêm', 'Add'),
                    style: TextStyle(color: c.accent))),
          ],
        ),
      ),
    );
    if (ok != true || nameCtrl.text.trim().isEmpty) return;
    await _act(
      () => widget.api.addMarketplace({
        'name': nameCtrl.text.trim(),
        'type': type,
        if (type == 'git') 'url': urlCtrl.text.trim(),
        'local_path': type == 'local' ? urlCtrl.text.trim() : '',
        'priority': 0,
        'enabled': true,
      }),
      tr('Đã thêm nguồn', 'Source added'),
    );
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    return Scaffold(
      backgroundColor: Colors.transparent,
      floatingActionButton: FloatingActionButton(
        onPressed: _add,
        backgroundColor: c.accent,
        foregroundColor: Colors.white,
        child: const Icon(Icons.add),
      ),
      body: _buildBody(),
    );
  }

  Widget _buildBody() {
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    if (_sources.isEmpty) {
      return EmptyState(
        icon: Icons.store_outlined,
        message: tr('Chưa có nguồn', 'No sources yet'),
        hint: tr('Thêm nguồn git hoặc cục bộ', 'Add a git or local source'),
      );
    }
    return RefreshIndicator(
      onRefresh: _load,
      color: c.accent,
      backgroundColor: c.surface,
      child: ListView.builder(
        padding: const EdgeInsets.fromLTRB(12, 12, 12, 88),
        itemCount: _sources.length,
        itemBuilder: (ctx, i) {
          final s = _sources[i];
          return Card(
            color: c.surfaceAlt,
            margin: const EdgeInsets.only(bottom: 8),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(12),
              side: BorderSide(color: c.border),
            ),
            child: ListTile(
              leading: Icon(
                  s.type == 'git' ? Icons.cloud_outlined : Icons.folder_outlined,
                  color: AppTokens.cyan),
              title: Text(s.name, style: TextStyle(color: c.textPrimary)),
              subtitle: Text(s.url ?? s.localPath,
                  style: TextStyle(color: c.textMuted, fontSize: 12),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis),
              trailing: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (s.type == 'git')
                    IconButton(
                      icon: Icon(Icons.sync,
                          color: c.textSecondary, size: 20),
                      onPressed: () => _act(
                          () => widget.api.syncMarketplace(s.id),
                          tr('Đã đồng bộ', 'Synced')),
                    ),
                  IconButton(
                    icon: Icon(Icons.delete_outline,
                        color: c.textMuted, size: 20),
                    onPressed: () => _act(
                        () => widget.api.deleteMarketplace(s.id),
                        tr('Đã xoá', 'Deleted')),
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}

// ─── Hooks (raw JSON editor) ─────────────────────────────────────────────────

class _HooksTab extends StatefulWidget {
  final PluginsApi api;
  const _HooksTab({required this.api});

  @override
  State<_HooksTab> createState() => _HooksTabState();
}

class _HooksTabState extends State<_HooksTab>
    with AutomaticKeepAliveClientMixin {
  final _ctrl = TextEditingController();
  bool _loading = true;
  bool _saving = false;
  String? _error;

  @override
  bool get wantKeepAlive => true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _ctrl.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final json = await widget.api.getHooksJson();
      if (!mounted) return;
      setState(() {
        _ctrl.text = json;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = '$e';
        _loading = false;
      });
    }
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      await widget.api.saveHooksJson(_ctrl.text);
      if (mounted) _toast(context, tr('Đã lưu hooks', 'Hooks saved'));
    } catch (e) {
      if (mounted) {
        _toast(context,
            tr('JSON không hợp lệ hoặc lỗi: $e', 'Invalid JSON or error: $e'));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    super.build(context);
    final c = context.colors;
    if (_loading) return const LoadingState();
    if (_error != null) return ErrorState(message: _error!, onRetry: _load);
    return Column(
      children: [
        Expanded(
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: TextField(
              controller: _ctrl,
              maxLines: null,
              expands: true,
              textAlignVertical: TextAlignVertical.top,
              style: TextStyle(
                  color: c.textPrimary, fontFamily: 'monospace', fontSize: 12),
              decoration: InputDecoration(
                hintText: tr('Cấu hình hooks (JSON)…',
                    'Hooks configuration (JSON)…'),
                hintStyle: TextStyle(color: c.textMuted),
                filled: true,
                fillColor: c.surfaceAlt,
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(10),
                  borderSide: BorderSide(color: c.border),
                ),
                enabledBorder: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(10),
                  borderSide: BorderSide(color: c.border),
                ),
              ),
            ),
          ),
        ),
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 0, 12, 12),
          child: SizedBox(
            width: double.infinity,
            child: ElevatedButton(
              onPressed: _saving ? null : _save,
              style: ElevatedButton.styleFrom(
                backgroundColor: c.accent,
                foregroundColor: Colors.white,
                padding: const EdgeInsets.symmetric(vertical: 14),
              ),
              child: _saving
                  ? const SizedBox(
                      width: 18,
                      height: 18,
                      child: CircularProgressIndicator(
                          strokeWidth: 2, color: Colors.white))
                  : Text(tr('Lưu hooks', 'Save hooks')),
            ),
          ),
        ),
      ],
    );
  }
}
