import 'dart:async';
import 'dart:convert';
import 'dart:io' show File;
import 'dart:math';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:audioplayers/audioplayers.dart';
import 'package:record/record.dart';
import 'package:path_provider/path_provider.dart';
import 'package:http/http.dart' as http;
import '../chat/audio_service.dart' show audioServiceProvider;
import 'package:qr_flutter/qr_flutter.dart';
import '../../core/transport/connection.dart';
import '../../theme/theme_mode_provider.dart';
import '../../theme/tokens.dart';
import '../chat/agents_provider.dart';
import '../chat/new_chat_dialog.dart' show llmConfigsProvider, LlmConfig;
import 'entity_providers.dart';
import 'settings_providers.dart';

const _sections = [
  ('appearance', 'Appearance', Icons.palette_outlined),
  ('general', 'General', Icons.tune),
  ('channels', 'Channels', Icons.hub_outlined),
  ('agents', 'Profiles', Icons.badge_outlined),
  ('rules', 'Tool Rules', Icons.rule_folder_outlined),
  ('llm', 'LLM Models', Icons.smart_toy_outlined),
  ('local', 'Local Models', Icons.memory),
  ('embedding', 'Embedding', Icons.scatter_plot_outlined),
  ('memory', 'Memory', Icons.account_tree_outlined),
  ('whisper', 'Speech-to-Text', Icons.mic_none_outlined),
  ('tts', 'Text-to-Speech', Icons.volume_up_outlined),
  ('ocr', 'OCR', Icons.document_scanner_outlined),
];

final _settingsSectionProvider = StateProvider<String>((ref) => 'general');

class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final section = ref.watch(_settingsSectionProvider);

    return Row(
      children: [
        SizedBox(
          width: 240,
          child: Container(
            color: c.sidebar,
            child: ListView(
              padding: const EdgeInsets.symmetric(vertical: AppTokens.s12),
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(AppTokens.s16,
                      AppTokens.s8, AppTokens.s16, AppTokens.s12),
                  child: Text('Settings',
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w700)),
                ),
                for (final (key, label, icon) in _sections)
                  _SectionItem(
                    icon: icon,
                    label: label,
                    active: section == key,
                    onTap: () =>
                        ref.read(_settingsSectionProvider.notifier).state = key,
                  ),
              ],
            ),
          ),
        ),
        Container(width: 1, color: c.border),
        Expanded(
          child: switch (section) {
            'appearance' => const _AppearanceSection(),
            'channels' => const _ChannelsSection(),
            'agents' => const _AgentsSection(),
            'rules' => const _ToolRulesSection(),
            'llm' => const _LlmSection(),
            'local' => const _LocalModelsSection(),
            'embedding' => const _EmbeddingSection(),
            'memory' => const _MemorySection(),
            'whisper' =>
              const _MediaModelsSection(domain: 'whisper', title: 'Speech-to-Text (Whisper)'),
            'tts' =>
              const _MediaModelsSection(domain: 'tts', title: 'Text-to-Speech'),
            'ocr' => const _MediaModelsSection(domain: 'ocr', title: 'OCR'),
            _ => const _GeneralSection(),
          },
        ),
      ],
    );
  }
}

class _SectionItem extends StatelessWidget {
  const _SectionItem({
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
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s8, vertical: 1),
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

/// Common scrollable section body with a title.
class _Body extends StatefulWidget {
  const _Body({required this.title, required this.children, this.onRefresh});
  final String title;
  final List<Widget> children;

  /// Re-fetches this section's API data. Called automatically every time the
  /// user navigates to the section, and exposed as a reload button beside the
  /// title.
  final VoidCallback? onRefresh;

  @override
  State<_Body> createState() => _BodyState();
}

class _BodyState extends State<_Body> {
  @override
  void initState() {
    super.initState();
    if (widget.onRefresh != null) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) widget.onRefresh!();
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return ListView(
      padding: const EdgeInsets.all(AppTokens.s24),
      children: [
        Row(
          children: [
            Text(widget.title,
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 16,
                    fontWeight: FontWeight.w700)),
            const Spacer(),
            if (widget.onRefresh != null)
              IconButton(
                tooltip: 'Reload',
                icon: Icon(Icons.refresh, size: 18, color: c.textSecondary),
                onPressed: widget.onRefresh,
              ),
          ],
        ),
        const SizedBox(height: AppTokens.s16),
        ...widget.children,
      ],
    );
  }
}

class _ToggleRow extends StatelessWidget {
  const _ToggleRow({
    required this.label,
    required this.desc,
    required this.value,
    required this.onChanged,
  });
  final String label;
  final String desc;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                if (desc.isNotEmpty)
                  Text(desc,
                      style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          Switch(value: value, onChanged: onChanged),
        ],
      ),
    );
  }
}

// ── Appearance (theme mode) ───────────────────────────────────────────────
class _AppearanceSection extends ConsumerWidget {
  const _AppearanceSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final mode = ref.watch(themeModeProvider);
    const opts = [
      (ThemeMode.system, 'System', Icons.brightness_auto_outlined),
      (ThemeMode.light, 'Light', Icons.light_mode_outlined),
      (ThemeMode.dark, 'Dark', Icons.dark_mode_outlined),
    ];
    return _Body(
      title: 'Appearance',
      children: [
        Text('Theme',
            style: TextStyle(
                color: c.textSecondary, fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s12),
        Row(
          children: [
            for (final (m, label, icon) in opts)
              Padding(
                padding: const EdgeInsets.only(right: AppTokens.s12),
                child: _ThemeCard(
                  icon: icon,
                  label: label,
                  selected: mode == m,
                  onTap: () => ref.read(themeModeProvider.notifier).set(m),
                ),
              ),
          ],
        ),
        const SizedBox(height: AppTokens.s12),
        Text(
          'System follows your OS appearance setting and switches automatically.',
          style: TextStyle(color: c.textMuted, fontSize: 12),
        ),
      ],
    );
  }
}

class _ThemeCard extends StatelessWidget {
  const _ThemeCard({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.rLg),
      onTap: onTap,
      child: Container(
        width: 132,
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s16, vertical: AppTokens.s16),
        decoration: BoxDecoration(
          color: selected ? c.accentSoft : c.surface,
          border: Border.all(
              color: selected ? c.accent : c.border,
              width: selected ? 1.5 : 1),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
        child: Column(
          children: [
            Icon(icon, size: 22, color: selected ? c.accent : c.textSecondary),
            const SizedBox(height: AppTokens.s8),
            Text(label,
                style: TextStyle(
                    color: selected ? c.accent : c.textPrimary,
                    fontWeight: FontWeight.w600)),
          ],
        ),
      ),
    );
  }
}

// ── General (permissions + behavior) ──────────────────────────────────────
class _GeneralSection extends ConsumerWidget {
  const _GeneralSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final perms = ref.watch(adminPermsProvider);
    final behavior = ref.watch(agentBehaviorProvider);
    final api = ref.read(settingsApiProvider);

    return _Body(
      title: 'General',
      children: [
        Text('Permissions',
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        perms.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (p) => Column(children: [
            _ToggleRow(
              label: 'Skip all-agent permissions',
              desc: 'Auto-accept tool calls for every agent.',
              value: p['skipAllAgentsPermissions'] == true,
              onChanged: (v) => api.post(
                  '/api/admin-permissions',
                  {...p, 'skipAllAgentsPermissions': v},
                  adminPermsProvider),
            ),
            _ToggleRow(
              label: 'Skip main-agent permissions',
              desc: 'Auto-accept tool calls for the main agent only.',
              value: p['skipMainAgentPermissions'] == true,
              onChanged: (v) => api.post(
                  '/api/admin-permissions',
                  {...p, 'skipMainAgentPermissions': v},
                  adminPermsProvider),
            ),
          ]),
        ),
        const SizedBox(height: AppTokens.s16),
        Text('Agent behavior',
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        behavior.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (b) => Column(children: [
            _ToggleRow(
              label: 'After-process hook',
              desc: 'Run the post-processing step after each turn.',
              value: b['afterProcess'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'afterProcess': v}, agentBehaviorProvider),
            ),
            _ToggleRow(
              label: 'Pre-cognitive recall',
              desc: 'Inject relevant memories before processing.',
              value: b['preCognitive'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'preCognitive': v}, agentBehaviorProvider),
            ),
            _ToggleRow(
              label: 'Memory recall',
              desc: 'Consolidate dropped history into memory files and '
                  'inject relevant saved memories into each request.',
              value: b['memoryRecall'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'memoryRecall': v}, agentBehaviorProvider),
            ),
            _ToggleRow(
              label: 'Pre-trigger skill',
              desc: 'Evaluate trigger skills before the main turn.',
              value: b['preTriggerSkill'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'preTriggerSkill': v}, agentBehaviorProvider),
            ),
          ]),
        ),
      ],
    );
  }
}

// ── Channels ──────────────────────────────────────────────────────────────
class _ChannelsSection extends ConsumerWidget {
  const _ChannelsSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final channels = ref.watch(channelsProvider);
    return _Body(
      title: 'Channels',
      onRefresh: () {
        ref.read(channelsProvider.notifier).refresh();
        ref.read(bindingsProvider.notifier).refresh();
      },
      children: [
        Align(
          alignment: Alignment.centerRight,
          child: Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s12),
            child: FilledButton.icon(
              onPressed: () => showDialog(
                  context: context, builder: (_) => const _ChannelEditor()),
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Add channel'),
            ),
          ),
        ),
        if (channels.isEmpty)
          Text('No channels connected.',
              style: TextStyle(color: c.textMuted)),
        for (final ch in channels)
          Container(
            margin: const EdgeInsets.only(bottom: AppTokens.s8),
            padding: const EdgeInsets.all(AppTokens.s12),
            decoration: BoxDecoration(
              color: c.surface,
              border: Border.all(color: c.border),
              borderRadius: BorderRadius.circular(AppTokens.rMd),
            ),
            child: Row(
              children: [
                Container(
                  width: 8,
                  height: 8,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: ch.connectionState == 'connected'
                        ? AppTokens.success
                        : c.textMuted,
                  ),
                ),
                const SizedBox(width: AppTokens.s12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(ch.name.isEmpty ? ch.platformType : ch.name,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontWeight: FontWeight.w600)),
                      Text('${ch.platformType} · ${ch.connectionState}',
                          style:
                              TextStyle(color: c.textMuted, fontSize: 12)),
                    ],
                  ),
                ),
                Switch(
                  value: ch.enabled,
                  onChanged: (v) =>
                      ref.read(channelsProvider.notifier).setEnabled(ch.id, v),
                ),
                IconButton(
                  tooltip: 'Edit',
                  icon: Icon(Icons.edit_outlined,
                      size: 16, color: context.colors.textSecondary),
                  onPressed: () => showDialog(
                      context: context,
                      builder: (_) => _ChannelEditor(existing: ch)),
                ),
                IconButton(
                  tooltip: 'Remove',
                  icon: const Icon(Icons.delete_outline,
                      size: 16, color: AppTokens.danger),
                  onPressed: () =>
                      ref.read(channelsProvider.notifier).delete(ch.id),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Add a messaging channel (Telegram / Feishu / QQ / WeChat).
class _ChannelEditor extends ConsumerStatefulWidget {
  const _ChannelEditor({this.existing});
  /// When set, edits this channel (rename + reconfigure) instead of adding one.
  final ChannelInfo? existing;
  @override
  ConsumerState<_ChannelEditor> createState() => _ChannelEditorState();
}

class _ChannelEditorState extends ConsumerState<_ChannelEditor> {
  String _platform = 'telegram';
  final _name = TextEditingController();
  final _botToken = TextEditingController();
  final _appId = TextEditingController();
  final _appSecret = TextEditingController();
  final _hubUrl = TextEditingController(text: 'https://hub.senclaw.ai');
  bool _sandbox = false;
  bool _registering = false;
  String _chatType = 'group';
  bool _requiresTrigger = false;

  bool get _editing => widget.existing != null;

  @override
  void initState() {
    super.initState();
    final e = widget.existing;
    if (e != null) {
      _platform = e.platformType.isEmpty ? 'telegram' : e.platformType;
      _name.text = e.name;
      final cr = e.credentials;
      _botToken.text = '${cr['botToken'] ?? ''}';
      _appId.text = '${cr['appId'] ?? ''}';
      _appSecret.text = '${cr['appSecret'] ?? ''}';
      if (cr['hubUrl'] != null) _hubUrl.text = '${cr['hubUrl']}';
      _chatType = '${cr['chatType'] ?? 'group'}';
      _requiresTrigger = cr['requiresTrigger'] == true;
      _sandbox = cr['sandbox'] == true;
    }
  }

  static const _platforms = ['telegram', 'feishu', 'qq', 'wechat', 'senclaw'];

  static const _platformLabels = {
    'telegram': 'Telegram',
    'feishu': 'Feishu',
    'qq': 'QQ',
    'wechat': 'WeChat',
    'senclaw': 'Connector',
  };
  static const _platformIcons = {
    'telegram': Icons.send_rounded,
    'feishu': Icons.business_center_rounded,
    'qq': Icons.chat_bubble_rounded,
    'wechat': Icons.forum_rounded,
    'senclaw': Icons.qr_code_rounded,
  };

  @override
  void dispose() {
    _name.dispose();
    _botToken.dispose();
    _appId.dispose();
    _appSecret.dispose();
    _hubUrl.dispose();
    super.dispose();
  }

  Map<String, dynamic> _credentials() {
    switch (_platform) {
      case 'senclaw':
        return {'hubUrl': _hubUrl.text.trim()};
      case 'telegram':
        return {
          if (_botToken.text.trim().isNotEmpty)
            'botToken': _botToken.text.trim(),
          'chatType': _chatType,
          'requiresTrigger': _requiresTrigger,
        };
      case 'feishu':
        return {
          'appId': _appId.text.trim(),
          'appSecret': _appSecret.text.trim(),
          'requiresTrigger': _requiresTrigger,
        };
      case 'qq':
        return {
          'appId': _appId.text.trim(),
          'appSecret': _appSecret.text.trim(),
          'sandbox': _sandbox,
        };
      default: // wechat
        return {
          'appId': _appId.text.trim(),
          'appSecret': _appSecret.text.trim(),
        };
    }
  }

  // ── Small building blocks ─────────────────────────────────────────────

  /// A field caption sitting above its input, antd-style.
  Widget _labeled(BuildContext context, String label, Widget child,
      {String? hint}) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label,
            style: TextStyle(
                color: c.textSecondary,
                fontSize: 12,
                fontWeight: FontWeight.w600)),
        const SizedBox(height: AppTokens.s6),
        child,
        if (hint != null) ...[
          const SizedBox(height: AppTokens.s4),
          Text(hint, style: TextStyle(color: c.textMuted, fontSize: 11)),
        ],
      ],
    );
  }

  /// A tappable platform tile with icon + label.
  Widget _platformTile(BuildContext context, String key) {
    final c = context.colors;
    final selected = _platform == key;
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.rMd),
      // Platform can't change once a channel exists — lock it while editing.
      onTap: _editing ? null : () => setState(() => _platform = key),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 120),
        padding: const EdgeInsets.symmetric(
            vertical: AppTokens.s12, horizontal: AppTokens.s8),
        decoration: BoxDecoration(
          color: selected ? c.accentSoft : c.surfaceAlt,
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          border: Border.all(
            color: selected ? c.accent : c.border,
            width: selected ? 1.5 : 1,
          ),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(_platformIcons[key],
                size: 22, color: selected ? c.accent : c.textSecondary),
            const SizedBox(height: AppTokens.s6),
            Text(_platformLabels[key]!,
                style: TextStyle(
                  fontSize: 12,
                  color: selected ? c.accent : c.textSecondary,
                  fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                )),
          ],
        ),
      ),
    );
  }

  /// One cell of an inline segmented control.
  Widget _segment(BuildContext context, String value, String group,
      String label, ValueChanged<String> onTap) {
    final c = context.colors;
    final selected = value == group;
    return Expanded(
      child: GestureDetector(
        onTap: () => onTap(value),
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: AppTokens.s6),
          decoration: BoxDecoration(
            color: selected ? c.surface : Colors.transparent,
            borderRadius: BorderRadius.circular(AppTokens.rSm),
            border: Border.all(
                color: selected ? c.accent : Colors.transparent, width: 1),
          ),
          alignment: Alignment.center,
          child: Text(label,
              style: TextStyle(
                fontSize: 13,
                color: selected ? c.accent : c.textSecondary,
                fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
              )),
        ),
      ),
    );
  }

  Widget _segmented(BuildContext context, List<List<String>> options,
      String group, ValueChanged<String> onTap) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.all(3),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Row(
        children: [
          for (final o in options)
            _segment(context, o[0], group, o[1], onTap),
        ],
      ),
    );
  }

  /// A bordered card hosting a label + trailing switch.
  Widget _toggleCard(BuildContext context, String title, String subtitle,
      bool value, ValueChanged<bool> onChanged) {
    final c = context.colors;
    return Container(
      padding: const EdgeInsets.fromLTRB(
          AppTokens.s12, AppTokens.s8, AppTokens.s8, AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surfaceAlt,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title,
                    style: TextStyle(
                        color: c.textPrimary,
                        fontSize: 13,
                        fontWeight: FontWeight.w500)),
                const SizedBox(height: 2),
                Text(subtitle,
                    style: TextStyle(color: c.textMuted, fontSize: 11)),
              ],
            ),
          ),
          Switch(value: value, onChanged: onChanged),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final isTelegram = _platform == 'telegram';
    return AlertDialog(
      backgroundColor: c.surface,
      titlePadding:
          const EdgeInsets.fromLTRB(AppTokens.s24, AppTokens.s24, AppTokens.s24, 0),
      contentPadding: const EdgeInsets.fromLTRB(
          AppTokens.s24, AppTokens.s16, AppTokens.s24, 0),
      actionsPadding: const EdgeInsets.all(AppTokens.s16),
      title: Row(
        children: [
          Container(
            width: 38,
            height: 38,
            decoration: BoxDecoration(
              color: c.accentSoft,
              borderRadius: BorderRadius.circular(AppTokens.rMd),
            ),
            child: Icon(Icons.add_link_rounded, size: 20, color: c.accent),
          ),
          const SizedBox(width: AppTokens.s12),
          Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(_editing ? 'Edit channel' : 'Add channel',
                  style: const TextStyle(
                      fontSize: 17, fontWeight: FontWeight.w600)),
              Text(
                  _editing
                      ? 'Rename or reconfigure this channel'
                      : 'Connect a messaging platform to your agent',
                  style: TextStyle(color: c.textMuted, fontSize: 12)),
            ],
          ),
        ],
      ),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _labeled(
                context,
                'PLATFORM',
                GridView.count(
                  shrinkWrap: true,
                  physics: const NeverScrollableScrollPhysics(),
                  crossAxisCount: 4,
                  mainAxisSpacing: AppTokens.s8,
                  crossAxisSpacing: AppTokens.s8,
                  childAspectRatio: 1.05,
                  children: [
                    for (final p in _platforms) _platformTile(context, p),
                  ],
                ),
              ),
              const SizedBox(height: AppTokens.s16),
              _labeled(
                context,
                'NAME',
                TextField(
                  controller: _name,
                  decoration:
                      const InputDecoration(hintText: 'My Telegram bot'),
                ),
              ),
              const SizedBox(height: AppTokens.s16),
              if (isTelegram) ...[
                _labeled(
                  context,
                  'BOT TOKEN',
                  TextField(
                    controller: _botToken,
                    obscureText: true,
                    decoration:
                        const InputDecoration(hintText: '123456:ABC-DEF…'),
                  ),
                  hint: 'Leave empty to use the .env default bot',
                ),
                const SizedBox(height: AppTokens.s16),
                _labeled(
                  context,
                  'CHAT TYPE',
                  _segmented(
                    context,
                    const [
                      ['group', 'Group'],
                      ['private', 'Private'],
                    ],
                    _chatType,
                    (v) => setState(() => _chatType = v),
                  ),
                ),
                const SizedBox(height: AppTokens.s16),
              ] else if (_platform == 'senclaw') ...[
                _labeled(
                  context,
                  'HUB URL',
                  TextField(
                    controller: _hubUrl,
                    decoration: const InputDecoration(
                        hintText: 'https://hub.senclaw.ai'),
                  ),
                  hint: 'Registers with the hub, then shows a QR code for the '
                      'Senclaw mobile app to scan.',
                ),
                const SizedBox(height: AppTokens.s12),
                // Already-registered connector → re-show its pairing QR (built
                // from the stored credentials) so the mobile app can connect.
                if (_editing)
                  Align(
                    alignment: Alignment.centerLeft,
                    child: OutlinedButton.icon(
                      onPressed: () {
                        final cr = widget.existing!.credentials;
                        final hub = '${cr['hubUrl'] ?? _hubUrl.text.trim()}';
                        final payload =
                            'senclaw://connect?hub=${Uri.encodeComponent(hub)}'
                            '&cid=${Uri.encodeComponent('${cr['channelId'] ?? ''}')}'
                            '&key=${Uri.encodeComponent('${cr['encryptionKey'] ?? ''}')}'
                            '&token=${Uri.encodeComponent('${cr['accessToken'] ?? ''}')}';
                        showDialog(
                            context: context,
                            builder: (_) =>
                                _SenclawQrDialog(payload: payload));
                      },
                      icon: const Icon(Icons.qr_code_2, size: 16),
                      label: const Text('Show pairing QR'),
                    ),
                  ),
                if (_editing) const SizedBox(height: AppTokens.s16),
              ] else ...[
                _labeled(
                  context,
                  'APP ID',
                  TextField(
                      controller: _appId,
                      decoration:
                          const InputDecoration(hintText: 'cli_xxx')),
                ),
                const SizedBox(height: AppTokens.s16),
                _labeled(
                  context,
                  'APP SECRET',
                  TextField(
                      controller: _appSecret,
                      obscureText: true,
                      decoration:
                          const InputDecoration(hintText: '••••••••')),
                ),
                const SizedBox(height: AppTokens.s16),
                if (_platform == 'qq')
                  _toggleCard(
                    context,
                    'Sandbox',
                    'Use the QQ sandbox environment',
                    _sandbox,
                    (v) => setState(() => _sandbox = v),
                  ),
                if (_platform == 'qq') const SizedBox(height: AppTokens.s16),
              ],
              if (isTelegram || _platform == 'feishu')
                _toggleCard(
                  context,
                  'Require @mention to trigger',
                  'Only reply when the bot is explicitly mentioned',
                  _requiresTrigger,
                  (v) => setState(() => _requiresTrigger = v),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel')),
        FilledButton(
          onPressed: _registering
              ? null
              : () async {
                  final name = _name.text.trim().isEmpty
                      ? _platform
                      : _name.text.trim();
                  if (_editing) {
                    // Merge old creds with the form so unchanged secrets (e.g.
                    // an untouched bot token) survive the replace.
                    final merged = {
                      ...widget.existing!.credentials,
                      ..._credentials(),
                    };
                    ref.read(channelsProvider.notifier).update(
                          widget.existing!.id,
                          name: name,
                          credentials: merged,
                        );
                    Navigator.of(context).pop();
                    return;
                  }
                  if (_platform == 'senclaw') {
                    await _registerSenclaw();
                    return;
                  }
                  ref.read(channelsProvider.notifier).register(
                        platformType: _platform,
                        name: name,
                        credentials: _credentials(),
                      );
                  Navigator.of(context).pop();
                },
          child: Text(_registering
              ? 'Registering…'
              : _editing
                  ? 'Save'
                  : _platform == 'senclaw'
                      ? 'Register & Get QR'
                      : 'Add channel'),
        ),
      ],
    );
  }

  /// Senclaw Connector: register with the hub (`POST <hub>/v1/channels/register`)
  /// → {channel_id, access_token}, mint a random 32-byte key, persist the
  /// channel locally, then show the pairing QR for the mobile app. Mirrors the
  /// web ChannelSettings flow.
  Future<void> _registerSenclaw() async {
    final hub = _hubUrl.text.trim().isEmpty
        ? 'https://hub.senclaw.ai'
        : _hubUrl.text.trim();
    setState(() => _registering = true);
    try {
      final res = await http.post(Uri.parse('$hub/v1/channels/register'));
      if (res.statusCode < 200 || res.statusCode >= 300) {
        throw 'hub returned ${res.statusCode}';
      }
      final data = jsonDecode(res.body) as Map<String, dynamic>;
      final channelId = '${data['channel_id'] ?? ''}';
      final accessToken = '${data['access_token'] ?? ''}';
      final keyBytes =
          List<int>.generate(32, (_) => Random.secure().nextInt(256));
      final encryptionKey = base64.encode(keyBytes);

      ref.read(channelsProvider.notifier).register(
        platformType: 'senclaw',
        name: _name.text.trim().isEmpty ? 'Connector' : _name.text.trim(),
        credentials: {
          'hubUrl': hub,
          'channelId': channelId,
          'encryptionKey': encryptionKey,
          'accessToken': accessToken,
        },
      );

      if (!mounted) return;
      Navigator.of(context).pop();
      final payload = 'senclaw://connect?hub=${Uri.encodeComponent(hub)}'
          '&cid=${Uri.encodeComponent(channelId)}'
          '&key=${Uri.encodeComponent(encryptionKey)}'
          '&token=${Uri.encodeComponent(accessToken)}';
      showDialog(
          context: context, builder: (_) => _SenclawQrDialog(payload: payload));
    } catch (e) {
      if (mounted) {
        setState(() => _registering = false);
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Pairing failed: $e')));
      }
    }
  }
}

/// Pairing QR for the Senclaw Connector — the mobile app scans this
/// `senclaw://connect?…` payload to bind to the channel.
class _SenclawQrDialog extends StatelessWidget {
  const _SenclawQrDialog({required this.payload});
  final String payload;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AlertDialog(
      backgroundColor: c.surface,
      title: Row(children: const [
        Icon(Icons.qr_code_2, size: 20),
        SizedBox(width: AppTokens.s8),
        Text('Scan to connect'),
      ]),
      content: SizedBox(
        width: 320,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              padding: const EdgeInsets.all(AppTokens.s12),
              decoration: BoxDecoration(
                color: Colors.white,
                borderRadius: BorderRadius.circular(AppTokens.rMd),
              ),
              child: QrImageView(
                data: payload,
                version: QrVersions.auto,
                size: 240,
              ),
            ),
            const SizedBox(height: AppTokens.s12),
            Text('Open the Senclaw mobile app and scan this code to pair.',
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
            const SizedBox(height: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () {
                Clipboard.setData(ClipboardData(text: payload));
                ScaffoldMessenger.of(context).showSnackBar(
                    const SnackBar(content: Text('Pairing link copied')));
              },
              icon: const Icon(Icons.copy, size: 14),
              label: const Text('Copy pairing link'),
            ),
          ],
        ),
      ),
      actions: [
        FilledButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Done')),
      ],
    );
  }
}

// ── Agents ────────────────────────────────────────────────────────────────
class _AgentsSection extends ConsumerWidget {
  const _AgentsSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final agents = ref.watch(agentsProvider).where((a) => !a.isSchedule).toList();
    // Watch bindings so the provider is warm before the editor opens (it
    // pre-selects bound channels in initState) and to show per-profile counts.
    final bindings = ref.watch(bindingsProvider);
    return _Body(
      title: 'Profiles',
      onRefresh: () {
        ref.read(agentsProvider.notifier).refresh();
        ref.read(bindingsProvider.notifier).refresh();
      },
      children: [
        Align(
          alignment: Alignment.centerRight,
          child: Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s12),
            child: FilledButton.icon(
              onPressed: () => showDialog(
                  context: context, builder: (_) => const _AgentEditor()),
              icon: const Icon(Icons.add, size: 16),
              label: const Text('New profile'),
            ),
          ),
        ),
        if (agents.isEmpty)
          Text('No agent profiles.', style: TextStyle(color: c.textMuted)),
        for (final a in agents)
          Container(
            margin: const EdgeInsets.only(bottom: AppTokens.s8),
            padding: const EdgeInsets.all(AppTokens.s12),
            decoration: BoxDecoration(
              color: c.surface,
              border: Border.all(color: c.border),
              borderRadius: BorderRadius.circular(AppTokens.rMd),
            ),
            child: Row(
              children: [
                Icon(Icons.badge_outlined, size: 18, color: c.accent),
                const SizedBox(width: AppTokens.s12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(a.name,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontWeight: FontWeight.w600)),
                      Builder(builder: (_) {
                        final n =
                            bindings.where((b) => b.agentId == a.id).length;
                        final suffix =
                            n > 0 ? ' · $n channel${n == 1 ? '' : 's'}' : '';
                        return Text('folder: ${a.folder}$suffix',
                            style:
                                TextStyle(color: c.textMuted, fontSize: 12));
                      }),
                    ],
                  ),
                ),
                TextButton.icon(
                  onPressed: () => showDialog(
                      context: context, builder: (_) => _AgentEditor(agent: a)),
                  icon: const Icon(Icons.edit_outlined, size: 16),
                  label: const Text('Edit'),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Create (when [agent] is null) or edit an agent profile. Also binds the
/// profile to channels that are still free (web "Bound channels" parity).
class _AgentEditor extends ConsumerStatefulWidget {
  const _AgentEditor({this.agent});
  final AgentInfo? agent;
  @override
  ConsumerState<_AgentEditor> createState() => _AgentEditorState();
}

class _AgentEditorState extends ConsumerState<_AgentEditor> {
  bool get _isCreate => widget.agent == null;

  late final TextEditingController _name =
      TextEditingController(text: widget.agent?.name ?? '');
  late final TextEditingController _folder =
      TextEditingController(text: widget.agent?.folder ?? '');
  late final TextEditingController _prompt =
      TextEditingController(text: widget.agent?.corePrompt ?? '');
  final TextEditingController _memory = TextEditingController();
  late String? _modelId = widget.agent?.modelId;
  bool _editMemory = false;
  bool _folderEdited = false;
  bool _saving = false;

  /// Channel ids the profile should be bound to (pre-filled in edit mode).
  final Set<int> _selectedChannels = {};

  @override
  void initState() {
    super.initState();
    if (_isCreate) {
      _prompt.text = '';
    } else {
      // Pull live SOUL.md + MEMORY.md from disk (the DB corePrompt copy can
      // drift) and pre-select the channels already bound to this profile.
      _loadFiles();
      final bindings = ref.read(bindingsProvider);
      for (final b in bindings) {
        if (b.agentId == widget.agent!.id) _selectedChannels.add(b.channelId);
      }
    }
  }

  Future<void> _loadFiles() async {
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/agents/${widget.agent!.folder}/files');
      if (!mounted || r is! Map) return;
      final soul = '${r['soul'] ?? ''}';
      setState(() {
        if (soul.isNotEmpty) _prompt.text = soul;
        _memory.text = '${r['memory'] ?? ''}';
      });
    } catch (_) {/* non-fatal — keep DB defaults */}
  }

  @override
  void dispose() {
    _name.dispose();
    _folder.dispose();
    _prompt.dispose();
    _memory.dispose();
    super.dispose();
  }

  String _slug(String s) => s
      .toLowerCase()
      .trim()
      .replaceAll(RegExp(r'[^a-z0-9]+'), '-')
      .replaceAll(RegExp(r'^-+|-+$'), '');

  void _onNameChanged(String v) {
    if (_isCreate && !_folderEdited) {
      _folder.text = _slug(v);
    }
  }

  /// Apply binding add/remove diffs for [agentId] against [_selectedChannels].
  void _syncBindings(int agentId) {
    final notifier = ref.read(bindingsProvider.notifier);
    final bindings = ref.read(bindingsProvider);
    final existing = {
      for (final b in bindings)
        if (b.agentId == agentId) b.channelId: b.id,
    };
    // Add newly-selected.
    for (final chId in _selectedChannels) {
      if (!existing.containsKey(chId)) notifier.bind(agentId, chId);
    }
    // Remove de-selected (only those we can identify by binding id).
    for (final entry in existing.entries) {
      if (!_selectedChannels.contains(entry.key) && entry.value >= 0) {
        notifier.unbind(entry.value);
      }
    }
  }

  Future<void> _save() async {
    if (_saving) return;
    final name = _name.text.trim();
    final folder = _slug(_folder.text.trim().isEmpty ? name : _folder.text);
    if (name.isEmpty || folder.isEmpty) return;
    setState(() => _saving = true);

    int? agentId = widget.agent?.id;
    if (_isCreate) {
      agentId = await ref.read(agentsProvider.notifier).registerAgent(
            folder: folder,
            name: name,
            corePrompt: _prompt.text,
            modelId: _modelId,
          );
    } else {
      ref.read(agentsProvider.notifier).updateAgent(
            agentId!,
            name: name,
            corePrompt: _prompt.text,
            modelId: _modelId ?? '',
          );
    }

    // Persist live SOUL.md + MEMORY.md to disk.
    try {
      await ref.read(apiClientProvider).put('/api/agents/$folder/files',
          body: {'soul': _prompt.text, 'memory': _memory.text});
    } catch (_) {/* non-fatal */}

    if (agentId != null) _syncBindings(agentId);

    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final channels = ref.watch(channelsProvider);
    final bindingsNotifier = ref.watch(bindingsProvider.notifier);
    final title = _isCreate
        ? 'New profile'
        : 'Edit agent · ${widget.agent!.folder}';

    return Dialog(
      backgroundColor: c.surface,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 640),
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
              const SizedBox(height: AppTokens.s16),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: TextField(
                      controller: _name,
                      onChanged: _onNameChanged,
                      decoration: const InputDecoration(
                          labelText: 'Name', hintText: 'My assistant'),
                    ),
                  ),
                  if (_isCreate) ...[
                    const SizedBox(width: AppTokens.s8),
                    Expanded(
                      child: TextField(
                        controller: _folder,
                        onChanged: (_) => _folderEdited = true,
                        decoration: const InputDecoration(
                            labelText: 'Folder', hintText: 'my-assistant'),
                      ),
                    ),
                  ],
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              ref.watch(llmConfigsProvider).maybeWhen(
                    data: (d) {
                      final ids = d.configs.map((m) => m.id).toSet();
                      final extra = (_modelId != null &&
                              _modelId!.isNotEmpty &&
                              !ids.contains(_modelId))
                          ? _modelId
                          : null;
                      return DropdownButtonFormField<String>(
                        initialValue: _modelId,
                        isExpanded: true,
                        decoration: const InputDecoration(labelText: 'Model'),
                        items: [
                          const DropdownMenuItem(
                              value: '', child: Text('Global default')),
                          if (extra != null)
                            DropdownMenuItem(
                                value: extra,
                                child: Text('$extra (current)',
                                    overflow: TextOverflow.ellipsis)),
                          for (final m in d.configs)
                            DropdownMenuItem(
                                value: m.id,
                                child: Text(m.label,
                                    overflow: TextOverflow.ellipsis)),
                        ],
                        onChanged: (v) => setState(() => _modelId = v),
                      );
                    },
                    orElse: () => const SizedBox.shrink(),
                  ),
              const SizedBox(height: AppTokens.s16),
              // ── Bound channels ──────────────────────────────────────────
              Text('BOUND CHANNELS',
                  style: TextStyle(
                      color: c.textSecondary,
                      fontSize: 12,
                      fontWeight: FontWeight.w600)),
              const SizedBox(height: AppTokens.s6),
              if (channels.isEmpty)
                Text('No channels — add one in the Channels tab.',
                    style: TextStyle(color: c.textMuted, fontSize: 12))
              else
                Wrap(
                  spacing: AppTokens.s8,
                  runSpacing: AppTokens.s8,
                  children: [
                    for (final ch in channels)
                      _channelChip(c, ch, bindingsNotifier),
                  ],
                ),
              const SizedBox(height: AppTokens.s16),
              Align(
                alignment: Alignment.centerLeft,
                child: SegmentedButton<bool>(
                  style:
                      const ButtonStyle(visualDensity: VisualDensity.compact),
                  segments: const [
                    ButtonSegment(value: false, label: Text('SOUL.md')),
                    ButtonSegment(value: true, label: Text('MEMORY.md')),
                  ],
                  selected: {_editMemory},
                  onSelectionChanged: (s) =>
                      setState(() => _editMemory = s.first),
                ),
              ),
              const SizedBox(height: AppTokens.s8),
              Expanded(
                child: TextField(
                  controller: _editMemory ? _memory : _prompt,
                  expands: true,
                  maxLines: null,
                  textAlignVertical: TextAlignVertical.top,
                  style: const TextStyle(
                      fontFamily: AppTokens.fontMono, fontSize: 13),
                  decoration: InputDecoration(
                      hintText: _editMemory
                          ? 'MEMORY.md (agent long-term memory)…'
                          : 'Core prompt (SOUL.md)…'),
                ),
              ),
              const SizedBox(height: AppTokens.s16),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  TextButton(
                      onPressed: _saving
                          ? null
                          : () => Navigator.of(context).pop(),
                      child: const Text('Cancel')),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed: _saving ? null : _save,
                    child: _saving
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(
                                strokeWidth: 2, color: Colors.white))
                        : Text(_isCreate ? 'Create' : 'Save'),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// A channel as a selectable chip — disabled when bound to another profile.
  Widget _channelChip(
      AppColors c, ChannelInfo ch, BindingsNotifier bindings) {
    final boundTo = bindings.boundAgentOf(ch.id);
    final takenByOther = boundTo != null && boundTo != widget.agent?.id;
    final selected = _selectedChannels.contains(ch.id);
    final label = ch.name.isEmpty ? ch.platformType : ch.name;
    return FilterChip(
      label: Text('$label · ${ch.platformType}',
          style: TextStyle(
              fontSize: 12,
              color: takenByOther
                  ? c.textMuted
                  : (selected ? c.accent : c.textSecondary))),
      selected: selected,
      showCheckmark: true,
      checkmarkColor: c.accent,
      backgroundColor: c.surfaceAlt,
      selectedColor: c.accentSoft,
      side: BorderSide(color: selected ? c.accent : c.border),
      tooltip: takenByOther ? 'Already bound to another profile' : null,
      onSelected: takenByOther
          ? null
          : (v) => setState(() =>
              v ? _selectedChannels.add(ch.id) : _selectedChannels.remove(ch.id)),
    );
  }
}

// ── Tool rules ────────────────────────────────────────────────────────────
class _ToolRulesSection extends ConsumerWidget {
  const _ToolRulesSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final rules = ref.watch(toolRulesProvider);
    final acceptAll = ref.watch(acceptAllProvider);
    return _Body(
      title: 'Tool Rules',
      children: [
        Container(
          margin: const EdgeInsets.only(bottom: AppTokens.s16),
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            color: c.surface,
            border: Border.all(
                color: acceptAll ? AppTokens.danger : c.border),
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          child: Row(
            children: [
              const Icon(Icons.warning_amber_rounded,
                  size: 18, color: AppTokens.danger),
              const SizedBox(width: AppTokens.s12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Dangerously accept all',
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w600)),
                    Text('Auto-accept every tool call without prompting.',
                        style: TextStyle(color: c.textMuted, fontSize: 12)),
                  ],
                ),
              ),
              Switch(
                value: acceptAll,
                onChanged: (v) => ref.read(acceptAllProvider.notifier).set(v),
              ),
            ],
          ),
        ),
        Row(
          children: [
            Expanded(
              child: Text('Auto-accept rules',
                  style: TextStyle(
                      color: c.textSecondary, fontWeight: FontWeight.w700)),
            ),
            TextButton.icon(
              onPressed: () => showDialog(
                  context: context, builder: (_) => const _RuleEditor()),
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Add rule'),
            ),
          ],
        ),
        const SizedBox(height: AppTokens.s8),
        if (rules.isEmpty)
          Text('No rules. Tool calls follow per-agent defaults.',
              style: TextStyle(color: c.textMuted)),
        for (final r in rules)
          Container(
            margin: const EdgeInsets.only(bottom: AppTokens.s8),
            padding: const EdgeInsets.all(AppTokens.s12),
            decoration: BoxDecoration(
              color: c.surface,
              border: Border.all(color: c.border),
              borderRadius: BorderRadius.circular(AppTokens.rMd),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                          r.description.isNotEmpty
                              ? r.description
                              : r.matcherLabel,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontWeight: FontWeight.w600)),
                      Text('${r.action} · ${r.matcherLabel}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 12,
                              fontFamily: AppTokens.fontMono)),
                    ],
                  ),
                ),
                Switch(
                  value: r.enabled,
                  onChanged: (v) =>
                      ref.read(toolRulesProvider.notifier).setEnabled(r, v),
                ),
                IconButton(
                  tooltip: 'Delete',
                  icon: const Icon(Icons.delete_outline,
                      size: 16, color: AppTokens.danger),
                  onPressed: () =>
                      ref.read(toolRulesProvider.notifier).remove(r.id),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

/// Create a tool auto-accept rule (matcher + action), mirroring the web
/// ToolRulesPanel "Add rule" form.
class _RuleEditor extends ConsumerStatefulWidget {
  const _RuleEditor();
  @override
  ConsumerState<_RuleEditor> createState() => _RuleEditorState();
}

class _RuleEditorState extends ConsumerState<_RuleEditor> {
  // value, label, the field key it needs (or null for none/special).
  static const _matcherTypes = [
    ('bash_glob', 'Bash glob', 'pattern'),
    ('bash_regex', 'Bash regex', 'pattern'),
    ('tool_exact', 'Tool name', 'tool_name'),
    ('skill_exact', 'Skill name', 'skill_name'),
    ('mcp_glob', 'MCP glob', 'pattern'),
    ('mcp_server', 'MCP server', 'mcp'),
    ('tool_category', 'Tool category', 'category'),
    ('always', 'All tools', null),
  ];
  static const _actions = [
    ('auto_accept', 'Auto accept'),
    ('auto_accept_and_allow', 'Accept + remember'),
    ('force_request', 'Always ask'),
    ('auto_deny', 'Auto deny'),
  ];
  static const _categories = ['bash', 'file_edit', 'skill', 'agent', 'mcp', 'all'];

  String _action = 'auto_accept';
  String _type = 'bash_glob';
  String _category = 'bash';
  final _pattern = TextEditingController();
  final _toolName = TextEditingController();
  final _skillName = TextEditingController();
  final _server = TextEditingController();
  final _mcpTool = TextEditingController();
  final _desc = TextEditingController();

  @override
  void dispose() {
    _pattern.dispose();
    _toolName.dispose();
    _skillName.dispose();
    _server.dispose();
    _mcpTool.dispose();
    _desc.dispose();
    super.dispose();
  }

  String get _fieldKey =>
      _matcherTypes.firstWhere((t) => t.$1 == _type).$3 ?? '';

  Map<String, dynamic>? _buildMatcher() {
    switch (_type) {
      case 'bash_glob':
      case 'bash_regex':
      case 'mcp_glob':
        if (_pattern.text.trim().isEmpty) return null;
        return {'type': _type, 'pattern': _pattern.text.trim()};
      case 'tool_exact':
        if (_toolName.text.trim().isEmpty) return null;
        return {'type': _type, 'tool_name': _toolName.text.trim()};
      case 'skill_exact':
        if (_skillName.text.trim().isEmpty) return null;
        return {'type': _type, 'skill_name': _skillName.text.trim()};
      case 'mcp_server':
        if (_server.text.trim().isEmpty) return null;
        return {
          'type': _type,
          'server': _server.text.trim(),
          'tool': _mcpTool.text.trim().isEmpty ? null : _mcpTool.text.trim(),
        };
      case 'tool_category':
        return {'type': _type, 'category': _category};
      case 'always':
        return {'type': _type};
      default:
        return null;
    }
  }

  void _submit() {
    final matcher = _buildMatcher();
    if (matcher == null) return; // required field missing
    final id = DateTime.now().microsecondsSinceEpoch.toRadixString(16);
    ref.read(toolRulesProvider.notifier).add(ToolRule(
          id: id,
          action: _action,
          enabled: true,
          description: _desc.text.trim(),
          matcher: matcher,
        ));
    Navigator.of(context).pop();
  }

  Widget _labeled(BuildContext context, String label, Widget child) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(label,
            style: TextStyle(
                color: c.textSecondary,
                fontSize: 12,
                fontWeight: FontWeight.w600)),
        const SizedBox(height: AppTokens.s6),
        child,
        const SizedBox(height: AppTokens.s16),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final key = _fieldKey;
    final valid = _buildMatcher() != null;
    return AlertDialog(
      backgroundColor: c.surface,
      titlePadding: const EdgeInsets.fromLTRB(
          AppTokens.s24, AppTokens.s24, AppTokens.s24, 0),
      contentPadding: const EdgeInsets.fromLTRB(
          AppTokens.s24, AppTokens.s16, AppTokens.s24, 0),
      actionsPadding: const EdgeInsets.all(AppTokens.s16),
      title: const Text('Add tool rule',
          style: TextStyle(fontSize: 17, fontWeight: FontWeight.w600)),
      content: SizedBox(
        width: 440,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _labeled(
                context,
                'ACTION',
                DropdownButtonFormField<String>(
                  initialValue: _action,
                  isExpanded: true,
                  items: [
                    for (final (v, l) in _actions)
                      DropdownMenuItem(value: v, child: Text(l)),
                  ],
                  onChanged: (v) => setState(() => _action = v ?? 'auto_accept'),
                ),
              ),
              _labeled(
                context,
                'MATCH',
                DropdownButtonFormField<String>(
                  initialValue: _type,
                  isExpanded: true,
                  items: [
                    for (final (v, l, _) in _matcherTypes)
                      DropdownMenuItem(value: v, child: Text(l)),
                  ],
                  onChanged: (v) => setState(() => _type = v ?? 'bash_glob'),
                ),
              ),
              if (key == 'pattern')
                _labeled(
                  context,
                  'PATTERN',
                  TextField(
                    controller: _pattern,
                    autofocus: true,
                    decoration: const InputDecoration(
                        hintText: 'e.g. git *  /  mcp__memory__*'),
                  ),
                ),
              if (key == 'tool_name')
                _labeled(
                  context,
                  'TOOL NAME',
                  TextField(
                    controller: _toolName,
                    autofocus: true,
                    decoration:
                        const InputDecoration(hintText: 'e.g. Edit, Write'),
                  ),
                ),
              if (key == 'skill_name')
                _labeled(
                  context,
                  'SKILL NAME',
                  TextField(
                    controller: _skillName,
                    autofocus: true,
                    decoration:
                        const InputDecoration(hintText: 'e.g. ssh-connect'),
                  ),
                ),
              if (key == 'mcp') ...[
                _labeled(
                  context,
                  'MCP SERVER',
                  TextField(
                    controller: _server,
                    autofocus: true,
                    decoration:
                        const InputDecoration(hintText: 'e.g. deepwiki-mcp'),
                  ),
                ),
                _labeled(
                  context,
                  'TOOL (optional — blank = all)',
                  TextField(
                    controller: _mcpTool,
                    decoration: const InputDecoration(hintText: 'e.g. search'),
                  ),
                ),
              ],
              if (key == 'category')
                _labeled(
                  context,
                  'CATEGORY',
                  DropdownButtonFormField<String>(
                    initialValue: _category,
                    isExpanded: true,
                    items: [
                      for (final cat in _categories)
                        DropdownMenuItem(value: cat, child: Text(cat)),
                    ],
                    onChanged: (v) => setState(() => _category = v ?? 'bash'),
                  ),
                ),
              _labeled(
                context,
                'DESCRIPTION (optional)',
                TextField(
                  controller: _desc,
                  decoration:
                      const InputDecoration(hintText: 'Why this rule exists'),
                ),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel')),
        FilledButton(
          onPressed: valid ? _submit : null,
          child: const Text('Add rule'),
        ),
      ],
    );
  }
}

// ── LLM models ────────────────────────────────────────────────────────────
/// Provider presets (baseURL + adapter), ported from the web LLMSettings.
/// Per-provider preset, mirrors web `LLMSettings.tsx` `PROVIDERS`.
/// `modelsUrl` is the OpenAI-style listing endpoint used by Fetch/Test when
/// the chat `baseUrl` speaks a different protocol (e.g. DeepSeek's
/// `/anthropic` chat base can't list models).
class _LlmProviderDef {
  final String name;
  final String baseUrl;
  final String adapt; // 'openai' | 'anthropic' compatible protocol
  final String? modelsUrl;
  final String? keyHint;
  final String? urlHint;
  const _LlmProviderDef(this.name, this.baseUrl, this.adapt,
      {this.modelsUrl, this.keyHint, this.urlHint});
}

const _llmProviders = <String, _LlmProviderDef>{
  'anthropic': _LlmProviderDef('Anthropic', 'https://api.anthropic.com',
      'anthropic',
      keyHint: 'Your Anthropic API key'),
  'openai': _LlmProviderDef('OpenAI', 'https://api.openai.com/v1', 'openai',
      keyHint: 'Your OpenAI API key'),
  'kimi': _LlmProviderDef('Kimi (Moonshot)', 'https://api.moonshot.cn/v1',
      'openai',
      keyHint: 'Your Moonshot API key'),
  'minimax': _LlmProviderDef(
      'MiniMax', 'https://api.minimaxi.com/anthropic', 'anthropic',
      keyHint: 'Your MiniMax API key'),
  'deepseek': _LlmProviderDef(
      'DeepSeek', 'https://api.deepseek.com/anthropic', 'anthropic',
      modelsUrl: 'https://api.deepseek.com/v1',
      keyHint: 'Your DeepSeek API key'),
  'glm': _LlmProviderDef(
      'GLM (Zhipu)', 'https://open.bigmodel.cn/api/paas/v4', 'openai',
      keyHint: 'Your Zhipu API key'),
  'openrouter': _LlmProviderDef(
      'OpenRouter', 'https://openrouter.ai/api', 'openai',
      modelsUrl: 'https://openrouter.ai/api/v1',
      keyHint: 'Your OpenRouter API key'),
  'qwen': _LlmProviderDef('Qwen (Alibaba)',
      'https://dashscope.aliyuncs.com/compatible-mode/v1', 'openai',
      keyHint: 'Your Alibaba Cloud API key'),
  'custom': _LlmProviderDef('Custom LLM endpoint', '', 'openai',
      urlHint: 'https://your-api.com/v1', keyHint: 'Your API key'),
};

/// Whether extended thinking is enabled (from /api/llm-config.thinkingEnabled).
final thinkingEnabledProvider = FutureProvider<bool>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/llm-config');
  return (r is Map ? r['thinkingEnabled'] : null) as bool? ?? true;
});

class _LlmSection extends ConsumerWidget {
  const _LlmSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final llm = ref.watch(llmConfigsProvider);
    final thinking = ref.watch(thinkingEnabledProvider).valueOrNull ?? true;
    return _Body(
      title: 'LLM Models',
      onRefresh: () => ref.invalidate(llmConfigsProvider),
      children: [
        Container(
          margin: const EdgeInsets.only(bottom: AppTokens.s12),
          padding: const EdgeInsets.symmetric(
              horizontal: AppTokens.s12, vertical: AppTokens.s4),
          decoration: BoxDecoration(
            color: c.surface,
            borderRadius: BorderRadius.circular(AppTokens.rMd),
            border: Border.all(color: c.border),
          ),
          child: Row(
            children: [
              Icon(Icons.psychology_outlined, size: 16, color: c.accent),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text('Extended thinking',
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w600)),
                    Text('Let the model reason before replying',
                        style:
                            TextStyle(color: c.textMuted, fontSize: 12)),
                  ],
                ),
              ),
              Switch(
                value: thinking,
                onChanged: (v) async {
                  await ref
                      .read(apiClientProvider)
                      .post('/api/thinking', body: {'enabled': v});
                  ref.invalidate(thinkingEnabledProvider);
                },
              ),
            ],
          ),
        ),
        Align(
          alignment: Alignment.centerRight,
          child: Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s12),
            child: FilledButton.icon(
              onPressed: () => showDialog(
                  context: context, builder: (_) => const _LlmEditor()),
              icon: const Icon(Icons.add, size: 16),
              label: const Text('Add endpoint'),
            ),
          ),
        ),
        llm.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (d) {
            Future<void> setRole(String id, String type) async {
              await ref.read(apiClientProvider).post('/api/llm-config/active',
                  body: {'id': id, 'type': type});
              ref.invalidate(llmConfigsProvider);
            }

            return Column(
              children: [
                for (final m in d.configs)
                  Container(
                    margin: const EdgeInsets.only(bottom: AppTokens.s8),
                    padding: const EdgeInsets.all(AppTokens.s12),
                    decoration: BoxDecoration(
                      color: c.surface,
                      border: Border.all(
                          color: m.id == d.activeId ? c.accent : c.border),
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    child: Row(
                      children: [
                        Icon(
                          m.id == d.activeId
                              ? Icons.radio_button_checked
                              : Icons.radio_button_unchecked,
                          size: 18,
                          color: m.id == d.activeId ? c.accent : c.textMuted,
                        ),
                        const SizedBox(width: AppTokens.s12),
                        Expanded(
                          child: Wrap(
                            crossAxisAlignment: WrapCrossAlignment.center,
                            spacing: AppTokens.s8,
                            children: [
                              Text(m.label,
                                  style: TextStyle(color: c.textPrimary)),
                              if (m.id == d.activeId)
                                _MiniTag('Main', AppTokens.brand),
                              if (m.id == d.activeCognitiveId)
                                _MiniTag('Cognitive', AppTokens.success),
                              if (m.id == d.activeQuickId)
                                _MiniTag('Quick', AppTokens.warning),
                            ],
                          ),
                        ),
                        PopupMenuButton<String>(
                          tooltip: 'Set role',
                          position: PopupMenuPosition.under,
                          onSelected: (t) => setRole(m.id, t),
                          itemBuilder: (_) => [
                            if (m.id != d.activeId)
                              const PopupMenuItem(
                                  value: 'main', child: Text('Set as Main')),
                            if (m.id != d.activeCognitiveId)
                              const PopupMenuItem(
                                  value: 'cognitive',
                                  child: Text('Set as Cognitive')),
                            if (m.id != d.activeQuickId)
                              const PopupMenuItem(
                                  value: 'quick', child: Text('Set as Quick')),
                          ],
                          child: Padding(
                            padding: const EdgeInsets.symmetric(
                                horizontal: AppTokens.s8, vertical: AppTokens.s4),
                            child: Row(mainAxisSize: MainAxisSize.min, children: [
                              Text('Set as…',
                                  style: TextStyle(
                                      color: c.accent, fontSize: 13)),
                              Icon(Icons.expand_more, size: 16, color: c.accent),
                            ]),
                          ),
                        ),
                        IconButton(
                          tooltip: 'Edit',
                          icon: Icon(Icons.edit_outlined,
                              size: 16, color: c.textSecondary),
                          onPressed: () => showDialog(
                              context: context,
                              builder: (_) => _LlmEditor(existing: m)),
                        ),
                        IconButton(
                          tooltip: 'Delete',
                          icon: const Icon(Icons.delete_outline,
                              size: 16, color: AppTokens.danger),
                          onPressed: () async {
                            final ok = await showDialog<bool>(
                              context: context,
                              builder: (dctx) => AlertDialog(
                                title: const Text('Delete endpoint?'),
                                content: Text(
                                    '"${m.label}" will be removed. Chats '
                                    'using it fall back to the active '
                                    'default model.'),
                                actions: [
                                  TextButton(
                                      onPressed: () =>
                                          Navigator.of(dctx).pop(false),
                                      child: const Text('Cancel')),
                                  FilledButton(
                                      style: FilledButton.styleFrom(
                                          backgroundColor: AppTokens.danger),
                                      onPressed: () =>
                                          Navigator.of(dctx).pop(true),
                                      child: const Text('Delete')),
                                ],
                              ),
                            );
                            if (ok != true) return;
                            await ref
                                .read(apiClientProvider)
                                .delete('/api/llm-config/${m.id}');
                            ref.invalidate(llmConfigsProvider);
                          },
                        ),
                      ],
                    ),
                  ),
              ],
            );
          },
        ),
      ],
    );
  }
}

/// Per-model token limits (prefix → (maxTokens, contextLength)), a compact
/// port of the web MODEL_LIMITS_TABLE. Ordered specific-before-general so the
/// first matching prefix wins. Falls back to (8192, 128000) like the web form.
const _llmLimits = <String, (int max, int ctx)>{
  'claude-opus-4': (32000, 200000),
  'claude-sonnet-4': (64000, 200000),
  'claude-haiku-4': (16000, 200000),
  'claude-3-5-sonnet': (8192, 200000),
  'claude-3-5-haiku': (8192, 200000),
  'claude-3': (4096, 200000),
  'gpt-4o-mini': (16384, 128000),
  'gpt-4o': (16384, 128000),
  'gpt-4-turbo': (4096, 128000),
  'gpt-4': (8192, 8192),
  'gpt-3.5-turbo': (4096, 16384),
  'o3': (100000, 200000),
  'o1': (32768, 200000),
  'deepseek-reasoner': (8192, 64000),
  'deepseek-r1': (32000, 64000),
  'deepseek-v3': (32000, 64000),
  'deepseek-chat': (8192, 64000),
  'kimi-k2': (32000, 131072),
  'moonshot-v1-128k': (8192, 128000),
  'glm-z1': (32768, 32768),
  'glm-4': (8192, 128000),
  'qwen3': (32768, 32768),
  'qwen-plus': (8192, 131072),
  'qwen-turbo': (8192, 131072),
  'qwen-max': (8192, 32000),
  'gemini-2.5-pro': (65536, 1000000),
  'gemini-2.5-flash': (65536, 1000000),
  'gemini-1.5': (8192, 1000000),
  'llama-3.3': (32768, 131072),
  'llama-3.1': (32768, 131072),
  'llama-3': (8192, 8192),
  'minimax-m1': (40960, 1000000),
};

/// Add a new custom LLM endpoint, or edit an existing one (when [existing] is
/// set). Editing recreates the endpoint (backend POST is create-only) and
/// re-applies the roles the old endpoint held.
class _LlmEditor extends ConsumerStatefulWidget {
  const _LlmEditor({this.existing});
  /// When set, the editor opens pre-seeded for editing this config.
  final LlmConfig? existing;
  @override
  ConsumerState<_LlmEditor> createState() => _LlmEditorState();
}

class _LlmEditorState extends ConsumerState<_LlmEditor> {
  late String _provider = (widget.existing?.provider.isNotEmpty ?? false)
      ? widget.existing!.provider
      : 'openai';
  late String _adapt = (widget.existing?.adapt.isNotEmpty ?? false)
      ? widget.existing!.adapt
      : (_llmProviders[_provider]?.adapt ?? 'openai');
  late final _baseUrl = TextEditingController(
      text: (widget.existing?.baseUrl.isNotEmpty ?? false)
          ? widget.existing!.baseUrl
          : (_llmProviders[_provider]?.baseUrl ?? ''));
  late final _apiKey =
      TextEditingController(text: widget.existing?.apiKey ?? '');
  late final _model =
      TextEditingController(text: widget.existing?.modelName ?? '');

  /// Vision override: null = auto-infer from model name (daemon side).
  late bool? _vision = widget.existing?.vision;
  String? _testResult;
  bool _busy = false;
  List<String> _availableModels = const [];

  /// Fetch/Test target: some providers list models on a different base than
  /// the chat endpoint (web parity: `PROVIDERS[p].modelsUrl ?? baseURL`).
  String get _probeBaseUrl =>
      _llmProviders[_provider]?.modelsUrl ?? _baseUrl.text.trim();

  bool get _isEdit => widget.existing != null;

  (int, int) _limitsFor(String model) {
    final lower = model.toLowerCase();
    for (final e in _llmLimits.entries) {
      if (lower.startsWith(e.key)) return e.value;
    }
    final ex = widget.existing;
    if (ex != null && ex.maxTokens > 0) {
      return (ex.maxTokens, ex.contextLength > 0 ? ex.contextLength : 128000);
    }
    return (8192, 128000);
  }

  Future<void> _fetchModels() async {
    setState(() {
      _busy = true;
      _testResult = null;
    });
    try {
      final r = await ref.read(apiClientProvider).post('/api/llm-config/models',
          body: {
            'baseURL': _probeBaseUrl,
            'apiKey': _apiKey.text.trim(),
            'adapt': _adapt,
          });
      final ok = r is Map && r['success'] == true;
      final models = ((r is Map ? r['models'] : null) as List?)
              ?.map((e) => '$e')
              .toList() ??
          const <String>[];
      setState(() {
        if (ok && models.isNotEmpty) {
          _availableModels = models;
          if (_model.text.isEmpty) _model.text = models.first;
          _testResult = '✓ Loaded ${models.length} model(s)';
        } else {
          _testResult = '✗ ${(r is Map ? r['message'] : null) ?? 'No models'}';
        }
      });
    } catch (e) {
      setState(() => _testResult = '✗ $e');
    } finally {
      setState(() => _busy = false);
    }
  }

  @override
  void dispose() {
    _baseUrl.dispose();
    _apiKey.dispose();
    _model.dispose();
    super.dispose();
  }

  Map<String, dynamic> get _body {
    final model = _model.text.trim();
    final (maxTokens, ctx) = _limitsFor(model);
    return {
      'provider': _provider,
      'baseURL': _baseUrl.text.trim(),
      'apiKey': _apiKey.text.trim(),
      'modelName': model,
      'adapt': _adapt,
      'maxTokens': maxTokens,
      'contextLength': ctx,
      'label':
          '$model (${_llmProviders[_provider]?.name ?? _provider})',
      // Only send an explicit override; omitting lets the daemon auto-infer.
      if (_vision != null) 'vision': _vision,
    };
  }

  Future<void> _test() async {
    setState(() {
      _busy = true;
      _testResult = null;
    });
    try {
      await ref.read(apiClientProvider).post('/api/llm-config/test', body: {
        'baseURL': _probeBaseUrl,
        'apiKey': _apiKey.text.trim(),
        'adapt': _adapt,
      });
      setState(() => _testResult = '✓ Connection OK');
    } catch (e) {
      setState(() => _testResult = '✗ $e');
    } finally {
      setState(() => _busy = false);
    }
  }

  Future<void> _save() async {
    if (_model.text.trim().isEmpty) {
      setState(() => _testResult = '✗ Model name is required');
      return;
    }
    setState(() => _busy = true);
    final api = ref.read(apiClientProvider);
    try {
      if (!_isEdit) {
        await api.post('/api/llm-config', body: _body);
      } else {
        // Backend POST is create-only (new id), so edit = recreate + restore
        // roles. Create the replacement first so a failure leaves the old one.
        final old = widget.existing!;
        final data = ref.read(llmConfigsProvider).valueOrNull;
        final roles = <String>[
          if (data?.activeId == old.id) 'main',
          if (data?.activeQuickId == old.id) 'quick',
          if (data?.activeCognitiveId == old.id) 'cognitive',
        ];
        final created = await api.post('/api/llm-config', body: _body);
        final newId = created is Map ? '${created['id'] ?? ''}' : '';
        await api.delete('/api/llm-config/${old.id}');
        if (newId.isNotEmpty) {
          for (final type in roles) {
            await api.post('/api/llm-config/active',
                body: {'id': newId, 'type': type});
          }
        }
      }
      ref.invalidate(llmConfigsProvider);
      if (mounted) Navigator.of(context).pop();
    } catch (e) {
      setState(() {
        _busy = false;
        _testResult = '✗ $e';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final providerKeys = _llmProviders.keys.toList();
    final def = _llmProviders[_provider];
    return AlertDialog(
      backgroundColor: c.surface,
      title: Text(_isEdit ? 'Edit LLM endpoint' : 'Add LLM endpoint'),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            DropdownButtonFormField<String>(
              initialValue: _provider,
              decoration: const InputDecoration(labelText: 'Provider'),
              items: [
                if (!providerKeys.contains(_provider))
                  DropdownMenuItem(value: _provider, child: Text(_provider)),
                for (final k in providerKeys)
                  DropdownMenuItem(
                      value: k, child: Text(_llmProviders[k]!.name)),
              ],
              onChanged: (v) {
                if (v == null) return;
                setState(() {
                  _provider = v;
                  final preset = _llmProviders[v];
                  if (preset != null) {
                    _baseUrl.text = preset.baseUrl;
                    _adapt = preset.adapt;
                  }
                });
              },
            ),
            const SizedBox(height: AppTokens.s8),
            TextField(
                controller: _baseUrl,
                decoration: InputDecoration(
                    labelText: 'Base URL',
                    hintText: def?.urlHint)),
            const SizedBox(height: AppTokens.s8),
            TextField(
                controller: _apiKey,
                obscureText: true,
                decoration: InputDecoration(
                    labelText: 'API key',
                    hintText: _isEdit
                        ? 'Stored key — edit to replace'
                        : def?.keyHint)),
            const SizedBox(height: AppTokens.s8),
            // Protocol the endpoint speaks — pre-set by the provider preset,
            // editable for custom/self-hosted gateways.
            DropdownButtonFormField<String>(
              initialValue: _adapt == 'anthropic' ? 'anthropic' : 'openai',
              decoration: const InputDecoration(
                  labelText: 'API type (compatibility)'),
              items: const [
                DropdownMenuItem(
                    value: 'openai', child: Text('OpenAI-compatible')),
                DropdownMenuItem(
                    value: 'anthropic', child: Text('Anthropic-compatible')),
              ],
              onChanged: (v) => setState(() => _adapt = v ?? 'openai'),
            ),
            const SizedBox(height: AppTokens.s8),
            Row(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Expanded(
                  child: TextField(
                      controller: _model,
                      onChanged: (_) => setState(() {}),
                      decoration:
                          const InputDecoration(labelText: 'Model name')),
                ),
                const SizedBox(width: AppTokens.s8),
                OutlinedButton(
                  onPressed: _busy ? null : _fetchModels,
                  child: const Text('Fetch'),
                ),
              ],
            ),
            if (_availableModels.isNotEmpty) ...[
              const SizedBox(height: AppTokens.s8),
              DropdownButtonFormField<String>(
                initialValue:
                    _availableModels.contains(_model.text) ? _model.text : null,
                isExpanded: true,
                decoration:
                    const InputDecoration(labelText: 'Available models'),
                items: [
                  for (final m in _availableModels)
                    DropdownMenuItem(
                        value: m,
                        child: Text(m,
                            maxLines: 1, overflow: TextOverflow.ellipsis)),
                ],
                onChanged: (v) => setState(() => _model.text = v ?? ''),
              ),
            ],
            if (_testResult != null) ...[
              const SizedBox(height: AppTokens.s12),
              Text(_testResult!,
                  style: TextStyle(
                      color: _testResult!.startsWith('✓')
                          ? AppTokens.success
                          : AppTokens.danger,
                      fontSize: 12)),
            ],
            const SizedBox(height: AppTokens.s8),
            // Vision tri-state: Auto follows the daemon's model-name
            // inference; the explicit options override it (web parity).
            DropdownButtonFormField<String>(
              initialValue: _vision == null ? 'auto' : (_vision! ? 'on' : 'off'),
              decoration:
                  const InputDecoration(labelText: 'Vision (image input)'),
              items: const [
                DropdownMenuItem(
                    value: 'auto', child: Text('Auto (infer from model name)')),
                DropdownMenuItem(value: 'on', child: Text('Supported')),
                DropdownMenuItem(value: 'off', child: Text('Not supported')),
              ],
              onChanged: (v) => setState(
                  () => _vision = v == 'auto' ? null : v == 'on'),
            ),
          ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('Cancel')),
        OutlinedButton(
            onPressed: _busy ? null : _test, child: const Text('Test')),
        FilledButton(
            onPressed: _busy ? null : _save, child: const Text('Save')),
      ],
    );
  }
}

// ── Local models ──────────────────────────────────────────────────────────
class _LocalModelsSection extends ConsumerStatefulWidget {
  const _LocalModelsSection();
  @override
  ConsumerState<_LocalModelsSection> createState() =>
      _LocalModelsSectionState();
}

class _LocalModelsSectionState extends ConsumerState<_LocalModelsSection> {
  Timer? _poll;

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  // Poll the list every ~1.5s while any model is downloading; stop otherwise.
  void _syncPoll(bool anyDownloading) {
    if (anyDownloading && _poll == null) {
      _poll = Timer.periodic(const Duration(milliseconds: 1500), (_) {
        ref.invalidate(localModelsProvider);
      });
    } else if (!anyDownloading && _poll != null) {
      _poll!.cancel();
      _poll = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final models = ref.watch(localModelsProvider);
    final list = models.valueOrNull ?? const [];
    _syncPoll(list.any((m) => m.downloading));
    final runtime = ref.watch(localModelsRuntimeProvider);
    return _Body(
      title: 'Local Models',
      children: [
        const _LocalInferenceSettings(),
        const SizedBox(height: AppTokens.s16),
        // Runtime environment (platform + models dir) — web LocalModelsSettings.
        runtime.maybeWhen(
          orElse: () => const SizedBox.shrink(),
          data: (rt) {
            final platform = '${rt['platform'] ?? ''}';
            final dir = '${rt['local_models_dir'] ?? ''}';
            final isMac = platform == 'macos';
            return Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (!isMac && platform.isNotEmpty)
                  Container(
                    padding: const EdgeInsets.all(AppTokens.s12),
                    margin: const EdgeInsets.only(bottom: AppTokens.s8),
                    decoration: BoxDecoration(
                      color: AppTokens.warning.withValues(alpha: 0.12),
                      border: Border.all(color: AppTokens.warning),
                      borderRadius: BorderRadius.circular(AppTokens.rMd),
                    ),
                    child: Row(children: [
                      const Icon(Icons.warning_amber_rounded,
                          size: 16, color: AppTokens.warning),
                      const SizedBox(width: AppTokens.s8),
                      Expanded(
                        child: Text(
                          'Platform: $platform — local MLX inference only runs '
                          'on macOS (Apple Silicon).',
                          style: TextStyle(
                              color: c.textSecondary, fontSize: 12),
                        ),
                      ),
                    ]),
                  ),
                if (dir.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: AppTokens.s12),
                    child: Row(children: [
                      Icon(Icons.folder_outlined,
                          size: 14, color: c.textMuted),
                      const SizedBox(width: AppTokens.s6),
                      Expanded(
                        child: SelectableText(
                          dir,
                          maxLines: 1,
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 11,
                              fontFamily: AppTokens.fontMono),
                        ),
                      ),
                    ]),
                  ),
              ],
            );
          },
        ),
        models.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (list) => Column(
            children: [
              for (final m in list)
                Container(
                  margin: const EdgeInsets.only(bottom: AppTokens.s8),
                  padding: const EdgeInsets.all(AppTokens.s12),
                  decoration: BoxDecoration(
                    color: c.surface,
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                  ),
                  child: Row(
                    children: [
                      Icon(
                        m.loaded
                            ? Icons.bolt
                            : m.installed
                                ? Icons.download_done
                                : Icons.cloud_outlined,
                        size: 18,
                        color: m.loaded
                            ? AppTokens.success
                            : m.installed
                                ? c.accent
                                : c.textMuted,
                      ),
                      const SizedBox(width: AppTokens.s12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(m.label,
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(color: c.textPrimary)),
                            if (m.downloading) ...[
                              const SizedBox(height: 4),
                              ClipRRect(
                                borderRadius:
                                    BorderRadius.circular(AppTokens.rSm),
                                child: LinearProgressIndicator(
                                    value: m.downloadProgress,
                                    minHeight: 4,
                                    backgroundColor: c.surfaceAlt),
                              ),
                              const SizedBox(height: 2),
                              Text(
                                  m.downloadProgress == null
                                      ? 'Downloading…'
                                      : 'Downloading ${(m.downloadProgress! * 100).toStringAsFixed(0)}%',
                                  style: TextStyle(
                                      color: c.accent, fontSize: 11)),
                            ] else
                              Text('${m.sizeGb.toStringAsFixed(1)} GB',
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                          ],
                        ),
                      ),
                      const SizedBox(width: AppTokens.s8),
                      _modelAction(ref, m),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _modelAction(WidgetRef ref, LocalModel m) {
    Future<void> hit(String action) async {
      // Model ids are HF repos with slashes — encode so they don't break the
      // URL path (matches the web `encodeURIComponent`).
      try {
        await ref
            .read(apiClientProvider)
            .post('/api/local-models/${Uri.encodeComponent(m.id)}/$action');
      } catch (e) {
        if (context.mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text('$e')));
        }
      }
      ref.invalidate(localModelsProvider);
    }

    if (m.downloading) {
      return TextButton.icon(
        onPressed: () => hit('cancel'),
        icon: const Icon(Icons.close, size: 16, color: AppTokens.danger),
        label: const Text('Cancel',
            style: TextStyle(color: AppTokens.danger)),
      );
    }
    if (!m.installed) {
      return TextButton.icon(
        onPressed: () => hit('download'),
        icon: const Icon(Icons.download, size: 16),
        label: const Text('Download'),
      );
    }
    // Installed: load/unload + use-as-LLM + delete (web LocalModelsSettings).
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        TextButton(
          onPressed: () async {
            await ref
                .read(apiClientProvider)
                .post('/api/local-models/${m.id}/use-as-llm');
            ref.invalidate(localModelsProvider);
            ref.invalidate(llmConfigsProvider);
          },
          child: const Text('Use as LLM'),
        ),
        TextButton(
          onPressed: () => hit(m.loaded ? 'unload' : 'load'),
          child: Text(m.loaded ? 'Unload' : 'Load'),
        ),
        IconButton(
          tooltip: 'Delete',
          icon: const Icon(Icons.delete_outline, size: 16,
              color: AppTokens.danger),
          onPressed: () async {
            await ref
                .read(apiClientProvider)
                .delete('/api/local-models/${m.id}');
            ref.invalidate(localModelsProvider);
          },
        ),
      ],
    );
  }
}

// ── Media models (whisper / tts / ocr) ────────────────────────────────────
class _MediaModelsSection extends ConsumerStatefulWidget {
  const _MediaModelsSection({required this.domain, required this.title});
  final String domain;
  final String title;
  @override
  ConsumerState<_MediaModelsSection> createState() =>
      _MediaModelsSectionState();
}

class _MediaModelsSectionState extends ConsumerState<_MediaModelsSection> {
  Timer? _poll;
  String get domain => widget.domain;

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  /// Refresh the list every 1.5s while any model is downloading, then stop —
  /// keeps the progress/button state live without polling forever (web parity).
  void _syncPoll(List<MediaModel> list) {
    final active = list.any((m) => m.downloading);
    if (active && _poll == null) {
      _poll = Timer.periodic(const Duration(milliseconds: 1500),
          (_) => ref.invalidate(mediaModelsProvider(domain)));
    } else if (!active && _poll != null) {
      _poll!.cancel();
      _poll = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final models = ref.watch(mediaModelsProvider(domain));
    return _Body(
      title: widget.title,
      children: [
        _MediaSettingsCard(domain: domain),
        const SizedBox(height: AppTokens.s12),
        models.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (list) {
            WidgetsBinding.instance
                .addPostFrameCallback((_) => _syncPoll(list));
            return Column(
            children: [
              for (final m in list)
                Container(
                  margin: const EdgeInsets.only(bottom: AppTokens.s8),
                  padding: const EdgeInsets.all(AppTokens.s12),
                  decoration: BoxDecoration(
                    color: c.surface,
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                  ),
                  child: Row(
                    children: [
                      Icon(
                        m.installed
                            ? Icons.download_done
                            : Icons.cloud_outlined,
                        size: 18,
                        color: m.installed ? AppTokens.success : c.textMuted,
                      ),
                      const SizedBox(width: AppTokens.s12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(m.label,
                                style: TextStyle(color: c.textPrimary)),
                            if (m.description.isNotEmpty)
                              Text(m.description,
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                            if (m.sizeGb > 0)
                              Text('${m.sizeGb.toStringAsFixed(2)} GB',
                                  style: TextStyle(
                                      color: c.textMuted, fontSize: 12)),
                          ],
                        ),
                      ),
                      const SizedBox(width: AppTokens.s8),
                      m.installed
                          ? IconButton(
                              tooltip: 'Delete',
                              icon: const Icon(Icons.delete_outline,
                                  size: 16, color: AppTokens.danger),
                              onPressed: () async {
                                try {
                                  await ref.read(apiClientProvider).delete(
                                      '/api/$domain/models/${Uri.encodeComponent(m.id)}');
                                } catch (e) {
                                  if (context.mounted) {
                                    ScaffoldMessenger.of(context).showSnackBar(
                                        SnackBar(content: Text('$e')));
                                  }
                                }
                                ref.invalidate(mediaModelsProvider(domain));
                              },
                            )
                          : m.downloading
                              ? _downloadingChip(c, m)
                              : TextButton.icon(
                                  onPressed: () async {
                                    try {
                                      await ref.read(apiClientProvider).post(
                                          '/api/$domain/models/${Uri.encodeComponent(m.id)}/download');
                                    } catch (e) {
                                      if (context.mounted) {
                                        ScaffoldMessenger.of(context)
                                            .showSnackBar(SnackBar(
                                                content: Text('$e')));
                                      }
                                    }
                                    ref.invalidate(
                                        mediaModelsProvider(domain));
                                  },
                                  icon: const Icon(Icons.download, size: 16),
                                  label: const Text('Download'),
                                ),
                    ],
                  ),
                ),
            ],
          );
          },
        ),
      ],
    );
  }

  /// Inline "Downloading NN%" indicator shown in place of the Download button
  /// while a fetch is in flight (disabled — the daemon rejects re-requests).
  Widget _downloadingChip(dynamic c, MediaModel m) {
    final pct = m.downloadProgress != null
        ? ' ${(m.downloadProgress! * 100).round()}%'
        : '';
    return Row(mainAxisSize: MainAxisSize.min, children: [
      SizedBox(
        width: 14,
        height: 14,
        child: CircularProgressIndicator(
          strokeWidth: 2,
          value: m.downloadProgress,
        ),
      ),
      const SizedBox(width: AppTokens.s8),
      Text('Downloading$pct',
          style: TextStyle(color: c.textMuted, fontSize: 12)),
    ]);
  }
}

// ── Embedding ─────────────────────────────────────────────────────────────
// Per-provider presets (baseURL + default modelName), mirrors the web.
const _embedPresets = {
  'none': ('', ''),
  'openai': ('https://api.openai.com/v1', 'text-embedding-3-small'),
  'openrouter': ('https://openrouter.ai/api/v1', 'openai/text-embedding-3-small'),
  'ollama': ('http://localhost:11434', 'nomic-embed-text'),
  'local': ('', 'all-MiniLM-L6-v2'),
};

class _EmbeddingSection extends ConsumerStatefulWidget {
  const _EmbeddingSection();
  @override
  ConsumerState<_EmbeddingSection> createState() => _EmbeddingSectionState();
}

class _EmbeddingSectionState extends ConsumerState<_EmbeddingSection> {
  String _provider = 'none';
  final _apiKey = TextEditingController();
  final _baseUrl = TextEditingController();
  final _modelName = TextEditingController();
  final _modelPath = TextEditingController();
  final _dimensions = TextEditingController();
  bool _seeded = false;
  bool _saving = false;

  @override
  void dispose() {
    _apiKey.dispose();
    _baseUrl.dispose();
    _modelName.dispose();
    _modelPath.dispose();
    _dimensions.dispose();
    super.dispose();
  }

  void _applyPreset(String p) {
    final preset = _embedPresets[p];
    setState(() {
      _provider = p;
      if (preset != null) {
        if (_baseUrl.text.isEmpty) _baseUrl.text = preset.$1;
        if (_modelName.text.isEmpty) _modelName.text = preset.$2;
      }
    });
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    try {
      await ref.read(apiClientProvider).post('/api/embedding-config', body: {
        'provider': _provider,
        'apiKey': _apiKey.text.trim(),
        'baseURL': _baseUrl.text.trim(),
        'modelName': _modelName.text.trim(),
        'modelPath': _modelPath.text.trim(),
        'dimensions': int.tryParse(_dimensions.text.trim()),
      });
      ref.invalidate(embeddingConfigProvider);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Embedding config saved')));
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

  @override
  Widget build(BuildContext context) {
    final cfg = ref.watch(embeddingConfigProvider);
    return cfg.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => _Body(title: 'Embedding', children: [Text('$e')]),
      data: (d) {
        if (!_seeded) {
          _seeded = true;
          _provider = '${d['provider'] ?? 'none'}';
          _apiKey.text = '${d['apiKey'] ?? ''}';
          _baseUrl.text = '${d['baseURL'] ?? ''}';
          _modelName.text = '${d['modelName'] ?? ''}';
          _modelPath.text = '${d['modelPath'] ?? ''}';
          _dimensions.text = d['dimensions'] == null ? '' : '${d['dimensions']}';
        }
        final needsKey = _provider == 'openai' || _provider == 'openrouter';
        final needsUrl = needsKey || _provider == 'ollama';
        final isLocal = _provider == 'local';
        return _Body(
          title: 'Embedding',
          children: [
            DropdownButtonFormField<String>(
              initialValue: _provider,
              decoration: const InputDecoration(labelText: 'Provider'),
              items: const [
                DropdownMenuItem(value: 'none', child: Text('None (disabled)')),
                DropdownMenuItem(value: 'openai', child: Text('OpenAI')),
                DropdownMenuItem(
                    value: 'openrouter', child: Text('OpenRouter')),
                DropdownMenuItem(value: 'ollama', child: Text('Ollama')),
                DropdownMenuItem(
                    value: 'local', child: Text('Local (on-device)')),
              ],
              onChanged: (v) => _applyPreset(v ?? 'none'),
            ),
            if (_provider != 'none') ...[
              const SizedBox(height: AppTokens.s12),
              if (needsKey)
                TextField(
                  controller: _apiKey,
                  obscureText: true,
                  decoration: const InputDecoration(labelText: 'API key'),
                ),
              if (needsUrl) ...[
                const SizedBox(height: AppTokens.s8),
                TextField(
                  controller: _baseUrl,
                  decoration: const InputDecoration(labelText: 'Base URL'),
                ),
              ],
              const SizedBox(height: AppTokens.s8),
              TextField(
                controller: _modelName,
                decoration: const InputDecoration(labelText: 'Model name'),
              ),
              if (isLocal) ...[
                const SizedBox(height: AppTokens.s8),
                TextField(
                  controller: _modelPath,
                  decoration: const InputDecoration(
                      labelText: 'Model path (optional)'),
                ),
              ],
              const SizedBox(height: AppTokens.s8),
              TextField(
                controller: _dimensions,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(
                    labelText: 'Dimensions (optional)'),
              ),
            ],
            const SizedBox(height: AppTokens.s16),
            Align(
              alignment: Alignment.centerLeft,
              child: FilledButton.icon(
                onPressed: _saving ? null : _save,
                icon: _saving
                    ? const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.save_outlined, size: 16),
                label: const Text('Save'),
              ),
            ),
            const SizedBox(height: AppTokens.s24),
            const _EmbeddingLocalModels(),
          ],
        );
      },
    );
  }
}

/// Browse + download curated local embedding models (web EmbeddingSettings
/// local section). Only shown when a local-embed backend is compiled in.
class _EmbeddingLocalModels extends ConsumerWidget {
  const _EmbeddingLocalModels();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final feats = ref.watch(embeddingFeaturesProvider).valueOrNull;
    final localOk = feats != null &&
        (feats['candle'] == true ||
            feats['candle_metal'] == true ||
            feats['mlx_static'] == true);
    if (!localOk) return const SizedBox.shrink();
    final models = ref.watch(embeddingModelsProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text('Local models',
            style:
                TextStyle(color: c.textPrimary, fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        models.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (list) => Column(
            children: [
              for (final m in list)
                Container(
                  margin: const EdgeInsets.only(bottom: AppTokens.s8),
                  padding: const EdgeInsets.all(AppTokens.s12),
                  decoration: BoxDecoration(
                    color: c.surface,
                    border: Border.all(color: c.border),
                    borderRadius: BorderRadius.circular(AppTokens.rMd),
                  ),
                  child: Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text('${m['id'] ?? ''}',
                                style: TextStyle(
                                    color: c.textPrimary,
                                    fontWeight: FontWeight.w600)),
                            Text(
                                '${m['repo'] ?? ''} · ${m['dimensions'] ?? '?'}d · ${m['size_hint'] ?? ''}',
                                style: TextStyle(
                                    color: c.textMuted, fontSize: 12)),
                          ],
                        ),
                      ),
                      if (m['installed'] == true)
                        Row(children: [
                          const Icon(Icons.download_done,
                              size: 16, color: AppTokens.success),
                          const SizedBox(width: 4),
                          Text('Installed',
                              style: TextStyle(
                                  color: AppTokens.success, fontSize: 12)),
                        ])
                      else
                        TextButton.icon(
                          onPressed: () async {
                            await ref.read(apiClientProvider).post(
                                '/api/embedding/download-model',
                                body: {'model': m['id']});
                            ref.invalidate(embeddingModelsProvider);
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                  const SnackBar(
                                      content: Text('Downloading model…')));
                            }
                          },
                          icon: const Icon(Icons.download, size: 16),
                          label: const Text('Download'),
                        ),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }
}

final embeddingFeaturesProvider =
    FutureProvider<Map<String, dynamic>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/embedding/features');
  return r is Map ? r.cast<String, dynamic>() : {};
});

final embeddingModelsProvider =
    FutureProvider<List<Map<String, dynamic>>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/embedding/models');
  return ((r is Map ? r['models'] : r) as List? ?? const [])
      .whereType<Map>()
      .map((e) => e.cast<String, dynamic>())
      .toList();
});

// ── Memory (cognitive config) ─────────────────────────────────────────────
class _MemorySection extends ConsumerStatefulWidget {
  const _MemorySection();
  @override
  ConsumerState<_MemorySection> createState() => _MemorySectionState();
}

class _MemorySectionState extends ConsumerState<_MemorySection> {
  bool _seeded = false;
  bool _enabled = true;
  bool _autoReflection = false;
  // Numeric tuning fields (web CognitiveSettings).
  final _maxConcurrent = TextEditingController();
  final _maxOutputChars = TextEditingController();
  final _reflectMinChars = TextEditingController();
  final _reflectMaxChars = TextEditingController();
  final _reflectCooldownMs = TextEditingController();
  final _maintenanceHours = TextEditingController();
  bool _saving = false;

  @override
  void dispose() {
    _maxConcurrent.dispose();
    _maxOutputChars.dispose();
    _reflectMinChars.dispose();
    _reflectMaxChars.dispose();
    _reflectCooldownMs.dispose();
    _maintenanceHours.dispose();
    super.dispose();
  }

  void _seed(Map<String, dynamic> d) {
    _enabled = d['enabled'] == true;
    _autoReflection = d['autoReflection'] == true;
    String s(String k) => d[k] == null ? '' : '${d[k]}';
    _maxConcurrent.text = s('maxConcurrent');
    _maxOutputChars.text = s('maxOutputChars');
    _reflectMinChars.text = s('reflectMinChars');
    _reflectMaxChars.text = s('reflectMaxChars');
    _reflectCooldownMs.text = s('reflectCooldownMs');
    _maintenanceHours.text = s('maintenanceIntervalHours');
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    int? n(TextEditingController c) => int.tryParse(c.text.trim());
    try {
      await ref.read(apiClientProvider).post('/api/cognitive-config', body: {
        'enabled': _enabled,
        'autoReflection': _autoReflection,
        if (n(_maxConcurrent) != null) 'maxConcurrent': n(_maxConcurrent),
        if (n(_maxOutputChars) != null) 'maxOutputChars': n(_maxOutputChars),
        if (n(_reflectMinChars) != null) 'reflectMinChars': n(_reflectMinChars),
        if (n(_reflectMaxChars) != null) 'reflectMaxChars': n(_reflectMaxChars),
        if (n(_reflectCooldownMs) != null)
          'reflectCooldownMs': n(_reflectCooldownMs),
        if (n(_maintenanceHours) != null)
          'maintenanceIntervalHours': n(_maintenanceHours),
      });
      ref.invalidate(cognitiveConfigProvider);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Cognitive config saved')));
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

  @override
  Widget build(BuildContext context) {
    final cfg = ref.watch(cognitiveConfigProvider);
    return cfg.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) => _Body(title: 'Memory (Cognitive)', children: [Text('$e')]),
      data: (d) {
        if (!_seeded) {
          _seeded = true;
          _seed(d);
        }
        return _Body(
          title: 'Memory (Cognitive)',
          children: [
            _ToggleRow(
              label: 'Enable cognitive layer',
              desc: 'Graph + Hebbian recall across sessions.',
              value: _enabled,
              onChanged: (v) => setState(() => _enabled = v),
            ),
            _ToggleRow(
              label: 'Auto-reflect on every user message',
              desc: 'Cognify each incoming message automatically.',
              value: _autoReflection,
              onChanged: (v) => setState(() => _autoReflection = v),
            ),
            const SizedBox(height: AppTokens.s20),
            _SettingsGroupLabel('Extraction'),
            _NumberRow(
              label: 'Max concurrent extractions',
              desc: 'Semaphore size for in-flight cognify calls. Keep low on '
                  'local models.',
              controller: _maxConcurrent,
            ),
            _NumberRow(
              label: 'Max LLM output chars',
              desc: 'Hard cap on cognify-LLM output; streams abort past this.',
              controller: _maxOutputChars,
            ),
            const SizedBox(height: AppTokens.s16),
            _SettingsGroupLabel('Reflection'),
            _NumberRow(
              label: 'Min chars',
              desc: 'Skip reflection for messages shorter than this.',
              controller: _reflectMinChars,
            ),
            _NumberRow(
              label: 'Max chars',
              desc: 'Truncate reflection input beyond this length.',
              controller: _reflectMaxChars,
            ),
            _NumberRow(
              label: 'Cooldown (ms)',
              desc: 'Minimum gap between reflections per agent.',
              controller: _reflectCooldownMs,
            ),
            const SizedBox(height: AppTokens.s16),
            _SettingsGroupLabel('Maintenance'),
            _NumberRow(
              label: 'Sweep interval (hours)',
              desc: 'How often the background decay/prune sweep runs.',
              controller: _maintenanceHours,
            ),
            const SizedBox(height: AppTokens.s16),
            Row(children: [
              FilledButton.icon(
                onPressed: _saving ? null : _save,
                icon: _saving
                    ? const SizedBox(
                        width: 14,
                        height: 14,
                        child: CircularProgressIndicator(strokeWidth: 2))
                    : const Icon(Icons.save_outlined, size: 16),
                label: const Text('Save'),
              ),
              const SizedBox(width: AppTokens.s8),
              OutlinedButton.icon(
                onPressed: () async {
                  await ref
                      .read(apiClientProvider)
                      .post('/api/cognitive/maintenance');
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(content: Text('Maintenance started')));
                  }
                },
                icon: const Icon(Icons.cleaning_services_outlined, size: 16),
                label: const Text('Run maintenance'),
              ),
            ]),
          ],
        );
      },
    );
  }
}

/// Small uppercase group label for grouping settings fields.
class _SettingsGroupLabel extends StatelessWidget {
  const _SettingsGroupLabel(this.text);
  final String text;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Padding(
      padding: const EdgeInsets.only(bottom: AppTokens.s8, left: 2),
      child: Text(text.toUpperCase(),
          style: TextStyle(
              color: c.textMuted,
              fontSize: 11,
              fontWeight: FontWeight.w700,
              letterSpacing: 0.6)),
    );
  }
}

/// A bordered card row: label + description on the left, a compact number
/// input on the right — visually consistent with `_ToggleRow`.
class _NumberRow extends StatelessWidget {
  const _NumberRow({
    required this.label,
    required this.desc,
    required this.controller,
  });
  final String label;
  final String desc;
  final TextEditingController controller;
  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s16, vertical: AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                Text(desc,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s12),
          SizedBox(
            width: 120,
            child: TextField(
              controller: controller,
              keyboardType: TextInputType.number,
              textAlign: TextAlign.center,
              decoration: const InputDecoration(
                isDense: true,
                contentPadding: EdgeInsets.symmetric(
                    horizontal: AppTokens.s12, vertical: AppTokens.s8),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ── Space Apps management (web settings/SpaceAppsSettings) ────────────────────
class SpaceApp {
  final String id;
  final String name;
  final String? description;
  final bool enabled;
  final Map<String, dynamic> manifest;
  const SpaceApp(
      this.id, this.name, this.description, this.enabled, this.manifest);
  factory SpaceApp.fromJson(Map<String, dynamic> j) {
    final m = (j['manifest'] is Map)
        ? (j['manifest'] as Map).cast<String, dynamic>()
        : const <String, dynamic>{};
    return SpaceApp(
      '${j['id'] ?? ''}',
      '${m['name'] ?? j['id'] ?? 'app'}',
      m['description'] as String?,
      j['enabled'] != false,
      m,
    );
  }
}

final spaceAppsProvider = FutureProvider<List<SpaceApp>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/space/apps');
  final list = (r is List ? r : (r is Map ? r['apps'] : null)) as List? ?? const [];
  return list
      .whereType<Map>()
      .map((e) => SpaceApp.fromJson(e.cast<String, dynamic>()))
      .toList();
});

class SpaceAppsSection extends ConsumerWidget {
  const SpaceAppsSection({super.key});

  Future<void> _registerUrl(BuildContext context, WidgetRef ref) async {
    final ctrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: const Text('Register Space App'),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: const InputDecoration(
              labelText: 'Manifest URL',
              hintText: 'https://…/senclaw-manifest.json'),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx, false),
              child: const Text('Cancel')),
          FilledButton(
              onPressed: () => Navigator.pop(dctx, true),
              child: const Text('Register')),
        ],
      ),
    );
    if (ok != true || ctrl.text.trim().isEmpty) return;
    try {
      await ref.read(apiClientProvider).post('/api/space/apps/register',
          body: {'manifest_url': ctrl.text.trim()});
      ref.invalidate(spaceAppsProvider);
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Space App registered')));
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Register failed: $e')));
      }
    }
  }

  Future<void> _installZip(BuildContext context, WidgetRef ref) async {
    final res = await FilePicker.platform.pickFiles(
        type: FileType.custom, allowedExtensions: ['zip'], withData: kIsWeb);
    final f = res?.files.firstOrNull;
    if (f == null) return;
    final cfg = ref.read(appConfigProvider);
    final uri = Uri.parse('http://${cfg.host}:${cfg.uiPort}/api/space/apps/install-zip');
    final req = http.MultipartRequest('POST', uri);
    if (kIsWeb && f.bytes != null) {
      req.files.add(http.MultipartFile.fromBytes('file', f.bytes!,
          filename: f.name));
    } else if (f.path != null) {
      req.files.add(http.MultipartFile.fromBytes(
          'file', await File(f.path!).readAsBytes(),
          filename: f.name));
    } else {
      return;
    }
    try {
      final streamed = await req.send();
      if (streamed.statusCode >= 300) {
        throw Exception('HTTP ${streamed.statusCode}');
      }
      ref.invalidate(spaceAppsProvider);
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Space App installed')));
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Install failed: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final apps = ref.watch(spaceAppsProvider);
    return _Body(
      title: 'Space Apps',
      onRefresh: () => ref.invalidate(spaceAppsProvider),
      children: [
        Text('Install, register, and remove embedded Space Apps.',
            style: TextStyle(color: c.textMuted, fontSize: 12)),
        const SizedBox(height: AppTokens.s12),
        Row(
          children: [
            FilledButton.icon(
              onPressed: () => _installZip(context, ref),
              icon: const Icon(Icons.upload_file, size: 16),
              label: const Text('Install ZIP'),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () => _registerUrl(context, ref),
              icon: const Icon(Icons.link, size: 16),
              label: const Text('Register URL'),
            ),
            const Spacer(),
            IconButton(
              tooltip: 'Refresh',
              icon: const Icon(Icons.refresh, size: 18),
              onPressed: () => ref.invalidate(spaceAppsProvider),
            ),
          ],
        ),
        const SizedBox(height: AppTokens.s16),
        apps.when(
          loading: () => const Center(child: Padding(
              padding: EdgeInsets.all(AppTokens.s24),
              child: CircularProgressIndicator())),
          error: (e, _) => Text('$e', style: const TextStyle(color: AppTokens.danger)),
          data: (list) => list.isEmpty
              ? Text('No Space Apps installed',
                  style: TextStyle(color: c.textMuted))
              : Column(
                  children: [
                    for (final a in list)
                      Container(
                        margin: const EdgeInsets.only(bottom: AppTokens.s8),
                        padding: const EdgeInsets.all(AppTokens.s12),
                        decoration: BoxDecoration(
                          color: c.surface,
                          border: Border.all(color: c.border),
                          borderRadius: BorderRadius.circular(AppTokens.rMd),
                        ),
                        child: Row(
                          children: [
                            Icon(Icons.apps, size: 18, color: c.accent),
                            const SizedBox(width: AppTokens.s12),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Row(children: [
                                    Flexible(
                                      child: Text(a.name,
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: TextStyle(
                                              color: c.textPrimary,
                                              fontWeight: FontWeight.w600)),
                                    ),
                                    const SizedBox(width: AppTokens.s8),
                                    Text(a.id,
                                        style: TextStyle(
                                            color: c.textMuted, fontSize: 11)),
                                  ]),
                                  if (a.description != null)
                                    Text(a.description!,
                                        maxLines: 2,
                                        overflow: TextOverflow.ellipsis,
                                        style: TextStyle(
                                            color: c.textMuted, fontSize: 12)),
                                ],
                              ),
                            ),
                            IconButton(
                              tooltip: 'Details',
                              icon: const Icon(Icons.info_outline, size: 16),
                              onPressed: () => showDialog(
                                  context: context,
                                  builder: (_) => _SpaceAppDetailDialog(app: a)),
                            ),
                            TextButton(
                              onPressed: () async {
                                await ref.read(apiClientProvider).post(
                                    '/api/space/apps/${a.id}/restart');
                                if (context.mounted) {
                                  ScaffoldMessenger.of(context).showSnackBar(
                                      const SnackBar(
                                          content: Text('Restarting…')));
                                }
                              },
                              child: const Text('Restart'),
                            ),
                            IconButton(
                              tooltip: 'Uninstall',
                              icon: const Icon(Icons.delete_outline,
                                  size: 16, color: AppTokens.danger),
                              onPressed: () async {
                                await ref
                                    .read(apiClientProvider)
                                    .delete('/api/space/apps/${a.id}');
                                ref.invalidate(spaceAppsProvider);
                              },
                            ),
                          ],
                        ),
                      ),
                  ],
                ),
        ),
      ],
    );
  }
}

/// Space App detail (web SpaceAppDetailModal): manifest summary + declared MCP
/// + recent logs + restart.
class _SpaceAppDetailDialog extends ConsumerWidget {
  const _SpaceAppDetailDialog({required this.app});
  final SpaceApp app;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final m = app.manifest;
    final integration = (m['integration'] as Map?)?.cast<String, dynamic>();
    final runtime = (m['runtime'] as Map?)?.cast<String, dynamic>();
    final skills = (m['skills'] as List?) ?? const [];

    Widget kv(String k, String v) => Padding(
          padding: const EdgeInsets.only(bottom: AppTokens.s6),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              SizedBox(
                  width: 96,
                  child: Text(k,
                      style: TextStyle(color: c.textMuted, fontSize: 12))),
              Expanded(
                  child: Text(v,
                      style: TextStyle(color: c.textSecondary, fontSize: 12))),
            ],
          ),
        );

    return Dialog(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 640, maxHeight: 640),
        child: Padding(
          padding: const EdgeInsets.all(AppTokens.s20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(children: [
                Icon(Icons.apps, size: 18, color: c.accent),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(app.name,
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w700)),
                ),
                IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.pop(context)),
              ]),
              const SizedBox(height: AppTokens.s8),
              Expanded(
                child: ListView(
                  children: [
                    if (app.description != null)
                      Padding(
                        padding: const EdgeInsets.only(bottom: AppTokens.s12),
                        child: Text(app.description!,
                            style: TextStyle(color: c.textSecondary)),
                      ),
                    kv('ID', app.id),
                    if (m['version'] != null) kv('Version', '${m['version']}'),
                    if (integration != null)
                      kv('Integration',
                          '${integration['type'] ?? '?'} · ${integration['url'] ?? ''}'),
                    if (runtime?['kind'] != null)
                      kv('Runtime', '${runtime!['kind']}'),
                    if (skills.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(bottom: AppTokens.s6),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            SizedBox(
                                width: 96,
                                child: Text('Skills',
                                    style: TextStyle(
                                        color: c.textMuted, fontSize: 12))),
                            Expanded(
                              child: Wrap(
                                spacing: AppTokens.s6,
                                runSpacing: AppTokens.s6,
                                children: [
                                  for (final s in skills)
                                    _MiniTag(
                                        s is Map
                                            ? '${s['name'] ?? s}'
                                            : '$s',
                                        c.accent),
                                ],
                              ),
                            ),
                          ],
                        ),
                      ),
                    const SizedBox(height: AppTokens.s12),
                    // Declared MCP
                    _DetailFetchBlock(
                      title: 'MCP',
                      path: '/api/space/apps/${app.id}/mcp',
                      render: (data) {
                        final declared = (data is Map ? data['declared'] : null);
                        if (declared == null) {
                          return Text('No MCP declared',
                              style: TextStyle(
                                  color: c.textMuted, fontSize: 12));
                        }
                        return Text('${declared['name'] ?? declared}',
                            style: TextStyle(
                                color: c.textSecondary,
                                fontSize: 12,
                                fontFamily: AppTokens.fontMono));
                      },
                    ),
                    const SizedBox(height: AppTokens.s12),
                    // Logs
                    _DetailFetchBlock(
                      title: 'Logs',
                      path: '/api/space/apps/${app.id}/logs?max_bytes=65536',
                      mono: true,
                      render: (data) {
                        final text = data is Map
                            ? '${data['logs'] ?? data['content'] ?? ''}'
                            : '$data';
                        return Text(text.isEmpty ? '(no logs)' : text,
                            style: TextStyle(
                                color: c.textMuted,
                                fontSize: 11,
                                fontFamily: AppTokens.fontMono));
                      },
                    ),
                  ],
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  FilledButton.icon(
                    onPressed: () async {
                      await ref
                          .read(apiClientProvider)
                          .post('/api/space/apps/${app.id}/restart');
                      if (context.mounted) {
                        ScaffoldMessenger.of(context).showSnackBar(
                            const SnackBar(content: Text('Restarting…')));
                      }
                    },
                    icon: const Icon(Icons.restart_alt, size: 16),
                    label: const Text('Restart'),
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

/// A titled block that fetches a path and renders the result (collapsible-ish).
class _DetailFetchBlock extends ConsumerWidget {
  const _DetailFetchBlock(
      {required this.title,
      required this.path,
      required this.render,
      this.mono = false});
  final String title;
  final String path;
  final Widget Function(dynamic data) render;
  final bool mono;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(title,
            style: TextStyle(
                color: c.textSecondary,
                fontSize: 12,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s6),
        Container(
          width: double.infinity,
          constraints: BoxConstraints(maxHeight: mono ? 200 : 120),
          padding: const EdgeInsets.all(AppTokens.s8),
          decoration: BoxDecoration(
            color: c.surfaceAlt,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rSm),
          ),
          child: FutureBuilder(
            future: ref.read(apiClientProvider).get(path),
            builder: (_, snap) {
              if (snap.connectionState != ConnectionState.done) {
                return const SizedBox(
                    height: 20,
                    width: 20,
                    child: CircularProgressIndicator(strokeWidth: 2));
              }
              if (snap.hasError) {
                return Text('${snap.error}',
                    style: const TextStyle(
                        color: AppTokens.danger, fontSize: 11));
              }
              return SingleChildScrollView(child: render(snap.data));
            },
          ),
        ),
      ],
    );
  }
}

/// Small role/status pill used in settings lists.
class _MiniTag extends StatelessWidget {
  const _MiniTag(this.label, this.color);
  final String label;
  final Color color;
  @override
  Widget build(BuildContext context) => Container(
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

/// Inference settings for local (MLX) models — idle unload, KV cache, etc.
/// GET/PUT /api/local-models/settings, mirrors the web LocalModelsSettings.
class _LocalInferenceSettings extends ConsumerStatefulWidget {
  const _LocalInferenceSettings();
  @override
  ConsumerState<_LocalInferenceSettings> createState() =>
      _LocalInferenceSettingsState();
}

class _LocalInferenceSettingsState
    extends ConsumerState<_LocalInferenceSettings> {
  bool _loaded = false;
  bool _saving = false;

  // Numeric fields (empty = server default).
  final _idleUnload = TextEditingController();
  final _tqActivate = TextEditingController();
  final _maxPrompt = TextEditingController();
  final _maxNew = TextEditingController();
  final _maxKvTokens = TextEditingController();
  final _temperature = TextEditingController();
  final _repPenalty = TextEditingController();

  // Choice fields. _backend: auto|mlx|candle. _kvBits: -1=auto/0/3/4.
  // _mlxKvBits: -1=off/4/8.
  String _backend = 'auto';
  int _kvBits = -1;
  int _mlxKvBits = -1;
  bool _enableThinking = false;
  bool _releaseCache = false;

  @override
  void initState() {
    super.initState();
    ref.read(apiClientProvider).get('/api/local-models/settings').then((r) {
      if (r is! Map || !mounted) return;
      setState(() {
        _loaded = true;
        String s(String k) => r[k] == null ? '' : '${r[k]}';
        _idleUnload.text = s('idle_unload_secs');
        _tqActivate.text = s('tq_activate_at');
        _maxPrompt.text = s('max_prompt_tokens');
        _maxNew.text = s('max_new_tokens');
        _maxKvTokens.text = s('max_kv_tokens');
        _temperature.text = s('temperature');
        _repPenalty.text = s('repetition_penalty');
        _backend = (r['preferred_backend'] as String?) ?? 'auto';
        _kvBits = (r['kv_cache_bits'] as num?)?.toInt() ?? -1;
        _mlxKvBits = (r['mlx_kv_cache_bits'] as num?)?.toInt() ?? -1;
        _enableThinking = r['enable_thinking'] == true;
        _releaseCache = r['release_cache_after_session'] == true;
      });
    }).catchError((_) {
      if (mounted) setState(() => _loaded = true);
    });
  }

  @override
  void dispose() {
    _idleUnload.dispose();
    _tqActivate.dispose();
    _maxPrompt.dispose();
    _maxNew.dispose();
    _maxKvTokens.dispose();
    _temperature.dispose();
    _repPenalty.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    setState(() => _saving = true);
    int? n(TextEditingController c) => int.tryParse(c.text.trim());
    double? d(TextEditingController c) => double.tryParse(c.text.trim());
    try {
      await ref.read(apiClientProvider).put('/api/local-models/settings', body: {
        'preferred_backend': _backend == 'auto' ? null : _backend,
        'idle_unload_secs': n(_idleUnload),
        'kv_cache_bits': _kvBits < 0 ? null : _kvBits,
        'mlx_kv_cache_bits': _mlxKvBits < 0 ? null : _mlxKvBits,
        'tq_activate_at': n(_tqActivate),
        'enable_thinking': _enableThinking,
        'max_prompt_tokens': n(_maxPrompt),
        'max_new_tokens': n(_maxNew),
        'max_kv_tokens': n(_maxKvTokens),
        'temperature': d(_temperature),
        'repetition_penalty': d(_repPenalty),
        'release_cache_after_session': _releaseCache,
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Inference settings saved')));
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

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    if (!_loaded) return const LinearProgressIndicator();
    return Container(
      padding: const EdgeInsets.all(AppTokens.s16),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Inference settings',
              style: TextStyle(
                  color: c.textPrimary, fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s12),
          _choiceRow<String>(
            'Inference backend',
            'Engine for Load / Use as LLM. MLX is Apple-Silicon-only & fastest.',
            _backend,
            const [
              ('auto', 'Auto'),
              ('mlx', 'MLX native (~60–100 tok/s)'),
              ('candle', 'Candle (~12 tok/s)'),
            ],
            (v) => setState(() => _backend = v),
          ),
          _NumberRow(
              label: 'Idle unload (secs)',
              desc: '0 = never; ≥60 to free RAM after inactivity. Default 60.',
              controller: _idleUnload),
          _choiceRow<int>(
            'KV TurboQuant bits',
            'Quantize KV cache to save RAM on long generation.',
            _kvBits,
            const [
              (-1, 'Auto (4-bit for 4-bit models)'),
              (4, 'TQ4 — 4-bit total'),
              (3, 'TQ3 — 3-bit total'),
              (0, 'Off — FP16'),
            ],
            (v) => setState(() => _kvBits = v),
          ),
          _choiceRow<int>(
            'MLX packed KV (Metal)',
            'MLX-native GPU KV quantization. Reload the model after changing.',
            _mlxKvBits,
            const [
              (-1, 'Off — FP16'),
              (4, '4-bit packed'),
              (8, '8-bit packed'),
            ],
            (v) => setState(() => _mlxKvBits = v),
          ),
          _NumberRow(
              label: 'TQ activate after (tokens)',
              desc: 'Cached tokens before TurboQuant kicks in. Default 16384.',
              controller: _tqActivate),
          _NumberRow(
              label: 'Max prompt tokens',
              desc: 'Hard cap on prompt length (512–262144). Default 128000.',
              controller: _maxPrompt),
          _NumberRow(
              label: 'Max new tokens',
              desc: 'Max tokens generated per request (1–8192). Default 8192.',
              controller: _maxNew),
          _NumberRow(
              label: 'Max KV tokens',
              desc: 'KV-cache sliding window (128–262144). Default 16384.',
              controller: _maxKvTokens),
          _NumberRow(
              label: 'Temperature (MLX)',
              desc: '0 = greedy. Empty = server default (Gemma ≈0.65).',
              controller: _temperature),
          _NumberRow(
              label: 'Repetition penalty (MLX)',
              desc: '1 = off. Empty = server default (Gemma ≈1.15).',
              controller: _repPenalty),
          _switchRow(
            'Thinking mode (Qwen3)',
            'Chain-of-thought before answering. Off is faster.',
            _enableThinking,
            (v) => setState(() => _enableThinking = v),
          ),
          _switchRow(
            'Release cache after session (MLX)',
            'Drop per-session KV/prefix cache when a chat ends. Weights stay.',
            _releaseCache,
            (v) => setState(() => _releaseCache = v),
          ),
          const SizedBox(height: AppTokens.s8),
          Row(children: [
            FilledButton.icon(
              onPressed: _saving ? null : _save,
              icon: _saving
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.save_outlined, size: 16),
              label: const Text('Save'),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () async {
                await ref
                    .read(apiClientProvider)
                    .post('/api/local-models/unload-all');
                ref.invalidate(localModelsProvider);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(content: Text('Unloaded all models')));
                }
              },
              icon: const Icon(Icons.memory_outlined, size: 16),
              label: const Text('Unload all now'),
            ),
          ]),
        ],
      ),
    );
  }

  /// A label + description on the left, a dropdown of [options] on the right.
  Widget _choiceRow<T>(String label, String desc, T value,
      List<(T, String)> options, ValueChanged<T> onChanged) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s16, vertical: AppTokens.s8),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                Text(desc,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          const SizedBox(width: AppTokens.s12),
          SizedBox(
            width: 230,
            child: DropdownButtonFormField<T>(
              initialValue: value,
              isExpanded: true,
              decoration: const InputDecoration(
                isDense: true,
                contentPadding: EdgeInsets.symmetric(
                    horizontal: AppTokens.s12, vertical: AppTokens.s8),
              ),
              items: [
                for (final (v, l) in options)
                  DropdownMenuItem(
                      value: v,
                      child: Text(l,
                          maxLines: 1, overflow: TextOverflow.ellipsis)),
              ],
              onChanged: (v) {
                if (v != null) onChanged(v);
              },
            ),
          ),
        ],
      ),
    );
  }

  /// A label + description on the left, a switch on the right.
  Widget _switchRow(
      String label, String desc, bool value, ValueChanged<bool> onChanged) {
    final c = context.colors;
    return Container(
      margin: const EdgeInsets.only(bottom: AppTokens.s8),
      padding: const EdgeInsets.symmetric(
          horizontal: AppTokens.s16, vertical: AppTokens.s4),
      decoration: BoxDecoration(
        color: c.surface,
        border: Border.all(color: c.border),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label,
                    style: TextStyle(
                        color: c.textPrimary, fontWeight: FontWeight.w600)),
                Text(desc,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ),
          ),
          Switch(value: value, onChanged: onChanged),
        ],
      ),
    );
  }
}

/// Per-domain media settings (web Whisper/Tts/OcrSettings): active model +
/// language, plus voice/speed for TTS. GET/PUT `/api/$domain/settings`.
class _MediaSettingsCard extends ConsumerStatefulWidget {
  const _MediaSettingsCard({required this.domain});
  final String domain;
  @override
  ConsumerState<_MediaSettingsCard> createState() => _MediaSettingsCardState();
}

class _MediaSettingsCardState extends ConsumerState<_MediaSettingsCard> {
  String? _modelId;
  final _language = TextEditingController();
  final _voice = TextEditingController();
  final _speed = TextEditingController();
  bool _loaded = false;
  String? _flash;
  bool _testing = false;
  final _player = AudioPlayer();

  bool get _isTts => widget.domain == 'tts';
  bool get _isOcr => widget.domain == 'ocr';
  bool get _isWhisper => widget.domain == 'whisper';

  final _recorder = AudioRecorder();
  bool _recording = false;

  /// Whisper test: tap to record from the mic, tap again to stop → transcribe
  /// the clip with the active model and show the recognized text.
  Future<void> _testWhisper() async {
    if (_recording) {
      setState(() {
        _recording = false;
        _testing = true;
      });
      try {
        final out = await _recorder.stop();
        if (out == null) {
          if (mounted) setState(() => _testing = false);
          return;
        }
        Uint8List bytes;
        String filename;
        if (kIsWeb) {
          bytes = (await http.get(Uri.parse(out))).bodyBytes;
          filename = 'rec.webm';
        } else {
          bytes = await File(out).readAsBytes();
          filename = out.split('/').last;
        }
        final text =
            await ref.read(audioServiceProvider).transcribe(bytes, filename);
        if (!mounted) return;
        showDialog(
          context: context,
          builder: (dctx) => AlertDialog(
            backgroundColor: dctx.colors.surface,
            title: const Text('Transcription'),
            content: SizedBox(
              width: 480,
              child: SelectableText(
                  text.trim().isEmpty ? '(no speech recognized)' : text),
            ),
            actions: [
              TextButton(
                  onPressed: () => Navigator.of(dctx).pop(),
                  child: const Text('Close')),
            ],
          ),
        );
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context)
              .showSnackBar(SnackBar(content: Text('Transcribe failed: $e')));
        }
      } finally {
        if (mounted) setState(() => _testing = false);
      }
      return;
    }
    if (!await _recorder.hasPermission()) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('Microphone permission denied')));
      }
      return;
    }
    var path = '';
    if (!kIsWeb) {
      final dir = await getTemporaryDirectory();
      path = '${dir.path}/whisper_test.m4a';
    }
    await _recorder.start(const RecordConfig(), path: path);
    if (mounted) setState(() => _recording = true);
  }

  @override
  void initState() {
    super.initState();
    _load();
  }

  /// Pick an image and run OCR with the current model/language, then show the
  /// recognized text — a quick "does it work" preview.
  Future<void> _testOcr() async {
    final res = await FilePicker.platform
        .pickFiles(type: FileType.image, withData: kIsWeb);
    final f = res?.files.firstOrNull;
    if (f == null) return;
    setState(() => _testing = true);
    try {
      final cfg = ref.read(appConfigProvider);
      final req = http.MultipartRequest('POST',
          Uri.parse('http://${cfg.host}:${cfg.uiPort}/api/ocr/recognize'));
      if (_language.text.trim().isNotEmpty) {
        req.fields['language'] = _language.text.trim();
      }
      if (kIsWeb && f.bytes != null) {
        req.files.add(
            http.MultipartFile.fromBytes('image', f.bytes!, filename: f.name));
      } else if (f.path != null) {
        req.files.add(http.MultipartFile.fromBytes(
            'image', await File(f.path!).readAsBytes(),
            filename: f.name));
      } else {
        if (mounted) setState(() => _testing = false);
        return;
      }
      final resp = await req.send();
      final body = await resp.stream.bytesToString();
      if (!mounted) return;
      final ok = resp.statusCode >= 200 && resp.statusCode < 300;
      final text = ok
          ? '${(jsonDecode(body) as Map?)?['text'] ?? ''}'
          : 'OCR failed: $body';
      showDialog(
        context: context,
        builder: (dctx) => AlertDialog(
          backgroundColor: dctx.colors.surface,
          title: Text('OCR result — ${f.name}'),
          content: SizedBox(
            width: 520,
            child: SingleChildScrollView(
              child: SelectableText(
                  text.trim().isEmpty ? '(no text recognized)' : text,
                  style: const TextStyle(fontSize: 13, height: 1.4)),
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.of(dctx).pop(),
                child: const Text('Close')),
          ],
        ),
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('OCR failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _testing = false);
    }
  }

  /// Synthesize a short sample with the current settings and play it back so
  /// the user can preview the voice before saving.
  Future<void> _test() async {
    setState(() => _testing = true);
    try {
      final lang = _language.text.trim().toLowerCase();
      final sample = lang.startsWith('vi')
          ? 'Xin chào, đây là giọng đọc thử của SenClaw.'
          : 'Hello, this is a SenClaw voice test.';
      final cfg = ref.read(appConfigProvider);
      final resp = await http.post(
        Uri.parse('http://${cfg.host}:${cfg.uiPort}/api/tts/synthesize'),
        headers: {'Content-Type': 'application/json'},
        body: jsonEncode({
          'text': sample,
          if (lang.isNotEmpty) 'language': _language.text.trim(),
          if (_voice.text.trim().isNotEmpty) 'voice': _voice.text.trim(),
          if (double.tryParse(_speed.text.trim()) != null)
            'speed': double.parse(_speed.text.trim()),
        }),
      );
      if (resp.statusCode >= 200 && resp.statusCode < 300) {
        await _player.play(BytesSource(resp.bodyBytes, mimeType: 'audio/wav'));
      } else if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: Text('Test failed: ${resp.statusCode}')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text('Test failed: $e')));
      }
    } finally {
      if (mounted) setState(() => _testing = false);
    }
  }

  Future<void> _load() async {
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('/api/${widget.domain}/settings');
      if (mounted && r is Map) {
        _modelId = (r['model_id'] as String?)?.isEmpty == true
            ? null
            : r['model_id'] as String?;
        _language.text = '${r['language'] ?? ''}';
        _voice.text = '${r['voice'] ?? ''}';
        _speed.text = '${r['speed'] ?? ''}';
      }
    } catch (_) {}
    if (mounted) setState(() => _loaded = true);
  }

  Future<void> _save() async {
    final body = <String, dynamic>{
      if (_modelId != null) 'model_id': _modelId,
      'language': _language.text.trim(),
    };
    if (_isTts) {
      body['voice'] = _voice.text.trim();
      body['speed'] = double.tryParse(_speed.text.trim()) ?? 1.0;
    }
    try {
      await ref
          .read(apiClientProvider)
          .put('/api/${widget.domain}/settings', body: body);
      setState(() => _flash = 'Saved');
    } catch (e) {
      setState(() => _flash = 'Failed');
    }
  }

  @override
  void dispose() {
    _player.dispose();
    _recorder.dispose();
    _language.dispose();
    _voice.dispose();
    _speed.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final models = ref.watch(mediaModelsProvider(widget.domain)).valueOrNull ??
        const [];
    if (!_loaded) return const LinearProgressIndicator();
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Settings',
              style: TextStyle(
                  color: c.textPrimary, fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s8),
          Builder(builder: (_) {
            // DropdownButton asserts the value matches exactly one item, so a
            // saved model_id that isn't in the catalog must be added as its own
            // item (else Flutter throws). Keeps the current selection visible.
            final ids = models.map((m) => m.id).toSet();
            final extra = (_modelId != null &&
                    _modelId!.isNotEmpty &&
                    !ids.contains(_modelId))
                ? _modelId
                : null;
            return DropdownButtonFormField<String>(
              // `initialValue` only applies on first build; the settings load
              // is async, so re-key on `_modelId` to re-init the field once the
              // saved model arrives (otherwise it stays stuck on "(default)").
              key: ValueKey('${widget.domain}-model-$_modelId'),
              initialValue: _modelId,
              isExpanded: true,
              decoration: const InputDecoration(
                  labelText: 'Active model', isDense: true),
              items: [
                const DropdownMenuItem(value: null, child: Text('(default)')),
                if (extra != null)
                  DropdownMenuItem(
                      value: extra,
                      child: Text('$extra (current)',
                          maxLines: 1, overflow: TextOverflow.ellipsis)),
                for (final m in models)
                  DropdownMenuItem(
                      value: m.id,
                      child: Text(m.label,
                          maxLines: 1, overflow: TextOverflow.ellipsis)),
              ],
              onChanged: (v) => setState(() => _modelId = v),
            );
          }),
          const SizedBox(height: AppTokens.s8),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _language,
                  decoration: const InputDecoration(
                      labelText: 'Language', isDense: true, hintText: 'vi'),
                ),
              ),
              if (_isTts) ...[
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: TextField(
                    controller: _voice,
                    decoration: const InputDecoration(
                        labelText: 'Voice', isDense: true),
                  ),
                ),
                const SizedBox(width: AppTokens.s8),
                SizedBox(
                  width: 80,
                  child: TextField(
                    controller: _speed,
                    keyboardType: TextInputType.number,
                    decoration: const InputDecoration(
                        labelText: 'Speed', isDense: true),
                  ),
                ),
              ],
            ],
          ),
          const SizedBox(height: AppTokens.s8),
          Row(
            children: [
              FilledButton(onPressed: _save, child: const Text('Save')),
              if (_isTts) ...[
                const SizedBox(width: AppTokens.s8),
                OutlinedButton.icon(
                  onPressed: _testing ? null : _test,
                  icon: _testing
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.play_arrow_rounded, size: 18),
                  label: const Text('Test voice'),
                ),
              ],
              if (_isOcr) ...[
                const SizedBox(width: AppTokens.s8),
                OutlinedButton.icon(
                  onPressed: _testing ? null : _testOcr,
                  icon: _testing
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : const Icon(Icons.image_outlined, size: 16),
                  label: const Text('Test (pick image)'),
                ),
              ],
              if (_isWhisper) ...[
                const SizedBox(width: AppTokens.s8),
                OutlinedButton.icon(
                  onPressed: _testing ? null : _testWhisper,
                  icon: _testing
                      ? const SizedBox(
                          width: 14,
                          height: 14,
                          child: CircularProgressIndicator(strokeWidth: 2))
                      : Icon(_recording ? Icons.stop : Icons.mic_none,
                          size: 18,
                          color: _recording ? AppTokens.danger : null),
                  label: Text(_recording
                      ? 'Stop & transcribe'
                      : 'Record & transcribe'),
                ),
              ],
              if (_flash != null) ...[
                const SizedBox(width: AppTokens.s8),
                Text(_flash!,
                    style: TextStyle(color: c.textMuted, fontSize: 12)),
              ],
            ],
          ),
        ],
      ),
    );
  }
}
