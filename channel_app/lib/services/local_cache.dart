import 'dart:convert';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:path/path.dart' as p;
import 'package:sqflite/sqflite.dart';
import '../models/agent_model.dart';
import 'logger_service.dart';

/// Normalize a decoded JSON value into a list of string-keyed maps —
/// the shape [LocalCache.putDomainList] stores and API services re-hydrate.
List<Map<String, dynamic>> jsonMaps(dynamic v) => (v is List ? v : const [])
    .whereType<Map>()
    .map((e) => e.cast<String, dynamic>())
    .toList();

/// A server-authoritative chat message row held in the local cache.
///
/// [ts] is the daemon-parsed epoch-millis timestamp (the `ts` field of
/// `GET /api/chat/history`) — the same clock the sync cursor uses, so cache
/// ordering and delta fetches never depend on the device timezone.
class CachedMessage {
  final String id;
  final int ts;
  final String role; // 'user' | 'agent'
  final String content;
  final String sender;
  final bool isFromMe;
  final bool isBotReply;

  const CachedMessage({
    required this.id,
    required this.ts,
    required this.role,
    required this.content,
    this.sender = '',
    this.isFromMe = false,
    this.isBotReply = false,
  });

  Map<String, dynamic> toJson() => {
        'id': id,
        'ts': ts,
        'role': role,
        'content': content,
        'sender': sender,
        'isFromMe': isFromMe,
        'isBotReply': isBotReply,
      };

  factory CachedMessage.fromJson(Map<String, dynamic> json) => CachedMessage(
        id: (json['id'] ?? '').toString(),
        ts: (json['ts'] as num?)?.toInt() ?? 0,
        role: (json['role'] ?? 'user').toString(),
        content: (json['content'] ?? '').toString(),
        sender: (json['sender'] ?? '').toString(),
        isFromMe: json['isFromMe'] == true,
        isBotReply: json['isBotReply'] == true,
      );
}

/// On-device SQLite cache for the chat surface (agents list + message
/// history) so screens render instantly from disk and only deltas travel
/// over the slow relay tunnel.
///
/// Sync cursor semantics: `meta['cursor:<jid>']` holds the max daemon-parsed
/// epoch-ms `ts` ever stored for that jid from a REST delta fetch. The next
/// delta fetch asks the daemon for messages with `ts > cursor` only.
///
/// No-ops on web (sqflite has no web backend) — callers degrade to the
/// existing full-fetch path.
class LocalCache {
  static final LocalCache _instance = LocalCache._internal();
  factory LocalCache() => _instance;
  LocalCache._internal();

  static const _dbName = 'senclaw_cache.db';

  Database? _db;
  Future<Database?>? _opening;

  Future<Database?> _open() async {
    if (kIsWeb) return null;
    if (_db != null) return _db;
    final inFlight = _opening;
    if (inFlight != null) return inFlight;

    final future = () async {
      try {
        final dir = await getDatabasesPath();
        final db = await openDatabase(
          p.join(dir, _dbName),
          version: 1,
          onCreate: (db, _) async {
            await db.execute(
              'CREATE TABLE groups('
              'jid TEXT PRIMARY KEY, name TEXT, folder TEXT, '
              'group_type TEXT, last_activity INTEGER, json TEXT)',
            );
            await db.execute(
              'CREATE TABLE messages('
              'jid TEXT NOT NULL, ts INTEGER NOT NULL, id TEXT NOT NULL, '
              'role TEXT, json TEXT, PRIMARY KEY(jid, ts, id))',
            );
            await db.execute(
              'CREATE INDEX idx_messages_jid_ts ON messages(jid, ts)',
            );
            await db.execute(
              'CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT)',
            );
          },
        );
        _db = db;
        return db;
      } catch (e) {
        Log.e('[LocalCache] open failed: $e');
        return null;
      } finally {
        _opening = null;
      }
    }();
    _opening = future;
    return future;
  }

  // ── Agents (groups table, group_type = 'agent') ───────────────────────────

  /// Replace the cached agent list for instant rendering on next launch.
  Future<void> upsertAgents(List<AgentInfo> agents) async {
    final db = await _open();
    if (db == null) return;
    try {
      final now = DateTime.now().millisecondsSinceEpoch;
      final batch = db.batch();
      // Full-replace: an unbound agent must disappear from the cache too.
      batch.delete('groups', where: "group_type = 'agent'");
      for (final a in agents) {
        batch.insert(
          'groups',
          {
            'jid': 'agent:${a.folder}',
            'name': a.name,
            'folder': a.folder,
            'group_type': 'agent',
            'last_activity': now,
            'json': jsonEncode({'folder': a.folder, 'name': a.name}),
          },
          conflictAlgorithm: ConflictAlgorithm.replace,
        );
      }
      await batch.commit(noResult: true);
    } catch (e) {
      Log.e('[LocalCache] upsertAgents failed: $e');
    }
  }

  Future<List<AgentInfo>> getAgents() async {
    final db = await _open();
    if (db == null) return const [];
    try {
      final rows = await db.query(
        'groups',
        where: "group_type = 'agent'",
        orderBy: 'name ASC',
      );
      return rows
          .map((r) => AgentInfo(
                jid: '',
                folder: (r['folder'] ?? '').toString(),
                name: (r['name'] ?? '').toString(),
                channel: 'app',
              ))
          .where((a) => a.folder.isNotEmpty)
          .toList();
    } catch (e) {
      Log.e('[LocalCache] getAgents failed: $e');
      return const [];
    }
  }

  // ── Messages ──────────────────────────────────────────────────────────────

  Future<void> upsertMessages(String jid, List<CachedMessage> messages) async {
    if (messages.isEmpty) return;
    final db = await _open();
    if (db == null) return;
    try {
      final batch = db.batch();
      for (final m in messages) {
        batch.insert(
          'messages',
          {
            'jid': jid,
            'ts': m.ts,
            'id': m.id,
            'role': m.role,
            'json': jsonEncode(m.toJson()),
          },
          conflictAlgorithm: ConflictAlgorithm.replace,
        );
      }
      await batch.commit(noResult: true);
    } catch (e) {
      Log.e('[LocalCache] upsertMessages failed: $e');
    }
  }

  /// Newest [limit] messages for [jid], returned oldest → newest.
  Future<List<CachedMessage>> getMessages(String jid, {int limit = 200}) async {
    final db = await _open();
    if (db == null) return const [];
    try {
      final rows = await db.query(
        'messages',
        columns: ['json'],
        where: 'jid = ?',
        whereArgs: [jid],
        orderBy: 'ts DESC, id DESC',
        limit: limit,
      );
      final out = rows
          .map((r) => CachedMessage.fromJson(
              jsonDecode((r['json'] ?? '{}').toString())
                  as Map<String, dynamic>))
          .toList();
      return out.reversed.toList();
    } catch (e) {
      Log.e('[LocalCache] getMessages failed: $e');
      return const [];
    }
  }

  /// Drop a chat's cached messages + cursor (e.g. user-forced full reload).
  Future<void> clearMessages(String jid) async {
    final db = await _open();
    if (db == null) return;
    try {
      await db.delete('messages', where: 'jid = ?', whereArgs: [jid]);
      await db.delete('meta', where: 'key = ?', whereArgs: ['cursor:$jid']);
    } catch (e) {
      Log.e('[LocalCache] clearMessages failed: $e');
    }
  }

  // ── Generic domain list caches (one SQLite table per domain) ─────────────
  //
  // Every list-shaped `/api/*` surface (workflows, cowork teams, notes,
  // schedules, apps…) gets its own `cache_<domain>` table so screens render
  // instantly from disk while the fresh list travels over the slow relay.
  // Semantics are server-authoritative full-replace per [scope]: rows keep
  // the server ordering via `ord`, and a successful re-fetch swaps the whole
  // scope atomically. [scope] partitions a domain (e.g. schedules per agent
  // folder); most domains use the default ''.

  final Set<String> _ensuredTables = {};

  static String _tableFor(String domain) {
    final safe = domain.toLowerCase().replaceAll(RegExp(r'[^a-z0-9_]'), '_');
    return 'cache_$safe';
  }

  Future<Database?> _domainDb(String domain) async {
    final db = await _open();
    if (db == null) return null;
    final table = _tableFor(domain);
    if (_ensuredTables.contains(table)) return db;
    try {
      await db.execute(
        'CREATE TABLE IF NOT EXISTS $table('
        'scope TEXT NOT NULL, ord INTEGER NOT NULL, '
        'json TEXT NOT NULL, ts INTEGER NOT NULL, '
        'PRIMARY KEY(scope, ord))',
      );
      _ensuredTables.add(table);
      return db;
    } catch (e) {
      Log.e('[LocalCache] ensure $table failed: $e');
      return null;
    }
  }

  /// Replace the cached list of [domain]/[scope] with [items] (server order).
  Future<void> putDomainList(
    String domain,
    List<Map<String, dynamic>> items, {
    String scope = '',
  }) async {
    final db = await _domainDb(domain);
    if (db == null) return;
    try {
      final table = _tableFor(domain);
      final now = DateTime.now().millisecondsSinceEpoch;
      final batch = db.batch();
      batch.delete(table, where: 'scope = ?', whereArgs: [scope]);
      for (var i = 0; i < items.length; i++) {
        batch.insert(table, {
          'scope': scope,
          'ord': i,
          'json': jsonEncode(items[i]),
          'ts': now,
        });
      }
      await batch.commit(noResult: true);
    } catch (e) {
      Log.e('[LocalCache] putDomainList($domain) failed: $e');
    }
  }

  /// Cached list of [domain]/[scope] in server order; empty when never synced.
  Future<List<Map<String, dynamic>>> getDomainList(
    String domain, {
    String scope = '',
  }) async {
    final db = await _domainDb(domain);
    if (db == null) return const [];
    try {
      final rows = await db.query(
        _tableFor(domain),
        columns: ['json'],
        where: 'scope = ?',
        whereArgs: [scope],
        orderBy: 'ord ASC',
      );
      return rows
          .map((r) {
            try {
              final v = jsonDecode((r['json'] ?? '{}').toString());
              return v is Map ? v.cast<String, dynamic>() : null;
            } catch (_) {
              return null;
            }
          })
          .whereType<Map<String, dynamic>>()
          .toList();
    } catch (e) {
      Log.e('[LocalCache] getDomainList($domain) failed: $e');
      return const [];
    }
  }

  // ── Sync cursors (meta) ───────────────────────────────────────────────────

  /// Max server-side `ts` (epoch ms) already synced for [jid]; 0 = never.
  Future<int> getSyncCursor(String jid) async {
    final db = await _open();
    if (db == null) return 0;
    try {
      final rows = await db.query(
        'meta',
        columns: ['value'],
        where: 'key = ?',
        whereArgs: ['cursor:$jid'],
      );
      if (rows.isEmpty) return 0;
      return int.tryParse((rows.first['value'] ?? '').toString()) ?? 0;
    } catch (e) {
      Log.e('[LocalCache] getSyncCursor failed: $e');
      return 0;
    }
  }

  Future<void> setSyncCursor(String jid, int ts) async {
    final db = await _open();
    if (db == null) return;
    try {
      await db.insert(
        'meta',
        {'key': 'cursor:$jid', 'value': '$ts'},
        conflictAlgorithm: ConflictAlgorithm.replace,
      );
    } catch (e) {
      Log.e('[LocalCache] setSyncCursor failed: $e');
    }
  }
}
