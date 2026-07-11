import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import '../../widgets/refresh_on_mount.dart';
import '../settings/entity_providers.dart' show toolRulesProvider, ToolRule;
import 'cowork_panel.dart' show CoworkPanel;
import '../settings/settings_screen.dart' show SpaceAppsSection;
import '../cognitive/cognitive_screen.dart' show CognitiveScreen;
import '../workflow/workflow_panel.dart' show WorkflowPanel;

// ── Models ──────────────────────────────────────────────────────────────────
class SkillInfo {
  final String name;
  final String description;
  final bool enabled;
  final bool builtin;
  final String source;
  const SkillInfo(
      this.name, this.description, this.enabled, this.builtin, this.source);

  factory SkillInfo.fromJson(Map<String, dynamic> j) {
    final dir = '${j['dir'] ?? ''}';
    final name = (j['name'] as String?) ??
        (dir.isNotEmpty ? dir.split('/').last : 'skill');
    final source = '${j['source'] ?? ''}';
    return SkillInfo(
        name,
        '${j['description'] ?? ''}',
        j['disabled'] != true,
        // bundled/global sources are read-only (no uninstall) like web. Skills
        // installed by a Space App (source "app:<id>") are managed by that app —
        // they can be enabled/disabled here but never edited or uninstalled.
        j['builtin'] == true ||
            source == 'bundled' ||
            source.startsWith('global') ||
            source.startsWith('app:'),
        source);
  }
}

/// Source → (label, color) classification, mirroring the web SkillsPanel.
({String label, Color color}) skillSource(String source) {
  if (source.startsWith('app:')) {
    return (label: 'App: ${source.substring(4)}', color: AppTokens.cyan);
  }
  switch (source) {
    case 'bundled':
      return (label: 'Bundled', color: AppTokens.brandAlt);
    case 'clawhub-managed':
      return (label: 'ClawHub', color: AppTokens.brand);
    case 'global-compat':
    case 'global-sema':
      return (label: 'Global', color: const Color(0xFF8A8A99));
    case 'workspace':
      return (label: 'Workspace', color: AppTokens.warning);
    case '':
      return (label: 'Other', color: const Color(0xFF8A8A99));
    default:
      return (label: source, color: const Color(0xFF8A8A99));
  }
}

const _skillSourceOrder = [
  'bundled',
  'clawhub-managed',
  'global-compat',
  'global-sema',
  'workspace',
];

class SubagentInfo {
  final String name;
  final String description;
  final List<String> tools;
  final String? model;
  final int maxConcurrent;
  final bool enabled;
  const SubagentInfo(this.name, this.description, this.tools, this.model,
      this.maxConcurrent, this.enabled);

  factory SubagentInfo.fromJson(Map<String, dynamic> j) => SubagentInfo(
        '${j['name'] ?? ''}',
        '${j['description'] ?? ''}',
        ((j['tools'] as List?) ?? const []).map((e) => '$e').toList(),
        j['model'] as String?,
        (j['maxConcurrent'] as num?)?.toInt() ?? 1,
        j['disabled'] != true,
      );
}

class McpTool {
  final String name;
  final String description;
  const McpTool(this.name, this.description);
  factory McpTool.fromJson(Map<String, dynamic> j) =>
      McpTool('${j['name'] ?? ''}', '${j['description'] ?? ''}');
}

class McpServer {
  final String name;
  final String description;
  final bool builtin;
  final String transport;
  final bool enabled;
  final List<McpTool> tools;

  /// Non-null when this server was registered by a Space App (the daemon injects
  /// `SENCLAW_SPACE_APP_ID` into its env). Such servers are managed by the app —
  /// they can be enabled/disabled here but never edited or deleted.
  final String? appId;

  const McpServer(this.name, this.description, this.builtin, this.transport,
      this.enabled, this.tools, this.appId);

  bool get fromSpaceApp => appId != null;

  factory McpServer.fromJson(Map<String, dynamic> j) {
    final env = (j['env'] as Map?)?.cast<String, dynamic>() ?? const {};
    final appId = '${env['SENCLAW_SPACE_APP_ID'] ?? ''}';
    return McpServer(
      '${j['name'] ?? ''}',
      '${j['description'] ?? ''}',
      j['builtin'] == true,
      '${j['transport'] ?? 'stdio'}',
      j['enabled'] != false,
      ((j['tools'] as List?) ?? const [])
          .whereType<Map>()
          .map((m) => McpTool.fromJson(m.cast<String, dynamic>()))
          .toList(),
      appId.isEmpty ? null : appId,
    );
  }
}


class MarketplaceSource {
  final String id;
  final String name;
  final String url;
  final bool enabled;
  final String? lastSynced;
  const MarketplaceSource(
      this.id, this.name, this.url, this.enabled, this.lastSynced);
  factory MarketplaceSource.fromJson(Map<String, dynamic> j) =>
      MarketplaceSource(
        '${j['id'] ?? ''}',
        '${j['name'] ?? j['id'] ?? ''}',
        '${j['url'] ?? j['source'] ?? ''}',
        j['enabled'] != false,
        j['last_synced'] as String?,
      );
}

// ── Providers ───────────────────────────────────────────────────────────────
List<Map<String, dynamic>> _list(dynamic r, String key) =>
    ((r is Map ? r[key] : r) as List? ?? const [])
        .whereType<Map>()
        .map((e) => e.cast<String, dynamic>())
        .toList();

final skillsProvider = FutureProvider<List<SkillInfo>>((ref) async =>
    _list(await ref.read(apiClientProvider).get('/api/skills'), 'skills')
        .map(SkillInfo.fromJson)
        .toList());

final mcpServersProvider = FutureProvider<List<McpServer>>((ref) async =>
    _list(await ref.read(apiClientProvider).get('/api/mcp-servers'), 'servers')
        .map(McpServer.fromJson)
        .toList());

final marketplaceSourcesProvider =
    FutureProvider<List<MarketplaceSource>>((ref) async => _list(
            await ref.read(apiClientProvider).get('/api/marketplace/sources'),
            'sources')
        .map(MarketplaceSource.fromJson)
        .toList());

/// Plugins inside one marketplace source (GET /api/marketplace/sources/:id).
final marketplaceSourcePluginsProvider =
    FutureProvider.family<List<Map<String, dynamic>>, String>((ref, id) async {
  final r = await ref.read(apiClientProvider).get('/api/marketplace/sources/$id');
  return _list(r, 'plugins');
});

final subagentsProvider = FutureProvider<List<SubagentInfo>>((ref) async =>
    _list(await ref.read(apiClientProvider).get('/api/subagents'), 'subagents')
        .map(SubagentInfo.fromJson)
        .toList());

final hooksProvider = FutureProvider<Map<String, dynamic>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/hooks');
  final h = (r is Map ? r['hooks'] : r);
  return h is Map ? h.cast<String, dynamic>() : {};
});

// ── Code artifacts (published snippets) ──────────────────────────────────────
class CodeArtifact {
  final String id;
  final String name;
  final String language;
  final String code;
  final String description;
  const CodeArtifact(
      this.id, this.name, this.language, this.code, this.description);
  factory CodeArtifact.fromJson(Map<String, dynamic> j) => CodeArtifact(
        '${j['id'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['language'] ?? ''}',
        '${j['code'] ?? ''}',
        '${j['description'] ?? ''}',
      );
}

final codeArtifactsProvider = FutureProvider<List<CodeArtifact>>((ref) async =>
    _list(await ref.read(apiClientProvider).get('/api/code/artifacts'),
            'artifacts')
        .map(CodeArtifact.fromJson)
        .toList());

/// A pending "load this artifact into the editor" request, consumed by the REPL.
final codeLoadRequestProvider = StateProvider<CodeArtifact?>((ref) => null);

/// Hook events with their accent color + description (mirrors the web
/// HooksPanel `ALL_EVENTS`).
const _allHookEvents = <(String, Color, String)>[
  ('UserPromptSubmit', Color(0xFF6366F1), 'Fired when user submits a prompt'),
  ('PreToolUse', Color(0xFF8B5CF6), 'Before any tool is called'),
  ('PostToolUse', Color(0xFF06B6D4), 'After any tool completes'),
  ('PermissionRequest', Color(0xFFF59E0B), 'When permission is requested'),
  ('Stop', Color(0xFFEF4444), 'When the agent stops'),
  ('SessionStart', Color(0xFF10B981), 'When a session begins'),
  ('SessionEnd', Color(0xFF6B7280), 'When a session ends'),
  ('PreCompact', Color(0xFFEC4899), 'Before context compaction'),
  ('PostCompact', Color(0xFF14B8A6), 'After context compaction'),
  ('Error', Color(0xFFF43F5E), 'On agent error'),
];

const _hookEvents = [
  'UserPromptSubmit',
  'PreToolUse',
  'PostToolUse',
  'PermissionRequest',
  'Stop',
  'SessionStart',
  'SessionEnd',
  'PreCompact',
  'PostCompact',
  'Error',
];

(Color, String) _hookEventMeta(String name) {
  for (final (n, color, desc) in _allHookEvents) {
    if (n == name) return (color, desc);
  }
  return (const Color(0xFF6B7280), '');
}

const _pluginsSections = [
  ('skills', 'Skills', Icons.bolt_outlined),
  ('subagents', 'Subagents', Icons.smart_toy_outlined),
  ('mcp', 'MCP servers', Icons.dns_outlined),
  ('hooks', 'Hooks', Icons.webhook_outlined),
  ('code', 'Code', Icons.code),
  ('apps', 'Space Apps', Icons.apps_outlined),
  ('cowork', 'Cowork', Icons.groups_outlined),
  ('workflow', 'Workflow', Icons.account_tree_outlined),
  ('memory', 'Memory', Icons.hub_outlined),
  ('marketplace', 'Marketplace', Icons.store_outlined),
];

/// Public so other screens (e.g. the new-session Workflow tab) can deep-link
/// straight to a Plugins section before navigating here.
final pluginsSectionProvider = StateProvider<String>((ref) => 'skills');

// ── Screen ──────────────────────────────────────────────────────────────────
/// Plugins management, laid out like the web app: a left section rail + a
/// content pane (instead of top tabs).
class PluginsScreen extends ConsumerWidget {
  const PluginsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final section = ref.watch(pluginsSectionProvider);
    return Row(
      children: [
        SizedBox(
          width: 220,
          child: Container(
            color: c.sidebar,
            child: ListView(
              padding: const EdgeInsets.symmetric(vertical: AppTokens.s12),
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(AppTokens.s16,
                      AppTokens.s8, AppTokens.s16, AppTokens.s12),
                  child: Text('Plugins',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w700)),
                ),
                for (final (key, label, icon) in _pluginsSections)
                  _NavItem(
                    icon: icon,
                    label: label,
                    active: section == key,
                    onTap: () =>
                        ref.read(pluginsSectionProvider.notifier).state = key,
                  ),
              ],
            ),
          ),
        ),
        Container(width: 1, color: c.border),
        Expanded(
          // Each API-backed tab re-fetches when opened (distinct keys keep
          // RefreshOnMount remounting across tab switches).
          child: switch (section) {
            'subagents' => RefreshOnMount(
                key: const ValueKey('subagents'),
                providers: [subagentsProvider],
                child: const _SubagentsTab()),
            'mcp' => RefreshOnMount(
                key: const ValueKey('mcp'),
                providers: [mcpServersProvider],
                child: const _McpTab()),
            'hooks' => RefreshOnMount(
                key: const ValueKey('hooks'),
                providers: [hooksProvider],
                child: const _HooksTab()),
            'code' => RefreshOnMount(
                key: const ValueKey('code'),
                providers: [codeArtifactsProvider],
                child: const _CodeTab()),
            'apps' => const SpaceAppsSection(),
            'cowork' => const CoworkPanel(),
            'workflow' => const WorkflowPanel(),
            'memory' => const CognitiveScreen(),
            'marketplace' => RefreshOnMount(
                key: const ValueKey('marketplace'),
                providers: [marketplaceSourcesProvider],
                child: const _MarketplaceTab()),
            _ => RefreshOnMount(
                key: const ValueKey('skills'),
                providers: [skillsProvider],
                child: const _SkillsTab()),
          },
        ),
      ],
    );
  }
}

/// Code-executor capabilities panel (web CodePanel) — reflects the live
/// `senclaw-js` sandbox: JavaScript runs today (QuickJS, isolated, with
/// timeout + memory limits); the rest of the languages are on the roadmap.
class _CodeTab extends StatelessWidget {
  const _CodeTab();

  static const _live = Color(0xFF10B981); // green — sandboxed & live
  static const _host = Color(0xFFF59E0B); // amber — live but runs on host

  // (name, dot color, status) — status ∈ 'sandbox' | 'host' | 'planned'
  static const _languages = [
    ('JavaScript', Color(0xFFF7DF1E), 'sandbox'),
    ('TypeScript', Color(0xFF3178C6), 'sandbox'),
    ('Bash', Color(0xFF4EAA25), 'sandbox'),
    ('Python', Color(0xFF3776AB), 'planned'),
    ('Go', Color(0xFF00ADD8), 'planned'),
    ('Rust', Color(0xFFDEA584), 'planned'),
  ];

  // The three tools exposed by the senclaw-js MCP server.
  static const _tools = [
    ('js_eval',
        'Run a JavaScript snippet; returns the value, console output, and any error'),
    ('js_eval_ts',
        'Run TypeScript — transpiled to JS (types stripped), then run in the sandbox'),
    ('js_eval_file',
        'Read a .js / .mjs file from disk and run it in the same sandbox'),
  ];

  // (icon, title, desc, status) — status ∈ 'live' | 'host' | 'planned'
  static const _features = [
    (
      Icons.code,
      'JavaScript Sandbox (QuickJS)',
      'Agents run JavaScript via the senclaw-js MCP server — standard ECMAScript intrinsics plus a captured console, with no filesystem, network, or process access.',
      'live',
    ),
    (
      Icons.shield_outlined,
      'Sandboxed Execution (JS / TS)',
      'JS/TS runs are bounded by a wall-clock timeout (default 5s, max 60s) and a memory cap (default 128 MiB, max 1 GiB). Infinite loops and over-allocation are killed — zero risk to the host.',
      'live',
    ),
    (
      Icons.code,
      'TypeScript support',
      'TypeScript snippets are transpiled to JS (types stripped, no type-checking) and run in the same sandbox — interfaces, generics, enums, and casts all work.',
      'live',
    ),
    (
      Icons.terminal,
      'Bash (brush sandbox)',
      'Bash runs in brush — a pure-Rust shell — with no env, empty PATH (external programs blocked), a temp working dir, and a kill-enforced timeout (killable child process). In-process isolation, not an OS jail.',
      'live',
    ),
    (
      Icons.bolt_outlined,
      'More language runtimes',
      'Python, Go, and Rust are on the roadmap, reusing the same isolation + resource-limit model.',
      'planned',
    ),
    (
      Icons.bug_report_outlined,
      'Integrated Debugging',
      'Set breakpoints, inspect variables, and step through execution. Supports stack-trace visualization and memory profiling.',
      'planned',
    ),
    (
      Icons.inventory_2_outlined,
      'Artifact Publishing',
      'Package and publish code outputs as reusable artifacts — share scripts, notebooks, and utilities across your agent network.',
      'planned',
    ),
  ];

  /// (accent color or null when planned, short label) for a status string.
  static (Color?, String) _statusStyle(String status) {
    switch (status) {
      case 'sandbox':
      case 'live':
        return (_live, 'LIVE');
      case 'host':
        return (_host, 'HOST');
      default:
        return (null, 'PLANNED');
    }
  }

  Widget _statusPill(String label, Color color) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
        decoration: BoxDecoration(
          color: color.withValues(alpha: 0.14),
          borderRadius: BorderRadius.circular(AppTokens.rSm),
          border: Border.all(color: color.withValues(alpha: 0.4)),
        ),
        child: Text(label,
            style: TextStyle(
                color: color, fontSize: 10, fontWeight: FontWeight.w700)),
      );

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return ListView(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s24, AppTokens.s8, AppTokens.s24, AppTokens.s24),
      children: [
        Row(children: [
          Icon(Icons.code, color: c.accent, size: 22),
          const SizedBox(width: AppTokens.s8),
          Text('Code executor',
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 18,
                  fontWeight: FontWeight.w700)),
          const SizedBox(width: AppTokens.s8),
          _statusPill('JS · TS · BASH', _live),
        ]),
        const SizedBox(height: AppTokens.s4),
        Text('Sandboxed JS/TS via senclaw-js, plus host-shell Bash',
            style: TextStyle(color: c.textMuted, fontSize: 13)),
        const SizedBox(height: AppTokens.s20),
        Text('LANGUAGES',
            style: TextStyle(
                color: c.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 0.5)),
        const SizedBox(height: AppTokens.s8),
        Wrap(spacing: AppTokens.s8, runSpacing: AppTokens.s8, children: [
          for (final (name, color, status) in _languages)
            Builder(builder: (_) {
              final (accent, label) = _statusStyle(status);
              final on = accent != null;
              return Container(
                padding: const EdgeInsets.symmetric(
                    horizontal: AppTokens.s12, vertical: AppTokens.s6),
                decoration: BoxDecoration(
                  color: on ? accent.withValues(alpha: 0.12) : c.surfaceAlt,
                  borderRadius: BorderRadius.circular(AppTokens.rFull),
                  border: Border.all(
                      color: on ? accent.withValues(alpha: 0.45) : c.border),
                ),
                child: Row(mainAxisSize: MainAxisSize.min, children: [
                  Container(
                      width: 8,
                      height: 8,
                      decoration:
                          BoxDecoration(color: color, shape: BoxShape.circle)),
                  const SizedBox(width: AppTokens.s8),
                  Text(name,
                      style: TextStyle(
                          color: on ? c.textPrimary : c.textMuted,
                          fontSize: 12)),
                  if (on) ...[
                    const SizedBox(width: AppTokens.s6),
                    Text(label,
                        style: TextStyle(
                            color: accent,
                            fontSize: 9,
                            fontWeight: FontWeight.w700,
                            letterSpacing: 0.5)),
                  ],
                ]),
              );
            }),
        ]),
        const SizedBox(height: AppTokens.s4),
        Text('JS & TS run sandboxed; Bash runs on the host (not isolated); the rest are planned.',
            style: TextStyle(color: c.textMuted, fontSize: 11)),
        const SizedBox(height: AppTokens.s20),
        // MCP tools exposed by senclaw-js.
        Row(children: [
          Icon(Icons.dns_outlined, size: 14, color: c.accent),
          const SizedBox(width: AppTokens.s6),
          Text('MCP TOOLS · senclaw-js',
              style: TextStyle(
                  color: c.textMuted,
                  fontSize: 11,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 0.5)),
        ]),
        const SizedBox(height: AppTokens.s8),
        Container(
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            color: c.surface,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            border: Border.all(color: c.border),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              for (final (name, desc) in _tools)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 3),
                  child: Row(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Icon(Icons.bolt, size: 12, color: c.textMuted),
                        const SizedBox(width: AppTokens.s6),
                        Expanded(
                          child: RichText(
                            text: TextSpan(children: [
                              TextSpan(
                                  text: name,
                                  style: TextStyle(
                                      color: c.accent,
                                      fontSize: 12,
                                      fontFamily: AppTokens.fontMono)),
                              TextSpan(
                                  text: ' — $desc',
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                            ]),
                          ),
                        ),
                      ]),
                ),
            ],
          ),
        ),
        const SizedBox(height: AppTokens.s24),
        Text('TRY IT',
            style: TextStyle(
                color: c.textMuted,
                fontSize: 11,
                fontWeight: FontWeight.w700,
                letterSpacing: 0.5)),
        const SizedBox(height: AppTokens.s8),
        const _JsRepl(),
        const SizedBox(height: AppTokens.s20),
        Row(children: [
          Text('ARTIFACTS',
              style: TextStyle(
                  color: c.textMuted,
                  fontSize: 11,
                  fontWeight: FontWeight.w700,
                  letterSpacing: 0.5)),
          const Spacer(),
          Consumer(builder: (context, ref, _) {
            return IconButton(
              tooltip: 'Reload',
              icon: const Icon(Icons.refresh, size: 16),
              onPressed: () => ref.invalidate(codeArtifactsProvider),
            );
          }),
        ]),
        const SizedBox(height: AppTokens.s8),
        const _ArtifactsSection(),
        const SizedBox(height: AppTokens.s24),
        for (final (icon, title, desc, status) in _features)
          Builder(builder: (_) {
            final (accent, label) = _statusStyle(status);
            final pillColor = accent ?? const Color(0xFF8A8A99);
            return Container(
              margin: const EdgeInsets.only(bottom: AppTokens.s12),
              padding: const EdgeInsets.all(AppTokens.s16),
              decoration: BoxDecoration(
                color: c.surface,
                borderRadius: BorderRadius.circular(AppTokens.rMd),
                border: Border.all(
                    color: accent != null
                        ? accent.withValues(alpha: 0.4)
                        : c.border),
              ),
              child:
                  Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
                Icon(icon, size: 18, color: accent ?? c.accent),
                const SizedBox(width: AppTokens.s12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(children: [
                        Expanded(
                          child: Text(title,
                              style: TextStyle(
                                  color: c.textPrimary,
                                  fontSize: 14,
                                  fontWeight: FontWeight.w600)),
                        ),
                        _statusPill(label, pillColor),
                      ]),
                      const SizedBox(height: AppTokens.s4),
                      Text(desc,
                          style: TextStyle(
                              color: c.textMuted, fontSize: 12, height: 1.4)),
                    ],
                  ),
                ),
              ]),
            );
          }),
      ],
    );
  }
}

/// Live JavaScript REPL — posts to /api/code/run (sandboxed senclaw-js engine)
/// and renders the result / console output / error.
class _JsRepl extends ConsumerStatefulWidget {
  const _JsRepl();
  @override
  ConsumerState<_JsRepl> createState() => _JsReplState();
}

class _JsReplState extends ConsumerState<_JsRepl> {
  static const _jsSample =
      '// Sandboxed JavaScript — no fs, network, or process access.\n'
      'const xs = [1, 2, 3, 4];\n'
      "console.log('sum', xs.reduce((a, b) => a + b, 0));\n"
      'xs.map(x => x * x)';
  static const _tsSample =
      '// Sandboxed TypeScript — transpiled to JS, then run in the sandbox.\n'
      'interface Point { x: number; y: number }\n'
      'const pts: Point[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];\n'
      'const dist = (p: Point): number => Math.hypot(p.x, p.y);\n'
      "console.log('distances', pts.map(dist));\n"
      'pts.length';
  static const _bashSample =
      '# Bash in the brush sandbox (pure-Rust): no env, empty PATH\n'
      '# (external programs like ls/curl are blocked), temp dir.\n'
      'name="brush"\n'
      'for i in 1 2 3; do echo "hello \$name #\$i"; done\n'
      'echo "sum = \$((2 + 3 * 4))"';

  static const _samples = {
    'javascript': _jsSample,
    'typescript': _tsSample,
    'bash': _bashSample,
  };

  final _controller = TextEditingController(text: _jsSample);
  String _lang = 'javascript';
  bool _running = false;
  bool _saving = false;
  Map<String, dynamic>? _out;
  String? _err;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _switchLang(String next) {
    // Swap the sample when the editor still holds an untouched sample, so
    // toggling shows a relevant example without clobbering real edits.
    if (_samples.containsValue(_controller.text)) {
      _controller.text = _samples[next] ?? _jsSample;
    }
    setState(() => _lang = next);
  }

  Future<void> _run() async {
    setState(() {
      _running = true;
      _err = null;
    });
    try {
      final r = await ref.read(apiClientProvider).post('/api/code/run',
          body: {'code': _controller.text, 'language': _lang});
      setState(() => _out = (r is Map)
          ? r.cast<String, dynamic>()
          : {'ok': false, 'error': '$r'});
    } catch (e) {
      setState(() {
        _err = '$e';
        _out = null;
      });
    } finally {
      if (mounted) setState(() => _running = false);
    }
  }

  Future<void> _save() async {
    final name = await _promptName(context);
    if (name == null || name.trim().isEmpty) return;
    setState(() => _saving = true);
    try {
      await ref.read(apiClientProvider).post('/api/code/artifacts', body: {
        'name': name.trim(),
        'language': _lang,
        'code': _controller.text,
      });
      ref.invalidate(codeArtifactsProvider);
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Saved "${name.trim()}"')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Save failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<String?> _promptName(BuildContext context) {
    final ctrl = TextEditingController();
    return showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Save as artifact'),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: const InputDecoration(labelText: 'Name'),
          onSubmitted: (v) => Navigator.pop(ctx, v),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx), child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, ctrl.text),
              child: const Text('Save')),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final out = _out;
    // Consume a pending "load artifact into editor" request.
    ref.listen<CodeArtifact?>(codeLoadRequestProvider, (_, next) {
      if (next == null) return;
      _controller.text = next.code;
      setState(() => _lang = next.language);
      ref.read(codeLoadRequestProvider.notifier).state = null;
    });
    return Container(
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: _CodeTab._live.withValues(alpha: 0.4)),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Row(children: [
          Icon(Icons.play_circle_outline, size: 16, color: _CodeTab._live),
          const SizedBox(width: AppTokens.s6),
          Text('Interactive REPL',
              style: TextStyle(
                  color: c.textPrimary,
                  fontSize: 14,
                  fontWeight: FontWeight.w600)),
          const Spacer(),
          SegmentedButton<String>(
            style: const ButtonStyle(visualDensity: VisualDensity.compact),
            segments: const [
              ButtonSegment(value: 'javascript', label: Text('JS')),
              ButtonSegment(value: 'typescript', label: Text('TS')),
              ButtonSegment(value: 'bash', label: Text('Bash')),
            ],
            selected: {_lang},
            showSelectedIcon: false,
            onSelectionChanged: (s) => _switchLang(s.first),
          ),
        ]),
        if (_lang == 'bash') ...[
          const SizedBox(height: AppTokens.s8),
          Container(
            padding: const EdgeInsets.symmetric(
                horizontal: AppTokens.s12, vertical: AppTokens.s8),
            decoration: BoxDecoration(
              color: _CodeTab._host.withValues(alpha: 0.12),
              borderRadius: BorderRadius.circular(AppTokens.rSm),
              border: Border.all(color: _CodeTab._host.withValues(alpha: 0.4)),
            ),
            child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
              const Icon(Icons.warning_amber_rounded,
                  size: 14, color: _CodeTab._host),
              const SizedBox(width: AppTokens.s6),
              Expanded(
                child: Text(
                    'Bash runs in the brush sandbox (pure-Rust): no env, empty PATH '
                    '(external programs blocked), temp dir, kill-enforced timeout. '
                    'In-process isolation, not an OS jail.',
                    style: TextStyle(color: c.textSecondary, fontSize: 11)),
              ),
            ]),
          ),
        ],
        const SizedBox(height: AppTokens.s12),
        TextField(
          controller: _controller,
          minLines: 5,
          maxLines: 16,
          style: const TextStyle(fontFamily: AppTokens.fontMono, fontSize: 12),
          decoration: InputDecoration(
            border: const OutlineInputBorder(),
            filled: true,
            fillColor: c.surfaceAlt,
            isDense: true,
          ),
        ),
        const SizedBox(height: AppTokens.s8),
        Row(children: [
          Expanded(
            child: Text(
                'Runs in the senclaw-js sandbox · 5s / 128 MiB limits',
                style: TextStyle(color: c.textMuted, fontSize: 11)),
          ),
          OutlinedButton.icon(
            onPressed: _saving ? null : _save,
            icon: _saving
                ? const SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(Icons.bookmark_add_outlined, size: 16),
            label: const Text('Save'),
          ),
          const SizedBox(width: AppTokens.s8),
          FilledButton.icon(
            onPressed: _running ? null : _run,
            icon: _running
                ? const SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(Icons.play_arrow, size: 16),
            label: const Text('Run'),
          ),
        ]),
        if (_err != null) ...[
          const SizedBox(height: AppTokens.s8),
          Text(_err!,
              style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
        ],
        if (out != null) ...[
          const SizedBox(height: AppTokens.s12),
          _ReplOutput(out: out),
        ],
      ]),
    );
  }
}

class _ReplOutput extends StatelessWidget {
  const _ReplOutput({required this.out});
  final Map<String, dynamic> out;

  Widget _label(String t, AppColors c) => Text(t,
      style: TextStyle(
          color: c.textMuted,
          fontSize: 10,
          fontWeight: FontWeight.w700,
          letterSpacing: 0.5));

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final ok = out['ok'] == true;
    final logs =
        ((out['logs'] as List?) ?? const []).map((e) => '$e').toList();
    final timedOut = out['timed_out'] == true;
    const mono = TextStyle(fontFamily: AppTokens.fontMono, fontSize: 12);
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: c.border),
      ),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        if (logs.isNotEmpty) ...[
          _label('CONSOLE', c),
          for (final l in logs)
            SelectableText(l, style: mono.copyWith(color: c.textSecondary)),
          const SizedBox(height: AppTokens.s8),
        ],
        if (ok) ...[
          Row(children: [
            _label('RESULT', c),
            if (out['result_type'] != null) ...[
              const SizedBox(width: AppTokens.s6),
              Text('${out['result_type']}',
                  style: TextStyle(color: c.textMuted, fontSize: 10)),
            ],
          ]),
          SelectableText('${out['result'] ?? 'undefined'}',
              style: mono.copyWith(color: const Color(0xFF10B981))),
        ] else ...[
          _label(timedOut ? 'TIMED OUT' : 'ERROR', c),
          SelectableText('${out['error'] ?? 'unknown error'}',
              style: mono.copyWith(color: AppTokens.danger)),
        ],
        if (out['duration_ms'] != null) ...[
          const SizedBox(height: AppTokens.s6),
          Text('${out['duration_ms']} ms',
              style: TextStyle(color: c.textMuted, fontSize: 10)),
        ],
      ]),
    );
  }
}

/// List of published code artifacts — load into editor / run / delete.
class _ArtifactsSection extends ConsumerStatefulWidget {
  const _ArtifactsSection();
  @override
  ConsumerState<_ArtifactsSection> createState() => _ArtifactsSectionState();
}

class _ArtifactsSectionState extends ConsumerState<_ArtifactsSection> {
  final Map<String, Map<String, dynamic>> _runOut = {};
  String? _busy;

  Future<void> _run(CodeArtifact a) async {
    setState(() => _busy = a.id);
    try {
      final r = await ref
          .read(apiClientProvider)
          .post('/api/code/artifacts/${a.id}/run');
      setState(() => _runOut[a.id] =
          (r is Map) ? r.cast<String, dynamic>() : {'ok': false, 'error': '$r'});
    } catch (e) {
      setState(() => _runOut[a.id] = {'ok': false, 'error': '$e'});
    } finally {
      if (mounted) setState(() => _busy = null);
    }
  }

  Future<void> _delete(CodeArtifact a) async {
    try {
      await ref.read(apiClientProvider).delete('/api/code/artifacts/${a.id}');
      ref.invalidate(codeArtifactsProvider);
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final artifacts = ref.watch(codeArtifactsProvider);
    return artifacts.when(
      loading: () => const Padding(
          padding: EdgeInsets.all(AppTokens.s16),
          child: Center(child: CircularProgressIndicator())),
      error: (e, _) => Text('$e', style: TextStyle(color: c.textMuted)),
      data: (list) {
        if (list.isEmpty) {
          return Container(
            padding: const EdgeInsets.all(AppTokens.s16),
            decoration: BoxDecoration(
              color: c.surface,
              borderRadius: BorderRadius.circular(AppTokens.rMd),
              border: Border.all(color: c.border),
            ),
            child: Text(
                'No artifacts yet — write code above and tap “Save”.',
                style: TextStyle(color: c.textMuted, fontSize: 12)),
          );
        }
        return Column(
          children: [
            for (final a in list)
              Container(
                margin: const EdgeInsets.only(bottom: AppTokens.s8),
                padding: const EdgeInsets.all(AppTokens.s12),
                decoration: BoxDecoration(
                  color: c.surface,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                  border: Border.all(color: c.border),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(children: [
                      _MiniTag(_langLabel(a.language), AppTokens.brandAlt),
                      const SizedBox(width: AppTokens.s8),
                      Expanded(
                        child: Text(a.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                                color: c.textPrimary,
                                fontWeight: FontWeight.w600)),
                      ),
                      IconButton(
                        tooltip: 'Load into editor',
                        icon: const Icon(Icons.input, size: 16),
                        onPressed: () => ref
                            .read(codeLoadRequestProvider.notifier)
                            .state = a,
                      ),
                      IconButton(
                        tooltip: 'Run',
                        icon: _busy == a.id
                            ? const SizedBox(
                                width: 14,
                                height: 14,
                                child:
                                    CircularProgressIndicator(strokeWidth: 2))
                            : const Icon(Icons.play_arrow, size: 16),
                        onPressed: _busy == null ? () => _run(a) : null,
                      ),
                      IconButton(
                        tooltip: 'Delete',
                        icon: const Icon(Icons.delete_outline,
                            size: 16, color: AppTokens.danger),
                        onPressed: () => _delete(a),
                      ),
                    ]),
                    if (a.description.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(top: 2),
                        child: Text(a.description,
                            maxLines: 2,
                            overflow: TextOverflow.ellipsis,
                            style:
                                TextStyle(color: c.textMuted, fontSize: 12)),
                      ),
                    if (_runOut[a.id] != null) ...[
                      const SizedBox(height: AppTokens.s8),
                      _ReplOutput(out: _runOut[a.id]!),
                    ],
                  ],
                ),
              ),
          ],
        );
      },
    );
  }

  String _langLabel(String l) {
    switch (l) {
      case 'javascript':
        return 'JS';
      case 'typescript':
        return 'TS';
      case 'bash':
        return 'Bash';
      default:
        return l;
    }
  }
}

class _NavItem extends StatelessWidget {
  const _NavItem({
    required this.icon,
    required this.label,
    required this.active,
    required this.onTap,
  });
  final IconData icon;
  final String label;
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 1),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: AppTokens.s12),
          decoration: BoxDecoration(
            color: active ? c.accentSoft : Colors.transparent,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          child: Row(
            children: [
              Icon(icon, size: 16, color: active ? c.accent : c.textMuted),
              const SizedBox(width: AppTokens.s12),
              Text(label,
                  style: TextStyle(
                      color: active ? c.accent : c.textPrimary,
                      fontSize: 14,
                      fontWeight: active ? FontWeight.w600 : FontWeight.w400)),
            ],
          ),
        ),
      ),
    );
  }
}

/// Subagents (personas) — model/tools/concurrency metadata + create + toggle.
class _SubagentsTab extends ConsumerWidget {
  const _SubagentsTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final subs = ref.watch(subagentsProvider);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s16, AppTokens.s16, 0),
          child: Row(
            children: [
              Text('Subagents',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const Spacer(),
              IconButton(
                tooltip: 'Reload',
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () => ref.invalidate(subagentsProvider),
              ),
              const SizedBox(width: AppTokens.s4),
              FilledButton.icon(
                onPressed: () => showDialog(
                    context: context,
                    builder: (_) => const _SubagentEditor()),
                icon: const Icon(Icons.add, size: 16),
                label: const Text('New subagent'),
              ),
            ],
          ),
        ),
        Expanded(
          child: subs.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (list) => list.isEmpty
                ? Center(
                    child: Text('No subagents',
                        style: TextStyle(color: c.textMuted)))
                : ListView.builder(
                    padding: const EdgeInsets.all(AppTokens.s16),
                    itemCount: list.length,
                    itemBuilder: (_, i) =>
                        _SubagentRow(sub: list[i], c: c, ref: ref),
                  ),
          ),
        ),
      ],
    );
  }
}

class _SubagentRow extends StatelessWidget {
  const _SubagentRow({required this.sub, required this.c, required this.ref});
  final SubagentInfo sub;
  final dynamic c;
  final WidgetRef ref;
  @override
  Widget build(BuildContext context) {
    final s = sub;
    return _Card(
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Flexible(
                      child: Text(s.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontWeight: FontWeight.w600)),
                    ),
                    const SizedBox(width: AppTokens.s8),
                    _MiniTag('max ${s.maxConcurrent}', AppTokens.brandAlt),
                    if (s.model != null && s.model!.isNotEmpty) ...[
                      const SizedBox(width: 4),
                      _MiniTag(s.model!, AppTokens.brand),
                    ],
                    if (s.tools.isNotEmpty) ...[
                      const SizedBox(width: 4),
                      _MiniTag('${s.tools.length} tools',
                          const Color(0xFF8A8A99)),
                    ],
                    if (!s.enabled) ...[
                      const SizedBox(width: 4),
                      _MiniTag('off', AppTokens.danger),
                    ],
                  ],
                ),
                const SizedBox(height: 2),
                Text(
                    s.description.isEmpty ? 'No description' : s.description,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          IconButton(
            tooltip: 'README',
            icon: const Icon(Icons.description_outlined, size: 16),
            onPressed: () => showDialog(
                context: context,
                builder: (_) => _ReadmeDialog(
                    path: '/api/subagents/${s.name}/readme', title: s.name)),
          ),
          Switch(
            value: s.enabled,
            onChanged: (v) async {
              await ref.read(apiClientProvider).post(
                  '/api/subagents/${s.name}/${v ? 'enable' : 'disable'}');
              ref.invalidate(subagentsProvider);
            },
          ),
        ],
      ),
    );
  }
}

class _MiniTag extends StatelessWidget {
  const _MiniTag(this.label, this.color);
  final String label;
  final Color color;
  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
      ),
      child: Text(label,
          style: TextStyle(
              color: color, fontSize: 11, fontWeight: FontWeight.w600)),
    );
  }
}

/// Create a subagent persona: name + markdown content → POST /api/subagents/create.
class _SubagentEditor extends ConsumerStatefulWidget {
  const _SubagentEditor();
  @override
  ConsumerState<_SubagentEditor> createState() => _SubagentEditorState();
}

class _SubagentEditorState extends ConsumerState<_SubagentEditor> {
  final _name = TextEditingController();
  final _content = TextEditingController(text: '''---
name: my-agent
description: What this persona is for
model: claude-sonnet-4-6
maxConcurrent: 1
---

You are a focused subagent. Describe the role and behavior here.
''');
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _name.dispose();
    _content.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_name.text.trim().isEmpty || _content.text.trim().isEmpty) {
      setState(() => _error = 'Name and content are required');
      return;
    }
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref.read(apiClientProvider).post('/api/subagents/create',
          body: {'name': _name.text.trim(), 'content': _content.text});
      ref.invalidate(subagentsProvider);
      if (mounted) Navigator.pop(context);
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 620, maxHeight: 620),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('New subagent',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _name,
                decoration: const InputDecoration(
                    labelText: 'Name', hintText: 'my-agent'),
              ),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: TextField(
                  controller: _content,
                  expands: true,
                  maxLines: null,
                  textAlignVertical: TextAlignVertical.top,
                  style: TextStyle(
                      fontFamily: AppTokens.fontMono, fontSize: 12),
                  decoration: const InputDecoration(
                    labelText: 'Persona file (Markdown + frontmatter)',
                    alignLabelWithHint: true,
                  ),
                ),
              ),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(top: AppTokens.s8),
                  child: Text(_error!,
                      style: const TextStyle(color: AppTokens.danger)),
                ),
              const SizedBox(height: AppTokens.s12),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.pop(context),
                      child: const Text('Cancel')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed: _saving ? null : _save,
                    child: _saving
                        ? const SizedBox(
                            width: 14,
                            height: 14,
                            child: CircularProgressIndicator(strokeWidth: 2))
                        : const Text('Create'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Shared card chrome for list rows.
class _Card extends StatelessWidget {
  const _Card({required this.child});
  final Widget child;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: child,
    );
  }
}

// ── Skills ──────────────────────────────────────────────────────────────────
class _SkillsTab extends ConsumerWidget {
  const _SkillsTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final skills = ref.watch(skillsProvider);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s16, AppTokens.s16, 0),
          child: Row(
            children: [
              Text('Skills',
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const Spacer(),
              IconButton(
                tooltip: 'Reload',
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () => ref.invalidate(skillsProvider),
              ),
              const SizedBox(width: AppTokens.s4),
              OutlinedButton.icon(
                onPressed: () => showDialog(
                    context: context, builder: (_) => const _SkillCreateDialog()),
                icon: const Icon(Icons.add, size: 16),
                label: const Text('Create skill'),
              ),
              const SizedBox(width: AppTokens.s4),
              FilledButton.icon(
                onPressed: () => showDialog(
                    context: context, builder: (_) => const _ClawHubDialog()),
                icon: const Icon(Icons.cloud_download_outlined, size: 16),
                label: const Text('Install from ClawHub'),
              ),
            ],
          ),
        ),
        Expanded(child: _skillsList(context, ref, c, skills)),
      ],
    );
  }

  Widget _skillsList(BuildContext context, WidgetRef ref, dynamic c,
      AsyncValue<List<SkillInfo>> skills) {
    return skills.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => Center(child: Text('$e')),
      data: (list) {
        if (list.isEmpty) {
          return Center(
              child: Text('No skills', style: TextStyle(color: c.textMuted)));
        }
        // Classify by source, ordered like the web SkillsPanel.
        final bySource = <String, List<SkillInfo>>{};
        for (final s in list) {
          bySource.putIfAbsent(s.source, () => []).add(s);
        }
        final ordered = [
          ..._skillSourceOrder.where(bySource.containsKey),
          ...bySource.keys.where((k) => !_skillSourceOrder.contains(k)),
        ];
        return ListView(
          padding: const EdgeInsets.all(AppTokens.s16),
          children: [
            for (final src in ordered) ...[
              _SkillSourceHeader(source: src, count: bySource[src]!.length),
              for (final s in bySource[src]!)
                _SkillRow(skill: s, c: c, ref: ref),
              const SizedBox(height: AppTokens.s16),
            ],
          ],
        );
      },
    );
  }
}

class _SkillSourceHeader extends StatelessWidget {
  const _SkillSourceHeader({required this.source, required this.count});
  final String source;
  final int count;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8, top: AppTokens.s4),
      child: Row(
        children: [
          _SourceBadge(source: source),
          const SizedBox(width: AppTokens.s8),
          Text('$count',
              style: TextStyle(color: c.textMuted, fontSize: 12)),
        ],
      ),
    );
  }
}

class _SourceBadge extends StatelessWidget {
  const _SourceBadge({required this.source});
  final String source;
  @override
  Widget build(BuildContext context) {
    final info = skillSource(source);
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: AppTokens.s8, vertical: 2),
      decoration: BoxDecoration(
        color: info.color.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
        border: Border.all(color: info.color.withValues(alpha: 0.4)),
      ),
      child: Text(info.label,
          style: TextStyle(
              color: info.color, fontSize: 11, fontWeight: FontWeight.w600)),
    );
  }
}

class _SkillRow extends StatelessWidget {
  const _SkillRow({required this.skill, required this.c, required this.ref});
  final SkillInfo skill;
  final dynamic c;
  final WidgetRef ref;
  @override
  Widget build(BuildContext context) {
    final s = skill;
    return _Card(
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Flexible(
                      child: Text(s.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontWeight: FontWeight.w600)),
                    ),
                    if (!s.enabled) ...[
                      const SizedBox(width: AppTokens.s8),
                      Container(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 6, vertical: 1),
                        decoration: BoxDecoration(
                          color: AppTokens.danger.withValues(alpha: 0.14),
                          borderRadius: BorderRadius.circular(AppTokens.rSm),
                        ),
                        child: const Text('off',
                            style: TextStyle(
                                color: AppTokens.danger, fontSize: 11)),
                      ),
                    ],
                  ],
                ),
                const SizedBox(height: 2),
                Text(s.description,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          IconButton(
            tooltip: 'README',
            icon: const Icon(Icons.description_outlined, size: 16),
            onPressed: () => showDialog(
                context: context,
                builder: (_) => _ReadmeDialog(
                    path: '/api/skills/${s.name}/readme', title: s.name)),
          ),
          Switch(
            value: s.enabled,
            onChanged: (v) async {
              await ref.read(apiClientProvider).post(
                  '/api/skills/${s.name}/${v ? 'enable' : 'disable'}');
              ref.invalidate(skillsProvider);
            },
          ),
          if (!s.builtin)
            IconButton(
              tooltip: 'Uninstall',
              icon: const Icon(Icons.delete_outline,
                  size: 16, color: AppTokens.danger),
              onPressed: () async {
                await ref
                    .read(apiClientProvider)
                    .delete('/api/skills/${s.name}');
                ref.invalidate(skillsProvider);
              },
            ),
        ],
      ),
    );
  }
}

class _ReadmeDialog extends ConsumerWidget {
  const _ReadmeDialog({required this.path, required this.title});
  final String path;
  final String title;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final future = ref.read(apiClientProvider).get(path);
    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(title,
                  style: TextStyle(
                      color: c.textPrimary,
                      fontSize: 16,
                      fontWeight: FontWeight.w700)),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: FutureBuilder(
                  future: future,
                  builder: (_, snap) {
                    if (!snap.hasData) {
                      return const Center(child: CircularProgressIndicator());
                    }
                    final r = snap.data;
                    final text = r is Map
                        ? '${r['readme'] ?? r['content'] ?? ''}'
                        : '$r';
                    return SingleChildScrollView(
                      child: SelectableText(text,
                          style: TextStyle(
                              color: c.textSecondary,
                              fontFamily: AppTokens.fontMono,
                              fontSize: 12,
                              height: 1.5)),
                    );
                  },
                ),
              ),
              Align(
                alignment: Alignment.centerRight,
                child: TextButton(
                    onPressed: () => Navigator.of(context).pop(),
                    child: const Text('Close')),
              ),
            ],
          ),
        ),
      ),
    );
  }
}


// ── MCP servers ─────────────────────────────────────────────────────────────
class _McpTab extends ConsumerWidget {
  const _McpTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final servers = ref.watch(mcpServersProvider);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.end,
            children: [
              IconButton(
                tooltip: 'Reload',
                icon: const Icon(Icons.refresh, size: 18),
                onPressed: () => ref.invalidate(mcpServersProvider),
              ),
              const SizedBox(width: AppTokens.s4),
              FilledButton.icon(
                onPressed: () => showDialog(
                  context: context, builder: (_) => const _McpEditor()),
                icon: const Icon(Icons.add, size: 16),
                label: const Text('Add server'),
              ),
            ],
          ),
        ),
        Expanded(
          child: servers.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (list) => ListView.builder(
              padding: const EdgeInsets.all(AppTokens.s16),
              itemCount: list.length,
              itemBuilder: (_, i) => _McpRow(server: list[i]),
            ),
          ),
        ),
      ],
    );
  }
}

class _McpRow extends ConsumerStatefulWidget {
  const _McpRow({required this.server});
  final McpServer server;
  @override
  ConsumerState<_McpRow> createState() => _McpRowState();
}

class _McpRowState extends ConsumerState<_McpRow> {
  McpServer get server => widget.server;
  bool _expanded = false;

  Future<void> _act(WidgetRef ref, String action) async {
    await ref.read(apiClientProvider).post('/api/mcp-servers/${server.name}/$action');
    ref.invalidate(mcpServersProvider);
  }

  // Per-server / per-tool auto-accept rule helpers (web MCPSettings parity).
  String _ruleId([String? tool]) =>
      tool != null ? 'mcp:${server.name}:$tool' : 'mcp:${server.name}:*';

  bool _autoAccepted(List<ToolRule> rules, [String? tool]) {
    bool ok(String id) => rules.any(
        (r) => r.id == id && r.enabled && r.action == 'auto_accept');
    if (ok(_ruleId())) return true; // wildcard covers all tools
    return tool != null && ok(_ruleId(tool));
  }

  void _setAutoAccept(WidgetRef ref, String? tool, bool on) {
    final n = ref.read(toolRulesProvider.notifier);
    if (on) {
      n.add(ToolRule(
        id: _ruleId(tool),
        action: 'auto_accept',
        enabled: true,
        description: tool != null
            ? 'Auto-accept ${server.name}:$tool'
            : 'Auto-accept all tools in ${server.name}',
        matcher: {'type': 'mcp_server', 'server': server.name, 'tool': tool},
      ));
    } else {
      n.remove(_ruleId(tool));
    }
  }

  void _allowAll(WidgetRef ref) {
    final n = ref.read(toolRulesProvider.notifier);
    for (final t in server.tools) {
      n.remove(_ruleId(t.name));
    }
    _setAutoAccept(ref, null, true);
  }

  void _revokeAll(WidgetRef ref) {
    final n = ref.read(toolRulesProvider.notifier);
    for (final t in server.tools) {
      n.remove(_ruleId(t.name));
    }
    n.remove(_ruleId());
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final rules = ref.watch(toolRulesProvider);
    final allAuto = _autoAccepted(rules);
    return _Card(
      child: Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(top: AppTokens.s8),
          onExpansionChanged: (v) => setState(() => _expanded = v),
          leading: Icon(Icons.dns_outlined, size: 18, color: c.accent),
          // Enable toggle + chevron pinned to the right in `trailing` so it
          // stays aligned regardless of name/badge width.
          trailing: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              if (!server.builtin)
                Switch(
                  value: server.enabled,
                  onChanged: (v) async {
                    await ref.read(apiClientProvider).post(
                        '/api/mcp-servers/${server.name}/enabled',
                        body: {'enabled': v});
                    ref.invalidate(mcpServersProvider);
                  },
                ),
              const SizedBox(width: AppTokens.s4),
              Icon(_expanded ? Icons.expand_less : Icons.expand_more,
                  size: 20, color: c.textMuted),
            ],
          ),
          title: Row(
            children: [
              Flexible(
                child: Text(server.name,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
              ),
              if (server.builtin) ...[
                const SizedBox(width: AppTokens.s8),
                _badge(c, 'built-in'),
              ],
              if (server.fromSpaceApp) ...[
                const SizedBox(width: AppTokens.s8),
                _badge(c, 'app: ${server.appId}'),
              ],
              const SizedBox(width: AppTokens.s8),
              _badge(c, server.transport),
            ],
          ),
          subtitle: Text(server.description,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: c.textMuted, fontSize: 12)),
          children: [
            Wrap(
              crossAxisAlignment: WrapCrossAlignment.center,
              spacing: AppTokens.s4,
              children: [
                Text('${server.tools.length} tools · ',
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
                if (server.tools.isNotEmpty)
                  allAuto
                      ? TextButton(
                          onPressed: () => _revokeAll(ref),
                          child: const Text('Revoke auto-accept'))
                      : TextButton(
                          onPressed: () => _allowAll(ref),
                          child: const Text('Auto-accept all')),
                if (!server.builtin) ...[
                  TextButton(
                      onPressed: () => _act(ref, 'connect'),
                      child: const Text('Connect')),
                  TextButton(
                      onPressed: () => _act(ref, 'disconnect'),
                      child: const Text('Disconnect')),
                  TextButton(
                      onPressed: () => _act(ref, 'test'),
                      child: const Text('Test')),
                  // Space-App servers are owned by their app: editing/deleting
                  // them here would desync the app's manifest, so only the
                  // enable/disable toggle + operational actions are offered.
                  if (!server.fromSpaceApp) ...[
                    IconButton(
                      tooltip: 'Edit',
                      icon: const Icon(Icons.edit_outlined, size: 16),
                      onPressed: () => showDialog(
                          context: context,
                          builder: (_) => _McpEditor(existing: server)),
                    ),
                    IconButton(
                      tooltip: 'Delete',
                      icon: const Icon(Icons.delete_outline,
                          size: 16, color: AppTokens.danger),
                      onPressed: () async {
                        await ref
                            .read(apiClientProvider)
                            .delete('/api/mcp-servers/${server.name}');
                        ref.invalidate(mcpServersProvider);
                      },
                    ),
                  ],
                ],
              ],
            ),
            for (final t in server.tools)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 2),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Icon(Icons.bolt, size: 12, color: c.textMuted),
                    const SizedBox(width: AppTokens.s6),
                    Expanded(
                      child: RichText(
                        text: TextSpan(children: [
                          TextSpan(
                              text: t.name,
                              style: TextStyle(
                                  color: c.textSecondary,
                                  fontSize: 12,
                                  fontFamily: AppTokens.fontMono)),
                          if (t.description.isNotEmpty)
                            TextSpan(
                                text: ' — ${t.description}',
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 12)),
                        ]),
                      ),
                    ),
                    // Per-tool auto-accept toggle (disabled when wildcard on).
                    Tooltip(
                      message: 'Auto-accept this tool',
                      child: Transform.scale(
                        scale: 0.7,
                        child: Switch(
                          value: _autoAccepted(rules, t.name),
                          onChanged: allAuto
                              ? null
                              : (v) => _setAutoAccept(ref, t.name, v),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _badge(AppColors c, String text) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
        decoration: BoxDecoration(
          color: c.accentSoft,
          borderRadius: BorderRadius.circular(AppTokens.rSm),
        ),
        child: Text(text, style: TextStyle(color: c.accent, fontSize: 11)),
      );
}

/// Create a new local skill via `POST /api/skills/create` (name + description +
/// optional markdown body → scaffolds a `SKILL.md`).
class _SkillCreateDialog extends ConsumerStatefulWidget {
  const _SkillCreateDialog();
  @override
  ConsumerState<_SkillCreateDialog> createState() => _SkillCreateDialogState();
}

class _SkillCreateDialogState extends ConsumerState<_SkillCreateDialog> {
  final _name = TextEditingController();
  final _description = TextEditingController();
  final _content = TextEditingController();
  bool _saving = false;
  String? _error;

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _content.dispose();
    super.dispose();
  }

  bool get _validName =>
      RegExp(r'^[A-Za-z0-9_-]+$').hasMatch(_name.text.trim());

  Future<void> _save() async {
    if (!_validName || _saving) return;
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await ref.read(apiClientProvider).post('/api/skills/create', body: {
        'name': _name.text.trim(),
        'description': _description.text.trim(),
        'content': _content.text.trim(),
      });
      ref.invalidate(skillsProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      if (mounted) {
        setState(() {
          _saving = false;
          _error = '$e';
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final nameErr = _name.text.trim().isNotEmpty && !_validName;
    return Dialog(
      backgroundColor: c.surface,
      shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.rXl)),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Icon(Icons.bolt_outlined, color: c.accent, size: 20),
                  const SizedBox(width: AppTokens.s8),
                  Text('Create skill',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 18,
                          fontWeight: FontWeight.w700)),
                ],
              ),
              const SizedBox(height: AppTokens.s20),
              TextField(
                controller: _name,
                autofocus: true,
                onChanged: (_) => setState(() {}),
                decoration: InputDecoration(
                  labelText: 'Name (slug)',
                  hintText: 'e.g. my-skill',
                  helperText: 'Letters, digits, - and _ only',
                  errorText: nameErr ? 'Invalid slug' : null,
                  border: const OutlineInputBorder(),
                  isDense: true,
                ),
                style: const TextStyle(fontFamily: AppTokens.fontMono),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _description,
                onChanged: (_) => setState(() {}),
                decoration: const InputDecoration(
                  labelText: 'Description',
                  hintText: 'When should the agent use this skill?',
                  border: OutlineInputBorder(),
                  isDense: true,
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _content,
                minLines: 5,
                maxLines: 12,
                decoration: const InputDecoration(
                  labelText: 'Instructions (markdown)',
                  hintText: 'Leave empty to scaffold a starter template…',
                  border: OutlineInputBorder(),
                  alignLabelWithHint: true,
                ),
                style: const TextStyle(
                    fontFamily: AppTokens.fontMono, fontSize: 12),
              ),
              if (_error != null) ...[
                const SizedBox(height: AppTokens.s8),
                Text(_error!,
                    style: const TextStyle(
                        color: AppTokens.danger, fontSize: 12)),
              ],
              const SizedBox(height: AppTokens.s24),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: _saving
                          ? null
                          : () => Navigator.of(context).pop(),
                      child: const Text('Cancel')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton.icon(
                    onPressed: _validName && !_saving ? _save : null,
                    icon: _saving
                        ? const SizedBox(
                            width: 14,
                            height: 14,
                            child: CircularProgressIndicator(strokeWidth: 2))
                        : const Icon(Icons.add, size: 16),
                    label: const Text('Create'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _McpEditor extends ConsumerStatefulWidget {
  const _McpEditor({this.existing});
  final McpServer? existing;
  @override
  ConsumerState<_McpEditor> createState() => _McpEditorState();
}

class _McpEditorState extends ConsumerState<_McpEditor> {
  String _transport = 'stdio';
  String _scope = 'user';
  bool _enabled = true;
  final _name = TextEditingController();
  final _description = TextEditingController();
  final _command = TextEditingController();
  final _args = TextEditingController();
  final _url = TextEditingController();
  final _env = TextEditingController();
  final _headers = TextEditingController();

  bool get _isEdit => widget.existing != null;
  bool get _stdio => _transport == 'stdio';
  bool get _canSave => _name.text.trim().isNotEmpty &&
      (_stdio ? _command.text.trim().isNotEmpty : _url.text.trim().isNotEmpty);

  @override
  void initState() {
    super.initState();
    final ex = widget.existing;
    if (ex != null) {
      _name.text = ex.name;
      _transport = ex.transport;
      _description.text = ex.description;
      _enabled = ex.enabled;
      // Fetch the full config (command/args/env/url/headers/scope) to prefill.
      ref.read(apiClientProvider).get('/api/mcp-servers/${ex.name}').then((r) {
        if (r is! Map || !mounted) return;
        setState(() {
          _command.text = '${r['command'] ?? ''}';
          _args.text = ((r['args'] as List?) ?? const []).join(' ');
          _url.text = '${r['url'] ?? ''}';
          if (r['scope'] != null) _scope = '${r['scope']}';
          final env = (r['env'] as Map?) ?? const {};
          _env.text = env.entries.map((e) => '${e.key}=${e.value}').join('\n');
          final h = (r['headers'] as Map?) ?? const {};
          _headers.text = h.entries.map((e) => '${e.key}: ${e.value}').join('\n');
        });
      }).catchError((_) {});
    }
  }

  @override
  void dispose() {
    _name.dispose();
    _description.dispose();
    _command.dispose();
    _args.dispose();
    _url.dispose();
    _env.dispose();
    _headers.dispose();
    super.dispose();
  }

  Map<String, String> _parseKv(String text, String sep) {
    final map = <String, String>{};
    for (final line in text.split('\n')) {
      final t = line.trim();
      final i = t.indexOf(sep);
      if (i > 0) map[t.substring(0, i).trim()] = t.substring(i + 1).trim();
    }
    return map;
  }

  Future<void> _save() async {
    if (!_canSave) return;
    await ref.read(apiClientProvider).post('/api/mcp-servers', body: {
      'name': _name.text.trim(),
      'transport': _transport,
      'description': _description.text.trim(),
      'scope': _scope,
      'enabled': _enabled,
      'command': _stdio ? _command.text.trim() : null,
      'args': _stdio
          ? _args.text.split(' ').where((a) => a.isNotEmpty).toList()
          : [],
      'env': _stdio ? _parseKv(_env.text, '=') : {},
      'url': _stdio ? null : _url.text.trim(),
      'headers': _stdio ? {} : _parseKv(_headers.text, ':'),
    });
    ref.invalidate(mcpServersProvider);
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      backgroundColor: c.surface,
      shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.rXl)),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480),
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Icon(Icons.dns_outlined, color: c.accent, size: 20),
                  const SizedBox(width: AppTokens.s8),
                  Text(_isEdit ? 'Edit MCP server' : 'Add MCP server',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 18,
                          fontWeight: FontWeight.w700)),
                ],
              ),
              const SizedBox(height: AppTokens.s20),
              TextField(
                controller: _name,
                enabled: !_isEdit, // name is the key — fixed when editing
                onChanged: (_) => setState(() {}),
                decoration: const InputDecoration(
                  labelText: 'Server name',
                  hintText: 'e.g. filesystem-server',
                  border: OutlineInputBorder(),
                  isDense: true,
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              // Transport segmented.
              Row(
                children: [
                  Text('Transport',
                      style: TextStyle(color: c.textSecondary, fontSize: 13)),
                  const SizedBox(width: AppTokens.s12),
                  Expanded(
                    child: SegmentedButton<String>(
                      style: const ButtonStyle(
                          visualDensity: VisualDensity.compact),
                      segments: const [
                        ButtonSegment(value: 'stdio', label: Text('stdio')),
                        ButtonSegment(value: 'sse', label: Text('sse')),
                        ButtonSegment(value: 'http', label: Text('http')),
                      ],
                      selected: {_transport},
                      onSelectionChanged: (s) =>
                          setState(() => _transport = s.first),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _description,
                decoration: const InputDecoration(
                  labelText: 'Description',
                  hintText: 'Optional',
                  border: OutlineInputBorder(),
                  isDense: true,
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              Row(
                children: [
                  Expanded(
                    child: DropdownButtonFormField<String>(
                      initialValue: _scope,
                      decoration: const InputDecoration(
                          labelText: 'Scope', border: OutlineInputBorder()),
                      items: const [
                        DropdownMenuItem(value: 'user', child: Text('User')),
                        DropdownMenuItem(
                            value: 'project', child: Text('Project')),
                      ],
                      onChanged: (v) => setState(() => _scope = v ?? 'user'),
                    ),
                  ),
                  const SizedBox(width: AppTokens.s12),
                  Row(
                    children: [
                      Text('Enabled',
                          style:
                              TextStyle(color: c.textSecondary, fontSize: 13)),
                      Switch(
                        value: _enabled,
                        onChanged: (v) => setState(() => _enabled = v),
                      ),
                    ],
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              if (_stdio) ...[
                TextField(
                  controller: _command,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    labelText: 'Command',
                    hintText: 'npx -y @modelcontextprotocol/server-filesystem',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  style: const TextStyle(fontFamily: AppTokens.fontMono),
                ),
                const SizedBox(height: AppTokens.s12),
                TextField(
                  controller: _args,
                  decoration: const InputDecoration(
                    labelText: 'Arguments (space-separated)',
                    hintText: '/path/to/allowed',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  style: const TextStyle(fontFamily: AppTokens.fontMono),
                ),
                const SizedBox(height: AppTokens.s12),
                TextField(
                  controller: _env,
                  minLines: 2,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    labelText: 'Environment (KEY=VALUE per line)',
                    hintText: 'API_KEY=xxx',
                    border: OutlineInputBorder(),
                    alignLabelWithHint: true,
                  ),
                  style: const TextStyle(
                      fontFamily: AppTokens.fontMono, fontSize: 12),
                ),
              ] else ...[
                TextField(
                  controller: _url,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    labelText: 'URL',
                    hintText: 'http://localhost:8080/sse',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  style: const TextStyle(fontFamily: AppTokens.fontMono),
                ),
                const SizedBox(height: AppTokens.s12),
                TextField(
                  controller: _headers,
                  minLines: 2,
                  maxLines: 4,
                  decoration: const InputDecoration(
                    labelText: 'Headers (Name: Value per line)',
                    hintText: 'Authorization: Bearer xxx',
                    border: OutlineInputBorder(),
                    alignLabelWithHint: true,
                  ),
                  style: const TextStyle(
                      fontFamily: AppTokens.fontMono, fontSize: 12),
                ),
              ],
              const SizedBox(height: AppTokens.s24),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.of(context).pop(),
                      child: const Text('Cancel')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed: _canSave ? _save : null,
                    child: Text(_isEdit ? 'Save' : 'Add'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

// ── Marketplace ─────────────────────────────────────────────────────────────
class _MarketplaceTab extends ConsumerWidget {
  const _MarketplaceTab();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final sources = ref.watch(marketplaceSourcesProvider);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Align(
            alignment: Alignment.centerRight,
            child: FilledButton.icon(
              onPressed: () => _addSource(context, ref),
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Add source'),
            ),
          ),
        ),
        Expanded(
          child: sources.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (list) => list.isEmpty
                ? Center(
                    child: Text('No marketplace sources',
                        style: TextStyle(color: c.textMuted)))
                : ListView.builder(
                    padding: const EdgeInsets.all(AppTokens.s16),
                    itemCount: list.length,
                    itemBuilder: (_, i) =>
                        _MarketplaceSourceCard(source: list[i]),
                  ),
          ),
        ),
      ],
    );
  }

  Future<void> _addSource(BuildContext context, WidgetRef ref) async {
    final ctrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: const Text('Add marketplace source'),
        content: TextField(
            controller: ctrl,
            autofocus: true,
            decoration: const InputDecoration(
                labelText: 'Source URL (git repo or index)')),
        actions: [
          TextButton(
              onPressed: () => Navigator.of(dctx).pop(false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.of(dctx).pop(true),
              child: const Text('Add')),
        ],
      ),
    );
    if (ok == true && ctrl.text.trim().isNotEmpty) {
      await ref
          .read(apiClientProvider)
          .post('/api/marketplace/sources', body: {'url': ctrl.text.trim()});
      ref.invalidate(marketplaceSourcesProvider);
    }
  }
}

// ── Hooks ───────────────────────────────────────────────────────────────────
class _HooksTab extends ConsumerWidget {
  const _HooksTab();

  Future<void> _save(WidgetRef ref, Map<String, dynamic> hooks) async {
    await ref.read(apiClientProvider).put('/api/hooks', body: {'hooks': hooks});
    ref.invalidate(hooksProvider);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final hooksAsync = ref.watch(hooksProvider);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
              AppTokens.s16, AppTokens.s12, AppTokens.s16, 0),
          child: Align(
            alignment: Alignment.centerRight,
            child: FilledButton.icon(
              onPressed: () async {
                final hooks = {...(hooksAsync.valueOrNull ?? const {})};
                final added = await showDialog<Map<String, dynamic>>(
                    context: context, builder: (_) => const _HookEditor());
                if (added == null) return;
                final ev = added['event'] as String;
                final matcher = (added['matcher'] as String?) ?? '';
                final hook = added['hook'] as Map<String, dynamic>;
                final list = [...((hooks[ev] as List?) ?? const [])];
                list.add({
                  if (matcher.isNotEmpty) 'matcher': matcher,
                  'hooks': [hook],
                });
                hooks[ev] = list;
                await _save(ref, hooks);
              },
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Add hook'),
            ),
          ),
        ),
        Expanded(
          child: hooksAsync.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (e, _) => Center(child: Text('$e')),
            data: (hooks) {
              final events =
                  _hookEvents.where((e) => (hooks[e] as List?)?.isNotEmpty ?? false);
              if (events.isEmpty) {
                return Center(
                    child: Text('No hooks configured',
                        style: TextStyle(color: c.textMuted)));
              }
              return ListView(
                padding: const EdgeInsets.all(AppTokens.s16),
                children: [
                  for (final ev in events) ...[
                    Padding(
                      padding: const EdgeInsets.only(
                          top: AppTokens.s8, bottom: AppTokens.s4),
                      child: Row(
                        children: [
                          Container(
                            width: 8,
                            height: 8,
                            decoration: BoxDecoration(
                                color: _hookEventMeta(ev).$1,
                                shape: BoxShape.circle),
                          ),
                          const SizedBox(width: AppTokens.s8),
                          Text(ev.toUpperCase(),
                              style: TextStyle(
                                  color: c.textMuted,
                                  fontSize: 12,
                                  fontWeight: FontWeight.w700,
                                  letterSpacing: 1)),
                        ],
                      ),
                    ),
                    for (var gi = 0; gi < (hooks[ev] as List).length; gi++)
                      _hookGroup(context, ref, hooks, ev, gi),
                  ],
                ],
              );
            },
          ),
        ),
      ],
    );
  }

  Widget _hookGroup(BuildContext context, WidgetRef ref,
      Map<String, dynamic> hooks, String ev, int gi) {
    final c = context.colors;
    final g = ((hooks[ev] as List)[gi] as Map).cast<String, dynamic>();
    final matcher = '${g['matcher'] ?? '*'}';
    final cmds = ((g['hooks'] as List?) ?? const [])
        .whereType<Map>()
        .map((h) => '${h['command'] ?? h['prompt'] ?? ''}')
        .where((s) => s.isNotEmpty)
        .toList();
    return _Card(
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('matcher: $matcher',
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                for (final cmd in cmds)
                  Text('\$ $cmd',
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                          color: c.textMuted,
                          fontSize: 12,
                          fontFamily: AppTokens.fontMono)),
              ],
            ),
          ),
          IconButton(
            tooltip: 'Edit',
            icon: const Icon(Icons.edit_outlined, size: 16),
            onPressed: () async {
              final firstHook =
                  ((g['hooks'] as List?)?.whereType<Map>().firstOrNull)
                          ?.cast<String, dynamic>() ??
                      <String, dynamic>{};
              final edited = await showDialog<Map<String, dynamic>>(
                context: context,
                builder: (_) => _HookEditor(initial: {
                  'event': ev,
                  'matcher': g['matcher'] ?? '',
                  'hook': firstHook,
                }),
              );
              if (edited == null) return;
              final newEv = edited['event'] as String;
              final newMatcher = (edited['matcher'] as String?) ?? '';
              final newEntry = {
                if (newMatcher.isNotEmpty) 'matcher': newMatcher,
                'hooks': [edited['hook']],
              };
              final next = {...hooks};
              // Remove the old entry, then add the edited one under its event.
              final oldList = [...(next[ev] as List)]..removeAt(gi);
              if (oldList.isEmpty) {
                next.remove(ev);
              } else {
                next[ev] = oldList;
              }
              final dest = [...((next[newEv] as List?) ?? const []), newEntry];
              next[newEv] = dest;
              await _save(ref, next);
            },
          ),
          IconButton(
            tooltip: 'Remove',
            icon: const Icon(Icons.delete_outline,
                size: 16, color: AppTokens.danger),
            onPressed: () async {
              final next = {...hooks};
              final list = [...(next[ev] as List)]..removeAt(gi);
              if (list.isEmpty) {
                next.remove(ev);
              } else {
                next[ev] = list;
              }
              await _save(ref, next);
            },
          ),
        ],
      ),
    );
  }
}

class _HookEditor extends StatefulWidget {
  /// When non-null, prefill from an existing entry ({event, matcher, hook}) and
  /// render in edit mode instead of add mode.
  const _HookEditor({this.initial});
  final Map<String, dynamic>? initial;
  @override
  State<_HookEditor> createState() => _HookEditorState();
}

class _HookEditorState extends State<_HookEditor> {
  String _event = 'PostToolUse';
  String _type = 'command'; // command | prompt
  final _matcher = TextEditingController();
  final _command = TextEditingController();
  final _prompt = TextEditingController();
  final _timeout = TextEditingController(text: '10');
  final _historyLimit = TextEditingController(text: '10');
  bool _blocking = true;
  bool _async = false;
  bool _includeHistory = false;

  bool get _editing => widget.initial != null;

  @override
  void initState() {
    super.initState();
    final init = widget.initial;
    if (init == null) return;
    _event = '${init['event'] ?? _event}';
    _matcher.text = '${init['matcher'] ?? ''}';
    final h = (init['hook'] as Map?)?.cast<String, dynamic>() ?? const {};
    _type = '${h['type'] ?? 'command'}';
    _command.text = '${h['command'] ?? ''}';
    _prompt.text = '${h['prompt'] ?? ''}';
    if (h['timeout'] != null) _timeout.text = '${h['timeout']}';
    _blocking = h['blocking'] != false;
    _async = h['async'] == true;
    _includeHistory = h['include_history'] == true;
    if (h['history_limit'] != null) _historyLimit.text = '${h['history_limit']}';
  }

  @override
  void dispose() {
    _matcher.dispose();
    _command.dispose();
    _prompt.dispose();
    _timeout.dispose();
    _historyLimit.dispose();
    super.dispose();
  }

  bool get _canAdd => _type == 'command'
      ? _command.text.trim().isNotEmpty
      : _prompt.text.trim().isNotEmpty;

  void _submit() {
    if (!_canAdd) return;
    final hook = <String, dynamic>{'type': _type};
    if (_type == 'command') {
      hook['command'] = _command.text.trim();
    } else {
      hook['prompt'] = _prompt.text.trim();
      if (_includeHistory) {
        hook['include_history'] = true;
        final hl = int.tryParse(_historyLimit.text.trim());
        if (hl != null) hook['history_limit'] = hl;
      }
    }
    final t = int.tryParse(_timeout.text.trim());
    if (t != null) hook['timeout'] = t;
    hook['blocking'] = _blocking;
    if (_async) hook['async'] = true;
    Navigator.of(context).pop({
      'event': _event,
      'matcher': _matcher.text.trim(),
      'hook': hook,
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final (evColor, evDesc) = _hookEventMeta(_event);
    return Dialog(
      backgroundColor: c.surface,
      shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.rXl)),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 480),
        child: SingleChildScrollView(
          padding: const EdgeInsets.all(AppTokens.s24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Icon(Icons.webhook_outlined, color: c.accent, size: 20),
                  const SizedBox(width: AppTokens.s8),
                  Text(_editing ? 'Edit hook' : 'Add hook',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 18,
                          fontWeight: FontWeight.w700)),
                ],
              ),
              const SizedBox(height: AppTokens.s20),
              // Event (with color dot + description).
              DropdownButtonFormField<String>(
                initialValue: _event,
                isExpanded: true,
                decoration: const InputDecoration(
                    labelText: 'Event', border: OutlineInputBorder()),
                items: [
                  for (final (name, color, _) in _allHookEvents)
                    DropdownMenuItem(
                      value: name,
                      child: Row(children: [
                        Container(
                            width: 8,
                            height: 8,
                            decoration: BoxDecoration(
                                color: color, shape: BoxShape.circle)),
                        const SizedBox(width: AppTokens.s8),
                        Text(name,
                            style: const TextStyle(
                                fontFamily: AppTokens.fontMono, fontSize: 13)),
                      ]),
                    ),
                ],
                onChanged: (v) =>
                    setState(() => _event = v ?? 'PostToolUse'),
              ),
              if (evDesc.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 4, left: 4),
                  child: Text(evDesc,
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
                ),
              const SizedBox(height: AppTokens.s12),
              // Type segmented (command | prompt).
              Row(
                children: [
                  Text('Type',
                      style: TextStyle(color: c.textSecondary, fontSize: 13)),
                  const SizedBox(width: AppTokens.s12),
                  Expanded(
                    child: SegmentedButton<String>(
                      style: const ButtonStyle(
                          visualDensity: VisualDensity.compact),
                      segments: const [
                        ButtonSegment(
                            value: 'command',
                            icon: Icon(Icons.terminal, size: 14),
                            label: Text('Command')),
                        ButtonSegment(
                            value: 'prompt',
                            icon: Icon(Icons.chat_bubble_outline, size: 14),
                            label: Text('Prompt')),
                      ],
                      selected: {_type},
                      onSelectionChanged: (s) =>
                          setState(() => _type = s.first),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _matcher,
                decoration: const InputDecoration(
                  labelText: 'Matcher',
                  hintText: 'e.g. Bash — empty = all tools',
                  border: OutlineInputBorder(),
                  isDense: true,
                ),
                style: const TextStyle(fontFamily: AppTokens.fontMono),
              ),
              const SizedBox(height: AppTokens.s12),
              if (_type == 'command')
                TextField(
                  controller: _command,
                  autofocus: true,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    labelText: 'Command',
                    hintText: 'e.g. echo done',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  style: const TextStyle(fontFamily: AppTokens.fontMono),
                )
              else
                TextField(
                  controller: _prompt,
                  autofocus: true,
                  minLines: 2,
                  maxLines: 4,
                  onChanged: (_) => setState(() {}),
                  decoration: const InputDecoration(
                    labelText: 'Prompt',
                    hintText: 'e.g. Review this tool call for security issues',
                    border: OutlineInputBorder(),
                    alignLabelWithHint: true,
                  ),
                ),
              const SizedBox(height: AppTokens.s16),
              // Advanced options.
              Container(
                padding: const EdgeInsets.all(AppTokens.s12),
                decoration: BoxDecoration(
                  color: c.surfaceAlt,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                  border: Border.all(color: c.border),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('ADVANCED',
                        style: TextStyle(
                            color: c.textMuted,
                            fontSize: 10,
                            fontWeight: FontWeight.w700,
                            letterSpacing: 0.5)),
                    const SizedBox(height: AppTokens.s8),
                    Wrap(
                      spacing: AppTokens.s16,
                      runSpacing: AppTokens.s8,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        _numField('timeout (s)', _timeout),
                        _switchField('blocking', _blocking,
                            (v) => setState(() => _blocking = v)),
                        _switchField('async', _async,
                            (v) => setState(() => _async = v)),
                        if (_type == 'prompt')
                          _switchField('include_history', _includeHistory,
                              (v) => setState(() => _includeHistory = v)),
                        if (_type == 'prompt' && _includeHistory)
                          _numField('history_limit', _historyLimit),
                      ],
                    ),
                  ],
                ),
              ),
              const SizedBox(height: AppTokens.s24),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: () => Navigator.of(context).pop(),
                      child: const Text('Cancel')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton.icon(
                    onPressed: _canAdd ? _submit : null,
                    icon: Icon(_editing ? Icons.check : Icons.add, size: 16),
                    label: Text(_editing ? 'Save' : 'Add'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _numField(String label, TextEditingController ctrl) {
    final c = context.colors;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(label, style: TextStyle(color: c.textSecondary, fontSize: 12)),
        const SizedBox(width: AppTokens.s6),
        SizedBox(
          width: 56,
          child: TextField(
            controller: ctrl,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(
                isDense: true, border: OutlineInputBorder()),
            style: const TextStyle(fontSize: 13),
          ),
        ),
      ],
    );
  }

  Widget _switchField(String label, bool value, ValueChanged<bool> onChanged) {
    final c = context.colors;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(label, style: TextStyle(color: c.textSecondary, fontSize: 12)),
        const SizedBox(width: AppTokens.s4),
        Switch(value: value, onChanged: onChanged),
      ],
    );
  }
}

/// Search & install skills from ClawHub (web ClawHubSearchDialog):
///   `GET  /api/skills/remote-search?q=...` → results [{slug,displayName,summary,version,installed}]
///   `POST /api/skills/install { slug }`
class _ClawHubDialog extends ConsumerStatefulWidget {
  const _ClawHubDialog();
  @override
  ConsumerState<_ClawHubDialog> createState() => _ClawHubDialogState();
}

class _ClawHubDialogState extends ConsumerState<_ClawHubDialog> {
  final _q = TextEditingController();
  List<Map<String, dynamic>> _results = [];
  bool _loading = false;
  String? _error;
  String? _installing;

  @override
  void dispose() {
    _q.dispose();
    super.dispose();
  }

  Future<void> _search() async {
    final query = _q.text.trim();
    if (query.isEmpty) return;
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final r = await ref.read(apiClientProvider).get(
          '/api/skills/remote-search?q=${Uri.encodeQueryComponent(query)}');
      final res = (r is Map ? r['results'] : null) as List? ?? const [];
      setState(() => _results = res.whereType<Map>()
          .map((e) => e.cast<String, dynamic>())
          .toList());
    } catch (e) {
      setState(() {
        _error = '$e';
        _results = [];
      });
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Future<void> _install(String slug) async {
    setState(() => _installing = slug);
    try {
      await ref
          .read(apiClientProvider)
          .post('/api/skills/install', body: {'slug': slug});
      ref.invalidate(skillsProvider);
      setState(() {
        for (final r in _results) {
          if (r['slug'] == slug) r['installed'] = true;
        }
      });
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Installed $slug')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Install failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _installing = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 560, maxHeight: 560),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Icon(Icons.cloud_outlined, size: 18, color: c.accent),
                  const SizedBox(width: AppTokens.s8),
                  Text('Install from ClawHub',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w700)),
                  const Spacer(),
                  IconButton(
                      icon: const Icon(Icons.close, size: 18),
                      onPressed: () => Navigator.pop(context)),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              TextField(
                controller: _q,
                autofocus: true,
                decoration: InputDecoration(
                  hintText: 'Search skills on clawhub.ai…',
                  prefixIcon: const Icon(Icons.search, size: 16),
                  suffixIcon: TextButton(
                      onPressed: _search, child: const Text('Search')),
                ),
                onSubmitted: (_) => _search(),
              ),
              const SizedBox(height: AppTokens.s12),
              Expanded(
                child: _loading
                    ? const Center(child: CircularProgressIndicator())
                    : _error != null
                        ? Center(
                            child: Text(_error!,
                                style: const TextStyle(
                                    color: AppTokens.danger)))
                        : _results.isEmpty
                            ? Center(
                                child: Text('Type a keyword to search ClawHub',
                                    style: TextStyle(color: c.textMuted)))
                            : ListView.separated(
                                itemCount: _results.length,
                                separatorBuilder: (_, i) =>
                                    const SizedBox(height: AppTokens.s8),
                                itemBuilder: (_, i) {
                                  final r = _results[i];
                                  final slug = '${r['slug']}';
                                  final installed = r['installed'] == true;
                                  return _Card(
                                    child: Row(
                                      children: [
                                        Expanded(
                                          child: Column(
                                            crossAxisAlignment:
                                                CrossAxisAlignment.start,
                                            children: [
                                              Row(children: [
                                                Flexible(
                                                  child: Text(
                                                      '${r['displayName'] ?? slug}',
                                                      maxLines: 1,
                                                      overflow:
                                                          TextOverflow.ellipsis,
                                                      style: TextStyle(
                                                          color: c.textPrimary,
                                                          fontWeight:
                                                              FontWeight.w600)),
                                                ),
                                                if (r['version'] != null) ...[
                                                  const SizedBox(width: 6),
                                                  Text('v${r['version']}',
                                                      style: TextStyle(
                                                          color: c.textMuted,
                                                          fontSize: 11)),
                                                ],
                                              ]),
                                              if (r['summary'] != null)
                                                Text('${r['summary']}',
                                                    maxLines: 2,
                                                    overflow:
                                                        TextOverflow.ellipsis,
                                                    style: TextStyle(
                                                        color: c.textMuted,
                                                        fontSize: 12)),
                                            ],
                                          ),
                                        ),
                                        const SizedBox(width: AppTokens.s8),
                                        installed
                                            ? Row(children: [
                                                Icon(Icons.check_circle,
                                                    size: 16,
                                                    color: AppTokens.success),
                                                const SizedBox(width: 4),
                                                Text('Installed',
                                                    style: TextStyle(
                                                        color: c.textMuted,
                                                        fontSize: 12)),
                                              ])
                                            : FilledButton(
                                                onPressed: _installing == slug
                                                    ? null
                                                    : () => _install(slug),
                                                child: _installing == slug
                                                    ? const SizedBox(
                                                        width: 14,
                                                        height: 14,
                                                        child:
                                                            CircularProgressIndicator(
                                                                strokeWidth: 2))
                                                    : const Text('Install'),
                                              ),
                                      ],
                                    ),
                                  );
                                },
                              ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// A marketplace source as an expandable card: header (name/enabled/sync/delete)
/// + its plugins with per-plugin enable toggles (web MarketplacePanel).
class _MarketplaceSourceCard extends ConsumerWidget {
  const _MarketplaceSourceCard({required this.source});
  final MarketplaceSource source;

  Future<void> _post(WidgetRef ref, String path) async {
    await ref.read(apiClientProvider).post(path);
    ref.invalidate(marketplaceSourcePluginsProvider(source.id));
    ref.invalidate(marketplaceSourcesProvider);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final s = source;
    return _Card(
      child: Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          tilePadding: EdgeInsets.zero,
          childrenPadding: const EdgeInsets.only(top: AppTokens.s8),
          leading: Icon(Icons.store_outlined, size: 18, color: c.accent),
          title: Row(
            children: [
              Flexible(
                child: Text(s.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
              ),
              const SizedBox(width: AppTokens.s8),
              _MiniTag(s.enabled ? 'enabled' : 'off',
                  s.enabled ? AppTokens.success : const Color(0xFF8A8A99)),
            ],
          ),
          subtitle: Text(s.url,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(color: c.textMuted, fontSize: 12)),
          trailing: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextButton(
                onPressed: () => _post(
                    ref, '/api/marketplace/sources/${s.id}/sync'),
                child: const Text('Sync'),
              ),
              IconButton(
                tooltip: 'Remove',
                icon: const Icon(Icons.delete_outline,
                    size: 16, color: AppTokens.danger),
                onPressed: () async {
                  await ref
                      .read(apiClientProvider)
                      .delete('/api/marketplace/sources/${s.id}');
                  ref.invalidate(marketplaceSourcesProvider);
                },
              ),
            ],
          ),
          children: [
            Row(
              children: [
                if (s.lastSynced != null)
                  Text('Synced: ${s.lastSynced}',
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                const Spacer(),
                TextButton(
                    onPressed: () => _post(ref,
                        '/api/marketplace/sources/${s.id}/enable-all'),
                    child: const Text('Enable all')),
                TextButton(
                    onPressed: () => _post(ref,
                        '/api/marketplace/sources/${s.id}/disable-all'),
                    child: const Text('Disable all')),
              ],
            ),
            Consumer(builder: (_, ref, _) {
              final plugins =
                  ref.watch(marketplaceSourcePluginsProvider(s.id));
              return plugins.when(
                loading: () => const Padding(
                    padding: EdgeInsets.all(AppTokens.s12),
                    child: Center(child: CircularProgressIndicator())),
                error: (e, _) => Text('$e',
                    style: const TextStyle(color: AppTokens.danger)),
                data: (list) => list.isEmpty
                    ? Padding(
                        padding: const EdgeInsets.all(AppTokens.s8),
                        child: Text('No plugins (sync the source)',
                            style: TextStyle(color: c.textMuted, fontSize: 12)),
                      )
                    : Column(
                        children: [
                          for (final p in list)
                            Padding(
                              padding:
                                  const EdgeInsets.symmetric(vertical: 2),
                              child: Row(
                                children: [
                                  Expanded(
                                    child: Column(
                                      crossAxisAlignment:
                                          CrossAxisAlignment.start,
                                      children: [
                                        Text('${p['name'] ?? ''}',
                                            style: TextStyle(
                                                color: c.textSecondary,
                                                fontSize: 13)),
                                        if (p['description'] != null)
                                          Text('${p['description']}',
                                              maxLines: 2,
                                              overflow: TextOverflow.ellipsis,
                                              style: TextStyle(
                                                  color: c.textMuted,
                                                  fontSize: 11)),
                                      ],
                                    ),
                                  ),
                                  Transform.scale(
                                    scale: 0.8,
                                    child: Switch(
                                      value: p['enabled'] == true,
                                      onChanged: (_) => _post(ref,
                                          '/api/marketplace/sources/${s.id}/plugins/${p['name']}/toggle'),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                        ],
                      ),
              );
            }),
          ],
        ),
      ),
    );
  }
}
