/// Models for the built-in Kanban board (daemon REST at /api/kanban/*).
library;

import 'dart:convert';

class KanbanBoard {
  final int id;
  final String title;
  final String description;

  /// Where this board's dispatched workers run (task outputs land here).
  final String? workspaceDir;
  final int columnCount;
  final int cardCount;

  const KanbanBoard(this.id, this.title, this.description, this.workspaceDir,
      this.columnCount, this.cardCount);

  factory KanbanBoard.fromJson(Map<String, dynamic> j) => KanbanBoard(
        (j['id'] as num?)?.toInt() ?? 0,
        '${j['title'] ?? ''}',
        '${j['description'] ?? ''}',
        j['workspace_dir'] as String?,
        (j['column_count'] as num?)?.toInt() ?? 0,
        (j['card_count'] as num?)?.toInt() ?? 0,
      );
}

/// One column in a board template.
class KanbanTemplateColumn {
  final String title;
  final String role;
  final String? color;
  final int? wipLimit;
  const KanbanTemplateColumn(this.title, this.role, this.color, this.wipLimit);

  factory KanbanTemplateColumn.fromJson(Map<String, dynamic> j) =>
      KanbanTemplateColumn(
        '${j['title'] ?? ''}',
        '${j['role'] ?? 'custom'}',
        j['color'] as String?,
        (j['wip_limit'] as num?)?.toInt(),
      );

  Map<String, dynamic> toJson() => {
        'title': title,
        'role': role,
        if (color != null) 'color': color,
        if (wipLimit != null) 'wip_limit': wipLimit,
      };
}

/// A reusable set of workflow columns (builtin or user-managed).
class KanbanTemplate {
  final String id;
  final String name;
  final String description;
  final bool builtin;
  final List<KanbanTemplateColumn> columns;
  const KanbanTemplate(
      this.id, this.name, this.description, this.builtin, this.columns);

  factory KanbanTemplate.fromJson(Map<String, dynamic> j) => KanbanTemplate(
        '${j['id'] ?? ''}',
        '${j['name'] ?? ''}',
        '${j['description'] ?? ''}',
        j['builtin'] == true,
        ((j['columns'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => KanbanTemplateColumn.fromJson(m.cast<String, dynamic>()))
            .toList(),
      );

  Map<String, dynamic> toJson() => {
        'name': name,
        'description': description,
        'columns': [for (final c in columns) c.toJson()],
      };
}

/// One row of a board's recent-activity feed.
class KanbanActivityItem {
  final int cardId;
  final String cardTitle;
  final String author;
  final String body;
  final String kind;
  final int createdAt;
  const KanbanActivityItem(this.cardId, this.cardTitle, this.author, this.body,
      this.kind, this.createdAt);

  factory KanbanActivityItem.fromJson(Map<String, dynamic> j) =>
      KanbanActivityItem(
        (j['card_id'] as num?)?.toInt() ?? 0,
        '${j['card_title'] ?? ''}',
        '${j['author'] ?? ''}',
        '${j['body'] ?? ''}',
        '${j['kind'] ?? 'comment'}',
        (j['created_at'] as num?)?.toInt() ?? 0,
      );
}

/// Live activity: tasks being worked now + the recent worker feed.
class KanbanActivity {
  final List<KanbanCard> running;
  final List<KanbanActivityItem> recent;
  const KanbanActivity(this.running, this.recent);

  factory KanbanActivity.fromJson(Map<String, dynamic> j) => KanbanActivity(
        ((j['running'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => KanbanCard.fromJson(m.cast<String, dynamic>()))
            .toList(),
        ((j['recent'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => KanbanActivityItem.fromJson(m.cast<String, dynamic>()))
            .toList(),
      );
}

/// A persona/worker profile (from /api/cowork/personas).
class WorkerProfile {
  final String name;
  final String description;
  const WorkerProfile(this.name, this.description);

  factory WorkerProfile.fromJson(Map<String, dynamic> j) =>
      WorkerProfile('${j['name'] ?? ''}', '${j['description'] ?? ''}');
}

class KanbanColumn {
  final int id;
  final String title;

  /// Workflow role: triage|todo|ready|in_progress|blocked|done|custom.
  final String role;
  final String? color;
  final int? wipLimit;
  final List<KanbanCard> cards;

  const KanbanColumn(
      this.id, this.title, this.role, this.color, this.wipLimit, this.cards);

  factory KanbanColumn.fromJson(Map<String, dynamic> j) => KanbanColumn(
        (j['id'] as num?)?.toInt() ?? 0,
        '${j['title'] ?? ''}',
        '${j['role'] ?? 'custom'}',
        j['color'] as String?,
        (j['wip_limit'] as num?)?.toInt(),
        ((j['cards'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => KanbanCard.fromJson(m.cast<String, dynamic>()))
            .toList(),
      );
}

class KanbanCard {
  final int id;
  final int columnId;
  final String title;
  final String description;

  /// low | medium | high | urgent.
  final String? priority;

  /// Worker / persona this task is routed to (drives worker lanes).
  final String? assignee;
  final String? tenant;
  final List<String> labels;
  final int? dueDate; // unix seconds
  final bool done;
  final int commentCount;

  /// Open (not-done) dependency parents — card is blocked while > 0.
  final int openDeps;
  final int childTotal;
  final int childDone;

  const KanbanCard(
      this.id,
      this.columnId,
      this.title,
      this.description,
      this.priority,
      this.assignee,
      this.tenant,
      this.labels,
      this.dueDate,
      this.done,
      this.commentCount,
      this.openDeps,
      this.childTotal,
      this.childDone);

  factory KanbanCard.fromJson(Map<String, dynamic> j) {
    // `labels` is stored as a JSON-encoded array string (e.g. '["bug","ui"]').
    List<String> labels = const [];
    final raw = j['labels'];
    if (raw is String && raw.trim().startsWith('[')) {
      try {
        labels = (json.decode(raw) as List).map((e) => '$e').toList();
      } catch (_) {}
    } else if (raw is List) {
      labels = raw.map((e) => '$e').toList();
    }
    return KanbanCard(
      (j['id'] as num?)?.toInt() ?? 0,
      (j['column_id'] as num?)?.toInt() ?? 0,
      '${j['title'] ?? ''}',
      '${j['description'] ?? ''}',
      j['priority'] as String?,
      j['assignee'] as String?,
      j['tenant'] as String?,
      labels,
      (j['due_date'] as num?)?.toInt(),
      j['done'] == true,
      (j['comment_count'] as num?)?.toInt() ?? 0,
      (j['open_deps'] as num?)?.toInt() ?? 0,
      (j['child_total'] as num?)?.toInt() ?? 0,
      (j['child_done'] as num?)?.toInt() ?? 0,
    );
  }
}

class KanbanComment {
  final int id;
  final String author;
  final String body;

  /// comment | complete | block | unblock | system.
  final String kind;
  final int createdAt;

  const KanbanComment(
      this.id, this.author, this.body, this.kind, this.createdAt);

  factory KanbanComment.fromJson(Map<String, dynamic> j) => KanbanComment(
        (j['id'] as num?)?.toInt() ?? 0,
        '${j['author'] ?? ''}',
        '${j['body'] ?? ''}',
        '${j['kind'] ?? 'comment'}',
        (j['created_at'] as num?)?.toInt() ?? 0,
      );
}

class KanbanLink {
  final int parentId;
  final int childId;
  final String parentTitle;
  final String childTitle;
  final bool parentDone;
  final bool childDone;

  const KanbanLink(this.parentId, this.childId, this.parentTitle,
      this.childTitle, this.parentDone, this.childDone);

  factory KanbanLink.fromJson(Map<String, dynamic> j) => KanbanLink(
        (j['parent_id'] as num?)?.toInt() ?? 0,
        (j['child_id'] as num?)?.toInt() ?? 0,
        '${j['parent_title'] ?? ''}',
        '${j['child_title'] ?? ''}',
        j['parent_done'] == true,
        j['child_done'] == true,
      );
}

class KanbanCardDetail {
  final KanbanCard card;
  final List<KanbanComment> comments;
  final List<KanbanLink> links;
  const KanbanCardDetail(this.card, this.comments, this.links);

  factory KanbanCardDetail.fromJson(Map<String, dynamic> j) =>
      KanbanCardDetail(
        KanbanCard.fromJson((j['card'] as Map).cast<String, dynamic>()),
        ((j['comments'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => KanbanComment.fromJson(m.cast<String, dynamic>()))
            .toList(),
        ((j['links'] as List?) ?? const [])
            .whereType<Map>()
            .map((m) => KanbanLink.fromJson(m.cast<String, dynamic>()))
            .toList(),
      );
}
