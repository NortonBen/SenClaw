import 'dart:async';
import 'dart:convert';
import 'dart:io' show File, InternetAddressType, NetworkInterface;
import 'dart:math';
import 'package:file_picker/file_picker.dart';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:hotkey_manager/hotkey_manager.dart';
import 'package:audioplayers/audioplayers.dart';
import 'package:record/record.dart';
import 'package:path_provider/path_provider.dart';
import 'package:http/http.dart' as http;
import '../chat/audio_service.dart' show audioServiceProvider;
import 'package:qr_flutter/qr_flutter.dart';
import '../../core/daemon/daemon_provider.dart';
import '../../core/daemon/daemon_supervisor.dart';
import '../../core/prefs.dart';
import '../../core/i18n/l10n.dart';
import '../../core/i18n/locale_provider.dart';
import '../../core/transport/connection.dart';
import '../../core/update/update_provider.dart';
import '../../core/update/update_service.dart' show bundlePath;
import '../../theme/theme_mode_provider.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_markdown.dart';
import '../capture/capture_hotkey.dart';
import '../capture/screen_capture.dart' show isCaptureSupported;
import '../chat/agents_provider.dart';
import '../chat/new_chat_dialog.dart' show llmConfigsProvider, LlmConfig;
import '../plugins/space_app_runtime_panel.dart';
import '../plugins/space_app_sandbox_dialog.dart';
import 'entity_providers.dart';
import 'provider_signin_section.dart';
import 'settings_providers.dart';

const _sections = [
  ('appearance', 'Appearance', Icons.palette_outlined),
  ('general', 'General', Icons.tune),
  ('channels', 'Channels', Icons.hub_outlined),
  ('agents', 'Profiles', Icons.badge_outlined),
  ('rules', 'Tool Rules', Icons.rule_folder_outlined),
  ('llm', 'LLM Models', Icons.smart_toy_outlined),
  ('signin', 'Provider Sign-in', Icons.link),
  ('local', 'Local Models', Icons.memory),
  ('embedding', 'Embedding', Icons.scatter_plot_outlined),
  ('memory', 'Knowledge', Icons.account_tree_outlined),
  ('whisper', 'Speech-to-Text', Icons.mic_none_outlined),
  ('tts', 'Text-to-Speech', Icons.volume_up_outlined),
  ('ocr', 'OCR', Icons.document_scanner_outlined),
  ('updates', 'Updates', Icons.system_update_alt),
];

/// Which settings section is showing. Public so the macOS app menu's
/// "Check for Updates…" can land the user directly on Updates.
final settingsSectionProvider = StateProvider<String>((ref) => 'general');

class SettingsScreen extends ConsumerWidget {
  const SettingsScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final section = ref.watch(settingsSectionProvider);

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
                  child: Text(context.tr('Settings'),
                      style: TextStyle(
                          color: c.textPrimary,
                          fontSize: 16,
                          fontWeight: FontWeight.w700)),
                ),
                for (final (key, label, icon) in _sections)
                  _SectionItem(
                    icon: icon,
                    label: context.tr(label),
                    active: section == key,
                    onTap: () =>
                        ref.read(settingsSectionProvider.notifier).state = key,
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
            'signin' => const ProviderSignInSection(),
            'local' => const _LocalModelsSection(),
            'embedding' => const _EmbeddingSection(),
            'memory' => const _MemorySection(),
            'whisper' => _MediaModelsSection(
                domain: 'whisper',
                title: context.tr('Speech-to-Text (Whisper)')),
            'tts' => _MediaModelsSection(
                domain: 'tts', title: context.tr('Text-to-Speech')),
            'ocr' => const _MediaModelsSection(domain: 'ocr', title: 'OCR'),
            'updates' => const UpdatesSection(),
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
class SettingsBody extends StatefulWidget {
  const SettingsBody({
    super.key,
    required this.title,
    required this.children,
    this.onRefresh,
  });
  final String title;
  final List<Widget> children;

  /// Re-fetches this section's API data. Called automatically every time the
  /// user navigates to the section, and exposed as a reload button beside the
  /// title.
  final VoidCallback? onRefresh;

  @override
  State<SettingsBody> createState() => _SettingsBodyState();
}

class _SettingsBodyState extends State<SettingsBody> {
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
                tooltip: context.tr('Reload'),
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

// ── Updates ───────────────────────────────────────────────────────────────

/// Public (unlike its sibling sections) so a widget test can render it without
/// standing up the whole settings screen and its API-backed providers.
class UpdatesSection extends ConsumerWidget {
  const UpdatesSection({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final s = ref.watch(updateProvider);
    final n = ref.read(updateProvider.notifier);
    final svc = ref.watch(updateServiceProvider);
    // The web console has no bundle to replace and no process to restart.
    final canUpdate = !kIsWeb && !svc.isDevBuild;

    return SettingsBody(
      title: context.tr('Updates'),
      children: [
        Container(
          padding: const EdgeInsets.all(AppTokens.s16),
          decoration: BoxDecoration(
            color: c.surface,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text('SenClaw Desktop',
                            style: TextStyle(
                                color: c.textPrimary, fontWeight: FontWeight.w600)),
                        const SizedBox(height: 2),
                        Text(
                          svc.isDevBuild
                              ? context.tr('Development build')
                              : context.trArgs('Version {v}',
                                      {'v': svc.currentVersion}) +
                                  (svc.buildTarget.isEmpty
                                      ? ''
                                      : ' · ${svc.buildTarget}'),
                          style: TextStyle(
                            color: c.textMuted,
                            fontSize: 12,
                            fontFamily: AppTokens.fontMono,
                          ),
                        ),
                        // The folder this copy runs from — and the one an
                        // in-app update will replace. `senclaw install desktop`
                        // manages its own default location, so when a machine
                        // has ended up with two copies, this line is how the
                        // user tells them apart.
                        if (!kIsWeb) ...[
                          const SizedBox(height: 2),
                          Text(
                            context.trArgs(
                                'Installed at {path}', {'path': bundlePath()}),
                            style: TextStyle(
                              color: c.textMuted,
                              fontSize: 11,
                              fontFamily: AppTokens.fontMono,
                            ),
                            overflow: TextOverflow.ellipsis,
                          ),
                        ],
                      ],
                    ),
                  ),
                  if (canUpdate) _UpdateActionButton(state: s),
                ],
              ),
              const SizedBox(height: AppTokens.s12),
              _UpdateStatusLine(state: s),
              if (s.phase == UpdatePhase.downloading) ...[
                const SizedBox(height: AppTokens.s8),
                LinearProgressIndicator(value: s.progress),
                const SizedBox(height: AppTokens.s4),
                Row(
                  children: [
                    Text('${(s.progress * 100).toStringAsFixed(0)}%',
                        style: TextStyle(color: c.textMuted, fontSize: 12)),
                    const Spacer(),
                    TextButton(
                      onPressed: n.cancelDownload,
                      child: Text(context.tr('Cancel')),
                    ),
                  ],
                ),
              ],
              if (s.error != null) ...[
                const SizedBox(height: AppTokens.s8),
                Text(s.error!, style: const TextStyle(color: Colors.redAccent, fontSize: 12)),
              ],
            ],
          ),
        ),
        const SizedBox(height: AppTokens.s16),

        if (!canUpdate)
          Text(
            kIsWeb
                ? context.tr(
                    'The web console is served by the daemon and updates with it. '
                    'Update the daemon on the host machine: senclaw update')
                : context.tr(
                    'This build has no release version, so it cannot be updated in place. '
                    'Rebuild from source, or install a release with: senclaw install desktop'),
            style: TextStyle(color: c.textMuted, fontSize: 12),
          )
        else ...[
          _ToggleRow(
            label: context.tr('Check for updates automatically'),
            desc: context.tr(
                'At every start and once a day after that, in the background. '
                'A new version pops up a notice; nothing installs without your '
                'say-so.'),
            value: s.autoCheck,
            onChanged: (v) => n.setAutoCheck(v),
          ),
          // The only place the user can undo "Skip this version" / "Remind me
          // later" — otherwise the popup they silenced is gone for good with no
          // trace of why.
          if (s.announcementSilenced) ...[
            const SizedBox(height: AppTokens.s8),
            Row(
              children: [
                Icon(Icons.notifications_off_outlined,
                    size: 16, color: c.textMuted),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(
                    s.skippedVersion == '${s.manifest?.version}'
                        ? context.trArgs(
                            'You asked not to be notified about {v}.',
                            {'v': s.manifest!.version})
                        : context.trArgs(
                            'Reminders about {v} are paused until {when}.', {
                            'v': s.manifest!.version,
                            'when': _snoozeLabel(context, s.snoozeUntil),
                          }),
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                ),
                TextButton(
                  onPressed: n.resumeAnnouncements,
                  child: Text(context.tr('Notify me again')),
                ),
              ],
            ),
          ],
          if (s.manifest != null && s.hasUpdate && (s.manifest!.notes?.isNotEmpty ?? false)) ...[
            const SizedBox(height: AppTokens.s8),
            Text(
                context.trArgs(
                    "What's new in {v}", {'v': s.manifest!.version}),
                style: TextStyle(color: c.textPrimary, fontWeight: FontWeight.w600)),
            const SizedBox(height: AppTokens.s8),
            Container(
              width: double.infinity,
              padding: const EdgeInsets.all(AppTokens.s16),
              decoration: BoxDecoration(
                color: c.surface,
                border: Border.all(color: c.border),
                borderRadius: BorderRadius.circular(AppTokens.rMd),
              ),
              child: AppMarkdown(s.manifest!.notes!),
            ),
          ],
        ],
      ],
    );
  }
}

/// When a paused reminder comes back. Hours while it is same-day, so "in 6h"
/// rather than a date the user has to subtract today from.
String _snoozeLabel(BuildContext context, DateTime? until) {
  if (until == null) return context.tr('later');
  final left = until.difference(DateTime.now());
  if (left.inMinutes <= 0) return context.tr('the next check');
  if (left.inHours < 1) return context.trArgs('in {n}m', {'n': left.inMinutes});
  if (left.inHours < 48) return context.trArgs('in {n}h', {'n': left.inHours});
  return context.trArgs('in {n}d', {'n': left.inDays});
}

class _UpdateStatusLine extends StatelessWidget {
  const _UpdateStatusLine({required this.state});
  final UpdateState state;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final v = state.manifest?.version;
    final text = switch (state.phase) {
      UpdatePhase.checking => context.tr('Checking…'),
      UpdatePhase.upToDate => context.tr('You are on the latest version.'),
      UpdatePhase.available =>
        context.trArgs('Version {v} is available.', {'v': v}),
      UpdatePhase.downloading => context.trArgs('Downloading {v}…', {'v': v}),
      UpdatePhase.ready =>
        context.trArgs('Version {v} is ready to install.', {'v': v}),
      UpdatePhase.applying => context.tr('Installing — SenClaw will restart…'),
      UpdatePhase.error => state.error ?? context.tr('Something went wrong.'),
      UpdatePhase.idle => state.lastCheck == null
          ? context.tr('Not checked yet.')
          : context.trArgs(
              'Last checked {ago}.', {'ago': _ago(context, state.lastCheck!)}),
    };
    return Text(text, style: TextStyle(color: c.textSecondary, fontSize: 13));
  }

  static String _ago(BuildContext context, DateTime t) {
    final d = DateTime.now().difference(t);
    if (d.inMinutes < 1) return context.tr('just now');
    if (d.inHours < 1) return context.trArgs('{n}m ago', {'n': d.inMinutes});
    if (d.inDays < 1) return context.trArgs('{n}h ago', {'n': d.inHours});
    return context.trArgs('{n}d ago', {'n': d.inDays});
  }
}

class _UpdateActionButton extends ConsumerWidget {
  const _UpdateActionButton({required this.state});
  final UpdateState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final n = ref.read(updateProvider.notifier);
    return switch (state.phase) {
      UpdatePhase.checking || UpdatePhase.downloading || UpdatePhase.applying =>
        const SizedBox(
          width: 20,
          height: 20,
          child: CircularProgressIndicator(strokeWidth: 2),
        ),
      UpdatePhase.available => FilledButton.icon(
          onPressed: n.download,
          icon: const Icon(Icons.download, size: 16),
          label: Text(context.tr('Download')),
        ),
      UpdatePhase.ready => FilledButton.icon(
          onPressed: () => _confirmAndApply(context, ref),
          icon: const Icon(Icons.restart_alt, size: 16),
          label: Text(context.tr('Install & Restart')),
        ),
      _ => OutlinedButton(
          onPressed: () => n.check(),
          child: Text(context.tr('Check now')),
        ),
    };
  }

  /// Installing kills the daemon and every running agent, so say so first —
  /// the user may be mid-conversation and have no idea a restart is coming.
  Future<void> _confirmAndApply(BuildContext context, WidgetRef ref) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(ctx.tr('Install update?')),
        content: Text(
          ctx.tr('SenClaw will quit, install the update, and reopen. '
              'Running agents and background tasks will be stopped.'),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(ctx.tr('Not now'))),
          FilledButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(ctx.tr('Install & Restart'))),
        ],
      ),
    );
    if (ok == true) await ref.read(updateProvider.notifier).applyAndRestart();
  }
}

// ── Appearance (theme mode + language) ────────────────────────────────────
class _AppearanceSection extends ConsumerWidget {
  const _AppearanceSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final mode = ref.watch(themeModeProvider);
    final lang = ref.watch(appLanguageProvider);
    const opts = [
      (ThemeMode.system, 'System', Icons.brightness_auto_outlined),
      (ThemeMode.light, 'Light', Icons.light_mode_outlined),
      (ThemeMode.dark, 'Dark', Icons.dark_mode_outlined),
    ];
    // Language endonyms stay untranslated on purpose: "Tiếng Việt" must be
    // findable by a Vietnamese speaker while the app is still in English.
    const langOpts = [
      (AppLanguage.system, 'System', Icons.language),
      (AppLanguage.en, 'English', Icons.abc),
      (AppLanguage.vi, 'Tiếng Việt', Icons.translate),
    ];
    return SettingsBody(
      title: context.tr('Appearance'),
      children: [
        Text(context.tr('Theme'),
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
                  label: context.tr(label),
                  selected: mode == m,
                  onTap: () => ref.read(themeModeProvider.notifier).set(m),
                ),
              ),
          ],
        ),
        const SizedBox(height: AppTokens.s12),
        Text(
          context.tr(
              'System follows your OS appearance setting and switches automatically.'),
          style: TextStyle(color: c.textMuted, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s24),
        Text(context.tr('Language'),
            style: TextStyle(
                color: c.textSecondary, fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s12),
        Row(
          children: [
            for (final (l, label, icon) in langOpts)
              Padding(
                padding: const EdgeInsets.only(right: AppTokens.s12),
                child: _ThemeCard(
                  icon: icon,
                  label: l == AppLanguage.system ? context.tr(label) : label,
                  selected: lang == l,
                  onTap: () => ref.read(appLanguageProvider.notifier).set(l),
                ),
              ),
          ],
        ),
        const SizedBox(height: AppTokens.s12),
        Text(
          context.tr(
              'Applies everywhere immediately. System follows your OS language (Vietnamese → Tiếng Việt, otherwise English).'),
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
/// API access token for daemons exposed beyond loopback
/// (`SENCLAW_UI_BIND_HOST=0.0.0.0` gates every /api route for non-loopback
/// peers). Leave empty for the default local daemon — loopback peers are
/// exempt. Persisted in prefs and applied to the live connection on save.
class _ApiTokenField extends ConsumerStatefulWidget {
  const _ApiTokenField();
  @override
  ConsumerState<_ApiTokenField> createState() => _ApiTokenFieldState();
}

class _ApiTokenFieldState extends ConsumerState<_ApiTokenField> {
  late final TextEditingController _token;

  @override
  void initState() {
    super.initState();
    String initial = '';
    try {
      initial = ref.read(prefsProvider).getString(kApiTokenKey) ?? '';
    } catch (_) {}
    if (initial.isEmpty) {
      initial = ref.read(appConfigProvider).apiToken ?? '';
    }
    _token = TextEditingController(text: initial);
  }

  @override
  void dispose() {
    _token.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    final value = _token.text.trim();
    try {
      await ref.read(prefsProvider).setString(kApiTokenKey, value);
    } catch (_) {}
    final updated =
        ref.read(appConfigProvider).copyWith(apiToken: value);
    ref.read(appConfigProvider.notifier).state = updated;
    ref.read(wsClientProvider).updateConfig(updated);
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(
          content:
              Text(context.tr('API token saved — applies to new requests.'))));
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(
        context.tr(
            'API access token — only needed when the daemon is exposed beyond '
            'localhost (SENCLAW_UI_BIND_HOST=0.0.0.0). The daemon machine keeps '
            'it in ~/.senclaw/api_token.'),
        style: TextStyle(color: c.textSecondary, fontSize: 12),
      ),
      const SizedBox(height: AppTokens.s8),
      Row(children: [
        Expanded(
          child: TextField(
            controller: _token,
            obscureText: true,
            decoration: InputDecoration(
              isDense: true,
              border: const OutlineInputBorder(),
              hintText: context.tr('Empty for the local daemon'),
            ),
          ),
        ),
        const SizedBox(width: AppTokens.s8),
        FilledButton(onPressed: _save, child: Text(context.tr('Save'))),
      ]),
    ]);
  }
}

/// Where the daemon listens: loopback only (private) or every interface
/// (public / LAN).
///
/// The daemon reads this as `SENCLAW_UI_BIND_HOST` once, at startup, so the
/// choice is persisted in prefs and handed to the supervisor — the running
/// daemon keeps its socket until it is restarted, which this card offers
/// inline rather than leaving as an unstated requirement.
///
/// Public is a real exposure decision, not a preference: the daemon then gates
/// every /api route behind the API token for non-loopback peers (loopback —
/// this app — stays exempt), so the panel points at where that token lives.
class NetworkBindField extends ConsumerStatefulWidget {
  const NetworkBindField({super.key});
  @override
  ConsumerState<NetworkBindField> createState() => _NetworkBindFieldState();
}

class _NetworkBindFieldState extends ConsumerState<NetworkBindField> {
  /// What the user has chosen; the supervisor still reports what the RUNNING
  /// daemon got, and the gap between the two is what the restart hint is for.
  late bool _public;
  bool _restarting = false;
  List<String> _lanAddrs = const [];

  @override
  void initState() {
    super.initState();
    _public = ref.read(daemonSupervisorProvider).isPublicBind;
    _loadLanAddresses();
  }

  Future<void> _loadLanAddresses() async {
    if (kIsWeb) return;
    try {
      final ifs = await NetworkInterface.list(
        type: InternetAddressType.IPv4,
        includeLoopback: false,
        includeLinkLocal: false,
      );
      final addrs = [
        for (final i in ifs)
          for (final a in i.addresses) a.address,
      ];
      if (mounted) setState(() => _lanAddrs = addrs);
    } catch (_) {
      // No addresses is a fine answer — the hint just omits the URL.
    }
  }

  Future<void> _choose(bool public) async {
    setState(() => _public = public);
    try {
      await ref.read(prefsProvider).setBool(kBindPublicKey, public);
    } catch (_) {}
    ref.read(daemonSupervisorProvider).bindHost = public
        ? DaemonSupervisor.kPublicBindHost
        : DaemonSupervisor.kPrivateBindHost;
  }

  Future<void> _restartDaemon() async {
    setState(() => _restarting = true);
    final messenger = ScaffoldMessenger.of(context);
    try {
      await ref.read(daemonSupervisorProvider).restart();
      if (!mounted) return;
      messenger.showSnackBar(SnackBar(
          content: Text(context.tr('Daemon restarted with the new setting.'))));
    } catch (e) {
      if (!mounted) return;
      messenger.showSnackBar(SnackBar(content: Text('$e')));
    } finally {
      if (mounted) setState(() => _restarting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final sup = ref.watch(daemonSupervisorProvider);
    final port = ref.watch(appConfigProvider).uiPort;
    // An adopted daemon was started by something else (a terminal, a leftover
    // process); our env would not have reached it whatever the setting says.
    final adopted = sup.phase == DaemonPhase.adopted;
    final needsRestart = sup.bindHostPending || adopted;

    return Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
      Text(
        context.tr(
            'Who can reach this daemon. Private keeps it on this machine; '
            'Public lets phones and other computers on your network use it.'),
        style: TextStyle(color: c.textSecondary, fontSize: 12),
      ),
      const SizedBox(height: AppTokens.s12),
      Row(children: [
        _BindCard(
          icon: Icons.lock_outline_rounded,
          label: context.tr('Private'),
          detail: '127.0.0.1',
          selected: !_public,
          onTap: () => _choose(false),
        ),
        const SizedBox(width: AppTokens.s12),
        _BindCard(
          icon: Icons.public_rounded,
          label: context.tr('Public'),
          detail: '0.0.0.0',
          selected: _public,
          onTap: () => _choose(true),
        ),
      ]),
      if (_public) ...[
        const SizedBox(height: AppTokens.s12),
        _NoticeBox(
          tone: _NoticeTone.warning,
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(
              context.tr(
                  'Anyone on your network can reach SenClaw. The daemon '
                  'requires the API token from every non-local device — it is '
                  'in ~/.senclaw/api_token on this machine.'),
              style: TextStyle(color: c.textPrimary, fontSize: 12),
            ),
            if (_lanAddrs.isNotEmpty) ...[
              const SizedBox(height: AppTokens.s8),
              SelectableText(
                _lanAddrs.map((a) => 'http://$a:$port').join('   '),
                style: TextStyle(
                    color: c.textSecondary, fontSize: 12, fontFamily: 'monospace'),
              ),
            ],
          ]),
        ),
      ],
      if (needsRestart) ...[
        const SizedBox(height: AppTokens.s12),
        Row(children: [
          Expanded(
            child: Text(
              adopted
                  ? context.tr(
                      'This daemon was started outside the app, so it keeps '
                      'its own setting until it is restarted here.')
                  : context.tr(
                      'The running daemon still uses the previous setting. '
                      'Restart it to apply.'),
              style: TextStyle(color: c.textMuted, fontSize: 12),
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          FilledButton.icon(
            onPressed: _restarting ? null : _restartDaemon,
            icon: _restarting
                ? const SizedBox(
                    width: 14,
                    height: 14,
                    child: CircularProgressIndicator(strokeWidth: 2))
                : const Icon(Icons.restart_alt, size: 18),
            label: Text(context.tr('Restart daemon')),
          ),
        ]),
      ],
    ]);
  }
}

class _BindCard extends StatelessWidget {
  const _BindCard({
    required this.icon,
    required this.label,
    required this.detail,
    required this.selected,
    required this.onTap,
  });
  final IconData icon;
  final String label;
  final String detail;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      borderRadius: BorderRadius.circular(AppTokens.rLg),
      onTap: onTap,
      child: Container(
        width: 190,
        padding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s16, vertical: AppTokens.s12),
        decoration: BoxDecoration(
          color: selected ? c.accentSoft : c.surface,
          border: Border.all(
              color: selected ? c.accent : c.border, width: selected ? 1.5 : 1),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
        child: Row(children: [
          Icon(icon, size: 20, color: selected ? c.accent : c.textSecondary),
          const SizedBox(width: AppTokens.s12),
          Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(label,
                style: TextStyle(
                    color: selected ? c.accent : c.textPrimary,
                    fontWeight: FontWeight.w600)),
            Text(detail,
                style: TextStyle(
                    color: c.textMuted, fontSize: 11, fontFamily: 'monospace')),
          ]),
        ]),
      ),
    );
  }
}

enum _NoticeTone { warning }

class _NoticeBox extends StatelessWidget {
  const _NoticeBox({required this.child, required this.tone});
  final Widget child;
  final _NoticeTone tone;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final accent = tone == _NoticeTone.warning ? AppTokens.warning : c.accent;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: accent.withValues(alpha: .10),
        border: Border.all(color: accent.withValues(alpha: .45)),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
      ),
      child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Icon(Icons.warning_amber_rounded, size: 18, color: accent),
        const SizedBox(width: AppTokens.s8),
        Expanded(child: child),
      ]),
    );
  }
}

class _GeneralSection extends ConsumerWidget {
  const _GeneralSection();
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final perms = ref.watch(adminPermsProvider);
    final behavior = ref.watch(agentBehaviorProvider);
    final api = ref.read(settingsApiProvider);

    return SettingsBody(
      title: context.tr('General'),
      children: [
        Text(context.tr('Network access'),
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        const NetworkBindField(),
        const SizedBox(height: AppTokens.s24),
        Text(context.tr('Connection'),
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        const _ApiTokenField(),
        const SizedBox(height: AppTokens.s16),
        Text(context.tr('Permissions'),
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        perms.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (p) => Column(children: [
            _ToggleRow(
              label: context.tr('Skip all-agent permissions'),
              desc: context.tr('Auto-accept tool calls for every agent.'),
              value: p['skipAllAgentsPermissions'] == true,
              onChanged: (v) => api.post(
                  '/api/admin-permissions',
                  {...p, 'skipAllAgentsPermissions': v},
                  adminPermsProvider),
            ),
            _ToggleRow(
              label: context.tr('Skip main-agent permissions'),
              desc:
                  context.tr('Auto-accept tool calls for the main agent only.'),
              value: p['skipMainAgentPermissions'] == true,
              onChanged: (v) => api.post(
                  '/api/admin-permissions',
                  {...p, 'skipMainAgentPermissions': v},
                  adminPermsProvider),
            ),
          ]),
        ),
        const SizedBox(height: AppTokens.s16),
        Text(context.tr('Agent behavior'),
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        behavior.when(
          loading: () => const LinearProgressIndicator(),
          error: (e, _) => Text('$e'),
          data: (b) => Column(children: [
            _ToggleRow(
              label: context.tr('After-process hook'),
              desc: context.tr('Run the post-processing step after each turn.'),
              value: b['afterProcess'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'afterProcess': v}, agentBehaviorProvider),
            ),
            _ToggleRow(
              label: context.tr('Pre-cognitive recall'),
              desc: context.tr('Inject relevant memories before processing.'),
              value: b['preCognitive'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'preCognitive': v}, agentBehaviorProvider),
            ),
            _ToggleRow(
              label: context.tr('Memory recall'),
              desc: context.tr(
                  'Consolidate dropped history into memory files and '
                  'inject relevant saved memories into each request.'),
              value: b['memoryRecall'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'memoryRecall': v}, agentBehaviorProvider),
            ),
            _ToggleRow(
              label: context.tr('Pre-trigger skill'),
              desc: context.tr('Evaluate trigger skills before the main turn.'),
              value: b['preTriggerSkill'] == true,
              onChanged: (v) => api.post('/api/agent-behavior',
                  {...b, 'preTriggerSkill': v}, agentBehaviorProvider),
            ),
          ]),
        ),
        const SizedBox(height: AppTokens.s16),
        Text(context.tr('Autonomous tasks'),
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
        const SizedBox(height: AppTokens.s8),
        ref.watch(dispatchConfigProvider).when(
              loading: () => const LinearProgressIndicator(),
              error: (e, _) => Text('$e'),
              data: (d) => _ToggleRow(
                label: context.tr('Auto-run Kanban tasks (dispatcher)'),
                desc: context.tr(
                    'Automatically assign a worker agent to each task in a '
                    'Kanban board\'s Ready column, run it, and complete or block '
                    'it. Agents act unattended — leave OFF unless you want that.'),
                value: d['enabled'] == true,
                onChanged: (v) => api.post(
                    '/api/dispatch-config', {'enabled': v}, dispatchConfigProvider),
              ),
            ),
        if (isCaptureSupported) ...[
          const SizedBox(height: AppTokens.s16),
          Text(context.tr('Screen capture'),
              style: TextStyle(
                  color: context.colors.textSecondary,
                  fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s8),
          const _CaptureHotkeyRow(),
        ],
      ],
    );
  }
}

/// Global shortcut for tray screen capture. Not Cmd+Shift+4: macOS owns that
/// combo and won't hand it over, so the default sits one modifier away.
class _CaptureHotkeyRow extends ConsumerStatefulWidget {
  const _CaptureHotkeyRow();
  @override
  ConsumerState<_CaptureHotkeyRow> createState() => _CaptureHotkeyRowState();
}

class _CaptureHotkeyRowState extends ConsumerState<_CaptureHotkeyRow> {
  bool _recording = false;

  void _start() {
    // Suspend the live hotkey first: while it's registered, macOS eats the
    // combo and the recorder would never see it.
    ref.read(captureHotkeyProvider.notifier).suspend();
    setState(() => _recording = true);
  }

  void _cancel() {
    ref.read(captureHotkeyProvider.notifier).resume();
    setState(() => _recording = false);
  }

  /// `HotKeyRecorder` calls this on EVERY key down — including the bare `Ctrl`
  /// at the start of a combo. Committing on the first callback would record
  /// `Ctrl` alone and close before the user finished typing the shortcut, so
  /// wait for a combo that's actually usable.
  void _onRecorded(HotKey hk) {
    if (hk.key == PhysicalKeyboardKey.escape) {
      _cancel();
      return;
    }
    if (!isUsableHotKey(hk)) return; // still mid-combo
    ref.read(captureHotkeyProvider.notifier).update(hk);
    setState(() => _recording = false);
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final st = ref.watch(captureHotkeyProvider);
    final n = ref.read(captureHotkeyProvider.notifier);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(context.tr('Capture shortcut'),
                      style: TextStyle(
                          color: c.textPrimary, fontWeight: FontWeight.w600)),
                  const SizedBox(height: 2),
                  Text(
                    context.tr(
                        'Press this anywhere to grab a region — the same selector as '
                        'macOS Cmd+Shift+4. Needs at least one modifier.'),
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                ],
              ),
            ),
            const SizedBox(width: AppTokens.s12),
            if (_recording) ...[
              Text(context.tr('Press the shortcut…'),
                  style: TextStyle(color: c.accent, fontSize: 12)),
              const SizedBox(width: AppTokens.s8),
              // The recorder swallows keystrokes while mounted, so it replaces
              // the chip rather than sitting alongside it.
              HotKeyRecorder(
                initalHotKey: st.hotKey,
                onHotKeyRecorded: _onRecorded,
              ),
              const SizedBox(width: AppTokens.s4),
              TextButton(
                  onPressed: _cancel, child: Text(context.tr('Cancel'))),
            ] else ...[
              OutlinedButton(
                onPressed: _start,
                child: Text(hotKeyLabel(st.hotKey),
                    style: const TextStyle(
                        fontFeatures: [FontFeature.tabularFigures()])),
              ),
              const SizedBox(width: AppTokens.s4),
              IconButton(
                tooltip: context.tr('Reset to default (⌃ ⇧ 4)'),
                icon: const Icon(Icons.restart_alt, size: 18),
                onPressed: n.resetToDefault,
              ),
            ],
          ],
        ),
        if (st.error != null)
          Padding(
            padding: const EdgeInsets.only(top: AppTokens.s8),
            child: Text(st.error!,
                style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
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
    return SettingsBody(
      title: context.tr('Channels'),
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
              label: Text(context.tr('Add channel')),
            ),
          ),
        ),
        if (channels.isEmpty)
          Text(context.tr('No channels connected.'),
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
                  tooltip: context.tr('Edit'),
                  icon: Icon(Icons.edit_outlined,
                      size: 16, color: context.colors.textSecondary),
                  onPressed: () => showDialog(
                      context: context,
                      builder: (_) => _ChannelEditor(existing: ch)),
                ),
                IconButton(
                  tooltip: context.tr('Remove'),
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
            Text(context.tr(_platformLabels[key]!),
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
            _segment(context, o[0], group, context.tr(o[1]), onTap),
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
              Text(
                  context
                      .tr(_editing ? 'Edit channel' : 'Add channel'),
                  style: const TextStyle(
                      fontSize: 17, fontWeight: FontWeight.w600)),
              Text(
                  context.tr(_editing
                      ? 'Rename or reconfigure this channel'
                      : 'Connect a messaging platform to your agent'),
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
                context.tr('PLATFORM'),
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
                context.tr('NAME'),
                TextField(
                  controller: _name,
                  decoration: InputDecoration(
                      hintText: context.tr('My Telegram bot')),
                ),
              ),
              const SizedBox(height: AppTokens.s16),
              if (isTelegram) ...[
                _labeled(
                  context,
                  context.tr('BOT TOKEN'),
                  TextField(
                    controller: _botToken,
                    obscureText: true,
                    decoration:
                        const InputDecoration(hintText: '123456:ABC-DEF…'),
                  ),
                  hint: context.tr('Leave empty to use the .env default bot'),
                ),
                const SizedBox(height: AppTokens.s16),
                _labeled(
                  context,
                  context.tr('CHAT TYPE'),
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
                  context.tr('HUB URL'),
                  TextField(
                    controller: _hubUrl,
                    decoration: const InputDecoration(
                        hintText: 'https://hub.senclaw.ai'),
                  ),
                  hint: context.tr(
                      'Registers with the hub, then shows a QR code for the '
                      'Senclaw mobile app to scan.'),
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
                      label: Text(context.tr('Show pairing QR')),
                    ),
                  ),
                if (_editing) const SizedBox(height: AppTokens.s16),
              ] else ...[
                _labeled(
                  context,
                  context.tr('APP ID'),
                  TextField(
                      controller: _appId,
                      decoration:
                          const InputDecoration(hintText: 'cli_xxx')),
                ),
                const SizedBox(height: AppTokens.s16),
                _labeled(
                  context,
                  context.tr('APP SECRET'),
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
                    context.tr('Sandbox'),
                    context.tr('Use the QQ sandbox environment'),
                    _sandbox,
                    (v) => setState(() => _sandbox = v),
                  ),
                if (_platform == 'qq') const SizedBox(height: AppTokens.s16),
              ],
              if (isTelegram || _platform == 'feishu')
                _toggleCard(
                  context,
                  context.tr('Require @mention to trigger'),
                  context.tr('Only reply when the bot is explicitly mentioned'),
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
            child: Text(context.tr('Cancel'))),
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
          child: Text(context.tr(_registering
              ? 'Registering…'
              : _editing
                  ? 'Save'
                  : _platform == 'senclaw'
                      ? 'Register & Get QR'
                      : 'Add channel')),
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
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content:
                Text(context.trArgs('Pairing failed: {e}', {'e': e}))));
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
      title: Row(children: [
        const Icon(Icons.qr_code_2, size: 20),
        const SizedBox(width: AppTokens.s8),
        Text(context.tr('Scan to connect')),
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
            Text(
                context.tr(
                    'Open the Senclaw mobile app and scan this code to pair.'),
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textMuted, fontSize: 12)),
            const SizedBox(height: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () {
                Clipboard.setData(ClipboardData(text: payload));
                ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                    content: Text(context.tr('Pairing link copied'))));
              },
              icon: const Icon(Icons.copy, size: 14),
              label: Text(context.tr('Copy pairing link')),
            ),
          ],
        ),
      ),
      actions: [
        FilledButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Done'))),
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
    return SettingsBody(
      title: context.tr('Profiles'),
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
              label: Text(context.tr('New profile')),
            ),
          ),
        ),
        if (agents.isEmpty)
          Text(context.tr('No agent profiles.'),
              style: TextStyle(color: c.textMuted)),
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
                        final suffix = n > 0
                            ? ' · ${context.trPlural(n, '{n} channel', '{n} channels')}'
                            : '';
                        return Text(
                            context.trArgs(
                                    'folder: {folder}', {'folder': a.folder}) +
                                suffix,
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
                  label: Text(context.tr('Edit')),
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
        ? context.tr('New profile')
        : context.trArgs(
            'Edit agent · {folder}', {'folder': widget.agent!.folder});

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
                      decoration: InputDecoration(
                          labelText: context.tr('Name'),
                          hintText: context.tr('My assistant')),
                    ),
                  ),
                  if (_isCreate) ...[
                    const SizedBox(width: AppTokens.s8),
                    Expanded(
                      child: TextField(
                        controller: _folder,
                        onChanged: (_) => _folderEdited = true,
                        decoration: InputDecoration(
                            labelText: context.tr('Folder'),
                            hintText: 'my-assistant'),
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
                        decoration:
                            InputDecoration(labelText: context.tr('Model')),
                        items: [
                          DropdownMenuItem(
                              value: '',
                              child: Text(context.tr('Global default'))),
                          if (extra != null)
                            DropdownMenuItem(
                                value: extra,
                                child: Text(
                                    context.trArgs(
                                        '{id} (current)', {'id': extra}),
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
              Text(context.tr('BOUND CHANNELS'),
                  style: TextStyle(
                      color: c.textSecondary,
                      fontSize: 12,
                      fontWeight: FontWeight.w600)),
              const SizedBox(height: AppTokens.s6),
              if (channels.isEmpty)
                Text(context.tr('No channels — add one in the Channels tab.'),
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
                      hintText: context.tr(_editMemory
                          ? 'MEMORY.md (agent long-term memory)…'
                          : 'Core prompt (SOUL.md)…')),
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
                      child: Text(context.tr('Cancel'))),
                  const SizedBox(width: AppTokens.s8),
                  FilledButton(
                    onPressed: _saving ? null : _save,
                    child: _saving
                        ? const SizedBox(
                            width: 16,
                            height: 16,
                            child: CircularProgressIndicator(
                                strokeWidth: 2, color: Colors.white))
                        : Text(context.tr(_isCreate ? 'Create' : 'Save')),
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
      tooltip: takenByOther
          ? context.tr('Already bound to another profile')
          : null,
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
    return SettingsBody(
      title: context.tr('Tool Rules'),
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
                    Text(context.tr('Dangerously accept all'),
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w600)),
                    Text(
                        context.tr(
                            'Auto-accept every tool call without prompting.'),
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
              child: Text(context.tr('Auto-accept rules'),
                  style: TextStyle(
                      color: c.textSecondary, fontWeight: FontWeight.w700)),
            ),
            TextButton.icon(
              onPressed: () => showDialog(
                  context: context, builder: (_) => const _RuleEditor()),
              icon: const Icon(Icons.add, size: 16),
              label: Text(context.tr('Add rule')),
            ),
          ],
        ),
        const SizedBox(height: AppTokens.s8),
        if (rules.isEmpty)
          Text(context.tr('No rules. Tool calls follow per-agent defaults.'),
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
                  tooltip: context.tr('Delete'),
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
      title: Text(context.tr('Add tool rule'),
          style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w600)),
      content: SizedBox(
        width: 440,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _labeled(
                context,
                context.tr('ACTION'),
                DropdownButtonFormField<String>(
                  initialValue: _action,
                  isExpanded: true,
                  items: [
                    for (final (v, l) in _actions)
                      DropdownMenuItem(value: v, child: Text(context.tr(l))),
                  ],
                  onChanged: (v) => setState(() => _action = v ?? 'auto_accept'),
                ),
              ),
              _labeled(
                context,
                context.tr('MATCH'),
                DropdownButtonFormField<String>(
                  initialValue: _type,
                  isExpanded: true,
                  items: [
                    for (final (v, l, _) in _matcherTypes)
                      DropdownMenuItem(value: v, child: Text(context.tr(l))),
                  ],
                  onChanged: (v) => setState(() => _type = v ?? 'bash_glob'),
                ),
              ),
              if (key == 'pattern')
                _labeled(
                  context,
                  context.tr('PATTERN'),
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
                  context.tr('TOOL NAME'),
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
                  context.tr('SKILL NAME'),
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
                  context.tr('MCP SERVER'),
                  TextField(
                    controller: _server,
                    autofocus: true,
                    decoration:
                        const InputDecoration(hintText: 'e.g. deepwiki-mcp'),
                  ),
                ),
                _labeled(
                  context,
                  context.tr('TOOL (optional — blank = all)'),
                  TextField(
                    controller: _mcpTool,
                    decoration: const InputDecoration(hintText: 'e.g. search'),
                  ),
                ),
              ],
              if (key == 'category')
                _labeled(
                  context,
                  context.tr('CATEGORY'),
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
                context.tr('DESCRIPTION (optional)'),
                TextField(
                  controller: _desc,
                  decoration: InputDecoration(
                      hintText: context.tr('Why this rule exists')),
                ),
              ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(context.tr('Cancel'))),
        FilledButton(
          onPressed: valid ? _submit : null,
          child: Text(context.tr('Add rule')),
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
    return SettingsBody(
      title: context.tr('LLM Models'),
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
                    Text(context.tr('Extended thinking'),
                        style: TextStyle(
                            color: c.textPrimary,
                            fontWeight: FontWeight.w600)),
                    Text(context.tr('Let the model reason before replying'),
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
              label: Text(context.tr('Add endpoint')),
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
                                _MiniTag(context.tr('Main'), AppTokens.brand),
                              if (m.id == d.activeCognitiveId)
                                _MiniTag(context.tr('Cognitive'),
                                    AppTokens.success),
                              if (m.id == d.activeQuickId)
                                _MiniTag(
                                    context.tr('Quick'), AppTokens.warning),
                            ],
                          ),
                        ),
                        PopupMenuButton<String>(
                          tooltip: context.tr('Set role'),
                          position: PopupMenuPosition.under,
                          onSelected: (t) => setRole(m.id, t),
                          itemBuilder: (_) => [
                            if (m.id != d.activeId)
                              PopupMenuItem(
                                  value: 'main',
                                  child: Text(context.tr('Set as Main'))),
                            if (m.id != d.activeCognitiveId)
                              PopupMenuItem(
                                  value: 'cognitive',
                                  child: Text(context.tr('Set as Cognitive'))),
                            if (m.id != d.activeQuickId)
                              PopupMenuItem(
                                  value: 'quick',
                                  child: Text(context.tr('Set as Quick'))),
                          ],
                          child: Padding(
                            padding: const EdgeInsets.symmetric(
                                horizontal: AppTokens.s8, vertical: AppTokens.s4),
                            child: Row(mainAxisSize: MainAxisSize.min, children: [
                              Text(context.tr('Set as…'),
                                  style: TextStyle(
                                      color: c.accent, fontSize: 13)),
                              Icon(Icons.expand_more, size: 16, color: c.accent),
                            ]),
                          ),
                        ),
                        IconButton(
                          tooltip: context.tr('Edit'),
                          icon: Icon(Icons.edit_outlined,
                              size: 16, color: c.textSecondary),
                          onPressed: () => showDialog(
                              context: context,
                              builder: (_) => _LlmEditor(existing: m)),
                        ),
                        IconButton(
                          tooltip: context.tr('Delete'),
                          icon: const Icon(Icons.delete_outline,
                              size: 16, color: AppTokens.danger),
                          onPressed: () async {
                            final ok = await showDialog<bool>(
                              context: context,
                              builder: (dctx) => AlertDialog(
                                title: Text(dctx.tr('Delete endpoint?')),
                                content: Text(dctx.trArgs(
                                    '"{label}" will be removed. Chats '
                                    'using it fall back to the active '
                                    'default model.',
                                    {'label': m.label})),
                                actions: [
                                  TextButton(
                                      onPressed: () =>
                                          Navigator.of(dctx).pop(false),
                                      child: Text(dctx.tr('Cancel'))),
                                  FilledButton(
                                      style: FilledButton.styleFrom(
                                          backgroundColor: AppTokens.danger),
                                      onPressed: () =>
                                          Navigator.of(dctx).pop(true),
                                      child: Text(dctx.tr('Delete'))),
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
          _testResult = context
              .trArgs('✓ Loaded {n} model(s)', {'n': models.length});
        } else {
          _testResult = '✗ ${(r is Map ? r['message'] : null) ?? context.tr('No models')}';
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
      setState(() => _testResult = context.tr('✓ Connection OK'));
    } catch (e) {
      setState(() => _testResult = '✗ $e');
    } finally {
      setState(() => _busy = false);
    }
  }

  Future<void> _save() async {
    if (_model.text.trim().isEmpty) {
      setState(() => _testResult = context.tr('✗ Model name is required'));
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
      title: Text(context
          .tr(_isEdit ? 'Edit LLM endpoint' : 'Add LLM endpoint')),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            DropdownButtonFormField<String>(
              initialValue: _provider,
              decoration:
                  InputDecoration(labelText: context.tr('Provider')),
              items: [
                if (!providerKeys.contains(_provider))
                  DropdownMenuItem(value: _provider, child: Text(_provider)),
                for (final k in providerKeys)
                  DropdownMenuItem(
                      value: k,
                      child: Text(context.tr(_llmProviders[k]!.name))),
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
                    labelText: context.tr('Base URL'),
                    hintText: def?.urlHint)),
            const SizedBox(height: AppTokens.s8),
            TextField(
                controller: _apiKey,
                obscureText: true,
                decoration: InputDecoration(
                    labelText: context.tr('API key'),
                    hintText: _isEdit
                        ? context.tr('Stored key — edit to replace')
                        : (def?.keyHint == null
                            ? null
                            : context.tr(def!.keyHint!)))),
            const SizedBox(height: AppTokens.s8),
            // Protocol the endpoint speaks — pre-set by the provider preset,
            // editable for custom/self-hosted gateways.
            DropdownButtonFormField<String>(
              initialValue: _adapt == 'anthropic' ? 'anthropic' : 'openai',
              decoration: InputDecoration(
                  labelText: context.tr('API type (compatibility)')),
              items: [
                DropdownMenuItem(
                    value: 'openai',
                    child: Text(context.tr('OpenAI-compatible'))),
                DropdownMenuItem(
                    value: 'anthropic',
                    child: Text(context.tr('Anthropic-compatible'))),
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
                      decoration: InputDecoration(
                          labelText: context.tr('Model name'))),
                ),
                const SizedBox(width: AppTokens.s8),
                OutlinedButton(
                  onPressed: _busy ? null : _fetchModels,
                  child: Text(context.tr('Fetch')),
                ),
              ],
            ),
            if (_availableModels.isNotEmpty) ...[
              const SizedBox(height: AppTokens.s8),
              DropdownButtonFormField<String>(
                initialValue:
                    _availableModels.contains(_model.text) ? _model.text : null,
                isExpanded: true,
                decoration: InputDecoration(
                    labelText: context.tr('Available models')),
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
              decoration: InputDecoration(
                  labelText: context.tr('Vision (image input)')),
              items: [
                DropdownMenuItem(
                    value: 'auto',
                    child: Text(context.tr('Auto (infer from model name)'))),
                DropdownMenuItem(
                    value: 'on', child: Text(context.tr('Supported'))),
                DropdownMenuItem(
                    value: 'off', child: Text(context.tr('Not supported'))),
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
            child: Text(context.tr('Cancel'))),
        OutlinedButton(
            onPressed: _busy ? null : _test, child: Text(context.tr('Test'))),
        FilledButton(
            onPressed: _busy ? null : _save, child: Text(context.tr('Save'))),
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
    return SettingsBody(
      title: context.tr('Local Models'),
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
                          context.trArgs(
                              'Platform: {platform} — local MLX inference only runs '
                              'on macOS (Apple Silicon).',
                              {'platform': platform}),
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
        _HfAddModelCard(
          key: const ValueKey('hf-add-local'),
          apiBase: '/api/local-models',
          onDownloaded: () => ref.invalidate(localModelsProvider),
        ),
        const SizedBox(height: AppTokens.s12),
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
                                      ? context.tr('Downloading…')
                                      : context.trArgs('Downloading {pct}%', {
                                          'pct': (m.downloadProgress! * 100)
                                              .toStringAsFixed(0)
                                        }),
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
    // Resolved up front: the toasts below fire after an await, and reaching
    // through `context` there would be a use across an async gap.
    final l10n = L10n.of(context);
    Future<void> hit(String action) async {
      // Model ids are HF repos with slashes — encode so they don't break the
      // URL path (matches the web `encodeURIComponent`).
      try {
        await ref
            .read(apiClientProvider)
            .post('/api/local-models/${Uri.encodeComponent(m.id)}/$action');
      } catch (e) {
        // State context — guard with the State's own `mounted` (the analyzer
        // flags `context.mounted` here as an unrelated check).
        if (mounted) {
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
        label: Text(context.tr('Cancel'),
            style: const TextStyle(color: AppTokens.danger)),
      );
    }
    if (!m.installed) {
      return TextButton.icon(
        onPressed: () => hit('download'),
        icon: const Icon(Icons.download, size: 16),
        label: Text(context.tr('Download')),
      );
    }
    void toast(String msg) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(msg)));
      }
    }

    /// `preferred_backend` from the inference settings — decides `/load` vs
    /// `/load-mlx` and the `?backend=` hint on use-as-llm (web parity).
    Future<String?> preferredBackend() async {
      try {
        final s = await ref.read(apiClientProvider).get(
              '/api/local-models/settings',
            );
        if (s is Map) return s['preferred_backend'] as String?;
      } catch (_) {}
      return null;
    }

    Future<void> useAsLlm() async {
      try {
        final backend = await preferredBackend();
        // Ids are HF repos with slashes: encode, or the path gains an extra
        // segment, misses the `:id/use-as-llm` route and silently falls
        // through to the SPA handler — the button appears to do nothing.
        final res = await ref.read(apiClientProvider).post(
              '/api/local-models/${Uri.encodeComponent(m.id)}/use-as-llm'
              '${backend == null ? '' : '?backend=$backend'}',
            );
        final label = (res is Map && res['config'] is Map)
            ? '${(res['config'] as Map)['label']}'
            : m.label;
        if (res is Map && res['existed'] == true) {
          toast(
              l10n.tArgs('Already in LLM Models: {label}', {'label': label}));
        } else if (res is Map && res['active'] == true) {
          toast(l10n.tArgs('Added as LLM profile and set active: {label}',
              {'label': label}));
        } else {
          toast(l10n.tArgs('Added as LLM profile: {label}', {'label': label}));
        }
      } catch (e) {
        toast(l10n.tArgs('Failed to add as LLM: {e}', {'e': e}));
      }
      ref.invalidate(localModelsProvider);
      ref.invalidate(llmConfigsProvider);
    }

    Future<void> load() async {
      // `/load` is Candle-only; MLX has its own endpoint.
      final useMlx = await preferredBackend() == 'mlx';
      await hit(useMlx ? 'load-mlx' : 'load');
    }

    Future<void> remove() async {
      try {
        await ref
            .read(apiClientProvider)
            .delete('/api/local-models/${Uri.encodeComponent(m.id)}');
        toast(l10n.tArgs('Removed {label}', {'label': m.label}));
      } catch (e) {
        toast(l10n.tArgs('Delete failed: {e}', {'e': e}));
      }
      ref.invalidate(localModelsProvider);
    }

    // Installed: load/unload + use-as-LLM + delete (web LocalModelsSettings).
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        TextButton(
          onPressed: useAsLlm,
          child: Text(context.tr('Use as LLM')),
        ),
        TextButton(
          onPressed: () => m.loaded ? hit('unload') : load(),
          child: Text(context.tr(m.loaded ? 'Unload' : 'Load')),
        ),
        IconButton(
          tooltip: context.tr('Delete'),
          icon: const Icon(Icons.delete_outline, size: 16,
              color: AppTokens.danger),
          onPressed: remove,
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
    return SettingsBody(
      title: widget.title,
      children: [
        // Key by domain: without it Flutter reuses the card's State when
        // switching between the Whisper/TTS/OCR sections (same widget type at
        // the same tree position), so e.g. the TTS card kept showing the
        // Whisper model as "current" — and Save would persist it.
        _MediaSettingsCard(key: ValueKey('media-settings-$domain'), domain: domain),
        const SizedBox(height: AppTokens.s12),
        // Custom HF model install with pre-download compatibility check
        // (validate endpoints exist for whisper + tts, not ocr).
        if (domain != 'ocr') ...[
          _HfAddModelCard(
            key: ValueKey('hf-add-$domain'),
            apiBase: '/api/$domain/models',
            onDownloaded: () => ref.invalidate(mediaModelsProvider(domain)),
          ),
          const SizedBox(height: AppTokens.s12),
        ],
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
                              tooltip: context.tr('Delete'),
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
                                  label: Text(context.tr('Download')),
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
    final label = m.downloadProgress != null
        ? context.trArgs('Downloading {pct}%',
            {'pct': (m.downloadProgress! * 100).round()})
        : context.tr('Downloading');
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
      Text(label, style: TextStyle(color: c.textMuted, fontSize: 12)),
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
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.tr('Embedding config saved'))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Save failed: {e}', {'e': e}))));
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
      error: (e, _) => SettingsBody(
          title: context.tr('Embedding'), children: [Text('$e')]),
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
        return SettingsBody(
          title: context.tr('Embedding'),
          children: [
            DropdownButtonFormField<String>(
              initialValue: _provider,
              decoration:
                  InputDecoration(labelText: context.tr('Provider')),
              items: [
                DropdownMenuItem(
                    value: 'none',
                    child: Text(context.tr('None (disabled)'))),
                const DropdownMenuItem(
                    value: 'openai', child: Text('OpenAI')),
                const DropdownMenuItem(
                    value: 'openrouter', child: Text('OpenRouter')),
                const DropdownMenuItem(
                    value: 'ollama', child: Text('Ollama')),
                DropdownMenuItem(
                    value: 'local',
                    child: Text(context.tr('Local (on-device)'))),
              ],
              onChanged: (v) => _applyPreset(v ?? 'none'),
            ),
            if (_provider != 'none') ...[
              const SizedBox(height: AppTokens.s12),
              if (needsKey)
                TextField(
                  controller: _apiKey,
                  obscureText: true,
                  decoration:
                      InputDecoration(labelText: context.tr('API key')),
                ),
              if (needsUrl) ...[
                const SizedBox(height: AppTokens.s8),
                TextField(
                  controller: _baseUrl,
                  decoration:
                      InputDecoration(labelText: context.tr('Base URL')),
                ),
              ],
              const SizedBox(height: AppTokens.s8),
              TextField(
                controller: _modelName,
                decoration:
                    InputDecoration(labelText: context.tr('Model name')),
              ),
              if (isLocal) ...[
                const SizedBox(height: AppTokens.s8),
                TextField(
                  controller: _modelPath,
                  decoration: InputDecoration(
                      labelText: context.tr('Model path (optional)')),
                ),
              ],
              const SizedBox(height: AppTokens.s8),
              TextField(
                controller: _dimensions,
                keyboardType: TextInputType.number,
                decoration: InputDecoration(
                    labelText: context.tr('Dimensions (optional)')),
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
                label: Text(context.tr('Save')),
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
        Text(context.tr('Local models'),
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
                          Text(context.tr('Installed'),
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
                                  SnackBar(
                                      content: Text(
                                          context.tr('Downloading model…'))));
                            }
                          },
                          icon: const Icon(Icons.download, size: 16),
                          label: Text(context.tr('Download')),
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
  final _reflectWindowIdleMs = TextEditingController();
  final _maintenanceHours = TextEditingController();
  bool _saving = false;

  @override
  void dispose() {
    _maxConcurrent.dispose();
    _maxOutputChars.dispose();
    _reflectMinChars.dispose();
    _reflectMaxChars.dispose();
    _reflectCooldownMs.dispose();
    _reflectWindowIdleMs.dispose();
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
    _reflectWindowIdleMs.text = s('reflectWindowIdleMs');
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
        if (n(_reflectWindowIdleMs) != null)
          'reflectWindowIdleMs': n(_reflectWindowIdleMs),
        if (n(_maintenanceHours) != null)
          'maintenanceIntervalHours': n(_maintenanceHours),
      });
      ref.invalidate(cognitiveConfigProvider);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.tr('Cognitive config saved'))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Save failed: {e}', {'e': e}))));
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
      error: (e, _) => SettingsBody(
          title: context.tr('Knowledge (Cognitive)'), children: [Text('$e')]),
      data: (d) {
        if (!_seeded) {
          _seeded = true;
          _seed(d);
        }
        return SettingsBody(
          title: context.tr('Knowledge (Cognitive)'),
          children: [
            _ToggleRow(
              label: context.tr('Enable cognitive layer'),
              desc: context.tr('Graph + Hebbian recall across sessions.'),
              value: _enabled,
              onChanged: (v) => setState(() => _enabled = v),
            ),
            _ToggleRow(
              label: context.tr('Auto-reflect on every user message'),
              desc:
                  context.tr('Cognify each incoming message automatically.'),
              value: _autoReflection,
              onChanged: (v) => setState(() => _autoReflection = v),
            ),
            const SizedBox(height: AppTokens.s20),
            _SettingsGroupLabel(context.tr('Extraction')),
            _NumberRow(
              label: context.tr('Max concurrent extractions'),
              desc: context.tr(
                  'Semaphore size for in-flight cognify calls. Keep low on '
                  'local models.'),
              controller: _maxConcurrent,
            ),
            _NumberRow(
              label: context.tr('Max LLM output chars'),
              desc: context.tr(
                  'Hard cap on cognify-LLM output; streams abort past this.'),
              controller: _maxOutputChars,
            ),
            const SizedBox(height: AppTokens.s16),
            _SettingsGroupLabel(context.tr('Reflection')),
            _NumberRow(
              label: context.tr('Min chars'),
              desc: context
                  .tr('Skip reflection for messages shorter than this.'),
              controller: _reflectMinChars,
            ),
            _NumberRow(
              label: context.tr('Max chars'),
              desc: context.tr(
                  'Window size: buffered turns flush to one extraction '
                  'call when they reach this length.'),
              controller: _reflectMaxChars,
            ),
            _NumberRow(
              label: context.tr('Cooldown (ms)'),
              desc:
                  context.tr('Minimum gap between window flushes per agent.'),
              controller: _reflectCooldownMs,
            ),
            _NumberRow(
              label: context.tr('Window idle (ms)'),
              desc: context.tr(
                  'Flush the conversation window after this much chat '
                  'silence. 0 = flush per message.'),
              controller: _reflectWindowIdleMs,
            ),
            const SizedBox(height: AppTokens.s16),
            _SettingsGroupLabel(context.tr('Maintenance')),
            _NumberRow(
              label: context.tr('Sweep interval (hours)'),
              desc: context
                  .tr('How often the background decay/prune sweep runs.'),
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
                label: Text(context.tr('Save')),
              ),
              const SizedBox(width: AppTokens.s8),
              OutlinedButton.icon(
                onPressed: () async {
                  await ref
                      .read(apiClientProvider)
                      .post('/api/cognitive/maintenance');
                  if (context.mounted) {
                    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                        content: Text(context.tr('Maintenance started'))));
                  }
                },
                icon: const Icon(Icons.cleaning_services_outlined, size: 16),
                label: Text(context.tr('Run maintenance')),
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

/// Result of checking one installed app against the hub package registry.
class SpaceAppUpdate {
  final String id;
  final String? installed;
  final String? latest;
  final bool hasUpdate;
  const SpaceAppUpdate(this.id, this.installed, this.latest, this.hasUpdate);
  factory SpaceAppUpdate.fromJson(Map<String, dynamic> j) => SpaceAppUpdate(
        '${j['id'] ?? ''}',
        j['installed'] as String?,
        j['latest'] as String?,
        j['hasUpdate'] == true,
      );
}

/// Available updates keyed by app id. Non-fatal: an unreachable hub yields an
/// empty map (no badges), never an error surface.
final spaceAppUpdatesProvider =
    FutureProvider<Map<String, SpaceAppUpdate>>((ref) async {
  try {
    final r = await ref.read(apiClientProvider).get('/api/space/apps/updates');
    final list = r is List ? r : const [];
    return {
      for (final e in list.whereType<Map>())
        '${e['id']}': SpaceAppUpdate.fromJson(e.cast<String, dynamic>()),
    };
  } catch (_) {
    return const {};
  }
});

class SpaceAppsSection extends ConsumerWidget {
  const SpaceAppsSection({super.key});

  Future<void> _registerUrl(BuildContext context, WidgetRef ref) async {
    final ctrl = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (dctx) => AlertDialog(
        backgroundColor: dctx.colors.surface,
        title: Text(dctx.tr('Register Space App')),
        content: TextField(
          controller: ctrl,
          autofocus: true,
          decoration: InputDecoration(
              labelText: dctx.tr('Manifest URL'),
              hintText: 'https://…/senclaw-manifest.json'),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(dctx, false),
              child: Text(dctx.tr('Cancel'))),
          FilledButton(
              onPressed: () => Navigator.pop(dctx, true),
              child: Text(dctx.tr('Register'))),
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
            SnackBar(content: Text(context.tr('Space App registered'))));
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content:
                Text(context.trArgs('Register failed: {e}', {'e': e}))));
      }
    }
  }

  Future<void> _installZip(BuildContext context, WidgetRef ref) async {
    final FilePickerResult? res;
    try {
      res = await FilePicker.platform.pickFiles(
          type: FileType.custom, allowedExtensions: ['zip'], withData: kIsWeb);
    } catch (e) {
      // file_picker errors before showing the panel (e.g. macOS entitlement
      // check) — surface it instead of failing silently.
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content:
                Text(context.trArgs('File picker error: {e}', {'e': e}))));
      }
      return;
    }
    final f = res?.files.firstOrNull;
    if (f == null) return;
    final cfg = ref.read(appConfigProvider);
    final uri = Uri.parse('http://${cfg.host}:${cfg.uiPort}/api/space/apps/install-zip');
    final req = http.MultipartRequest('POST', uri);
    req.headers.addAll(cfg.authHeaders);
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
            SnackBar(content: Text(context.tr('Space App installed'))));
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Install failed: {e}', {'e': e}))));
      }
    }
  }

  Future<void> _checkUpdates(BuildContext context, WidgetRef ref) async {
    ref.invalidate(spaceAppUpdatesProvider);
    try {
      final map = await ref.read(spaceAppUpdatesProvider.future);
      final n = map.values.where((u) => u.hasUpdate).length;
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(n > 0
                ? context.trPlural(
                    n, '{n} app has an update', '{n} apps have updates')
                : context.tr('All apps are up to date'))));
      }
    } catch (_) {/* non-fatal */}
  }

  Future<void> _updateApp(
      BuildContext context, WidgetRef ref, String id) async {
    final messenger = ScaffoldMessenger.of(context);
    messenger.showSnackBar(SnackBar(
        content: Text(context.tr('Updating…')),
        duration: const Duration(seconds: 60)));
    try {
      final r = await ref
          .read(apiClientProvider)
          .post('/api/space/apps/$id/update');
      messenger.hideCurrentSnackBar();
      final updated = (r is Map && r['updated'] == true);
      if (!context.mounted) return;
      messenger.showSnackBar(SnackBar(
          content: Text(updated
              ? context.trArgs(
                  'Updated {id} → {v}', {'id': id, 'v': r['latest']})
              : context.trArgs('{id} is already up to date', {'id': id}))));
      ref.invalidate(spaceAppsProvider);
      ref.invalidate(spaceAppUpdatesProvider);
    } catch (e) {
      messenger.hideCurrentSnackBar();
      if (!context.mounted) return;
      messenger.showSnackBar(SnackBar(
          content: Text(context.trArgs('Update failed: {e}', {'e': e}))));
    }
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final apps = ref.watch(spaceAppsProvider);
    final updates =
        ref.watch(spaceAppUpdatesProvider).valueOrNull ?? const {};
    return SettingsBody(
      title: 'Space Apps',
      onRefresh: () {
        ref.invalidate(spaceAppsProvider);
        ref.invalidate(spaceAppUpdatesProvider);
      },
      children: [
        Text(context.tr('Install, register, and remove embedded Space Apps.'),
            style: TextStyle(color: c.textMuted, fontSize: 12)),
        const SizedBox(height: AppTokens.s12),
        Row(
          children: [
            FilledButton.icon(
              onPressed: () => _installZip(context, ref),
              icon: const Icon(Icons.upload_file, size: 16),
              label: Text(context.tr('Install ZIP')),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () => _registerUrl(context, ref),
              icon: const Icon(Icons.link, size: 16),
              label: Text(context.tr('Register URL')),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () => _checkUpdates(context, ref),
              icon: const Icon(Icons.cloud_download_outlined, size: 16),
              label: Text(context.tr('Check updates')),
            ),
            const Spacer(),
            IconButton(
              tooltip: context.tr('Refresh'),
              icon: const Icon(Icons.refresh, size: 18),
              onPressed: () {
                ref.invalidate(spaceAppsProvider);
                ref.invalidate(spaceAppUpdatesProvider);
              },
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
              ? Text(context.tr('No Space Apps installed'),
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
                                    if (updates[a.id]?.hasUpdate == true) ...[
                                      const SizedBox(width: AppTokens.s8),
                                      Container(
                                        padding: const EdgeInsets.symmetric(
                                            horizontal: AppTokens.s6,
                                            vertical: 1),
                                        decoration: BoxDecoration(
                                          color: AppTokens.warning
                                              .withValues(alpha: 0.15),
                                          borderRadius: BorderRadius.circular(
                                              AppTokens.rSm),
                                        ),
                                        child: Text(
                                          '${updates[a.id]!.installed ?? '?'} → ${updates[a.id]!.latest}',
                                          style: const TextStyle(
                                              color: AppTokens.warning,
                                              fontSize: 10,
                                              fontWeight: FontWeight.w600),
                                        ),
                                      ),
                                    ],
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
                              tooltip: context.tr('Details'),
                              icon: const Icon(Icons.info_outline, size: 16),
                              onPressed: () => showDialog(
                                  context: context,
                                  builder: (_) => _SpaceAppDetailDialog(app: a)),
                            ),
                            IconButton(
                              tooltip: context.tr('Sandbox settings'),
                              icon: const Icon(Icons.science_outlined, size: 16),
                              onPressed: () => showDialog(
                                  context: context,
                                  builder: (_) => SpaceAppSandboxDialog(
                                      appId: a.id, appName: a.name)),
                            ),
                            if (updates[a.id]?.hasUpdate == true)
                              FilledButton(
                                onPressed: () =>
                                    _updateApp(context, ref, a.id),
                                child: Text(context.tr('Update')),
                              ),
                            TextButton(
                              onPressed: () async {
                                final messenger =
                                    ScaffoldMessenger.of(context);
                                messenger.showSnackBar(SnackBar(
                                  content: Text(context.tr('Restarting…')),
                                  duration: const Duration(seconds: 40),
                                ));
                                try {
                                  await ref.read(apiClientProvider).post(
                                      '/api/space/apps/${a.id}/restart');
                                  messenger.hideCurrentSnackBar();
                                  if (!context.mounted) return;
                                  messenger.showSnackBar(SnackBar(
                                      content:
                                          Text(context.tr('Restarted'))));
                                  ref.invalidate(spaceAppsProvider);
                                } catch (e) {
                                  messenger.hideCurrentSnackBar();
                                  if (!context.mounted) return;
                                  messenger.showSnackBar(SnackBar(
                                      content: Text(context.trArgs(
                                          'Restart failed: {e}', {'e': e}))));
                                }
                              },
                              child: Text(context.tr('Restart')),
                            ),
                            IconButton(
                              tooltip: context.tr('Uninstall'),
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
    final widgets = (m['widgets'] as List?) ?? const [];
    final toolAliases =
        ((m['mcp'] as Map?)?['toolAliases'] as List?) ?? const [];

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
                    if (m['version'] != null)
                      kv(context.tr('Version'), '${m['version']}'),
                    // A served app has a process worth watching; a static one
                    // has nothing to report, so the panel only appears for the
                    // apps it can say something true about.
                    if (runtime?['kind'] == 'server') ...[
                      const SizedBox(height: AppTokens.s12),
                      Row(children: [
                        Icon(Icons.monitor_heart_outlined,
                            size: 14, color: c.accent),
                        const SizedBox(width: 4),
                        Text(context.tr('Process monitor'),
                            style: TextStyle(
                                color: c.textPrimary,
                                fontSize: 12.5,
                                fontWeight: FontWeight.w700)),
                      ]),
                      const SizedBox(height: AppTokens.s6),
                      SpaceAppRuntimePanel(appId: app.id),
                      const Divider(height: AppTokens.s20),
                    ],
                    if (integration != null)
                      kv(context.tr('Integration'),
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
                    // Widgets the app declares (manifest widgets[]) — chat
                    // widgets are emitted via emit_widget kind "app"; the
                    // rest render on the Dashboard.
                    if (widgets.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(bottom: AppTokens.s6),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            SizedBox(
                                width: 96,
                                child: Text('Widgets',
                                    style: TextStyle(
                                        color: c.textMuted, fontSize: 12))),
                            Expanded(
                              child: Wrap(
                                spacing: AppTokens.s6,
                                runSpacing: AppTokens.s6,
                                children: [
                                  for (final w in widgets.whereType<Map>())
                                    Tooltip(
                                      message: '${w['description'] ?? ''}',
                                      child: _MiniTag(
                                          '${w['name'] ?? w['id']} · '
                                          '${(w['surfaces'] as List?)?.join('/') ?? 'dashboard'}',
                                          AppTokens.cyan),
                                    ),
                                ],
                              ),
                            ),
                          ],
                        ),
                      ),
                    // MCP tool aliases the app requests (mcp.toolAliases) —
                    // imported DISABLED; the user enables them in
                    // Plugins → Alias.
                    if (toolAliases.isNotEmpty)
                      Padding(
                        padding: const EdgeInsets.only(bottom: AppTokens.s6),
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            SizedBox(
                                width: 96,
                                child: Text('Alias',
                                    style: TextStyle(
                                        color: c.textMuted, fontSize: 12))),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  for (final a in toolAliases.whereType<Map>())
                                    Padding(
                                      padding: const EdgeInsets.only(
                                          bottom: AppTokens.s4),
                                      child: Text(
                                        '${a['alias'] ?? '?'} → '
                                        '${a['tool'] ?? a['target'] ?? '?'}',
                                        style: TextStyle(
                                            color: c.textSecondary,
                                            fontSize: 11,
                                            fontFamily: AppTokens.fontMono),
                                      ),
                                    ),
                                  Text(
                                      context.tr(
                                          '(imported disabled — enable in Plugins → Alias)'),
                                      style: TextStyle(
                                          color: c.textMuted, fontSize: 10)),
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
                          return Text(context.tr('No MCP declared'),
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
                    // Logs — auto-reloads every 2s while this dialog is open.
                    _LogsBlock(appId: app.id),
                  ],
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  FilledButton.icon(
                    onPressed: () async {
                      final messenger = ScaffoldMessenger.of(context);
                      messenger.showSnackBar(SnackBar(
                        content: Text(context.tr('Restarting…')),
                        duration: const Duration(seconds: 40),
                      ));
                      try {
                        await ref
                            .read(apiClientProvider)
                            .post('/api/space/apps/${app.id}/restart');
                        messenger.hideCurrentSnackBar();
                        if (!context.mounted) return;
                        messenger.showSnackBar(SnackBar(
                            content: Text(context.tr('Restarted'))));
                        ref.invalidate(spaceAppsProvider);
                      } catch (e) {
                        messenger.hideCurrentSnackBar();
                        if (!context.mounted) return;
                        messenger.showSnackBar(SnackBar(
                            content: Text(context.trArgs(
                                'Restart failed: {e}', {'e': e}))));
                      }
                    },
                    icon: const Icon(Icons.restart_alt, size: 16),
                    label: Text(context.tr('Restart')),
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
      {required this.title, required this.path, required this.render});
  final String title;
  final String path;
  final Widget Function(dynamic data) render;

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
          constraints: const BoxConstraints(maxHeight: 120),
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

/// Runtime-logs panel for the Space-App detail dialog. Fetches on open and then
/// **auto-reloads every 3 s** while the dialog is mounted, so logs that the app
/// writes after you open the dialog show up without reopening it. Keeps the last
/// content during a refresh (no spinner flicker), offers a manual refresh + a
/// copy-all button, and renders the log as `SelectableText` so you can select
/// and copy individual lines. Only rebuilds when the content actually changes,
/// so an in-progress text selection survives idle auto-refreshes.
class _LogsBlock extends ConsumerStatefulWidget {
  const _LogsBlock({required this.appId});
  final String appId;
  @override
  ConsumerState<_LogsBlock> createState() => _LogsBlockState();
}

class _LogsBlockState extends ConsumerState<_LogsBlock> {
  String _content = '';
  Object? _error;
  bool _loading = true;
  bool _inFlight = false;
  Timer? _poll;

  @override
  void initState() {
    super.initState();
    _fetch();
    _poll = Timer.periodic(const Duration(seconds: 3), (_) => _fetch());
  }

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  Future<void> _fetch() async {
    if (_inFlight) return; // don't stack requests if one is slow
    _inFlight = true;
    try {
      final data = await ref
          .read(apiClientProvider)
          .get('/api/space/apps/${widget.appId}/logs?max_bytes=65536');
      if (!mounted) return;
      final text = data is Map
          ? '${data['content'] ?? data['logs'] ?? ''}'
          : '$data';
      // Skip the rebuild when nothing changed so an in-progress SelectableText
      // selection is not cleared by the 3 s auto-refresh. Still clear the
      // loading/error state on the first successful fetch.
      if (text == _content && _error == null && !_loading) return;
      setState(() {
        _content = text;
        _error = null;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e;
        _loading = false;
      });
    } finally {
      _inFlight = false;
    }
  }

  void _copy() {
    if (_content.isEmpty) return;
    Clipboard.setData(ClipboardData(text: _content));
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.tr('Log copied'))));
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final hasLogs = _content.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(context.tr('Logs'),
                style: TextStyle(
                    color: c.textSecondary,
                    fontSize: 12,
                    fontWeight: FontWeight.w700)),
            const SizedBox(width: AppTokens.s6),
            Container(
              padding:
                  const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
              decoration: BoxDecoration(
                color: c.accent.withValues(alpha: 0.14),
                borderRadius: BorderRadius.circular(AppTokens.rSm),
              ),
              child: Text(context.tr('auto'),
                  style: TextStyle(
                      color: c.accent,
                      fontSize: 10,
                      fontWeight: FontWeight.w600)),
            ),
            const Spacer(),
            IconButton(
              tooltip: context.tr('Copy all logs'),
              visualDensity: VisualDensity.compact,
              padding: EdgeInsets.zero,
              constraints: const BoxConstraints(),
              icon: Icon(Icons.copy_all,
                  size: 15, color: hasLogs ? c.textMuted : c.textMuted.withValues(alpha: 0.4)),
              onPressed: hasLogs ? _copy : null,
            ),
            const SizedBox(width: AppTokens.s8),
            IconButton(
              tooltip: context.tr('Refresh logs'),
              visualDensity: VisualDensity.compact,
              padding: EdgeInsets.zero,
              constraints: const BoxConstraints(),
              icon: Icon(Icons.refresh, size: 15, color: c.textMuted),
              onPressed: _fetch,
            ),
          ],
        ),
        const SizedBox(height: AppTokens.s6),
        Container(
          width: double.infinity,
          constraints: const BoxConstraints(maxHeight: 200),
          padding: const EdgeInsets.all(AppTokens.s8),
          decoration: BoxDecoration(
            color: c.surfaceAlt,
            border: Border.all(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rSm),
          ),
          child: _loading && _content.isEmpty
              ? const SizedBox(
                  height: 20,
                  width: 20,
                  child: CircularProgressIndicator(strokeWidth: 2))
              : _error != null && _content.isEmpty
                  ? Text('$_error',
                      style: const TextStyle(
                          color: AppTokens.danger, fontSize: 11))
                  : SingleChildScrollView(
                      reverse: true,
                      child: SelectableText(
                          _content.isEmpty ? context.tr('(no logs)') : _content,
                          style: TextStyle(
                              color: c.textMuted,
                              fontSize: 11,
                              fontFamily: AppTokens.fontMono)),
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
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.tr('Inference settings saved'))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Save failed: {e}', {'e': e}))));
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
          Text(context.tr('Inference settings'),
              style: TextStyle(
                  color: c.textPrimary, fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s12),
          _choiceRow<String>(
            context.tr('Inference backend'),
            context.tr(
                'Engine for Load / Use as LLM. MLX is Apple-Silicon-only & fastest.'),
            _backend,
            const [
              ('auto', 'Auto'),
              ('mlx', 'MLX native (~60–100 tok/s)'),
              ('candle', 'Candle (~12 tok/s)'),
            ],
            (v) => setState(() => _backend = v),
          ),
          _NumberRow(
              label: context.tr('Idle unload (secs)'),
              desc: context.tr(
                  '0 = never; ≥60 to free RAM after inactivity. Default 60.'),
              controller: _idleUnload),
          _choiceRow<int>(
            context.tr('KV TurboQuant bits'),
            context.tr('Quantize KV cache to save RAM on long generation.'),
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
            context.tr('MLX packed KV (Metal)'),
            context.tr(
                'MLX-native GPU KV quantization. Reload the model after changing.'),
            _mlxKvBits,
            const [
              (-1, 'Off — FP16'),
              (4, '4-bit packed'),
              (8, '8-bit packed'),
            ],
            (v) => setState(() => _mlxKvBits = v),
          ),
          _NumberRow(
              label: context.tr('TQ activate after (tokens)'),
              desc: context.tr(
                  'Cached tokens before TurboQuant kicks in. Default 16384.'),
              controller: _tqActivate),
          _NumberRow(
              label: context.tr('Max prompt tokens'),
              desc: context.tr(
                  'Hard cap on prompt length (512–262144). Default 128000.'),
              controller: _maxPrompt),
          _NumberRow(
              label: context.tr('Max new tokens'),
              desc: context.tr(
                  'Max tokens generated per request (1–8192). Default 8192.'),
              controller: _maxNew),
          _NumberRow(
              label: context.tr('Max KV tokens'),
              desc: context
                  .tr('KV-cache sliding window (128–262144). Default 16384.'),
              controller: _maxKvTokens),
          _NumberRow(
              label: context.tr('Temperature (MLX)'),
              desc: context
                  .tr('0 = greedy. Empty = server default (Gemma ≈0.65).'),
              controller: _temperature),
          _NumberRow(
              label: context.tr('Repetition penalty (MLX)'),
              desc: context
                  .tr('1 = off. Empty = server default (Gemma ≈1.15).'),
              controller: _repPenalty),
          _switchRow(
            context.tr('Thinking mode (Qwen3)'),
            context.tr('Chain-of-thought before answering. Off is faster.'),
            _enableThinking,
            (v) => setState(() => _enableThinking = v),
          ),
          _switchRow(
            context.tr('Release cache after session (MLX)'),
            context.tr(
                'Drop per-session KV/prefix cache when a chat ends. Weights stay.'),
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
              label: Text(context.tr('Save')),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: () async {
                await ref
                    .read(apiClientProvider)
                    .post('/api/local-models/unload-all');
                ref.invalidate(localModelsProvider);
                if (context.mounted) {
                  ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                      content: Text(context.tr('Unloaded all models'))));
                }
              },
              icon: const Icon(Icons.memory_outlined, size: 16),
              label: Text(context.tr('Unload all now')),
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
                      child: Text(context.tr(l),
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

/// "Add model from Hugging Face" card with pre-download compatibility check.
///
/// Works for any domain whose API exposes `GET  $apiBase/:id/validate` and
/// `POST $apiBase/:id/download` (tts, whisper, local-models). The Check step
/// asks the daemon to inspect the repo's config.json + file tree against the
/// actual native loader BEFORE anything heavy is fetched; Download is only
/// offered when the model is supported (or when the check was inconclusive —
/// e.g. HF unreachable — in which case it's explicitly a "try anyway").
class _HfAddModelCard extends ConsumerStatefulWidget {
  const _HfAddModelCard({
    super.key,
    required this.apiBase,
    required this.onDownloaded,
  });
  final String apiBase;
  final VoidCallback onDownloaded;
  @override
  ConsumerState<_HfAddModelCard> createState() => _HfAddModelCardState();
}

class _HfAddModelCardState extends ConsumerState<_HfAddModelCard> {
  final _input = TextEditingController();
  bool _busy = false;
  Map<String, dynamic>? _report; // last validate result
  String? _error;

  @override
  void dispose() {
    _input.dispose();
    super.dispose();
  }

  String get _enc => Uri.encodeComponent(_input.text.trim());

  Future<void> _check() async {
    setState(() {
      _busy = true;
      _report = null;
      _error = null;
    });
    try {
      final r = await ref
          .read(apiClientProvider)
          .get('${widget.apiBase}/$_enc/validate');
      if (mounted && r is Map) {
        setState(() => _report = r.cast<String, dynamic>());
      }
    } catch (e) {
      if (mounted) {
        setState(
            () => _error = context.trArgs('Check failed: {e}', {'e': e}));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _download() async {
    setState(() => _busy = true);
    try {
      await ref
          .read(apiClientProvider)
          .post('${widget.apiBase}/$_enc/download');
      widget.onDownloaded();
      if (mounted) {
        setState(() {
          _report = null;
          _input.clear();
        });
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.tr(
                'Download started — progress shows in the list below'))));
      }
    } catch (e) {
      if (mounted) {
        setState(
            () => _error = context.trArgs('Download failed: {e}', {'e': e}));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final rep = _report;
    final supported = rep?['supported'] == true;
    final inconclusive = rep?['inconclusive'] == true;
    final sizeGb = ((rep?['total_size_bytes'] as num?) ?? 0) / (1 << 30);
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
          Text(context.tr('Add model from Hugging Face'),
              style:
                  TextStyle(color: c.textPrimary, fontWeight: FontWeight.w700)),
          const SizedBox(height: AppTokens.s8),
          Row(children: [
            Expanded(
              child: TextField(
                controller: _input,
                onSubmitted: (_) => _check(),
                onChanged: (_) {
                  // A new id invalidates the previous verdict.
                  if (_report != null || _error != null) {
                    setState(() {
                      _report = null;
                      _error = null;
                    });
                  }
                },
                decoration: InputDecoration(
                    isDense: true,
                    hintText: context.tr(
                        'org/repo or URL (e.g. facebook/mms-tts-vie)')),
              ),
            ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              onPressed: _busy || _input.text.trim().isEmpty ? null : _check,
              icon: _busy
                  ? const SizedBox(
                      width: 14,
                      height: 14,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.rule, size: 16),
              label: Text(context.tr('Check')),
            ),
          ]),
          if (_error != null) ...[
            const SizedBox(height: AppTokens.s8),
            Text(_error!,
                style: const TextStyle(color: AppTokens.danger, fontSize: 12)),
          ],
          if (rep != null) ...[
            const SizedBox(height: AppTokens.s8),
            Container(
              padding: const EdgeInsets.all(AppTokens.s8),
              decoration: BoxDecoration(
                color: (supported
                        ? AppTokens.success
                        : inconclusive
                            ? AppTokens.warning
                            : AppTokens.danger)
                    .withValues(alpha: 0.10),
                borderRadius: BorderRadius.circular(AppTokens.rSm),
              ),
              child: Row(children: [
                Icon(
                  supported
                      ? Icons.check_circle_outline
                      : inconclusive
                          ? Icons.help_outline
                          : Icons.block,
                  size: 16,
                  color: supported
                      ? AppTokens.success
                      : inconclusive
                          ? AppTokens.warning
                          : AppTokens.danger,
                ),
                const SizedBox(width: AppTokens.s8),
                Expanded(
                  child: Text(
                    [
                      '${rep['reason'] ?? ''}',
                      if ((rep['architecture'] as String?)?.isNotEmpty ??
                          false)
                        'arch: ${rep['architecture']}',
                      if (sizeGb > 0.001)
                        'size: ${sizeGb.toStringAsFixed(2)} GB',
                    ].join('  ·  '),
                    style: TextStyle(color: c.textSecondary, fontSize: 12),
                  ),
                ),
                if (supported || inconclusive) ...[
                  const SizedBox(width: AppTokens.s8),
                  FilledButton.icon(
                    onPressed: _busy ? null : _download,
                    icon: const Icon(Icons.download, size: 16),
                    label: Text(
                        context.tr(supported ? 'Download' : 'Try anyway')),
                  ),
                ],
              ]),
            ),
          ],
        ],
      ),
    );
  }
}

/// Per-domain media settings (web Whisper/Tts/OcrSettings): active model +
/// language, plus voice/speed for TTS. GET/PUT `/api/$domain/settings`.
class _MediaSettingsCard extends ConsumerStatefulWidget {
  const _MediaSettingsCard({super.key, required this.domain});
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

  static String _langLabel(String code) => switch (code) {
        'vi' => 'vi — Tiếng Việt',
        'en' => 'en — English',
        'zh' => 'zh — 中文',
        'ja' => 'ja — 日本語',
        'ko' => 'ko — 한국어',
        _ => code,
      };

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
            title: Text(dctx.tr('Transcription')),
            content: SizedBox(
              width: 480,
              child: SelectableText(text.trim().isEmpty
                  ? dctx.tr('(no speech recognized)')
                  : text),
            ),
            actions: [
              TextButton(
                  onPressed: () => Navigator.of(dctx).pop(),
                  child: Text(dctx.tr('Close'))),
            ],
          ),
        );
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(SnackBar(
              content:
                  Text(context.trArgs('Transcribe failed: {e}', {'e': e}))));
        }
      } finally {
        if (mounted) setState(() => _testing = false);
      }
      return;
    }
    if (!await _recorder.hasPermission()) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.tr('Microphone permission denied'))));
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

  @override
  void didUpdateWidget(covariant _MediaSettingsCard old) {
    super.didUpdateWidget(old);
    // Safety net if this State is ever reused for another domain (the section
    // also keys the card by domain): reload that domain's settings.
    if (old.domain != widget.domain) {
      _loaded = false;
      _modelId = null;
      _flash = null;
      _load();
    }
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
      req.headers.addAll(cfg.authHeaders);
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
          : context.trArgs('OCR failed: {e}', {'e': body});
      showDialog(
        context: context,
        builder: (dctx) => AlertDialog(
          backgroundColor: dctx.colors.surface,
          title: Text(dctx.trArgs('OCR result — {name}', {'name': f.name})),
          content: SizedBox(
            width: 520,
            child: SingleChildScrollView(
              child: SelectableText(
                  text.trim().isEmpty
                      ? dctx.tr('(no text recognized)')
                      : text,
                  style: const TextStyle(fontSize: 13, height: 1.4)),
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.of(dctx).pop(),
                child: Text(dctx.tr('Close'))),
          ],
        ),
      );
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('OCR failed: {e}', {'e': e}))));
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
        headers: {'Content-Type': 'application/json', ...cfg.authHeaders},
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
        // Never hide a model swap: the daemon reports transparent fallback
        // (e.g. model missing or a build without local-mlx-tts) via headers.
        final fallback = resp.headers['x-tts-fallback'];
        final backend = resp.headers['x-tts-backend'];
        if (mounted) {
          if (fallback != null && fallback.isNotEmpty) {
            ScaffoldMessenger.of(context).showSnackBar(SnackBar(
                duration: const Duration(seconds: 6),
                content: Text(context.trArgs(
                    'Fallback voice used: {voice}', {'voice': fallback}))));
          } else if (backend != null && backend.isNotEmpty) {
            setState(() => _flash = context
                .trArgs('Spoke via {backend}', {'backend': backend}));
          }
        }
      } else if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Test failed: {e}', {
          'e':
              '${resp.statusCode} ${utf8.decode(resp.bodyBytes, allowMalformed: true)}'
        }))));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(
            content: Text(context.trArgs('Test failed: {e}', {'e': e}))));
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
      final speedRaw = _speed.text.trim();
      final speed = speedRaw.isEmpty ? 1.0 : double.tryParse(speedRaw);
      if (speed == null || speed < 0.25 || speed > 4.0) {
        setState(() => _flash = context.tr('Speed must be 0.25–4.0'));
        return;
      }
      body['voice'] = _voice.text.trim();
      body['speed'] = speed;
    }
    try {
      await ref
          .read(apiClientProvider)
          .put('/api/${widget.domain}/settings', body: body);
      setState(() => _flash = context.tr('Saved'));
    } catch (e) {
      setState(() => _flash = context.tr('Failed'));
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
          Text(context.tr('Settings'),
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
              decoration: InputDecoration(
                  labelText: context.tr('Active model'), isDense: true),
              items: [
                DropdownMenuItem(
                    value: null, child: Text(context.tr('(default)'))),
                if (extra != null)
                  DropdownMenuItem(
                      value: extra,
                      child: Text(
                          context.trArgs('{id} (current)', {'id': extra}),
                          maxLines: 1, overflow: TextOverflow.ellipsis)),
                for (final m in models)
                  DropdownMenuItem(
                      value: m.id,
                      child: Text(m.label,
                          maxLines: 1, overflow: TextOverflow.ellipsis)),
              ],
              onChanged: (v) {
                setState(() {
                  _modelId = v;
                  final m = models.where((m) => m.id == v).firstOrNull;
                  // Snap language to what the newly selected model supports.
                  if (m != null && m.languages.isNotEmpty) {
                    final cur = _language.text.trim().toLowerCase();
                    if (!m.languages.contains(cur)) {
                      _language.text = m.defaultLanguage ?? m.languages.first;
                    }
                  }
                  // Snap voice to the model's preset list (VieNeu/macOS).
                  if (m != null && m.voices.isNotEmpty) {
                    final names = m.voices.map((v) => v['name']).toSet();
                    if (!names.contains(_voice.text.trim())) {
                      _voice.text =
                          m.defaultVoice ?? m.voices.first['name'] ?? '';
                    }
                  }
                });
              },
            );
          }),
          const SizedBox(height: AppTokens.s8),
          Builder(builder: (_) {
            final selected =
                models.where((m) => m.id == _modelId).firstOrNull;
            // Language choices: the selected model's list, else the union
            // across the catalog. Empty (e.g. whisper accepts any ISO code)
            // → keep the free-text field.
            final langs = selected != null && selected.languages.isNotEmpty
                ? List<String>.from(selected.languages)
                : models.expand((m) => m.languages).toSet().toList()
              ..sort();
            final curLang = _language.text.trim().toLowerCase();
            if (curLang.isNotEmpty && !langs.contains(curLang)) {
              langs.insert(0, curLang);
            }
            // Voice applies when the model exposes preset voices (VieNeu's 14
            // named speakers, macOS system voices) — single-speaker MMS models
            // ignore it, so hide the field there.
            final voiceOptions = selected?.voices ?? const [];
            final voiceApplies = _isTts &&
                (voiceOptions.isNotEmpty ||
                    _modelId == null ||
                    _modelId!.startsWith('macos-speech'));
            return Row(
              children: [
                Expanded(
                  child: langs.isEmpty
                      ? TextField(
                          controller: _language,
                          decoration: InputDecoration(
                              labelText: context.tr('Language'),
                              isDense: true,
                              hintText: 'vi'),
                        )
                      : DropdownButtonFormField<String>(
                          key: ValueKey(
                              '${widget.domain}-lang-$curLang-${langs.join(',')}'),
                          initialValue: curLang.isEmpty ? null : curLang,
                          isExpanded: true,
                          decoration: InputDecoration(
                              labelText: context.tr('Language'),
                              isDense: true),
                          items: [
                            for (final l in langs)
                              DropdownMenuItem(
                                  value: l, child: Text(_langLabel(l))),
                          ],
                          onChanged: (v) =>
                              setState(() => _language.text = v ?? ''),
                        ),
                ),
                if (_isTts && voiceApplies) ...[
                  const SizedBox(width: AppTokens.s8),
                  Expanded(
                    child: voiceOptions.isEmpty
                        ? TextField(
                            controller: _voice,
                            decoration: InputDecoration(
                                labelText: context.tr('Voice'),
                                isDense: true,
                                hintText: 'Linh / Samantha…'),
                          )
                        : Builder(builder: (_) {
                            final names = voiceOptions
                                .map((v) => v['name'] ?? '')
                                .where((n) => n.isNotEmpty)
                                .toList();
                            var cur = _voice.text.trim();
                            if (!names.contains(cur)) {
                              cur = selected?.defaultVoice ?? names.first;
                            }
                            return DropdownButtonFormField<String>(
                              key: ValueKey(
                                  '${widget.domain}-voice-$cur-${names.length}'),
                              initialValue: cur,
                              isExpanded: true,
                              decoration: InputDecoration(
                                  labelText: context.tr('Voice'),
                                  isDense: true),
                              items: [
                                for (final v in voiceOptions)
                                  DropdownMenuItem(
                                    value: v['name'],
                                    child: Text(
                                      (v['description'] ?? '').isEmpty
                                          ? (v['name'] ?? '')
                                          : '${v['name']} — ${v['description']}',
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                    ),
                                  ),
                              ],
                              onChanged: (v) =>
                                  setState(() => _voice.text = v ?? ''),
                            );
                          }),
                  ),
                ],
                if (_isTts) ...[
                  const SizedBox(width: AppTokens.s8),
                  SizedBox(
                    width: 96,
                    child: TextField(
                      controller: _speed,
                      keyboardType: TextInputType.number,
                      decoration: InputDecoration(
                          labelText: context.tr('Speed'),
                          isDense: true,
                          hintText: '1.0'),
                    ),
                  ),
                ],
              ],
            );
          }),
          const SizedBox(height: AppTokens.s8),
          Row(
            children: [
              FilledButton(
                  onPressed: _save, child: Text(context.tr('Save'))),
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
                  label: Text(context.tr('Test voice')),
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
                  label: Text(context.tr('Test (pick image)')),
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
                  label: Text(context.tr(_recording
                      ? 'Stop & transcribe'
                      : 'Record & transcribe')),
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
