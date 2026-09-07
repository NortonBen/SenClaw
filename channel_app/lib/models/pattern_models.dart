/// Zen Patterns — named system prompts for one text transform.
///
/// A pattern is text in, text out: **one** LLM call, no tools, no loop. The
/// on-disk format is Fabric's, so the daemon's ~250-pattern bundled library
/// imports with no converter. See `docs/zen-patterns.md`.
///
/// The daemon serialises these camelCase (`#[serde(rename_all = "camelCase")]`
/// on `PatternEntry` / `PatternSource` / `CatalogEntry`), unlike the snake_case
/// `/api/background/*` surface.
library;

/// One row of `GET /api/patterns` — enough to pick from, no bodies loaded.
class PatternEntry {
  final String name;

  /// Id of the source this name actually resolves to.
  final String source;

  /// First prose line of `system.md`, capped by the daemon at 240 bytes.
  final String description;

  /// Other sources carrying the same name that are being shadowed. Surfaced so
  /// "I edited it and nothing changed" has a visible cause.
  final List<String> shadowedIn;

  /// False for a git checkout — editing there is reverted by the next sync.
  final bool writable;

  const PatternEntry({
    required this.name,
    required this.source,
    this.description = '',
    this.shadowedIn = const [],
    this.writable = false,
  });

  factory PatternEntry.fromJson(Map<String, dynamic> j) => PatternEntry(
    name: (j['name'] ?? '').toString(),
    source: (j['source'] ?? '').toString(),
    description: (j['description'] ?? '').toString(),
    shadowedIn: (j['shadowedIn'] as List?)
            ?.map((e) => e.toString())
            .toList() ??
        const [],
    writable: j['writable'] == true,
  );
}

/// One place patterns are read from. `user` resolves first, then git checkouts.
class PatternSource {
  final String id;
  final String name;

  /// `local` | `git`.
  final String kind;
  final String? url;

  /// Branch or tag. A source that matters should pin a tag: a pattern lands in
  /// the **system-prompt** position, so following a moving branch lets an
  /// upstream commit rewrite instructions the agent obeys.
  final String gitRef;
  final String subdir;
  final String? strategiesSubdir;

  /// A disabled source keeps its files but contributes nothing to the registry.
  final bool enabled;

  /// `kit:<id>` when a Zen Kit added it; null = added by the user.
  final String? installedBy;
  final String? lastSyncedAt;

  /// Why the last sync failed, cleared on the next success — so a stale source
  /// reads as stale instead of silently empty.
  final String? lastError;

  /// Decorations `patterns_list` adds on top of the stored source.
  final int count;
  final bool writable;

  const PatternSource({
    required this.id,
    this.name = '',
    this.kind = 'local',
    this.url,
    this.gitRef = 'main',
    this.subdir = '',
    this.strategiesSubdir,
    this.enabled = true,
    this.installedBy,
    this.lastSyncedAt,
    this.lastError,
    this.count = 0,
    this.writable = false,
  });

  bool get isGit => kind == 'git';

  /// A tag or a full sha is pinned; a branch name is not. Mirrors the daemon's
  /// `SourceSyncOutcome::pinned` intent — the UI says so because an unpinned
  /// source can silently rewrite a system prompt.
  bool get pinned {
    if (!isGit) return true;
    final r = gitRef.trim();
    if (r.isEmpty) return false;
    if (RegExp(r'^[0-9a-f]{7,40}$').hasMatch(r)) return true;
    return !const {'main', 'master', 'HEAD', 'dev', 'develop'}.contains(r);
  }

  factory PatternSource.fromJson(Map<String, dynamic> j) => PatternSource(
    id: (j['id'] ?? '').toString(),
    name: (j['name'] ?? '').toString(),
    kind: (j['kind'] ?? 'local').toString(),
    url: j['url']?.toString(),
    gitRef: (j['gitRef'] ?? j['ref'] ?? 'main').toString(),
    subdir: (j['subdir'] ?? '').toString(),
    strategiesSubdir: j['strategiesSubdir']?.toString(),
    enabled: j['enabled'] != false,
    installedBy: j['installedBy']?.toString(),
    lastSyncedAt: j['lastSyncedAt']?.toString(),
    lastError: j['lastError']?.toString(),
    count: (j['count'] as num?)?.toInt() ?? 0,
    writable: j['writable'] == true,
  );
}

/// A reasoning strategy (cot / tot / reflexion …) appended to the system prompt.
class PatternStrategy {
  final String name;
  final String description;
  final String prompt;

  const PatternStrategy({
    required this.name,
    this.description = '',
    this.prompt = '',
  });

  factory PatternStrategy.fromJson(Map<String, dynamic> j) => PatternStrategy(
    name: (j['name'] ?? '').toString(),
    description: (j['description'] ?? '').toString(),
    prompt: (j['prompt'] ?? '').toString(),
  );
}

/// `GET /api/patterns` — the list travels with its sources and strategies
/// because the UI groups by source; a second round-trip is not worth it.
class PatternCatalogPage {
  final List<PatternEntry> patterns;
  final List<PatternSource> sources;
  final List<PatternStrategy> strategies;

  const PatternCatalogPage({
    this.patterns = const [],
    this.sources = const [],
    this.strategies = const [],
  });
}

/// The full body of one pattern (`GET /api/patterns/:name`).
class PatternFiles {
  final String name;
  final String source;
  final String system;

  /// `user.md` — an optional user-message template some Fabric patterns ship.
  /// When absent the caller's input becomes the user message directly.
  final String? user;
  final String path;
  final bool writable;

  const PatternFiles({
    required this.name,
    required this.source,
    required this.system,
    this.user,
    this.path = '',
    this.writable = false,
  });

  factory PatternFiles.fromJson(Map<String, dynamic> j) => PatternFiles(
    name: (j['name'] ?? '').toString(),
    source: (j['source'] ?? '').toString(),
    system: (j['system'] ?? '').toString(),
    user: j['user']?.toString(),
    path: (j['path'] ?? '').toString(),
    writable: j['writable'] == true,
  );
}

/// Result of `POST /api/patterns/run`.
class PatternRunResult {
  final String pattern;
  final String source;
  final String text;
  final String model;
  final String finish;
  final int latencyMs;

  /// `{{placeholder}}` names the render could not fill. They stay **verbatim**
  /// in the prompt (blanking one silently deletes an instruction), so this is
  /// the only signal that a pattern misbehaved for a fillable reason.
  final List<String> unresolved;

  /// True when the server only rendered the prompt and made no LLM call.
  final bool dryRun;
  final String renderedSystem;
  final String renderedUser;

  const PatternRunResult({
    required this.pattern,
    this.source = '',
    this.text = '',
    this.model = '',
    this.finish = '',
    this.latencyMs = 0,
    this.unresolved = const [],
    this.dryRun = false,
    this.renderedSystem = '',
    this.renderedUser = '',
  });

  factory PatternRunResult.fromJson(Map<String, dynamic> j) {
    final rendered = j['rendered'];
    final r = rendered is Map ? rendered.cast<String, dynamic>() : const {};
    return PatternRunResult(
      pattern: (j['pattern'] ?? '').toString(),
      source: (j['source'] ?? '').toString(),
      text: (j['text'] ?? '').toString(),
      model: (j['model'] ?? '').toString(),
      finish: (j['finish'] ?? '').toString(),
      latencyMs: (j['latencyMs'] as num?)?.toInt() ?? 0,
      unresolved:
          (j['unresolved'] as List?)?.map((e) => e.toString()).toList() ??
              const [],
      dryRun: j['dryRun'] == true,
      renderedSystem: (r['system'] ?? '').toString(),
      renderedUser: (r['user'] ?? '').toString(),
    );
  }
}

/// One offer from `GET /api/patterns/catalog` — what the "add a source" screen
/// can propose before the user types anything.
class PatternCatalogEntry {
  final String id;
  final String name;
  final String description;

  /// `bundled` installs offline; `git` clones.
  final String kind;
  final int count;
  final String license;
  final String? url;
  final String? gitRef;
  final String? subdir;
  final String? strategiesSubdir;

  /// Already in this daemon's ledger — the card says "installed" instead of
  /// offering the same thing twice.
  final bool installed;

  /// False when the ref is a moving branch.
  final bool pinned;

  const PatternCatalogEntry({
    required this.id,
    required this.name,
    this.description = '',
    this.kind = 'git',
    this.count = 0,
    this.license = '',
    this.url,
    this.gitRef,
    this.subdir,
    this.strategiesSubdir,
    this.installed = false,
    this.pinned = false,
  });

  factory PatternCatalogEntry.fromJson(Map<String, dynamic> j) =>
      PatternCatalogEntry(
        id: (j['id'] ?? '').toString(),
        name: (j['name'] ?? '').toString(),
        description: (j['description'] ?? '').toString(),
        kind: (j['kind'] ?? 'git').toString(),
        count: (j['count'] as num?)?.toInt() ?? 0,
        license: (j['license'] ?? '').toString(),
        url: j['url']?.toString(),
        gitRef: j['gitRef']?.toString(),
        subdir: j['subdir']?.toString(),
        strategiesSubdir: j['strategiesSubdir']?.toString(),
        installed: j['installed'] == true,
        pinned: j['pinned'] == true,
      );
}
