use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::bridge::DispatchBridge;
use super::dag::{build_augmented_prompt, is_ready};
use super::locks::lock_path_for;
use super::resume::build_dispatch_resume_hint;
use super::traits::{DispatchBridgeApi, NoopDispatchBridge};
use super::types::{DispatchParent, DispatchTask, DispatchTaskStatus};

#[test]
fn noop_bridge_returns_no_parents() {
    let b = NoopDispatchBridge;
    assert!(b.get_parents().is_empty());
    assert!(build_dispatch_resume_hint(Some(&b), "main").is_none());
}

#[test]
fn resume_hint_handles_no_bridge() {
    assert!(build_dispatch_resume_hint(None, "main").is_none());
}

struct FakeBridge {
    parents: Vec<DispatchParent>,
}
impl DispatchBridgeApi for FakeBridge {
    fn get_parents(&self) -> Vec<DispatchParent> {
        self.parents.clone()
    }
}

#[test]
fn resume_hint_renders_active_parents_only() {
    let now = "2025-01-01T00:00:00Z".to_string();
    let parents = vec![
        DispatchParent {
            id: "p1".into(),
            goal: "goal-1".into(),
            admin_folder: "main".into(),
            shared_workspace: None,
            status: "active".into(),
            created_at: now.clone(),
            completed_at: None,
            tasks: vec![DispatchTask {
                id: "t1".into(),
                label: "writer".into(),
                agent_id: "writer-agent".into(),
                agent_jid: String::new(),
                depends_on: vec![],
                prompt: "do thing".into(),
                status: DispatchTaskStatus::Processing,
                result: None,
                created_at: now.clone(),
                started_at: None,
                timeout_seconds: 0,
                timeout_at: None,
                completed_at: None,
                is_virtual: false,
                persona_name: None,
            }],
        },
        DispatchParent {
            id: "p2".into(),
            goal: "goal-2".into(),
            admin_folder: "main".into(),
            shared_workspace: None,
            status: "completed".into(),
            created_at: now.clone(),
            completed_at: None,
            tasks: vec![],
        },
        DispatchParent {
            id: "p3".into(),
            goal: "goal-3".into(),
            admin_folder: "other".into(),
            shared_workspace: None,
            status: "active".into(),
            created_at: now,
            completed_at: None,
            tasks: vec![],
        },
    ];
    let hint = build_dispatch_resume_hint(Some(&FakeBridge { parents }), "main").unwrap();
    assert!(hint.contains("Task group p1"));
    assert!(hint.contains("processing"));
    assert!(!hint.contains("p2"));
    assert!(!hint.contains("p3"));
}

fn tmp_state_path(suffix: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "senclaw-dispatch-{}-{}.json",
        suffix,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(lock_path_for(&p));
    p
}

fn make_task(id: &str, label: &str, jid: &str) -> DispatchTask {
    DispatchTask {
        id: id.into(),
        label: label.into(),
        agent_id: "writer".into(),
        agent_jid: jid.into(),
        depends_on: vec![],
        prompt: "do".into(),
        status: DispatchTaskStatus::Processing,
        result: None,
        created_at: "2025-01-01T00:00:00Z".into(),
        started_at: Some("2025-01-01T00:00:01Z".into()),
        timeout_seconds: 60,
        timeout_at: None,
        completed_at: None,
        is_virtual: false,
        persona_name: None,
    }
}

#[test]
fn modify_state_round_trips_through_disk() {
    let path = tmp_state_path("roundtrip");
    let bridge = DispatchBridge::new(&path);
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "p1".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: "2025-01-01T00:00:00Z".into(),
                completed_at: None,
                tasks: vec![make_task("d1", "writer", "jid-a")],
            });
        })
        .unwrap();

    // Re-open and confirm the state survives a fresh bridge instance.
    let bridge2 = DispatchBridge::new(&path);
    let parents = bridge2.get_parents();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].tasks[0].agent_jid, "jid-a");
    assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Processing);
    let _ = std::fs::remove_file(path);
}

#[test]
fn notify_task_done_marks_terminal_and_completes_parent() {
    let path = tmp_state_path("done");
    let bridge = DispatchBridge::new(&path);
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "p1".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: "2025-01-01T00:00:00Z".into(),
                completed_at: None,
                tasks: vec![make_task("d1", "only", "jid-a")],
            });
        })
        .unwrap();

    bridge.notify_task_done("d1", "result-text");

    let parents = bridge.get_parents();
    assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Done);
    assert_eq!(parents[0].tasks[0].result.as_deref(), Some("result-text"));
    assert_eq!(parents[0].status, "done");
    assert!(parents[0].completed_at.is_some());
    let _ = std::fs::remove_file(path);
}

#[test]
fn notify_reply_resolves_earliest_processing_task() {
    let path = tmp_state_path("reply");
    let bridge = DispatchBridge::new(&path);
    let mut t_old = make_task("d_old", "old", "jid-a");
    t_old.started_at = Some("2025-01-01T00:00:01Z".into());
    let mut t_new = make_task("d_new", "new", "jid-a");
    t_new.started_at = Some("2025-01-01T00:00:09Z".into());
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "p1".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: "2025-01-01T00:00:00Z".into(),
                completed_at: None,
                tasks: vec![t_old, t_new],
            });
        })
        .unwrap();
    // Both are tracked as in-flight against the same jid.
    bridge.add_active_task("d_old", "jid-a");
    bridge.add_active_task("d_new", "jid-a");

    bridge.notify_reply("jid-a", "old-result");

    let parents = bridge.get_parents();
    let by_id: HashMap<_, _> = parents[0].tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    assert_eq!(by_id["d_old"].status, DispatchTaskStatus::Done);
    assert_eq!(by_id["d_new"].status, DispatchTaskStatus::Processing);
    let _ = std::fs::remove_file(path);
}

#[test]
fn is_ready_with_terminal_deps_returns_true() {
    let mut a = make_task("a", "a", "j");
    a.status = DispatchTaskStatus::Done;
    let mut b = make_task("b", "b", "j");
    b.status = DispatchTaskStatus::Error; // continue-on-error
    let mut c = make_task("c", "c", "j");
    c.depends_on = vec!["a".into(), "b".into()];
    c.status = DispatchTaskStatus::Registered;
    let all = vec![a, b, c.clone()];
    assert!(is_ready(&c, &all));

    // Flip one dep back to processing → not ready.
    let mut all2 = all.clone();
    all2[0].status = DispatchTaskStatus::Processing;
    assert!(!is_ready(&c, &all2));
}

#[test]
fn build_augmented_prompt_includes_parent_goal_and_prereq_results() {
    let mut dep = make_task("d_dep", "writer", "j");
    dep.status = DispatchTaskStatus::Done;
    dep.result = Some("dep-result".into());
    dep.prompt = "draft a thing".into();

    let mut other = make_task("d_other", "reviewer", "j");
    other.status = DispatchTaskStatus::Processing;
    other.prompt = "review later".into();

    let mut me = make_task("d_me", "publisher", "j");
    me.depends_on = vec!["writer".into()];
    me.prompt = "publish it".into();

    let parent = DispatchParent {
        id: "p1".into(),
        goal: "ship the thing".into(),
        admin_folder: "main".into(),
        shared_workspace: None,
        status: "active".into(),
        created_at: "2025-01-01T00:00:00Z".into(),
        completed_at: None,
        tasks: vec![dep, other, me.clone()],
    };
    let augmented = build_augmented_prompt(&parent, &me);
    assert!(augmented.contains("<parent_goal>ship the thing</parent_goal>"));
    assert!(augmented.contains("<prerequisites>"));
    assert!(augmented.contains("<result>dep-result</result>"));
    assert!(augmented.contains("<other_tasks>"));
    assert!(augmented.contains("review later"));
    assert!(augmented.ends_with("\n\npublish it"));
}

#[test]
fn process_pending_launches_ready_task_via_callback() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let path = tmp_state_path("scheduler");
    let bridge = DispatchBridge::new(&path);
    let fired = Arc::new(AtomicBool::new(false));
    {
        let f = Arc::clone(&fired);
        bridge.set_send_to_agent(Arc::new(
            move |jid: &str, task_id: &str, prompt: &str, _ws: &str| {
                assert_eq!(jid, "jid-x");
                assert_eq!(task_id, "d1");
                assert!(prompt.contains("<parent_goal>g</parent_goal>"));
                f.store(true, Ordering::SeqCst);
            },
        ));
    }
    let mut t = make_task("d1", "only", "jid-x");
    t.status = DispatchTaskStatus::Registered;
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "p1".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: "2025-01-01T00:00:00Z".into(),
                completed_at: None,
                tasks: vec![t],
            });
        })
        .unwrap();
    bridge.process_pending();
    assert!(fired.load(Ordering::SeqCst));
    let parents = bridge.get_parents();
    assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Processing);
    assert!(parents[0].tasks[0].started_at.is_some());
    assert!(parents[0].tasks[0].timeout_at.is_some());
    let _ = std::fs::remove_file(path);
}

#[test]
fn activate_next_queued_promotes_oldest_and_picks_up_admin_workspace() {
    // state file lives under a tmp dir so the workspace-state file we
    // write next to it is found via state_path.parent().
    let dir = std::env::temp_dir().join(format!("senclaw-q-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let state_path = dir.join("dispatch-state.json");
    let _ = std::fs::remove_file(&state_path);
    let _ = std::fs::remove_file(lock_path_for(&state_path));
    std::fs::write(
        dir.join("workspace-state-main.json"),
        r#"{"currentDir":"/tmp/admin-workspace"}"#,
    )
    .unwrap();

    let bridge = DispatchBridge::new(&state_path);
    let now = chrono::Utc::now();
    let older = (now - chrono::Duration::seconds(10)).to_rfc3339();
    let newer = now.to_rfc3339();
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "p_old".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "queued".into(),
                created_at: older,
                completed_at: None,
                tasks: vec![],
            });
            s.parents.push(DispatchParent {
                id: "p_new".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "queued".into(),
                created_at: newer,
                completed_at: None,
                tasks: vec![],
            });
        })
        .unwrap();

    bridge.activate_next_queued("main");
    let parents = bridge.get_parents();
    let by_id: HashMap<_, _> = parents.iter().map(|p| (p.id.as_str(), p)).collect();
    assert_eq!(by_id["p_old"].status, "active");
    assert_eq!(
        by_id["p_old"].shared_workspace.as_deref(),
        Some("/tmp/admin-workspace")
    );
    assert_eq!(by_id["p_new"].status, "queued");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cleanup_drops_old_done_parents() {
    let path = tmp_state_path("cleanup");
    let bridge = DispatchBridge::new(&path);
    let stale = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    let fresh = chrono::Utc::now().to_rfc3339();
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "old".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "done".into(),
                created_at: stale.clone(),
                completed_at: Some(stale),
                tasks: vec![],
            });
            s.parents.push(DispatchParent {
                id: "new".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "done".into(),
                created_at: fresh.clone(),
                completed_at: Some(fresh),
                tasks: vec![],
            });
        })
        .unwrap();
    bridge.cleanup();
    let ids: Vec<_> = bridge.get_parents().iter().map(|p| p.id.clone()).collect();
    assert_eq!(ids, vec!["new".to_string()]);
    let _ = std::fs::remove_file(path);
}

#[test]
fn cancel_admin_parents_marks_active_jids_and_clears_tasks() {
    let path = tmp_state_path("cancel");
    let bridge = DispatchBridge::new(&path);
    bridge
        .modify_state(|s| {
            s.parents.push(DispatchParent {
                id: "p1".into(),
                goal: "g".into(),
                admin_folder: "main".into(),
                shared_workspace: None,
                status: "active".into(),
                created_at: "2025-01-01T00:00:00Z".into(),
                completed_at: None,
                tasks: vec![make_task("d1", "x", "jid-a")],
            });
        })
        .unwrap();
    bridge.add_active_task("d1", "jid-a");

    let affected = bridge.cancel_admin_parents("main");
    assert_eq!(affected, vec!["jid-a".to_string()]);
    let parents = bridge.get_parents();
    assert_eq!(parents[0].status, "done");
    assert_eq!(parents[0].tasks[0].status, DispatchTaskStatus::Error);
    assert!(!bridge.has_active_jid_tasks("jid-a"));
    let _ = std::fs::remove_file(path);
}
