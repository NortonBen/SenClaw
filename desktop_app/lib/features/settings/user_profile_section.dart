import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/i18n/l10n.dart';
import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'settings_providers.dart';
import 'settings_screen.dart';

/// Soul Core — who the *human* is.
///
/// Deliberately separate from Settings → Profiles, which edits each agent's
/// `SOUL.md` (who the *agent* is). These three files are global: one answer to
/// "who is my owner" shared by every agent profile, rather than the same
/// details typed once per folder.
///
/// The public/private tier is the part users get wrong, so the section leads
/// with what it means and ends with two side-by-side previews of the actual
/// rendered block. Without those, "private" is a promise the user cannot check.
class UserProfileSection extends ConsumerStatefulWidget {
  const UserProfileSection({super.key});

  @override
  ConsumerState<UserProfileSection> createState() => _UserProfileSectionState();
}

/// Fields the form always offers, in the order they read as a sentence about a
/// person. Keys mirror `DEFAULT_PUBLIC_FIELDS` / the label map in
/// `src/user_profile/`.
const _knownFields = <(String, String, String)>[
  ('name', 'Full name', 'Nguyễn Văn A'),
  ('preferred_name', 'What to call you', 'anh A'),
  ('pronouns', 'Pronouns', ''),
  ('language', 'Language', 'vi'),
  ('timezone', 'Timezone', 'Asia/Ho_Chi_Minh'),
  ('occupation', 'Occupation', ''),
  ('email', 'Email', 'a@example.com'),
  ('location', 'Location', 'Hà Nội'),
  ('phone', 'Phone', ''),
];

const _defaultPublic = {
  'name',
  'preferred_name',
  'pronouns',
  'language',
  'timezone',
  'occupation',
};

class _UserProfileSectionState extends ConsumerState<UserProfileSection> {
  final _controllers = <String, TextEditingController>{};
  final _tiers = <String, String>{};
  final _notes = TextEditingController();
  final _tools = TextEditingController();
  final _rules = TextEditingController();
  String? _previewFull;
  String? _previewPublic;
  List<Map<String, dynamic>> _directives = const [];
  bool _loaded = false;
  bool _saving = false;

  @override
  void dispose() {
    for (final c in _controllers.values) {
      c.dispose();
    }
    _notes.dispose();
    _tools.dispose();
    _rules.dispose();
    super.dispose();
  }

  /// Seed the form from server state.
  ///
  /// Guarded by `_loaded` so it runs once per load rather than on every
  /// provider tick — re-seeding mid-typing would fight the user's cursor. The
  /// reload button clears the flag, which is what makes a refresh actually
  /// show new values (the agent writes this file too, via `profile_update`).
  void _hydrate(Map<String, dynamic> profile, String tools, String rules) {
    if (_loaded) return;
    _loaded = true;
    final fields = (profile['fields'] as List? ?? const [])
        .whereType<Map>()
        .map((m) => m.cast<String, dynamic>())
        .toList();

    // Reuse controllers across reloads. Replacing them would leak the old ones
    // and detach the widgets still holding a reference.
    void seed(String key, String value, String? tier, String fallbackTier) {
      (_controllers[key] ??= TextEditingController()).text = value;
      _tiers[key] = tier ?? fallbackTier;
    }

    for (final (key, _, _) in _knownFields) {
      final f = fields.where((f) => f['key'] == key).firstOrNull;
      seed(
        key,
        f?['value'] as String? ?? '',
        f?['tier'] as String?,
        _defaultPublic.contains(key) ? 'public' : 'private',
      );
    }
    // Fields added by hand or written by the agent. Shown rather than dropped:
    // a save that silently deletes what it did not recognise is worse than an
    // unfamiliar row.
    for (final f in fields) {
      final key = f['key'] as String? ?? '';
      if (key.isEmpty || _knownFields.any((k) => k.$1 == key)) continue;
      seed(key, f['value'] as String? ?? '', f['tier'] as String?, 'private');
    }
    _notes.text = profile['notes'] as String? ?? '';
    _tools.text = tools;
    _rules.text = rules;
    _previewFull = profile['preview_full'] as String?;
    _previewPublic = profile['preview_public'] as String?;
    _directives = (profile['directives'] as List? ?? const [])
        .whereType<Map>()
        .map((m) => m.cast<String, dynamic>())
        .toList();
  }

  /// Re-fetch all three files and re-seed the form.
  ///
  /// Worth a button because this screen is not the only writer: the agent
  /// edits the same profile through `profile_update` during a chat, and a
  /// stale form would overwrite that on the next save.
  void _reload() {
    _loaded = false;
    ref.invalidate(userProfileProvider);
    ref.invalidate(toolsNotesProvider);
    ref.invalidate(agentsRulesProvider);
  }

  Future<void> _saveProfile() async {
    final okMsg = context.tr('Saved. Agents use it from the next chat session.');
    setState(() => _saving = true);
    try {
      final r = await ref.read(apiClientProvider).put(
        '/api/user-profile',
        body: {
          'fields': [
            for (final e in _controllers.entries)
              {
                'key': e.key,
                'value': e.value.text,
                'tier': _tiers[e.key] ?? 'private',
              },
          ],
          'notes': _notes.text,
        },
      );
      final m = r is Map ? r.cast<String, dynamic>() : <String, dynamic>{};
      setState(() {
        _previewFull = m['preview_full'] as String?;
        _previewPublic = m['preview_public'] as String?;
      });
      ref.invalidate(userProfileProvider);
      _toast(okMsg);
    } catch (e) {
      _toast('$e', error: true);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _saveFlat(String path, String content, ProviderOrFamily p) async {
    final okMsg = context.tr('Saved');
    setState(() => _saving = true);
    try {
      await ref.read(apiClientProvider).put(path, body: {'content': content});
      ref.invalidate(p);
      _toast(okMsg);
    } catch (e) {
      _toast('$e', error: true);
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  void _toast(String msg, {bool error = false}) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(
      content: Text(msg),
      backgroundColor: error ? AppTokens.danger : null,
    ));
  }

  @override
  Widget build(BuildContext context) {
    final profile = ref.watch(userProfileProvider);
    final tools = ref.watch(toolsNotesProvider);
    final rules = ref.watch(agentsRulesProvider);

    if (profile.isLoading || tools.isLoading || rules.isLoading) {
      return SettingsBody(
        title: context.tr('Your profile'),
        onRefresh: _reload,
        children: const [LinearProgressIndicator()],
      );
    }
    _hydrate(
      profile.valueOrNull ?? const {},
      (tools.valueOrNull ?? const {})['content'] as String? ?? '',
      (rules.valueOrNull ?? const {})['content'] as String? ?? '',
    );

    return SettingsBody(
      title: context.tr('Your profile'),
      onRefresh: _reload,
      children: [
        Text(
          context.tr(
              'What agents know about YOU — as opposed to a Profile\'s persona, '
              'which is who the agent is. Shared by every agent profile.'),
          style: TextStyle(color: context.colors.textSecondary),
        ),
        const SizedBox(height: AppTokens.s16),
        _notice(context),
        const SizedBox(height: AppTokens.s24),
        _heading(context, context.tr('Details')),
        for (final (key, label, hint) in _knownFields) _fieldRow(key, label, hint),
        for (final key in _controllers.keys
            .where((k) => !_knownFields.any((f) => f.$1 == k)))
          _fieldRow(key, key, ''),
        const SizedBox(height: AppTokens.s16),
        _heading(context, context.tr('Extra notes')),
        _multiline(_notes, 3),
        const SizedBox(height: AppTokens.s24),
        _heading(context, context.tr('Rules the agent learned')),
        _directivesList(context),
        const SizedBox(height: AppTokens.s24),
        _heading(context, context.tr('What the agent actually receives')),
        _preview(context, context.tr('Private conversation'), _previewFull),
        const SizedBox(height: AppTokens.s8),
        _preview(context, context.tr('Group chat'), _previewPublic),
        const SizedBox(height: AppTokens.s16),
        FilledButton(
          onPressed: _saving ? null : _saveProfile,
          child: Text(context.tr('Save profile')),
        ),
        const SizedBox(height: AppTokens.s32),
        _heading(context, context.tr('Local environment notes (TOOLS.md)')),
        Text(
          context.tr(
              'SSH hosts, device names, preferred TTS voices — kept out of skills '
              'so skills stay shareable. Private conversations only.'),
          style: TextStyle(
              color: context.colors.textSecondary, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s8),
        _multiline(_tools, 8, mono: true),
        const SizedBox(height: AppTokens.s8),
        FilledButton.tonal(
          onPressed: _saving
              ? null
              : () => _saveFlat('/api/tools-notes', _tools.text, toolsNotesProvider),
          child: Text(context.tr('Save')),
        ),
        const SizedBox(height: AppTokens.s32),
        _heading(context, context.tr('Operating rules (AGENTS.md)')),
        Text(
          context.tr(
              'Rules applied in every session, appended to the system prompt. '
              'SenClaw\'s built-in safety section always wins over anything here.'),
          style: TextStyle(
              color: context.colors.textSecondary, fontSize: 12),
        ),
        const SizedBox(height: AppTokens.s8),
        _multiline(_rules, 8, mono: true),
        const SizedBox(height: AppTokens.s8),
        FilledButton.tonal(
          onPressed: _saving
              ? null
              : () =>
                  _saveFlat('/api/agents-rules', _rules.text, agentsRulesProvider),
          child: Text(context.tr('Save')),
        ),
      ],
    );
  }

  Widget _heading(BuildContext context, String text) => Padding(
        padding: const EdgeInsets.only(bottom: AppTokens.s8),
        child: Text(text,
            style: TextStyle(
                color: context.colors.textSecondary,
                fontWeight: FontWeight.w700)),
      );

  Widget _notice(BuildContext context) => Container(
        padding: const EdgeInsets.all(AppTokens.s12),
        decoration: BoxDecoration(
          color: context.colors.surfaceAlt,
          borderRadius: BorderRadius.circular(AppTokens.rLg),
          border: Border.all(color: context.colors.border),
        ),
        child: Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Icon(Icons.info_outline, size: 18, color: context.colors.textSecondary),
          const SizedBox(width: AppTokens.s8),
          Expanded(
            child: Text(
              context.tr(
                  'Public fields go everywhere, including Telegram/Feishu group '
                  'chats. Private fields appear only in your own 1-1 conversations. '
                  'Email, address and phone default to private.'),
              style: TextStyle(color: context.colors.textSecondary, fontSize: 12),
            ),
          ),
        ]),
      );

  Widget _fieldRow(String key, String label, String hint) => Padding(
        padding: const EdgeInsets.only(bottom: AppTokens.s8),
        child: Row(children: [
          SizedBox(
            width: 150,
            child: Text(context.tr(label),
                style: TextStyle(color: context.colors.textSecondary)),
          ),
          Expanded(
            child: TextField(
              controller: _controllers[key],
              decoration: InputDecoration(
                hintText: hint,
                isDense: true,
                border: const OutlineInputBorder(),
              ),
            ),
          ),
          const SizedBox(width: AppTokens.s8),
          // A plain controlled DropdownButton, not DropdownButtonFormField:
          // the FormField variant takes its selection from `initialValue` and
          // then owns it, so a reload that changed the tier on the server
          // would leave the old choice on screen — and the next save would
          // write that stale choice back, silently re-publishing a field the
          // user had made private.
          SizedBox(
            width: 150,
            child: InputDecorator(
              decoration: const InputDecoration(
                  isDense: true, border: OutlineInputBorder()),
              child: DropdownButtonHideUnderline(
                child: DropdownButton<String>(
                  value: _tiers[key] ?? 'private',
                  isDense: true,
                  isExpanded: true,
                  items: [
                    DropdownMenuItem(
                        value: 'public', child: Text(context.tr('Public'))),
                    DropdownMenuItem(
                        value: 'private', child: Text(context.tr('Private'))),
                  ],
                  onChanged: (v) => setState(() => _tiers[key] = v ?? 'private'),
                ),
              ),
            ),
          ),
        ]),
      );

  Widget _multiline(TextEditingController c, int lines, {bool mono = false}) =>
      TextField(
        controller: c,
        maxLines: lines,
        style: mono ? const TextStyle(fontFamily: 'monospace', fontSize: 13) : null,
        decoration: const InputDecoration(border: OutlineInputBorder()),
      );

  Widget _directivesList(BuildContext context) {
    if (_directives.isEmpty) {
      return Text(
        context.tr(
            'None yet. Tell the agent "from now on, keep replies short" and it '
            'records the rule here.'),
        style: TextStyle(color: context.colors.textSecondary, fontSize: 12),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (final d in _directives)
          Padding(
            padding: const EdgeInsets.only(bottom: AppTokens.s4),
            child: Row(children: [
              Icon(
                d['status'] == 'active'
                    ? Icons.check_circle_outline
                    : Icons.history,
                size: 16,
                color: d['status'] == 'active'
                    ? AppTokens.success
                    : context.colors.textSecondary,
              ),
              const SizedBox(width: AppTokens.s8),
              Expanded(
                child: Text(
                  '${d['text']}',
                  style: TextStyle(
                    color: d['status'] == 'active'
                        ? context.colors.textPrimary
                        : context.colors.textSecondary,
                    decoration: d['status'] == 'active'
                        ? null
                        : TextDecoration.lineThrough,
                  ),
                ),
              ),
              Text('${d['observed']}',
                  style: TextStyle(
                      color: context.colors.textSecondary, fontSize: 11)),
            ]),
          ),
      ],
    );
  }

  Widget _preview(BuildContext context, String label, String? body) => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(label,
              style: TextStyle(
                  color: context.colors.textPrimary,
                  fontWeight: FontWeight.w600,
                  fontSize: 12)),
          const SizedBox(height: AppTokens.s4),
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(AppTokens.s8),
            decoration: BoxDecoration(
              color: context.colors.surfaceAlt,
              borderRadius: BorderRadius.circular(AppTokens.rLg),
              border: Border.all(color: context.colors.border),
            ),
            child: SelectableText(
              body ?? context.tr('(nothing)'),
              style: const TextStyle(fontFamily: 'monospace', fontSize: 12),
            ),
          ),
        ],
      );
}
