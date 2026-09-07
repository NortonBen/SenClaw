import 'package:flutter/material.dart';
import '../../models/pattern_models.dart';
import '../../services/language_service.dart';
import '../../services/patterns_api.dart';
import '../../services/relay_manager.dart';
import '../../theme/tokens.dart';
import '../../util/format.dart';
import '../../widgets/states.dart';
import 'pattern_run_screen.dart';
import 'pattern_sources_tab.dart';

/// Zen Patterns over `/api/patterns/*`.
///
/// A pattern is a named system prompt for one text transform — text in, text
/// out, one LLM call. This screen is the picker; running one happens in
/// [PatternRunScreen].
///
/// Patterns are deliberately **not** skills: the daemon loads every skill into
/// one registry whose triggers feed the pre-turn matcher, so a few hundred
/// pattern entries would drown the real skills. The whole design is N patterns
/// behind a constant-size surface — which is why this is a list, not a menu.
class PatternsScreen extends StatefulWidget {
  const PatternsScreen({super.key});

  @override
  State<PatternsScreen> createState() => _PatternsScreenState();
}

class _PatternsScreenState extends State<PatternsScreen>
    with SingleTickerProviderStateMixin {
  final _api = PatternsApi();
  late final TabController _tabs = TabController(length: 2, vsync: this);

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
            Text(tr('Pattern', 'Patterns'),
                style: TextStyle(color: c.textPrimary)),
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
          indicatorColor: c.accent,
          labelColor: c.accent,
          unselectedLabelColor: c.textMuted,
          tabs: [
            Tab(
                icon: const Icon(Icons.auto_awesome_outlined),
                text: tr('Thư viện', 'Library')),
            Tab(
                icon: const Icon(Icons.source_outlined),
                text: tr('Nguồn', 'Sources')),
          ],
        ),
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: TabBarView(
          controller: _tabs,
          children: [
            PatternLibraryTab(api: _api),
            PatternSourcesTab(api: _api),
          ],
        ),
      ),
    );
  }
}

/// Searchable list, grouped by the source each name actually resolves to.
class PatternLibraryTab extends StatefulWidget {
  final PatternsApi api;
  const PatternLibraryTab({super.key, required this.api});

  @override
  State<PatternLibraryTab> createState() => _PatternLibraryTabState();
}

class _PatternLibraryTabState extends State<PatternLibraryTab> {
  final _search = TextEditingController();
  PatternCatalogPage? _page;
  String? _error;
  bool _loading = true;
  String? _sourceFilter;

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final page = await widget.api
          .list(query: _search.text.trim(), source: _sourceFilter);
      if (!mounted) return;
      setState(() {
        _page = page;
        _loading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _error = e.toString();
        _loading = false;
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final page = _page;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(12, 12, 12, 8),
          child: TextField(
            controller: _search,
            style: TextStyle(color: c.textPrimary, fontSize: 14),
            textInputAction: TextInputAction.search,
            onSubmitted: (_) => _load(),
            decoration: InputDecoration(
              isDense: true,
              hintText: tr('Tìm pattern…', 'Search patterns…'),
              hintStyle: TextStyle(color: c.textMuted, fontSize: 14),
              prefixIcon: Icon(Icons.search, size: 20, color: c.textMuted),
              suffixIcon: IconButton(
                icon: Icon(Icons.arrow_forward, size: 20, color: c.accent),
                onPressed: _load,
              ),
              filled: true,
              fillColor: c.surfaceAlt,
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(AppTokens.rMd),
                borderSide: BorderSide(color: c.border),
              ),
              enabledBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(AppTokens.rMd),
                borderSide: BorderSide(color: c.border),
              ),
            ),
          ),
        ),
        if (page != null && page.sources.length > 1)
          _sourceChips(c, page.sources),
        Expanded(
          child: _loading
              ? const LoadingState()
              : _error != null
                  ? ErrorState(message: _error!, onRetry: _load)
                  : page == null || page.patterns.isEmpty
                      ? EmptyState(
                          icon: Icons.auto_awesome_outlined,
                          message: tr('Chưa có pattern nào',
                              'No patterns yet'),
                          hint: tr(
                            'Mở tab Nguồn và cài "Thư viện đi kèm" — có sẵn trong bản cài, không cần mạng.',
                            'Open the Sources tab and install the bundled library — it ships with SenClaw and needs no network.',
                          ),
                        )
                      : RefreshIndicator(
                          color: c.accent,
                          onRefresh: _load,
                          child: ListView.builder(
                            padding: const EdgeInsets.fromLTRB(12, 4, 12, 24),
                            itemCount: page.patterns.length,
                            itemBuilder: (_, i) => _PatternTile(
                              entry: page.patterns[i],
                              strategies: page.strategies,
                              onChanged: _load,
                            ),
                          ),
                        ),
        ),
      ],
    );
  }

  Widget _sourceChips(AppColors c, List<PatternSource> sources) => SizedBox(
        height: 42,
        child: ListView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 12),
          children: [
            _chip(c, tr('Tất cả', 'All'), null),
            for (final s in sources)
              _chip(c, '${s.name.isEmpty ? s.id : s.name} (${s.count})', s.id),
          ],
        ),
      );

  Widget _chip(AppColors c, String label, String? id) {
    final on = _sourceFilter == id;
    return Padding(
      padding: const EdgeInsets.only(right: 8),
      child: ChoiceChip(
        label: Text(label, style: const TextStyle(fontSize: 12)),
        selected: on,
        showCheckmark: false,
        selectedColor: c.accentSoft,
        backgroundColor: c.surfaceAlt,
        side: BorderSide(color: on ? c.accent : c.border),
        labelStyle: TextStyle(color: on ? c.accent : c.textSecondary),
        onSelected: (_) {
          setState(() => _sourceFilter = on ? null : id);
          _load();
        },
      ),
    );
  }
}

class _PatternTile extends StatelessWidget {
  final PatternEntry entry;
  final List<PatternStrategy> strategies;
  final VoidCallback onChanged;

  const _PatternTile({
    required this.entry,
    required this.strategies,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      color: c.surface,
      elevation: 0,
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        side: BorderSide(color: c.border),
      ),
      child: InkWell(
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        onTap: () async {
          await Navigator.of(context).push(MaterialPageRoute(
            builder: (_) => PatternRunScreen(
              name: entry.name,
              strategies: strategies,
            ),
          ));
          onChanged();
        },
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      entry.name,
                      style: TextStyle(
                        color: c.textPrimary,
                        fontWeight: FontWeight.w600,
                        fontSize: 14,
                      ),
                    ),
                  ),
                  _tag(c, entry.source, c.textMuted),
                ],
              ),
              if (entry.description.isNotEmpty) ...[
                const SizedBox(height: 6),
                Text(
                  entry.description,
                  maxLines: 3,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: c.textSecondary, fontSize: 12.5),
                ),
              ],
              // A shadowed name is why "I edited it and nothing changed"; the
              // daemon reports it, so it is shown rather than deduped away.
              if (entry.shadowedIn.isNotEmpty) ...[
                const SizedBox(height: 6),
                Text(
                  tr(
                    'Cũng có trong: ${entry.shadowedIn.join(", ")} (bị che)',
                    'Also in: ${entry.shadowedIn.join(", ")} (shadowed)',
                  ),
                  style: const TextStyle(
                      color: AppTokens.warning, fontSize: 11.5),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  static Widget _tag(AppColors c, String text, Color fg) => Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(4),
          border: Border.all(color: c.border),
        ),
        child: Text(text, style: TextStyle(color: fg, fontSize: 11)),
      );
}

/// Shared by the sources tab — a source row's "last synced / last error" line.
String sourceSubtitle(PatternSource s) {
  if (s.lastError != null && s.lastError!.isNotEmpty) {
    return tr('Lỗi đồng bộ: ${s.lastError}', 'Sync failed: ${s.lastError}');
  }
  if (s.lastSyncedAt != null && s.lastSyncedAt!.isNotEmpty) {
    return tr(
      'Đồng bộ ${timeAgoIso(s.lastSyncedAt)}',
      'Synced ${timeAgoIso(s.lastSyncedAt)}',
    );
  }
  return s.isGit
      ? tr('Chưa đồng bộ lần nào', 'Never synced')
      : tr('Nguồn cục bộ', 'Local source');
}
