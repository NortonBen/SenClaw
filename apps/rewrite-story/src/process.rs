//! The async rewrite pipeline — port of the Go `service/process` package
//! (`manager.go` + `job_rewrite.go`).
//!
//! Shape of the thing: `POST /api/processes` only enqueues. A background poller
//! claims queued rows, and each claimed row runs as its own task that rewrites
//! the story chunk by chunk, **persisting every finished chunk as it goes**.
//! That persistence is the whole design: retry resumes from the last completed
//! chunk instead of restarting a job that may already have burned an hour of
//! model time.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::db::{stage, status, Db};
use crate::llm::{self, RewriteParams};
use crate::state::{cancel::CancelToken, Core};
use crate::text;

/// How often the poller looks for queued work.
const QUEUE_POLL: Duration = Duration::from_secs(5);
/// How often the watchdog sweeps for stuck or stale processes.
const WATCHDOG_POLL: Duration = Duration::from_secs(60);
/// A `processing` row untouched for this long is presumed dead.
const STUCK_AFTER_MINUTES: i64 = 60;
/// A `queued` row this old is presumed abandoned.
const STALE_QUEUE_AFTER_HOURS: i64 = 24;

/// Fail anything left `processing` by a crash or restart. Runs once at boot,
/// before the poller starts, so a stale row can't occupy a concurrency slot.
pub fn reconcile_orphans(db: &Db) {
    match db.reconcile_orphans("Ứng dụng khởi động lại, tiến trình bị gián đoạn")
    {
        Ok(n) if n > 0 => println!("[process] reset {n} orphaned process(es) to failed"),
        Ok(_) => {}
        Err(e) => eprintln!("[process] orphan reconcile failed: {e}"),
    }
}

/// Start the queue poller and the watchdog.
pub fn spawn(core: Arc<Core>) {
    let poller = core.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(QUEUE_POLL).await;
            check_queue(&poller).await;
        }
    });

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(WATCHDOG_POLL).await;
            watchdog(&core);
        }
    });
}

async fn check_queue(core: &Arc<Core>) {
    let max_concurrent = core.db.setting_i64("max_concurrent_processes", 2).max(1);
    let running = core.running_count() as i64;
    let slots = max_concurrent - running;
    if slots <= 0 {
        return;
    }

    let pending = match core.db.pending_processes(slots) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[process] queue poll failed: {e}");
            return;
        }
    };

    for p in pending {
        // Reserve the in-process slot BEFORE claiming the row. A cancelled job
        // can still be blocked in a model call, and starting a second task for
        // it would give one process two workers. The guard releases the slot on
        // drop, including when the worker panics.
        let Some(guard) = core.job_guard(p.id) else {
            continue;
        };
        // The claim is atomic; if it loses, someone else took the row.
        match core.db.claim_process(p.id) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                eprintln!("[process] claim {} failed: {e}", p.id);
                continue;
            }
        }
        let core = core.clone();
        tokio::spawn(async move {
            let guard = guard;
            run_rewrite_job(core, p.id, guard.token().clone()).await;
        });
    }
}

fn watchdog(core: &Arc<Core>) {
    let db = &core.db;

    let stuck_cutoff = sql_datetime_ago_minutes(STUCK_AFTER_MINUTES);
    if let Ok(ids) = db.stale_processes(status::PROCESSING, "updated_at", &stuck_cutoff) {
        for id in ids {
            // Signal the task first — if it is alive but wedged in a request,
            // this stops it from writing after we mark it failed.
            core.cancel_job(id);
            let _ = db.update_progress(
                id,
                status::FAILED,
                stage::FAILED,
                0,
                0,
                0,
                Some(&format!(
                    "Tiến trình bị treo quá thời gian cho phép ({STUCK_AFTER_MINUTES} phút)"
                )),
                None,
            );
            emit_process(core, dashws_event_for(status::FAILED), id);
        }
    }

    // Swept on `updated_at`, not `created_at`: a retry re-queues the row without
    // minting a new one, so an old process the user just clicked "Chạy tiếp" on
    // would otherwise be killed within the minute for having been "queued over
    // 24h" — which is no longer true. `requeue_process` touches `updated_at`.
    let stale_cutoff = sql_datetime_ago_minutes(STALE_QUEUE_AFTER_HOURS * 60);
    if let Ok(ids) = db.stale_processes(status::QUEUED, "updated_at", &stale_cutoff) {
        for id in ids {
            let _ = db.update_progress(
                id,
                status::FAILED,
                stage::FAILED,
                0,
                0,
                0,
                Some(&format!(
                    "Tiến trình nằm trong hàng chờ quá {STALE_QUEUE_AFTER_HOURS} giờ"
                )),
                None,
            );
            emit_process(core, dashws_event_for(status::FAILED), id);
        }
    }
}

fn sql_datetime_ago_minutes(minutes: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(minutes))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

/// Progress percentage for a stage.
///
/// The Go original computed this as `((currentChunk-1)+subProgress)/total`,
/// which goes negative whenever it is called with `currentChunk = 0` — which the
/// pending and analyzing stages both do. Rewritten here as an explicit band per
/// stage: rewriting occupies 15-85%, everything before it is the first 15%, and
/// assembly/saving is the last 15%.
fn progress_for(stage_name: &str, done: i64, total: i64) -> i64 {
    match stage_name {
        stage::PENDING => 0,
        stage::ANALYZING => 5,
        stage::REWRITING => {
            if total <= 0 {
                15
            } else {
                15 + (70 * done.clamp(0, total)) / total
            }
        }
        stage::SAVING => 90,
        stage::COMPLETED => 100,
        _ => 0,
    }
}

fn dashws_event_for(status_name: &str) -> &'static str {
    use crate::dashws::event;
    match status_name {
        status::COMPLETED => event::PROCESS_COMPLETE,
        status::FAILED => event::PROCESS_FAILED,
        status::CANCELLED => event::PROCESS_CANCELLED,
        _ => event::PROCESS_UPDATE,
    }
}

/// Push the current process row to the UI.
fn emit_process(core: &Arc<Core>, event_name: &str, process_id: i64) {
    if let Ok(Some(p)) = core.db.get_process(process_id) {
        core.dash.emit(event_name, json!(p));
    }
}

/// Outcome of a worker progress write.
enum Report {
    /// Written; keep going.
    Applied,
    /// The DB refused it: the process reached a terminal state, or is no longer
    /// `processing` because it was cancelled and re-queued while this task was
    /// in flight. This task no longer owns the process and must stop quietly.
    Superseded,
    /// The write itself errored. Distinct from `Superseded` — treating a
    /// transient DB fault as supersession abandons the job at `processing` with
    /// no error message, leaving a live-looking row for the watchdog to
    /// mislabel an hour later.
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
fn report(
    core: &Arc<Core>,
    id: i64,
    status_name: &str,
    stage_name: &str,
    done: i64,
    total: i64,
    error: Option<&str>,
    result_story_id: Option<i64>,
) -> Report {
    let progress = if status_name == status::COMPLETED {
        100
    } else {
        progress_for(stage_name, done, total)
    };
    match core.db.update_progress_guarded(
        id,
        status_name,
        stage_name,
        progress,
        done,
        total,
        error,
        result_story_id,
        true,
    ) {
        Ok(true) => {
            emit_process(core, dashws_event_for(status_name), id);
            Report::Applied
        }
        Ok(false) => Report::Superseded,
        Err(e) => Report::Failed(e.to_string()),
    }
}

fn fail(core: &Arc<Core>, id: i64, message: &str) {
    eprintln!("[process {id}] failed: {message}");
    report(
        core,
        id,
        status::FAILED,
        stage::FAILED,
        0,
        0,
        Some(message),
        None,
    );
}

fn cancelled(core: &Arc<Core>, id: i64) {
    report(
        core,
        id,
        status::CANCELLED,
        stage::CANCELLED,
        0,
        0,
        Some("Bị hủy bởi người dùng"),
        None,
    );
}

/// Source chunks for a story, splitting and caching them on first use.
///
/// Chunks are keyed by story, not by process, so every rewrite run of the same
/// story reuses the same split — which is what makes chunk indices stable enough
/// for resume to mean anything.
///
/// The source text is fetched lazily: on every run after the first, the split is
/// already cached and there is no reason to pull a multi-million-character
/// column out of SQLite only to drop it.
fn get_or_generate_chunks(db: &Db, story_id: i64) -> anyhow::Result<Vec<String>> {
    let existing = db.get_chunks(story_id)?;
    if !existing.is_empty() {
        return Ok(existing);
    }

    let text_body = db
        .story_text(story_id)?
        .ok_or_else(|| anyhow::anyhow!("không tìm thấy truyện {story_id}"))?;

    let mut min_size = db
        .setting_i64(
            "hybrid_split_min_size",
            (crate::llm::MAX_CHUNK_CHARS as i64) * 3 / 5,
        )
        .max(1) as usize;
    let mut max_size = db
        .setting_i64("hybrid_split_max_size", crate::llm::MAX_CHUNK_CHARS as i64)
        .max(1) as usize;
    if min_size > max_size {
        std::mem::swap(&mut min_size, &mut max_size);
    }
    let threshold = db
        .setting_f64("hybrid_split_threshold", 0.2)
        .clamp(0.0, 1.0);

    let chunks = text::hybrid_split(&text_body, min_size, max_size, threshold);
    db.save_chunks(story_id, &chunks)?;
    Ok(chunks)
}

/// Continuity hint handed to chunk `i`: the tail of chunk `i-1`.
///
/// Prefers the *rewritten* tail, which is what keeps the prose seamless. When
/// chunk `i-1` is a sibling still in flight in the same batch, falls back to the
/// tail of the **source** text — the model still gets the narrative bridge, just
/// not the new phrasing. That is the whole trade `parallel_chunks` makes; at the
/// default of 1 the fallback is unreachable and output is bit-identical to the
/// strictly sequential path.
fn tail_for(i: i64, tails: &std::collections::HashMap<i64, String>, chunks: &[String]) -> String {
    if i == 0 {
        return String::new();
    }
    tails
        .get(&(i - 1))
        .cloned()
        .unwrap_or_else(|| text::continuity_tail(&chunks[(i - 1) as usize]))
}

async fn run_rewrite_job(core: Arc<Core>, id: i64, token: CancelToken) {
    let started = std::time::Instant::now();

    let Ok(Some(proc)) = core.db.get_process(id) else {
        eprintln!("[process {id}] vanished before it could start");
        return;
    };
    let story_id = proc.story_id;
    let Ok(Some(story_name)) = core.db.story_name(story_id) else {
        fail(&core, id, "Không tìm thấy truyện nguồn");
        return;
    };

    report(
        &core,
        id,
        status::PROCESSING,
        stage::ANALYZING,
        0,
        0,
        None,
        None,
    );

    // ---- split ----
    let chunks = match get_or_generate_chunks(&core.db, story_id) {
        Ok(c) => c,
        Err(e) => return fail(&core, id, &format!("Lỗi tách chunk truyện: {e}")),
    };
    let total = chunks.len() as i64;
    if total == 0 {
        return fail(
            &core,
            id,
            "Không có nội dung để viết lại (truyện rỗng hoặc lỗi tách chunk)",
        );
    }

    // ---- resume ----
    // This one query decides whether hours of finished model work get reused or
    // silently redone. Defaulting it to "nothing completed" on error would make
    // a transient SQLITE_BUSY restart the whole novel — and `INSERT OR REPLACE`
    // would then overwrite the evidence. It must be fatal.
    let completed: std::collections::HashSet<i64> = match core.db.rewritten_indices(id) {
        Ok(v) => v.into_iter().collect(),
        Err(e) => {
            return fail(
                &core,
                id,
                &format!("Không đọc được tiến độ đã lưu, dừng để tránh viết lại từ đầu: {e}"),
            )
        }
    };

    // Log where an interrupted run left off. Only the contiguous prefix counts
    // as "resumed from"; chunks saved past a gap are still reused, they just
    // don't move this marker.
    let resume_from = (0..total).take_while(|i| completed.contains(i)).count() as i64;
    if resume_from > 0 {
        println!(
            "[process {id}] resuming at chunk {}/{total}",
            resume_from + 1
        );
    }

    let system_instruction = proc
        .system_instruction
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(llm::DEFAULT_SYSTEM_INSTRUCTION)
        .to_string();
    let target_style = proc.version_plan.clone().unwrap_or_default();
    let extra = proc.user_prompt.clone().unwrap_or_default();

    let batch_size = core.db.setting_i64("parallel_chunks", 1).clamp(1, 8) as usize;
    let max_output_tokens = core
        .db
        .setting_i64("max_output_tokens", llm::DEFAULT_MAX_OUTPUT_TOKENS as i64)
        .clamp(2048, 200_000) as u32;

    // Indices still to do, in order.
    let pending: Vec<i64> = (0..total).filter(|i| !completed.contains(i)).collect();

    // Continuity tails by chunk index. Only the finished chunks that actually
    // precede pending work get loaded — normally just the one at the resume
    // boundary. Holding every completed chunk here would keep the entire
    // rewritten novel resident for the whole run.
    let mut tails: std::collections::HashMap<i64, String> = std::collections::HashMap::new();
    for &p in &pending {
        let prev = p - 1;
        if p > 0 && completed.contains(&prev) && !tails.contains_key(&prev) {
            match core.db.rewritten_chunk(id, prev) {
                Ok(Some(t)) => {
                    tails.insert(prev, text::continuity_tail(&t));
                }
                Ok(None) => {}
                Err(e) => return fail(&core, id, &format!("Lỗi đọc chunk {}: {e}", prev + 1)),
            }
        }
    }

    // ---- rewrite loop ----
    for batch in pending.chunks(batch_size) {
        if token.is_cancelled() {
            return cancelled(&core, id);
        }

        let first = batch[0];
        match report(
            &core,
            id,
            status::PROCESSING,
            stage::REWRITING,
            first,
            total,
            None,
            None,
        ) {
            Report::Applied => {}
            Report::Superseded => return,
            Report::Failed(e) => return fail(&core, id, &format!("Lỗi cập nhật tiến độ: {e}")),
        }

        // Fire the whole batch at once. With batch_size = 1 this is exactly the
        // sequential behaviour; the futures only interleave at their await
        // points, so the shared DB mutex is never held across one.
        let results = futures_util::future::join_all(batch.iter().map(|&i| {
            let previous = tail_for(i, &tails, &chunks);
            let system = &system_instruction;
            let source = &chunks[i as usize];
            let style = &target_style;
            let extra = &extra;
            async move {
                let params = RewriteParams {
                    target_style: style,
                    additional_requirements: extra,
                    previous_chunk_paragraph: &previous,
                    target_language: "Vietnamese",
                    creativity_ratio: proc.creativity_ratio,
                    target_length_variance: proc.target_length_variance,
                };
                (
                    i,
                    llm::rewrite_chunk(system, source, &params, max_output_tokens).await,
                )
            }
        }))
        .await;

        // Persist every success in the batch BEFORE acting on any failure.
        // Bailing at the first error would discard siblings that already
        // completed and were already paid for, and the next retry would redo
        // them — the exact waste chunk-level persistence exists to prevent.
        let mut first_error: Option<String> = None;
        for (i, result) in results {
            let rewritten = match result {
                Ok(t) => t,
                Err(e) => {
                    first_error.get_or_insert(format!("Lỗi LLM tại phần {}: {e}", i + 1));
                    continue;
                }
            };

            if let Err(e) = core
                .db
                .save_rewrite_chunk(id, i, &chunks[i as usize], &rewritten)
            {
                // Go logged this and carried on, which silently broke resume for
                // the index — the gap scan would stop there forever. Fatal here.
                first_error.get_or_insert(format!("Lỗi lưu chunk {}: {e}", i + 1));
                continue;
            }

            tails.insert(i, text::continuity_tail(&rewritten));
            // A short preview, not the chunk. Shipping the full rewritten text
            // pushed the entire novel through a 256-slot broadcast channel to
            // every connected browser, and the UI renders none of it; the text
            // is one paginated fetch away if a client ever wants it.
            core.dash.emit(
                crate::dashws::event::PROCESS_DELTA,
                json!({
                    "process_id": id,
                    "story_id": story_id,
                    "chunk_index": i,
                    "total_chunks": total,
                    "length": rewritten.chars().count(),
                    "preview": rewritten.chars().take(160).collect::<String>(),
                }),
            );
        }

        if let Some(message) = first_error {
            if token.is_cancelled() {
                return cancelled(&core, id);
            }
            return fail(&core, id, &message);
        }

        let done_through = batch[batch.len() - 1] + 1;
        match report(
            &core,
            id,
            status::PROCESSING,
            stage::REWRITING,
            done_through,
            total,
            None,
            None,
        ) {
            Report::Applied => {}
            Report::Superseded => return,
            Report::Failed(e) => return fail(&core, id, &format!("Lỗi cập nhật tiến độ: {e}")),
        }
    }

    if token.is_cancelled() {
        return cancelled(&core, id);
    }

    // ---- assemble & save ----
    report(
        &core,
        id,
        status::PROCESSING,
        stage::SAVING,
        total,
        total,
        None,
        None,
    );

    // Reassembled from the DB rather than from memory, so a resumed run and a
    // straight-through run produce byte-identical output.
    let full = match core.db.assemble_rewrite(id, total) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            return fail(
                &core,
                id,
                "Nội dung viết lại rỗng, không thể lưu truyện mới",
            )
        }
        Err(e) => return fail(&core, id, &format!("Lỗi ghép nội dung: {e}")),
    };

    let version = match core.db.next_version_number(story_id) {
        Ok(v) => v,
        // Falling back to 1 here would quietly mint a second "version 1" under
        // the same parent; nothing in the schema stops it.
        Err(e) => return fail(&core, id, &format!("Lỗi xác định số phiên bản: {e}")),
    };
    let new_story_id = match core.db.create_version(
        story_id,
        &story_name,
        &full,
        version,
        proc.creativity_ratio,
        proc.target_length_variance,
        started.elapsed().as_secs_f64(),
    ) {
        Ok(sid) => sid,
        Err(e) => return fail(&core, id, &format!("Lỗi lưu truyện mới: {e}")),
    };

    report(
        &core,
        id,
        status::COMPLETED,
        stage::COMPLETED,
        total,
        total,
        None,
        Some(new_story_id),
    );
    println!(
        "[process {id}] completed: story {} v{version} ({} chars in {:.1}s)",
        new_story_id,
        full.chars().count(),
        started.elapsed().as_secs_f64()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewProcess;

    #[test]
    fn progress_bands_are_monotonic_and_never_negative() {
        assert_eq!(progress_for(stage::PENDING, 0, 0), 0);
        assert_eq!(progress_for(stage::ANALYZING, 0, 0), 5);
        // The Go formula went negative here; this one cannot.
        assert_eq!(progress_for(stage::REWRITING, 0, 10), 15);
        assert_eq!(progress_for(stage::REWRITING, 5, 10), 50);
        assert_eq!(progress_for(stage::REWRITING, 10, 10), 85);
        assert_eq!(progress_for(stage::SAVING, 10, 10), 90);
        assert_eq!(progress_for(stage::COMPLETED, 10, 10), 100);
    }

    #[test]
    fn rewriting_progress_survives_a_zero_total() {
        assert_eq!(progress_for(stage::REWRITING, 3, 0), 15);
    }

    #[test]
    fn chunks_are_generated_once_and_reused() {
        let db = Db::open_memory().unwrap();
        let body = (1..=40)
            .map(|i| format!("Đoạn {i} của câu chuyện dài này kể về nhiều sự kiện."))
            .collect::<Vec<_>>()
            .join("\n");
        let sid = db.create_story("T", &body).unwrap();
        db.set_setting("hybrid_split_min_size", "200").unwrap();
        db.set_setting("hybrid_split_max_size", "400").unwrap();

        let first = get_or_generate_chunks(&db, sid).unwrap();
        let second = get_or_generate_chunks(&db, sid).unwrap();

        assert!(first.len() > 1, "expected the body to split");
        assert_eq!(first, second, "second call must reuse the cached split");
    }

    #[test]
    fn swapped_split_bounds_are_corrected() {
        let db = Db::open_memory().unwrap();
        let body = "Một đoạn.\nHai đoạn.\nBa đoạn.".to_string();
        let sid = db.create_story("T", &body).unwrap();
        db.set_setting("hybrid_split_min_size", "900").unwrap();
        db.set_setting("hybrid_split_max_size", "100").unwrap();

        // Must not panic or produce an empty split.
        let chunks = get_or_generate_chunks(&db, sid).unwrap();
        assert!(!chunks.is_empty());
    }

    fn sources() -> Vec<String> {
        vec![
            "Nguồn không.".to_string(),
            "Nguồn một.".to_string(),
            "Nguồn hai.".to_string(),
        ]
    }

    #[test]
    fn the_first_chunk_has_no_predecessor() {
        let tails = std::collections::HashMap::new();
        assert_eq!(tail_for(0, &tails, &sources()), "");
    }

    #[test]
    fn continuity_prefers_the_rewritten_tail() {
        let mut tails = std::collections::HashMap::new();
        tails.insert(0, "Đuôi đã viết lại.".to_string());

        assert_eq!(tail_for(1, &tails, &sources()), "Đuôi đã viết lại.");
    }

    /// With parallel_chunks > 1 the predecessor may still be in flight; the
    /// source tail is the fallback bridge. This is the quality trade the setting
    /// makes, so pin it.
    #[test]
    fn continuity_falls_back_to_the_source_tail_for_an_in_flight_sibling() {
        let tails = std::collections::HashMap::new();

        assert_eq!(tail_for(2, &tails, &sources()), "Nguồn một.");
    }

    /// At the default batch size of 1 the fallback must be unreachable, so the
    /// parallel path cannot silently change single-threaded output.
    #[test]
    fn sequential_batches_never_use_the_source_fallback() {
        let chunks = sources();
        let mut tails: std::collections::HashMap<i64, String> = std::collections::HashMap::new();

        for i in 0..chunks.len() as i64 {
            let used = tail_for(i, &tails, &chunks);
            if i > 0 {
                assert_eq!(
                    used,
                    format!("Viết lại {}.", i - 1),
                    "chunk {i} should continue from the rewritten predecessor"
                );
            }
            // Simulate finishing chunk i before starting i+1.
            tails.insert(i, format!("Viết lại {i}."));
        }
    }

    #[test]
    fn resume_scan_stops_at_the_first_gap() {
        let db = Db::open_memory().unwrap();
        let sid = db.create_story("T", "x").unwrap();
        let pid = db
            .create_process(&NewProcess {
                story_id: sid,
                creativity_ratio: 40,
                target_length_variance: 5,
                system_instruction: None,
                user_prompt: None,
                version_plan: None,
                model: None,
            })
            .unwrap();

        // Chunks 0 and 1 done, 2 missing, 3 done — only the 0..=1 prefix counts.
        db.save_rewrite_chunk(pid, 0, "a", "A").unwrap();
        db.save_rewrite_chunk(pid, 1, "b", "B").unwrap();
        db.save_rewrite_chunk(pid, 3, "d", "D").unwrap();

        let completed: std::collections::HashMap<i64, String> = db
            .get_rewrite_chunks(pid)
            .unwrap()
            .into_iter()
            .map(|c| (c.chunk_index, c.rewritten_content))
            .collect();

        let mut resume_from = 0i64;
        for i in 0..5 {
            if completed.contains_key(&i) {
                resume_from = i + 1;
            } else {
                break;
            }
        }
        assert_eq!(resume_from, 2, "must not skip over the gap at index 2");
    }
}
