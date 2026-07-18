//! The OpenClaw-style heartbeat. On a cadence it reads the feed and, driven by
//! the molty persona, decides what to engage with. In the default **draft** mode
//! nothing is published — every engagement lands in the approval queue. In
//! **live** mode it publishes immediately (with rate-limit guards). In
//! **observe** mode it only refreshes the local feed cache.
//!
//! [`execute_draft`] is the single code path that actually writes to Moltbook —
//! shared by the approve button (draft mode) and the live heartbeat — so the
//! publish semantics (and the anti-human verification handshake) live in one
//! place.

use crate::api::{client, now_ts, voice, AppState};
use crate::db::{CachedPost, Draft, DraftCreate};
use crate::llm::{self, FeedItem};
use crate::moltbook::Moltbook;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// Moltbook allows 1 post / 30 minutes. Keep the engine under that.
const POST_COOLDOWN_SECS: i64 = 30 * 60;
/// The heartbeat wakes this often to check whether the cadence has elapsed.
const TICK_SECS: u64 = 60;
/// Never let the configured cadence drop below this (avoid hammering the API).
const MIN_CADENCE_MINUTES: i64 = 5;

/// Spawn the background heartbeat. It is a no-op until the user connects an API
/// key AND enables the heartbeat in Settings.
pub fn spawn_heartbeat(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            let db = &state.db;
            if !db.get_bool("heartbeat_enabled", false) || !db.connected() {
                continue;
            }
            let cadence = db.get_i64("heartbeat_minutes", 60).max(MIN_CADENCE_MINUTES);
            let last = db.get_i64("last_heartbeat_at", 0);
            if now_ts() - last < cadence * 60 {
                continue;
            }
            let _ = run_once(&state, "engine").await;
        }
    });
}

/// Run exactly one heartbeat tick now. Returns a summary Value for the UI/MCP.
pub async fn run_once(state: &Arc<AppState>, source: &str) -> Value {
    let db = &state.db;
    let now = now_ts();
    db.set_i64("last_heartbeat_at", now).ok();

    if !db.connected() {
        let msg = "Chưa kết nối agent — thêm API key ở Cài đặt trước.";
        db.log("heartbeat", msg, "", now).ok();
        return json!({ "ok": false, "reason": msg });
    }

    let autonomy = db.autonomy();
    let client = client(db);

    // 1. Pull a feed slice (personalised feed first, global posts as fallback).
    let feed = match fetch_feed(&client).await {
        Ok(f) => f,
        Err(e) => {
            db.log("error", &format!("heartbeat: {e}"), "", now).ok();
            return json!({ "ok": false, "reason": e });
        }
    };
    // Cache for the UI (only real posts — never overwrite with nothing).
    if !feed.is_empty() {
        db.clear_live_cache().ok();
        db.upsert_posts(&feed.iter().map(|f| to_cache(f, now)).collect::<Vec<_>>()).ok();
    }

    if autonomy == "observe" {
        let msg = format!("Quan sát: đã làm mới {} bài trên feed.", feed.len());
        db.log("heartbeat", &msg, source, now).ok();
        return json!({ "ok": true, "mode": "observe", "fetched": feed.len(), "note": msg });
    }

    // 2. Only consider posts we haven't already engaged with.
    let fresh: Vec<&FeedItem> = feed.iter().filter(|f| !db.already_targeting(&f.id)).collect();
    if fresh.is_empty() {
        let msg = "Không có bài mới nào để tương tác.";
        db.log("heartbeat", msg, source, now).ok();
        return json!({ "ok": true, "mode": autonomy, "fetched": feed.len(), "note": msg });
    }

    // 3. Ask the persona to plan engagements within budget.
    let voice = voice(db);
    let budget = db.get_i64("engage_limit", 2).clamp(0, 10);
    let allow_new_post = now - db.get_i64("last_post_at", 0) > POST_COOLDOWN_SECS;
    let default_submolt = db.get_str("default_submolt", "general");
    let items: Vec<FeedItem> = fresh.into_iter().map(clone_item).collect();

    // Moltbook's #1 heartbeat action (per heartbeat.md): reply to molties who
    // replied to YOU. `/home` surfaces that under `activity_on_your_posts`.
    // Best-effort — an old/unauth daemon just yields an empty priority list.
    let priority: Vec<(String, String)> = client
        .home()
        .await
        .ok()
        .map(|h| extract_home_activity(&h))
        .unwrap_or_default()
        .into_iter()
        .filter(|(pid, _)| !db.already_targeting(pid))
        .collect();

    // How the human wants this molty steered: which subjects to engage with and
    // what they want it to post/ask about.
    let steer = build_steer(db);
    if steer.focus_only && steer.engage.is_empty() {
        // Respect the setting literally rather than quietly engaging with
        // everything — but say why nothing happened.
        let msg = "Đang ở chế độ 'chỉ chủ đề đã chọn' nhưng danh sách chủ đề trống — bỏ qua tương tác. Thêm chủ đề, hoặc chuyển sang 'toàn bộ feed'.";
        db.log("heartbeat", msg, source, now).ok();
        return json!({ "ok": true, "mode": autonomy, "fetched": feed.len(), "drafted": 0, "note": msg });
    }

    // Ground the plan in the molty's own memory (trí nhớ) + the shared wiki
    // (kho thông tin) for whatever the feed is talking about right now.
    let grounding = crate::api::grounding_for(db, &topics_of(&items, &priority)).await;
    if !grounding.is_empty() {
        db.log(
            "memory",
            &format!(
                "nạp ngữ cảnh: {}{}",
                if grounding.memory.trim().is_empty() { "" } else { "trí nhớ " },
                if grounding.wiki.trim().is_empty() { "" } else { "kho thông tin (wiki)" }
            ),
            source,
            now,
        )
        .ok();
    }

    let (plan, model) = match llm::plan_engagements(&voice, &items, &priority, &grounding, &steer, budget, &default_submolt, allow_new_post).await {
        Ok(v) => v,
        Err(e) => {
            db.log("error", &format!("heartbeat plan: {e}"), source, now).ok();
            return json!({ "ok": false, "reason": e });
        }
    };

    let title_of = |id: &str| {
        items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.title.clone())
            .or_else(|| priority.iter().find(|(p, _)| p == id).map(|(_, s)| crate::llm::truncate(s, 60)))
            .unwrap_or_default()
    };

    // 4. Materialise the plan — as drafts (draft mode) or live writes (live mode).
    let mut drafted = 0usize;
    let mut published = 0usize;
    let mut errors = 0usize;
    let live = autonomy == "live";

    // upvotes
    for pid in &plan.upvotes {
        let d = DraftCreate {
            kind: "vote".into(),
            vote_dir: "up".into(),
            target_post_id: pid.clone(),
            target_title: title_of(pid),
            reason: "heartbeat: bài đáng chú ý".into(),
            source: "engine".into(),
            model: model.clone(),
            ..Default::default()
        };
        apply(state, &d, live, now, &mut drafted, &mut published, &mut errors).await;
    }
    // comments
    for c in &plan.comments {
        let d = DraftCreate {
            kind: "comment".into(),
            target_post_id: c.post_id.clone(),
            target_title: title_of(&c.post_id),
            content: c.content.clone(),
            reason: if c.why.trim().is_empty() { "heartbeat".into() } else { c.why.clone() },
            source: "engine".into(),
            model: model.clone(),
            ..Default::default()
        };
        apply(state, &d, live, now, &mut drafted, &mut published, &mut errors).await;
    }
    // one new post
    if let Some(p) = &plan.new_post {
        if !p.title.trim().is_empty() {
            // Stamp the idea it came from so the next tick rotates to another.
            if let Some(i) = p.idea.filter(|i| *i > 0) {
                if let Some((topic_id, _)) = steer.ideas.get((i - 1) as usize) {
                    db.mark_topic_used(*topic_id, now).ok();
                }
            }
            let d = DraftCreate {
                kind: "post".into(),
                submolt: if p.submolt.trim().is_empty() { default_submolt.clone() } else { p.submolt.trim_start_matches("m/").to_string() },
                title: p.title.clone(),
                content: p.content.clone(),
                reason: if p.why.trim().is_empty() { "heartbeat".into() } else { p.why.clone() },
                source: "engine".into(),
                model: model.clone(),
                ..Default::default()
            };
            apply(state, &d, live, now, &mut drafted, &mut published, &mut errors).await;
        }
    }

    // Close the loop: collect what other agents said about our earlier posts and
    // refresh their wiki docs. Cheap when nothing changed.
    let harvest_summary = if db.get_bool("harvest_enabled", true) {
        harvest(state, None).await
    } else {
        Value::Null
    };

    // Once a day, summarise what the whole agent internet is talking about.
    // Off by default — it's an extra LLM call the user should opt into.
    let trending_summary = if db.get_bool("trending_daily", false) && !db.has_digest(&day_str(now)) {
        trending_digest(state, true).await
    } else {
        Value::Null
    };

    let note = if live {
        format!("Heartbeat (live): đã đăng {published}, lỗi {errors}. {}", plan.note)
    } else {
        format!("Heartbeat (draft): đã soạn {drafted} mục chờ duyệt. {}", plan.note)
    };
    db.log("heartbeat", &note, source, now).ok();
    json!({
        "harvest": harvest_summary,
        "trending": trending_summary,
        "ok": true,
        "mode": autonomy,
        "fetched": feed.len(),
        "considered": items.len(),
        "replies_to_you": priority.len(),
        "drafted": drafted,
        "published": published,
        "errors": errors,
        "note": note,
        "model": model,
    })
}

/// Draft-or-publish one planned engagement.
async fn apply(
    state: &Arc<AppState>,
    d: &DraftCreate,
    live: bool,
    now: i64,
    drafted: &mut usize,
    published: &mut usize,
    errors: &mut usize,
) {
    let db = &state.db;
    if live {
        // Persist a draft row first so we have something to record the result on.
        match db.create_draft(d, now) {
            Ok(id) => {
                if let Ok(Some(draft)) = db.get_draft(id) {
                    match execute_draft(state, &draft).await {
                        Ok(reference) => {
                            db.set_draft_result(id, "posted", &reference, "", now_ts()).ok();
                            db.log(&draft.kind, &format!("live: {}", describe_draft(&draft)), &reference, now_ts()).ok();
                            *published += 1;
                        }
                        Err(e) => {
                            db.set_draft_result(id, "error", "", &e, now_ts()).ok();
                            db.log("error", &format!("live {}: {e}", draft.kind), "", now_ts()).ok();
                            *errors += 1;
                        }
                    }
                }
            }
            Err(_) => *errors += 1,
        }
    } else {
        match db.create_draft(d, now) {
            Ok(id) => {
                db.log("draft", &format!("soạn {}: {}", d.kind, first_line(&draft_summary(d))), &id.to_string(), now).ok();
                *drafted += 1;
            }
            Err(_) => *errors += 1,
        }
    }
}

/// Read the human's steering list out of the DB: subjects to engage with, and
/// things they want posted/asked (least-recently-used first, so posting rotates
/// through the list instead of hammering the first idea).
fn build_steer(db: &crate::db::Db) -> llm::TopicSteer {
    let all = db.list_topics(true).unwrap_or_default();
    llm::TopicSteer {
        focus_only: db.topic_mode() == "focus",
        engage: all
            .iter()
            .filter(|t| t.kind == "engage" || t.kind == "both")
            .map(|t| t.text.clone())
            .collect(),
        ideas: all
            .iter()
            .filter(|t| t.kind == "post" || t.kind == "both")
            .map(|t| (t.id, t.text.clone()))
            .collect(),
    }
}

/// Topic string the memory recall + wiki search match on: what the feed (and the
/// people replying to you) are actually talking about this tick.
fn topics_of(items: &[FeedItem], priority: &[(String, String)]) -> String {
    let mut t: Vec<String> = priority
        .iter()
        .take(3)
        .map(|(_, s)| llm::truncate(s, 80))
        .filter(|s| !s.trim().is_empty())
        .collect();
    t.extend(items.iter().take(6).map(|i| i.title.clone()).filter(|s| !s.trim().is_empty()));
    t.join(" · ")
}

/// Execute a queued draft against Moltbook. THE single publish path (approve
/// button + live heartbeat both call this). Returns a reference id on success.
///
/// On success it also writes the molty's **trí nhớ** (what it actually said —
/// never what it merely drafted) and, when enabled, archives its own posts into
/// the **kho thông tin** (wiki).
pub async fn execute_draft(state: &Arc<AppState>, draft: &Draft) -> Result<String, String> {
    let result = execute_draft_inner(state, draft).await;
    if let Ok(reference) = &result {
        remember_published(&state.db, draft, reference).await;
        archive_own_post(&state.db, draft, reference).await;
        // Track it so later harvests can collect what other agents say about it.
        track_published_post(&state.db, draft, reference).await;
    }
    result
}

/// Save what we ACTUALLY published into the molty's knowledge space — its own
/// voice, so later heartbeats stay consistent and don't repeat themselves. Votes
/// / follows are skipped: they'd be memory noise.
async fn remember_published(db: &crate::db::Db, draft: &Draft, reference: &str) {
    if !db.get_bool("memory_enabled", true) {
        return;
    }
    let text = match draft.kind.as_str() {
        "post" => format!(
            "Trên Moltbook tôi đã đăng bài vào m/{}: \"{}\"\n{}",
            draft.submolt.trim_start_matches("m/"),
            draft.title,
            draft.content
        ),
        "comment" => format!(
            "Trên Moltbook tôi đã bình luận vào bài \"{}\" (post_id {}): {}",
            draft.target_title, draft.target_post_id, draft.content
        ),
        "submolt" => format!("Trên Moltbook tôi đã tạo submolt m/{}: {}", draft.submolt, draft.content),
        _ => return,
    };
    let space = crate::api::memory_space(db);
    match crate::senclaw::knowledge_save(&space, &text, &["moltbook"], &format!("moltbook:{reference}")).await {
        Ok(()) => {
            db.log("memory", &format!("đã ghi vào trí nhớ ({space})"), reference, now_ts()).ok();
        }
        Err(e) => {
            db.log("error", &format!("ghi trí nhớ thất bại: {e}"), reference, now_ts()).ok();
        }
    }
}

/// When `wiki_archive` is on, mirror a published post into the wiki so the
/// user's kho thông tin keeps a record of what the molty put into the world.
async fn archive_own_post(db: &crate::db::Db, draft: &Draft, reference: &str) {
    if draft.kind != "post" || !db.get_bool("wiki_enabled", true) || !db.get_bool("wiki_archive", false) {
        return;
    }
    let slug = crate::senclaw::slugify(&draft.title);
    let path = format!("moltbook/posts/{}.md", if slug.is_empty() { reference.to_string() } else { slug });
    let doc = format!(
        "# {}\n\n_Đăng bởi molty của tôi lên Moltbook m/{} · post_id `{}`_\n\n{}\n",
        draft.title,
        draft.submolt.trim_start_matches("m/"),
        reference,
        draft.content
    );
    match crate::senclaw::wiki_write(&path, &doc, &["moltbook", "post"], &format!("moltbook: lưu bài đã đăng '{}'", llm::truncate(&draft.title, 60))).await {
        Ok(()) => {
            db.log("wiki", &format!("đã lưu bài vào kho thông tin: {path}"), reference, now_ts()).ok();
        }
        Err(e) => {
            db.log("error", &format!("lưu wiki thất bại: {e}"), reference, now_ts()).ok();
        }
    }
}

// ---- trending digest: what the agent internet is talking about → wiki ----

/// Feeds sampled for a digest. `rising` surfaces what's climbing *now*, `hot`
/// what has traction, `top` what actually stuck — together they beat any single
/// sort at answering "what is being discussed".
const TRENDING_SORTS: [&str; 3] = ["hot", "rising", "top"];
/// Cap on posts handed to the model. Kept modest on purpose: the binding
/// constraint is the *response* size (a theme with why+takeaway is verbose), and
/// an over-long reply gets truncated at the token cap.
const TRENDING_MAX_POSTS: usize = 20;

/// `YYYY-MM-DD` (UTC) — the digest key.
pub fn day_str(secs: i64) -> String {
    let (y, m, d) = jd_to_ymd(secs.div_euclid(86400) + 2440588);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Sample the trending feeds, cluster them into themes, and write a dated
/// briefing into the wiki. Idempotent per day: re-running rewrites the same doc.
pub async fn trending_digest(state: &Arc<AppState>, write_wiki: bool) -> Value {
    let db = &state.db;
    let now = now_ts();
    let client = client(db);
    if !client.is_authenticated() {
        return json!({ "ok": false, "reason": "chưa kết nối agent Moltbook" });
    }

    // 1. Pull several sorts and merge, keeping the highest-scoring copy of each
    // post (the same post shows up in more than one feed).
    let mut seen: std::collections::HashMap<String, FeedItem> = std::collections::HashMap::new();
    let mut sources: Vec<String> = Vec::new();
    for sort in TRENDING_SORTS {
        match client.posts(sort, None, None).await {
            Ok(v) => {
                let items = extract_posts(&v);
                if !items.is_empty() {
                    sources.push(format!("{sort}({})", items.len()));
                }
                for it in items {
                    seen.entry(it.id.clone())
                        .and_modify(|e| {
                            if it.score > e.score {
                                e.score = it.score;
                            }
                        })
                        .or_insert(it);
                }
            }
            Err(e) => {
                db.log("error", &format!("trending {sort}: {e}"), "", now).ok();
            }
        }
    }
    let mut posts: Vec<FeedItem> = seen.into_values().collect();
    posts.sort_by(|a, b| b.score.cmp(&a.score));
    posts.truncate(TRENDING_MAX_POSTS);

    if posts.is_empty() {
        let msg = "Không lấy được bài nào từ Moltbook để tổng hợp xu hướng.";
        db.log("trending", msg, "", now).ok();
        return json!({ "ok": false, "reason": msg });
    }

    // 2. What does the user care about? Only used to flag relevance.
    let interests: Vec<String> = db
        .list_topics(true)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.kind == "engage" || t.kind == "both")
        .map(|t| t.text)
        .collect();

    let (report, model) = match llm::analyze_trending(&posts, &interests).await {
        Ok(v) => v,
        Err(e) => {
            db.log("error", &format!("phân tích xu hướng thất bại: {e}"), "", now).ok();
            return json!({ "ok": false, "reason": e });
        }
    };
    if report.topics.is_empty() {
        let msg = "LLM không rút ra được chủ đề nào từ feed.";
        db.log("trending", msg, "", now).ok();
        return json!({ "ok": false, "reason": msg, "posts": posts.len() });
    }

    // 3. Render the briefing.
    let day = day_str(now);
    let mut doc = format!("# Xu hướng Moltbook — {day}\n\n");
    if !report.summary.trim().is_empty() {
        doc.push_str(&format!("{}\n\n", report.summary.trim()));
    }
    doc.push_str("## Chủ đề nổi bật\n\n");
    for (i, t) in report.topics.iter().enumerate() {
        doc.push_str(&format!(
            "### {}. {}{}\n\n",
            i + 1,
            t.name,
            if t.relevant { "  ⭐ _(khớp chủ đề bạn quan tâm)_" } else { "" }
        ));
        if !t.why.trim().is_empty() {
            doc.push_str(&format!("**Vì sao nóng:** {}\n\n", t.why.trim()));
        }
        if !t.takeaway.trim().is_empty() {
            doc.push_str(&format!("**Điểm rút ra:** {}\n\n", t.takeaway.trim()));
        }
        if !t.posts.is_empty() {
            doc.push_str("Bài liên quan:\n");
            for idx in t.posts.iter().take(8) {
                if let Some(p) = posts.get(*idx) {
                    doc.push_str(&format!(
                        "- [{}]({}/post/{}) · {} · ⬆ {} · by {}\n",
                        p.title, crate::moltbook::DEFAULT_BASE, p.id, p.submolt, p.score, p.author
                    ));
                }
            }
            doc.push('\n');
        }
    }

    doc.push_str("## Bài nóng nhất\n\n| Điểm | Submolt | Bài | Tác giả |\n|---:|---|---|---|\n");
    for p in posts.iter().take(15) {
        let title = p.title.replace('|', "\\|");
        doc.push_str(&format!(
            "| {} | {} | [{}]({}/post/{}) | {} |\n",
            p.score, p.submolt, title, crate::moltbook::DEFAULT_BASE, p.id, p.author
        ));
    }
    doc.push_str(&format!(
        "\n---\n\n_Tổng hợp lúc {} · {} bài từ {} · mô hình {}_\n",
        fmt_ts(now),
        posts.len(),
        if sources.is_empty() { "feed".to_string() } else { sources.join(", ") },
        if model.is_empty() { "?".into() } else { model.clone() },
    ));

    // 4. Write the wiki doc (one per day, rewritten on re-run).
    let mut wiki_path = String::new();
    if write_wiki && db.get_bool("wiki_enabled", true) {
        let path = format!("moltbook/trending/{day}.md");
        match crate::senclaw::wiki_write(&path, &doc, &["moltbook", "trending"], &format!("moltbook: xu hướng {day}")).await {
            Ok(()) => {
                wiki_path = path.clone();
                db.log("wiki", &format!("ghi tổng hợp xu hướng: {path}"), "", now_ts()).ok();
            }
            Err(e) => {
                db.log("error", &format!("ghi wiki xu hướng thất bại: {e}"), "", now_ts()).ok();
            }
        }
    }

    // 5. Remember the gist, and record the digest.
    let names: Vec<String> = report.topics.iter().map(|t| t.name.clone()).collect();
    if db.get_bool("memory_enabled", true) {
        let memo = format!(
            "Xu hướng Moltbook ngày {day}: {}\nChủ đề: {}",
            report.summary.trim(),
            names.join(" · ")
        );
        let _ = crate::senclaw::knowledge_save(
            &crate::api::memory_space(db),
            &memo,
            &["moltbook", "trending"],
            &format!("moltbook:trending:{day}"),
        )
        .await;
    }
    db.upsert_digest(&day, &wiki_path, posts.len() as i64, names.len() as i64, &report.summary, &names, now_ts()).ok();

    let note = format!(
        "Xu hướng {day}: {} chủ đề từ {} bài{}.",
        names.len(),
        posts.len(),
        if wiki_path.is_empty() { String::new() } else { format!(" → {wiki_path}") }
    );
    db.log("trending", &note, "", now_ts()).ok();
    json!({
        "ok": true, "day": day, "posts": posts.len(), "topics": names.len(),
        "wiki_path": wiki_path, "summary": report.summary, "model": model, "note": note,
        "topic_list": report.topics.iter().map(|t| json!({
            "name": t.name, "why": t.why, "takeaway": t.takeaway,
            "relevant": t.relevant, "post_count": t.posts.len(),
        })).collect::<Vec<_>>(),
    })
}

// ---- feedback harvest: agent comments → synthesis → wiki doc ----

/// How many posts one harvest pass will check. Least-recently-checked first, so
/// successive passes rotate through everything without hammering the API.
const HARVEST_BATCH: i64 = 8;

/// Start tracking a post we just published, so later harvests can collect the
/// discussion on it.
async fn track_published_post(db: &crate::db::Db, draft: &Draft, post_id: &str) {
    if draft.kind != "post" || post_id.is_empty() || post_id == "ok" {
        return;
    }
    let _ = db.track_post(post_id, &draft.title, &draft.submolt, "", now_ts());
}

/// Does a harvested post actually belong to us?
///
/// `/home` activity includes posts we merely commented on, so auto-discovery
/// can surface another agent's thread. Claiming one as "my post" would write a
/// false statement into the user's wiki. When either name is unknown we keep the
/// post (a parsing quirk shouldn't silently drop a real post) — the check only
/// rejects on a positive mismatch.
fn is_our_post(me: &str, author: &str) -> bool {
    let (me, author) = (me.trim(), author.trim());
    me.is_empty() || author.is_empty() || author.eq_ignore_ascii_case(me)
}

/// Pull comments for a post as `(author, content)`.
fn extract_comments(v: &Value) -> Vec<(String, String)> {
    let arr = v
        .get("comments")
        .or_else(|| v.get("results"))
        .or_else(|| v.get("data"))
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();
    arr.iter()
        .filter_map(|c| {
            let author = c
                .get("author")
                .and_then(|a| a.get("name").and_then(|n| n.as_str()).or_else(|| a.as_str()))
                .or_else(|| c.get("author_name").and_then(|x| x.as_str()))
                .unwrap_or("molty")
                .to_string();
            let body = c
                .get("content")
                .or_else(|| c.get("body"))
                .or_else(|| c.get("text"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            (!body.is_empty()).then_some((author, body))
        })
        .collect()
}

/// Regenerate the wiki doc for one of our posts: the original text, the
/// synthesised feedback, the raw discussion, and the check metadata. Rewriting
/// the whole doc (rather than appending) keeps it idempotent — harvesting twice
/// can't stack duplicate sections.
async fn write_post_doc(
    post_id: &str,
    title: &str,
    submolt: &str,
    body: &str,
    synthesis: &str,
    comments: &[(String, String)],
    score: i64,
    checks: i64,
    existing_path: &str,
) -> Result<String, String> {
    let slug = crate::senclaw::slugify(title);
    let path = if existing_path.is_empty() {
        format!("moltbook/posts/{}.md", if slug.is_empty() { post_id.to_string() } else { slug })
    } else {
        existing_path.to_string()
    };

    let mut doc = format!(
        "# {title}\n\n_Molty của tôi đăng lên Moltbook m/{} · post_id `{post_id}`_\n\n{body}\n",
        submolt.trim_start_matches("m/")
    );
    if !synthesis.trim().is_empty() {
        doc.push_str(&format!("\n## Phản hồi từ các agent khác\n\n{}\n", synthesis.trim()));
    }
    if !comments.is_empty() {
        doc.push_str("\n## Thảo luận gốc\n\n");
        for (who, text) in comments.iter().take(40) {
            doc.push_str(&format!("- **{who}**: {text}\n"));
        }
    }
    // The check trail, so a human reading the doc knows how current it is.
    doc.push_str(&format!(
        "\n---\n\n_Cập nhật lúc {} · {} bình luận · {} điểm · đã kiểm tra {} lần_\n",
        fmt_ts(now_ts()),
        comments.len(),
        score,
        checks,
    ));

    crate::senclaw::wiki_write(
        &path,
        &doc,
        &["moltbook", "post"],
        &format!("moltbook: cập nhật phản hồi cho '{}'", llm::truncate(title, 60)),
    )
    .await?;
    Ok(path)
}

/// `YYYY-MM-DD HH:MM` (UTC) — good enough to stamp a doc.
fn fmt_ts(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    let (y, m, d) = jd_to_ymd(days + 2440588);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, (rem % 3600) / 60)
}
fn jd_to_ymd(jd: i64) -> (i64, i64, i64) {
    let a = jd + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    (100 * b + d - 4800 + m / 10, m + 3 - 12 * (m / 10), e - (153 * m + 2) / 5 + 1)
}

/// Collect what other agents said about our posts and refresh their wiki docs.
///
/// Only re-synthesises when the comment count actually grew since the last doc
/// write — an unchanged post costs one cheap GET and no LLM call. Pass
/// `only_post_id` to force one post (ignores the freshness check).
pub async fn harvest(state: &Arc<AppState>, only_post_id: Option<&str>) -> Value {
    let db = &state.db;
    let now = now_ts();
    let client = client(db);
    if !client.is_authenticated() {
        return json!({ "ok": false, "reason": "chưa kết nối agent Moltbook" });
    }

    // Auto-discover: any of our posts that people are replying to (per /home)
    // but that we aren't tracking yet — this backfills posts published before
    // tracking existed.
    let mut discovered = 0usize;
    if only_post_id.is_none() {
        if let Ok(home) = client.home().await {
            for (pid, _) in extract_home_activity(&home) {
                if !db.is_tracked(&pid) {
                    if db.track_post(&pid, "", "", "", now).is_ok() {
                        discovered += 1;
                    }
                }
            }
        }
    }

    let targets: Vec<crate::db::TrackedPost> = match only_post_id {
        Some(pid) => db.get_tracked(pid).ok().flatten().into_iter().collect(),
        None => db.list_tracked(HARVEST_BATCH).unwrap_or_default(),
    };
    if targets.is_empty() {
        let msg = if only_post_id.is_some() {
            "Bài này chưa được theo dõi.".to_string()
        } else {
            "Chưa có bài nào của bạn để thu thập phản hồi.".to_string()
        };
        return json!({ "ok": true, "checked": 0, "updated": 0, "discovered": discovered, "note": msg });
    }

    let wiki_on = db.get_bool("wiki_enabled", true);
    let mut checked = 0usize;
    let mut updated = 0usize;
    let mut errors = 0usize;
    let mut details: Vec<Value> = Vec::new();

    for t in targets {
        // 1. Current state of the post + its discussion.
        let post = match client.get_post(&t.post_id).await {
            Ok(p) => p,
            Err(e) => {
                db.record_check(&t.post_id, t.last_comment_count, t.last_score, &e.to_string(), now_ts()).ok();
                errors += 1;
                details.push(json!({ "post_id": t.post_id, "error": e.to_string() }));
                continue;
            }
        };
        let p = post.get("post").cloned().unwrap_or(post.clone());
        let title = p
            .get("title")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(&t.title)
            .to_string();
        let body = p.get("content").or_else(|| p.get("body")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let submolt = p
            .get("submolt_name")
            .or_else(|| p.get("submolt"))
            .and_then(|x| x.as_str())
            .unwrap_or(&t.submolt)
            .to_string();
        let score = p.get("score").or_else(|| p.get("upvotes")).and_then(|x| x.as_i64()).unwrap_or(t.last_score);

        // Only OUR OWN posts get a "my post" doc. `/home` activity also covers
        // posts we merely commented on, so auto-discovery can hand us someone
        // else's thread — writing "molty của tôi đăng" on that would put a false
        // claim in the user's wiki. Verify authorship and drop foreign posts.
        // (Use archive_post_to_wiki to save another agent's thread on purpose.)
        let author = p
            .get("author")
            .and_then(|a| a.get("name").and_then(|n| n.as_str()).or_else(|| a.as_str()))
            .or_else(|| p.get("author_name").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string();
        let me = db.get_str("agent_name", "");
        if !is_our_post(&me, &author) {
            db.untrack(&t.post_id).ok();
            db.log(
                "harvest",
                &format!("bỏ theo dõi bài '{}' — tác giả là {author}, không phải bạn", llm::truncate(&title, 50)),
                &t.post_id,
                now_ts(),
            )
            .ok();
            details.push(json!({
                "post_id": t.post_id, "title": title, "untracked": true,
                "reason": format!("không phải bài của bạn (tác giả: {author})"),
                "stale_doc": t.wiki_path,
            }));
            continue;
        }

        // Persist what we learned about the post (auto-discovery only knew the id).
        db.track_post(&t.post_id, &title, &submolt, "", t.posted_at).ok();

        let comments = match client.comments(&t.post_id, "best", None).await {
            Ok(c) => extract_comments(&c),
            Err(e) => {
                db.record_check(&t.post_id, t.last_comment_count, score, &e.to_string(), now_ts()).ok();
                errors += 1;
                details.push(json!({ "post_id": t.post_id, "error": e.to_string() }));
                continue;
            }
        };
        let count = comments.len() as i64;
        db.record_check(&t.post_id, count, score, "", now_ts()).ok();
        checked += 1;

        // 2. Nothing new → leave the doc alone (and skip the LLM).
        let stale = count > t.synced_comment_count;
        if !(stale || only_post_id.is_some()) || comments.is_empty() {
            details.push(json!({
                "post_id": t.post_id, "title": title, "comments": count,
                "updated": false, "reason": if comments.is_empty() { "chưa có bình luận" } else { "không có bình luận mới" },
            }));
            continue;
        }

        // 3. Synthesise what the other agents said.
        let (synthesis, model) = match llm::synthesize_feedback(&title, &body, &comments).await {
            Ok(v) => v,
            Err(e) => {
                db.log("error", &format!("tổng hợp phản hồi thất bại: {e}"), &t.post_id, now_ts()).ok();
                errors += 1;
                details.push(json!({ "post_id": t.post_id, "title": title, "error": e }));
                continue;
            }
        };

        // 4. Rewrite the wiki doc, then remember we did.
        let mut wiki_path = t.wiki_path.clone();
        if wiki_on {
            match write_post_doc(&t.post_id, &title, &submolt, &body, &synthesis, &comments, score, t.checks + 1, &t.wiki_path).await {
                Ok(p) => {
                    wiki_path = p;
                    db.log("wiki", &format!("cập nhật doc theo phản hồi: {wiki_path}"), &t.post_id, now_ts()).ok();
                }
                Err(e) => {
                    db.log("error", &format!("ghi wiki thất bại: {e}"), &t.post_id, now_ts()).ok();
                    errors += 1;
                }
            }
        }
        db.record_sync(&t.post_id, &synthesis, count, &wiki_path, now_ts()).ok();
        if db.get_bool("memory_enabled", true) {
            let memo = format!(
                "Phản hồi của các agent khác về bài Moltbook \"{title}\" ({count} bình luận):\n{synthesis}"
            );
            let _ = crate::senclaw::knowledge_save(
                &crate::api::memory_space(db),
                &memo,
                &["moltbook", "feedback"],
                &format!("moltbook:feedback:{}", t.post_id),
            )
            .await;
        }
        updated += 1;
        details.push(json!({
            "post_id": t.post_id, "title": title, "comments": count,
            "updated": true, "wiki_path": wiki_path, "model": model,
            "synthesis": llm::truncate(&synthesis, 400),
        }));
    }

    let note = format!(
        "Thu thập phản hồi: kiểm tra {checked} bài, cập nhật doc {updated}{}{}.",
        if discovered > 0 { format!(", phát hiện thêm {discovered} bài") } else { String::new() },
        if errors > 0 { format!(", {errors} lỗi") } else { String::new() },
    );
    db.log("harvest", &note, "", now_ts()).ok();
    json!({
        "ok": true, "checked": checked, "updated": updated,
        "discovered": discovered, "errors": errors, "note": note, "details": details,
    })
}

/// Archive ANY Moltbook post (usually someone else's good thread) into the wiki —
/// the "kho thông tin" side of the integration. Returns the wiki path written.
pub async fn archive_post_to_wiki(state: &Arc<AppState>, post_id: &str) -> Result<String, String> {
    let db = &state.db;
    let client = client(db);
    if !client.is_authenticated() {
        return Err("chưa kết nối agent Moltbook".into());
    }
    let post = client.get_post(post_id).await.map_err(|e| e.to_string())?;
    let p = post.get("post").cloned().unwrap_or(post.clone());
    let title = p.get("title").and_then(|x| x.as_str()).unwrap_or("(không tiêu đề)");
    let content = p.get("content").or_else(|| p.get("body")).and_then(|x| x.as_str()).unwrap_or("");
    let author = p
        .get("author")
        .and_then(|a| a.get("name").and_then(|n| n.as_str()).or_else(|| a.as_str()))
        .unwrap_or("unknown");
    let submolt = p
        .get("submolt_name")
        .or_else(|| p.get("submolt"))
        .and_then(|x| x.as_str())
        .unwrap_or("general");

    let mut doc = format!(
        "# {title}\n\n_Từ Moltbook m/{} · bởi {author} · post_id `{post_id}`_\n\n{content}\n",
        submolt.trim_start_matches("m/")
    );
    // Include the discussion — that's usually where the value is.
    if let Ok(cs) = client.comments(post_id, "best", None).await {
        let arr = cs.get("comments").and_then(|c| c.as_array()).cloned().unwrap_or_default();
        if !arr.is_empty() {
            doc.push_str("\n## Thảo luận\n\n");
            for c in arr.iter().take(20) {
                let who = c
                    .get("author")
                    .and_then(|a| a.get("name").and_then(|n| n.as_str()).or_else(|| a.as_str()))
                    .unwrap_or("molty");
                let body = c.get("content").or_else(|| c.get("body")).and_then(|x| x.as_str()).unwrap_or("");
                if !body.trim().is_empty() {
                    doc.push_str(&format!("- **{who}**: {body}\n"));
                }
            }
        }
    }

    let slug = crate::senclaw::slugify(title);
    let path = format!("moltbook/{}.md", if slug.is_empty() { post_id.to_string() } else { slug });
    crate::senclaw::wiki_write(&path, &doc, &["moltbook"], &format!("moltbook: lưu thảo luận '{}'", llm::truncate(title, 60))).await?;
    db.log("wiki", &format!("đã lưu vào kho thông tin: {path}"), post_id, now_ts()).ok();

    // Remember that we archived it, so the molty can refer back to it later.
    if db.get_bool("memory_enabled", true) {
        let memo = format!("Tôi đã lưu thảo luận Moltbook \"{title}\" (bởi {author}, m/{submolt}) vào wiki tại {path}.");
        let _ = crate::senclaw::knowledge_save(&crate::api::memory_space(db), &memo, &["moltbook", "wiki"], "moltbook:archive").await;
    }
    Ok(path)
}

async fn execute_draft_inner(state: &Arc<AppState>, draft: &Draft) -> Result<String, String> {
    let db = &state.db;
    let client = client(db);
    if !client.is_authenticated() {
        return Err("chưa cấu hình API key".into());
    }
    match draft.kind.as_str() {
        "vote" => {
            let v = if draft.vote_dir == "down" {
                client.downvote_post(&draft.target_post_id).await
            } else {
                client.upvote_post(&draft.target_post_id).await
            };
            v.map(|_| draft.target_post_id.clone()).map_err(|e| e.to_string())
        }
        "comment" => {
            let parent = if draft.parent_id.is_empty() { None } else { Some(draft.parent_id.as_str()) };
            let v = client
                .create_comment(&draft.target_post_id, &draft.content, parent)
                .await
                .map_err(|e| e.to_string())?;
            Ok(extract_id(&v, "comment").unwrap_or_else(|| draft.target_post_id.clone()))
        }
        "post" => {
            let submolt = if draft.submolt.is_empty() { db.get_str("default_submolt", "general") } else { draft.submolt.clone() };
            let url = if draft.url.is_empty() { None } else { Some(draft.url.as_str()) };
            let reference = create_post_verified(&client, db, &submolt, &draft.title, &draft.content, url).await?;
            db.set_i64("last_post_at", now_ts()).ok();
            Ok(reference)
        }
        "submolt" => {
            let name = if draft.submolt.is_empty() { draft.target_name.clone() } else { draft.submolt.clone() };
            client
                .create_submolt(&name, &draft.title, &draft.content, false)
                .await
                .map(|_| name)
                .map_err(|e| e.to_string())
        }
        "follow" => client.follow(&draft.target_name).await.map(|_| draft.target_name.clone()).map_err(|e| e.to_string()),
        "subscribe" => client.subscribe(&draft.target_name).await.map(|_| draft.target_name.clone()).map_err(|e| e.to_string()),
        other => Err(format!("loại nháp không hỗ trợ: {other}")),
    }
}

/// Create a post and, if Moltbook demands the anti-human math challenge, solve
/// it via the daemon LLM and submit the answer.
async fn create_post_verified(
    client: &Moltbook,
    db: &crate::db::Db,
    submolt: &str,
    title: &str,
    content: &str,
    url: Option<&str>,
) -> Result<String, String> {
    let resp = client.create_post(submolt, title, content, url, "text").await.map_err(|e| e.to_string())?;
    let post = resp.get("post").cloned().unwrap_or(resp.clone());
    let post_id = extract_id(&post, "post").unwrap_or_default();

    let status = post.get("verification_status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "pending" {
        let verification = post.get("verification").cloned().unwrap_or_default();
        let code = verification.get("verification_code").and_then(|s| s.as_str()).unwrap_or("");
        let challenge = verification.get("challenge_text").and_then(|s| s.as_str()).unwrap_or("");
        if code.is_empty() || challenge.is_empty() {
            return Err("bài cần xác minh nhưng thiếu challenge — thử lại sau".into());
        }
        let (answer, _model) = llm::solve_challenge(challenge).await.map_err(|e| format!("giải challenge thất bại: {e}"))?;
        client.verify(code, &answer).await.map_err(|e| format!("nộp đáp án challenge thất bại: {e}"))?;
        db.log("verify", &format!("giải challenge xác minh cho bài '{}' → {}", llm::truncate(title, 60), answer), &post_id, now_ts()).ok();
    }
    Ok(if post_id.is_empty() { "ok".into() } else { post_id })
}

// ---- feed fetch + shaping ----

async fn fetch_feed(client: &Moltbook) -> Result<Vec<FeedItem>, String> {
    // Personalised feed first; fall back to the global hot feed.
    let v = match client.feed("hot", "all", None).await {
        Ok(v) => v,
        Err(e1) => client.posts("hot", None, None).await.map_err(|e2| format!("{e1}; fallback: {e2}"))?,
    };
    Ok(extract_posts(&v))
}

/// Pull the posts array out of any of the feed response shapes Moltbook uses.
pub fn extract_posts(v: &Value) -> Vec<FeedItem> {
    let arr = v
        .get("posts")
        .or_else(|| v.get("results"))
        .or_else(|| v.get("data"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    arr.iter().filter_map(post_item).collect()
}

/// Pull `(post_id, snippet)` pairs from `/home`'s "activity on your posts"
/// section — the replies/comments other molties left on YOUR posts. Tolerant of
/// the exact shape (snake/camelCase, nested `post`), so a newer/older Moltbook
/// response still yields the priority list.
pub fn extract_home_activity(home: &Value) -> Vec<(String, String)> {
    let arr = home
        .get("activity_on_your_posts")
        .or_else(|| home.get("activityOnYourPosts"))
        .or_else(|| home.get("activity"))
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for it in arr.iter().take(10) {
        let pid = it
            .get("post_id")
            .or_else(|| it.get("postId"))
            .or_else(|| it.get("post").and_then(|p| p.get("id")))
            .or_else(|| it.get("id"))
            .and_then(|x| x.as_str().map(String::from).or_else(|| x.as_i64().map(|n| n.to_string())));
        let snippet = it
            .get("content")
            .or_else(|| it.get("comment"))
            .or_else(|| it.get("text"))
            .or_else(|| it.get("body"))
            .or_else(|| it.get("title"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(pid) = pid.filter(|p| !p.is_empty()) {
            out.push((pid, snippet));
        }
    }
    out
}

fn post_item(p: &Value) -> Option<FeedItem> {
    let id = p
        .get("id")
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))?;
    let submolt = p
        .get("submolt_name")
        .or_else(|| p.get("submolt"))
        .and_then(|v| v.as_str())
        .map(|s| if s.starts_with("m/") { s.to_string() } else { format!("m/{s}") })
        .unwrap_or_else(|| "m/general".into());
    let author = p
        .get("author")
        .and_then(|a| a.get("name").and_then(|n| n.as_str()).or_else(|| a.as_str()))
        .or_else(|| p.get("author_name").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();
    let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let content = p
        .get("content")
        .or_else(|| p.get("body"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let score = p.get("score").or_else(|| p.get("upvotes")).and_then(|v| v.as_i64()).unwrap_or(0);
    Some(FeedItem { id, submolt, author, title, content, score })
}

fn to_cache(f: &FeedItem, now: i64) -> CachedPost {
    CachedPost {
        post_id: f.id.clone(),
        submolt: f.submolt.clone(),
        author: f.author.clone(),
        title: f.title.clone(),
        content: f.content.clone(),
        url: String::new(),
        score: f.score,
        comment_count: 0,
        posted_at: now,
        cached_at: now,
        demo: false,
    }
}

fn clone_item(f: &FeedItem) -> FeedItem {
    FeedItem {
        id: f.id.clone(),
        submolt: f.submolt.clone(),
        author: f.author.clone(),
        title: f.title.clone(),
        content: f.content.clone(),
        score: f.score,
    }
}

/// Best-effort id extraction from a create response.
fn extract_id(v: &Value, nested: &str) -> Option<String> {
    v.get("id")
        .or_else(|| v.get(nested).and_then(|n| n.get("id")))
        .and_then(|x| x.as_str().map(String::from).or_else(|| x.as_i64().map(|n| n.to_string())))
}

fn describe_draft(d: &Draft) -> String {
    match d.kind.as_str() {
        "post" => format!("bài '{}' vào m/{}", llm::truncate(&d.title, 60), d.submolt.trim_start_matches("m/")),
        "comment" => format!("bình luận trên '{}'", llm::truncate(&d.target_title, 50)),
        "vote" => format!("{}vote '{}'", if d.vote_dir == "down" { "down" } else { "up" }, llm::truncate(&d.target_title, 50)),
        "submolt" => format!("submolt m/{}", d.submolt),
        "follow" => format!("theo dõi {}", d.target_name),
        "subscribe" => format!("đăng ký m/{}", d.target_name),
        k => k.to_string(),
    }
}

fn draft_summary(d: &DraftCreate) -> String {
    match d.kind.as_str() {
        "post" => format!("{} — {}", d.title, d.content),
        "comment" => d.content.clone(),
        "vote" => d.target_title.clone(),
        _ => d.target_name.clone(),
    }
}

fn first_line(s: &str) -> String {
    llm::truncate(s.lines().next().unwrap_or("").trim(), 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_posts_handles_moltbook_shape() {
        let v = json!({
            "posts": [
                { "id": "p1", "submolt_name": "existential", "author": { "name": "molty-a" },
                  "title": "hi", "content": "world", "score": 9 },
                { "id": 2, "submolt": "m/general", "author_name": "molty-b", "title": "t2", "body": "b2", "upvotes": 3 }
            ]
        });
        let items = extract_posts(&v);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "p1");
        assert_eq!(items[0].submolt, "m/existential");
        assert_eq!(items[0].author, "molty-a");
        assert_eq!(items[1].id, "2");
        assert_eq!(items[1].submolt, "m/general");
        assert_eq!(items[1].author, "molty-b");
        assert_eq!(items[1].content, "b2");
        assert_eq!(items[1].score, 3);
    }

    #[test]
    fn extract_id_from_nested_and_flat() {
        assert_eq!(extract_id(&json!({ "id": "x" }), "post"), Some("x".into()));
        assert_eq!(extract_id(&json!({ "post": { "id": 7 } }), "post"), Some("7".into()));
        assert_eq!(extract_id(&json!({ "nope": 1 }), "post"), None);
    }

    /// Regression: auto-discovery from /home surfaced posts we only COMMENTED
    /// on, and we wrote wiki docs claiming "molty của tôi đăng" on another
    /// agent's thread. Only a positive author mismatch may reject.
    #[test]
    fn only_our_own_posts_are_treated_as_ours() {
        assert!(is_our_post("SenClawAgent", "SenClawAgent"));
        assert!(is_our_post("SenClawAgent", "senclawagent")); // case-insensitive
        assert!(is_our_post("SenClawAgent", " SenClawAgent ")); // whitespace
        // The real bug: someone else's post must be rejected.
        assert!(!is_our_post("SenClawAgent", "hermesmolt_1782793439"));
        assert!(!is_our_post("SenClawAgent", "rossum"));
        // Unknown either side → keep (don't drop a real post on a parse quirk).
        assert!(is_our_post("", "whoever"));
        assert!(is_our_post("SenClawAgent", ""));
    }

    #[test]
    fn extract_comments_handles_shapes_and_skips_empty() {
        let v = json!({ "comments": [
            { "author": { "name": "molty-a" }, "content": "điểm hay" },
            { "author_name": "molty-b", "body": "phản biện" },
            { "author": "molty-c", "text": "   " },
            { "author": { "name": "molty-d" }, "content": "" }
        ]});
        let c = extract_comments(&v);
        assert_eq!(c.len(), 2, "blank comments must be skipped");
        assert_eq!(c[0], ("molty-a".to_string(), "điểm hay".to_string()));
        assert_eq!(c[1], ("molty-b".to_string(), "phản biện".to_string()));
    }

    #[test]
    fn extract_home_activity_tolerant() {
        let h = json!({
            "activity_on_your_posts": [
                { "post_id": "p1", "content": "great point, but what about X?" },
                { "post": { "id": 2 }, "comment": "disagree" },
                { "id": "p3", "text": "nice" },
                { "nope": true }
            ]
        });
        let a = extract_home_activity(&h);
        assert_eq!(a.len(), 3);
        assert_eq!(a[0], ("p1".to_string(), "great point, but what about X?".to_string()));
        assert_eq!(a[1].0, "2");
        assert_eq!(a[1].1, "disagree");
        assert_eq!(a[2].0, "p3");
    }
}
