/// Models for the Plugins feature (Skills / Subagents / MCP / Marketplace).
library;

class LocalSkill {
  final String name;
  final String description;
  final String version;
  final String source; // bundled|clawhub-managed|global-*|workspace
  final String dir;
  final bool disabled;

  LocalSkill({
    required this.name,
    this.description = '',
    this.version = '',
    this.source = '',
    this.dir = '',
    this.disabled = false,
  });

  factory LocalSkill.fromJson(Map<String, dynamic> j) => LocalSkill(
        name: j['name'] as String? ?? '',
        description: j['description'] as String? ?? '',
        version: j['version'] as String? ?? '',
        source: j['source'] as String? ?? '',
        dir: j['dir'] as String? ?? '',
        disabled: j['disabled'] as bool? ?? false,
      );
}

class RemoteSkill {
  final String slug;
  final String displayName;
  final String summary;
  final String version;
  final double score;
  final bool installed;

  RemoteSkill({
    required this.slug,
    this.displayName = '',
    this.summary = '',
    this.version = '',
    this.score = 0,
    this.installed = false,
  });

  factory RemoteSkill.fromJson(Map<String, dynamic> j) => RemoteSkill(
        slug: j['slug'] as String? ?? '',
        displayName: j['displayName'] as String? ?? '',
        summary: j['summary'] as String? ?? '',
        version: j['version'] as String? ?? '',
        score: (j['score'] as num?)?.toDouble() ?? 0,
        installed: j['installed'] as bool? ?? false,
      );
}

class Subagent {
  final String name;
  final String description;
  final List<String>? tools;
  final String? model;
  final int maxConcurrent;
  final String filePath;
  final bool disabled;

  Subagent({
    required this.name,
    this.description = '',
    this.tools,
    this.model,
    this.maxConcurrent = 1,
    this.filePath = '',
    this.disabled = false,
  });

  factory Subagent.fromJson(Map<String, dynamic> j) => Subagent(
        name: j['name'] as String? ?? '',
        description: j['description'] as String? ?? '',
        tools: (j['tools'] as List?)?.map((e) => e.toString()).toList(),
        model: j['model'] as String?,
        maxConcurrent: (j['maxConcurrent'] as num?)?.toInt() ?? 1,
        filePath: j['filePath'] as String? ?? '',
        disabled: j['disabled'] as bool? ?? false,
      );
}

class McpToolDef {
  final String name;
  final String? description;
  McpToolDef({required this.name, this.description});
  factory McpToolDef.fromJson(Map<String, dynamic> j) => McpToolDef(
        name: j['name'] as String? ?? '',
        description: j['description'] as String?,
      );
}

class McpServer {
  final String name;
  final String transport; // stdio|sse|http
  final String description;
  final bool enabled;
  final String? command;
  final List<String> args;
  final String? url;
  final String scope; // user|project
  final String status; // disconnected|connecting|connected|error
  final List<McpToolDef> tools;
  final String? error;
  final bool builtin;

  McpServer({
    required this.name,
    this.transport = 'stdio',
    this.description = '',
    this.enabled = true,
    this.command,
    this.args = const [],
    this.url,
    this.scope = 'user',
    this.status = 'disconnected',
    this.tools = const [],
    this.error,
    this.builtin = false,
  });

  factory McpServer.fromJson(Map<String, dynamic> j) => McpServer(
        name: j['name'] as String? ?? '',
        transport: j['transport'] as String? ?? 'stdio',
        description: j['description'] as String? ?? '',
        enabled: j['enabled'] as bool? ?? true,
        command: j['command'] as String?,
        args: (j['args'] as List?)?.map((e) => e.toString()).toList() ?? const [],
        url: j['url'] as String?,
        scope: j['scope'] as String? ?? 'user',
        status: j['status'] as String? ?? 'disconnected',
        tools: (j['tools'] as List?)
                ?.map((e) => McpToolDef.fromJson(e as Map<String, dynamic>))
                .toList() ??
            const [],
        error: j['error'] as String?,
        builtin: j['builtin'] as bool? ?? false,
      );
}

/// An installed plugin package (`/api/plugins`). snake_case JSON.
class Plugin {
  final String slug;
  final String displayName;
  final String summary;
  final String version;
  final String pluginType;
  final bool enabled;
  final String status; // running|stopped|error
  final String? errorMsg;

  Plugin({
    required this.slug,
    this.displayName = '',
    this.summary = '',
    this.version = '',
    this.pluginType = '',
    this.enabled = true,
    this.status = 'stopped',
    this.errorMsg,
  });

  factory Plugin.fromJson(Map<String, dynamic> j) => Plugin(
        slug: j['slug'] as String? ?? '',
        displayName: j['display_name'] as String? ?? '',
        summary: j['summary'] as String? ?? '',
        version: j['version'] as String? ?? '',
        pluginType: j['plugin_type'] as String? ?? '',
        enabled: j['enabled'] as bool? ?? true,
        status: j['status'] as String? ?? 'stopped',
        errorMsg: j['error_msg'] as String?,
      );
}

class MarketplaceSource {
  final String id;
  final String name;
  final String type; // git|local
  final String? url;
  final String? branch;
  final String localPath;
  final int priority;
  final bool enabled;
  final String? lastSynced;

  MarketplaceSource({
    required this.id,
    required this.name,
    this.type = 'git',
    this.url,
    this.branch,
    this.localPath = '',
    this.priority = 0,
    this.enabled = true,
    this.lastSynced,
  });

  factory MarketplaceSource.fromJson(Map<String, dynamic> j) => MarketplaceSource(
        id: j['id'] as String? ?? '',
        name: j['name'] as String? ?? '',
        type: j['type'] as String? ?? 'git',
        url: j['url'] as String?,
        branch: j['branch'] as String?,
        localPath: j['local_path'] as String? ?? '',
        priority: (j['priority'] as num?)?.toInt() ?? 0,
        enabled: j['enabled'] as bool? ?? true,
        lastSynced: j['last_synced'] as String?,
      );
}
