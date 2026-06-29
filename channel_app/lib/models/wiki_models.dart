/// Models mirroring the web Wiki feature (`/api/wiki/*`). All timestamps are
/// ISO-8601 strings; JSON is camelCase.
library;

class WikiFrontmatter {
  final String? created;
  final String? updated;
  final List<String> tags;
  final String? source;

  WikiFrontmatter({this.created, this.updated, this.tags = const [], this.source});

  factory WikiFrontmatter.fromJson(Map<String, dynamic> j) => WikiFrontmatter(
        created: j['created'] as String?,
        updated: j['updated'] as String?,
        tags: (j['tags'] as List?)?.map((e) => e.toString()).toList() ?? const [],
        source: j['source'] as String?,
      );
}

class WikiDirNode {
  final String name;
  final String path;
  final String type; // 'dir' | 'file'
  final List<WikiDirNode> children;
  final WikiFrontmatter? frontmatter;

  WikiDirNode({
    required this.name,
    required this.path,
    required this.type,
    this.children = const [],
    this.frontmatter,
  });

  bool get isDir => type == 'dir';

  factory WikiDirNode.fromJson(Map<String, dynamic> j) => WikiDirNode(
        name: j['name'] as String? ?? '',
        path: j['path'] as String? ?? '',
        type: j['type'] as String? ?? 'file',
        children: (j['children'] as List?)
                ?.map((e) => WikiDirNode.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
        frontmatter: j['frontmatter'] is Map
            ? WikiFrontmatter.fromJson(
                (j['frontmatter'] as Map).cast<String, dynamic>())
            : null,
      );
}

class WikiCommit {
  final String hash;
  final String date;
  final String message;

  WikiCommit({required this.hash, required this.date, required this.message});

  factory WikiCommit.fromJson(Map<String, dynamic> j) => WikiCommit(
        hash: j['hash'] as String? ?? '',
        date: j['date'] as String? ?? '',
        message: j['message'] as String? ?? '',
      );
}

class WikiDoc {
  final String path;
  final String content;
  final WikiFrontmatter frontmatter;
  final List<WikiCommit> gitLog;

  WikiDoc({
    required this.path,
    required this.content,
    required this.frontmatter,
    this.gitLog = const [],
  });

  factory WikiDoc.fromJson(Map<String, dynamic> j) => WikiDoc(
        path: j['path'] as String? ?? '',
        content: j['content'] as String? ?? '',
        frontmatter: j['frontmatter'] is Map
            ? WikiFrontmatter.fromJson(
                (j['frontmatter'] as Map).cast<String, dynamic>())
            : WikiFrontmatter(),
        gitLog: (j['gitLog'] as List?)
                ?.map((e) => WikiCommit.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
      );
}

class WikiSearchResult {
  final String path;
  final String title;
  final List<String> tags;
  final String? updated;
  final String? snippet;

  WikiSearchResult({
    required this.path,
    required this.title,
    this.tags = const [],
    this.updated,
    this.snippet,
  });

  factory WikiSearchResult.fromJson(Map<String, dynamic> j) => WikiSearchResult(
        path: j['path'] as String? ?? '',
        title: j['title'] as String? ?? '',
        tags: (j['tags'] as List?)?.map((e) => e.toString()).toList() ?? const [],
        updated: j['updated'] as String?,
        snippet: j['snippet'] as String?,
      );
}

class WikiCategory {
  final String dir;
  final int count;
  final String? lastUpdated;
  WikiCategory({required this.dir, required this.count, this.lastUpdated});
  factory WikiCategory.fromJson(Map<String, dynamic> j) => WikiCategory(
        dir: j['dir'] as String? ?? '',
        count: (j['count'] as num?)?.toInt() ?? 0,
        lastUpdated: j['lastUpdated'] as String?,
      );
}

class WikiTag {
  final String name;
  final int count;
  WikiTag({required this.name, required this.count});
  factory WikiTag.fromJson(Map<String, dynamic> j) => WikiTag(
        name: (j['name'] ?? j['tag'])?.toString() ?? '',
        count: (j['count'] as num?)?.toInt() ?? 0,
      );
}

class WikiRecentFile {
  final String path;
  final String title;
  final String? updated;
  WikiRecentFile({required this.path, required this.title, this.updated});
  factory WikiRecentFile.fromJson(Map<String, dynamic> j) => WikiRecentFile(
        path: j['path'] as String? ?? '',
        title: j['title'] as String? ?? '',
        updated: j['updated'] as String?,
      );
}

class WikiStats {
  final int totalFiles;
  final int totalDirs;
  final List<WikiCategory> byCategory;
  final List<WikiTag> byTag;
  final List<WikiRecentFile> recentFiles;

  WikiStats({
    required this.totalFiles,
    required this.totalDirs,
    this.byCategory = const [],
    this.byTag = const [],
    this.recentFiles = const [],
  });

  factory WikiStats.fromJson(Map<String, dynamic> j) => WikiStats(
        totalFiles: (j['totalFiles'] as num?)?.toInt() ?? 0,
        totalDirs: (j['totalDirs'] as num?)?.toInt() ?? 0,
        byCategory: (j['byCategory'] as List?)
                ?.map((e) => WikiCategory.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
        byTag: (j['byTag'] as List?)
                ?.map((e) => WikiTag.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
        recentFiles: (j['recentFiles'] as List?)
                ?.map((e) => WikiRecentFile.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
      );
}
