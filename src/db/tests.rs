use rusqlite::Connection;

use crate::config::Config;
use crate::types::{
    ContextMode, GroupBinding, RunStatus, ScheduleType, ScheduledTask, StoredMessage,
    TaskRunLogInsert, TaskStatus,
};

use super::Db;

fn cfg() -> Config {
    Config::from_env()
}

fn sample_group() -> GroupBinding {
    GroupBinding {
        jid: "tg:group:1".into(),
        folder: "team-a".into(),
        name: "Team A".into(),
        channel: "telegram".into(),
        group_type: "chat".into(),
        is_admin: true,
        requires_trigger: false,
        allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
        allowed_paths: None,
        allowed_work_dirs: Some(vec!["/tmp/work".into()]),
        bot_token: Some("tok".into()),
        max_messages: Some(50),
        last_active: None,
        added_at: "2026-04-28T00:00:00Z".into(),
    }
}

#[test]
fn open_in_memory_smoke() {
    Db::open_in_memory(&cfg()).unwrap();
}

#[test]
fn group_upsert_get_list_delete() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    let g = sample_group();
    db.upsert_group(&g).unwrap();
    let got = db.get_group(&g.jid).unwrap().unwrap();
    assert_eq!(got.folder, g.folder);
    assert_eq!(
        got.allowed_tools.as_deref(),
        Some(&["Read".into(), "Grep".into()][..])
    );
    assert_eq!(got.allowed_paths, None);
    assert_eq!(
        got.allowed_work_dirs.as_deref(),
        Some(&["/tmp/work".into()][..])
    );

    let mut g2 = g.clone();
    g2.name = "Renamed".into();
    db.upsert_group(&g2).unwrap();
    assert_eq!(db.get_group(&g.jid).unwrap().unwrap().name, "Renamed");

    let all = db.list_groups().unwrap();
    assert_eq!(all.len(), 1);

    db.delete_group(&g.jid).unwrap();
    assert!(db.get_group(&g.jid).unwrap().is_none());
}

#[test]
fn rename_group_jid_atomic() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    db.upsert_group(&sample_group()).unwrap();
    let renamed = db
        .rename_group_jid("tg:group:1", "tg:group:99")
        .unwrap()
        .unwrap();
    assert_eq!(renamed.jid, "tg:group:99");
    assert!(db.get_group("tg:group:1").unwrap().is_none());
    assert!(db.get_group("tg:group:99").unwrap().is_some());
}

#[test]
fn message_fifo_trims() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    for i in 0..5 {
        let msg = StoredMessage {
            message_id: format!("m{i}"),
            chat_jid: "tg:group:1".into(),
            sender_jid: "u".into(),
            sender_name: "u".into(),
            content: format!("hi {i}"),
            timestamp: format!("2026-04-28T00:00:0{i}Z"),
            is_from_me: false,
            is_bot_reply: false,
            reply_to_id: None,
            media_type: None,
        };
        db.insert_message(&msg, 3).unwrap();
    }
    let kept = db.get_messages("tg:group:1", None).unwrap();
    assert_eq!(kept.len(), 3);
    let ids: Vec<&str> = kept.iter().map(|m| m.message_id.as_str()).collect();
    assert_eq!(ids, ["m2", "m3", "m4"]);
}

#[test]
fn message_since_filter() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    for i in 0..3 {
        let msg = StoredMessage {
            message_id: format!("m{i}"),
            chat_jid: "tg:group:1".into(),
            sender_jid: "u".into(),
            sender_name: "u".into(),
            content: "x".into(),
            timestamp: format!("2026-04-28T00:00:0{i}Z"),
            is_from_me: false,
            is_bot_reply: false,
            reply_to_id: None,
            media_type: None,
        };
        db.insert_message(&msg, 100).unwrap();
    }
    let after = db
        .get_messages("tg:group:1", Some("2026-04-28T00:00:00Z"))
        .unwrap();
    assert_eq!(after.len(), 2);
}

#[test]
fn message_pagination() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    for i in 0..10 {
        let msg = StoredMessage {
            message_id: format!("m{i}"),
            chat_jid: "tg:group:1".into(),
            sender_jid: "u".into(),
            sender_name: "u".into(),
            content: format!("msg {i}"),
            timestamp: format!("2026-04-28T00:00:{:02}Z", i),
            is_from_me: false,
            is_bot_reply: false,
            reply_to_id: None,
            media_type: None,
        };
        db.insert_message(&msg, 100).unwrap();
    }

    let p1 = db.get_messages_paginated("tg:group:1", 3, 0).unwrap();
    assert_eq!(p1.len(), 3);
    assert_eq!(p1[0].message_id, "m9");
    assert_eq!(p1[1].message_id, "m8");
    assert_eq!(p1[2].message_id, "m7");

    let p2 = db.get_messages_paginated("tg:group:1", 3, 3).unwrap();
    assert_eq!(p2.len(), 3);
    assert_eq!(p2[0].message_id, "m6");
    assert_eq!(p2[1].message_id, "m5");
    assert_eq!(p2[2].message_id, "m4");
}

#[test]
fn task_lifecycle_and_logs() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    let task = ScheduledTask {
        id: "t1".into(),
        group_folder: "team-a".into(),
        chat_jid: "tg:group:1".into(),
        prompt: "do thing".into(),
        schedule_type: ScheduleType::Cron,
        schedule_value: "*/5 * * * *".into(),
        context_mode: ContextMode::Isolated,
        script_command: None,
        next_run: Some("2026-04-28T00:05:00Z".into()),
        last_run: None,
        last_result: None,
        status: TaskStatus::Active,
        created_at: "2026-04-28T00:00:00Z".into(),
    };
    db.insert_task(&task).unwrap();
    assert_eq!(db.get_tasks_by_group("team-a").unwrap().len(), 1);

    let due = db.get_due_tasks("2026-04-28T00:10:00Z").unwrap();
    assert_eq!(due.len(), 1);

    let big = "x".repeat(800);
    db.update_task_run(
        "t1",
        Some("2026-04-28T00:10:00Z"),
        "2026-04-28T00:05:00Z",
        Some(&big),
        TaskStatus::Active,
    )
    .unwrap();
    let after = &db.get_tasks_by_group("team-a").unwrap()[0];
    assert_eq!(after.last_result.as_deref().unwrap().chars().count(), 500);

    db.insert_task_run_log(&TaskRunLogInsert {
        task_id: "t1".into(),
        run_at: "2026-04-28T00:05:00Z".into(),
        duration_ms: Some(120),
        status: RunStatus::Success,
        result: Some("ok".into()),
        error: None,
    })
    .unwrap();
    let logs = db.get_task_run_logs("t1", 10).unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].status, RunStatus::Success);
    assert_eq!(logs[0].duration_ms, Some(120));

    assert!(db.delete_task("t1").unwrap());
    assert!(!db.delete_task("t1").unwrap());
}

#[test]
fn router_state_get_set() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    assert!(db.get_router_state("k").unwrap().is_none());
    db.set_router_state("k", "v").unwrap();
    assert_eq!(db.get_router_state("k").unwrap().as_deref(), Some("v"));
    db.set_router_state("k", "v2").unwrap();
    assert_eq!(db.get_router_state("k").unwrap().as_deref(), Some("v2"));

    db.set_last_agent_timestamp("tg:group:1", "2026-04-28T00:00:00Z")
        .unwrap();
    assert_eq!(
        db.get_last_agent_timestamp("tg:group:1")
            .unwrap()
            .as_deref(),
        Some("2026-04-28T00:00:00Z")
    );
}

#[test]
fn delete_messages_and_timestamp() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    for i in 0..5 {
        let msg = StoredMessage {
            message_id: format!("m{i}"),
            chat_jid: "tg:group:1".into(),
            sender_jid: "u".into(),
            sender_name: "u".into(),
            content: format!("hi {i}"),
            timestamp: format!("2026-04-28T00:00:0{i}Z"),
            is_from_me: false,
            is_bot_reply: false,
            reply_to_id: None,
            media_type: None,
        };
        db.insert_message(&msg, 100).unwrap();
    }
    assert_eq!(db.count_messages("tg:group:1").unwrap(), 5);

    db.set_last_agent_timestamp("tg:group:1", "2026-04-28T00:00:04Z")
        .unwrap();
    assert!(db.get_last_agent_timestamp("tg:group:1").unwrap().is_some());

    let deleted = db.delete_messages_for_jid("tg:group:1").unwrap();
    assert_eq!(deleted, 5);
    assert_eq!(db.count_messages("tg:group:1").unwrap(), 0);

    db.delete_agent_timestamp("tg:group:1").unwrap();
    assert!(db.get_last_agent_timestamp("tg:group:1").unwrap().is_none());
}

#[test]
fn count_messages_by_jid() {
    let db = Db::open_in_memory(&cfg()).unwrap();
    assert_eq!(db.count_messages("tg:group:1").unwrap(), 0);
    let msg = StoredMessage {
        message_id: "m1".into(),
        chat_jid: "tg:group:1".into(),
        sender_jid: "u".into(),
        sender_name: "u".into(),
        content: "hi".into(),
        timestamp: "2026-04-28T00:00:00Z".into(),
        is_from_me: false,
        is_bot_reply: false,
        reply_to_id: None,
        media_type: None,
    };
    db.insert_message(&msg, 100).unwrap();
    assert_eq!(db.count_messages("tg:group:1").unwrap(), 1);
    assert_eq!(db.count_messages("tg:group:2").unwrap(), 0);
}

#[test]
fn migration_adds_missing_columns_on_existing_db() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    {
        let conn = Connection::open(tmp.path()).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE groups (
              jid TEXT PRIMARY KEY, folder TEXT UNIQUE NOT NULL, name TEXT NOT NULL DEFAULT '',
              channel TEXT NOT NULL DEFAULT 'telegram', is_admin INTEGER NOT NULL DEFAULT 0,
              requires_trigger INTEGER NOT NULL DEFAULT 1, allowed_tools TEXT, allowed_paths TEXT,
              bot_token TEXT, max_messages INTEGER, last_active TEXT, added_at TEXT NOT NULL
            );
            CREATE TABLE scheduled_tasks (
              id TEXT PRIMARY KEY, group_folder TEXT NOT NULL, chat_jid TEXT NOT NULL,
              prompt TEXT NOT NULL, schedule_type TEXT NOT NULL, schedule_value TEXT NOT NULL,
              context_mode TEXT NOT NULL DEFAULT 'isolated', next_run TEXT, last_run TEXT,
              last_result TEXT, status TEXT NOT NULL DEFAULT 'active', created_at TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
    }
    let db = Db::open_at(tmp.path(), &cfg()).unwrap();
    db.upsert_group(&sample_group()).unwrap();
    let got = db.get_group("tg:group:1").unwrap().unwrap();
    assert_eq!(
        got.allowed_work_dirs.as_deref(),
        Some(&["/tmp/work".into()][..])
    );
}
