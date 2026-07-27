//! Runner — queue DB-backed + per-flow exclusion + cancel + watchdog (design §6.5).
//!
//! Phase 2 chạy **source step** (full_refresh / incremental_append); transform/export
//! là Phase 3 nên chỉ log skip. Kiến trúc:
//!   * `enqueue` — backpressure (tổng active ≥ cap → 429/Backpressure) rồi `run_create`
//!     (unique index `ux_run_flow_active` chặn 2 run/flow → FlowBusy).
//!   * `spawn` — hai loop nền: poller (tick 5s, `Semaphore(max_concurrent)`, claim
//!     nguyên tử rồi chạy) + watchdog (tick 60s).
//!   * `execute_run_at` — claim → parse+validate flow → topo → mỗi source: extract →
//!     `sync::apply_land` → step_run/step_interval/lineage; guarded status cuối.
//!   * Cancel: `CancelToken = Arc<AtomicBool>` poll giữa batch/step; registry cho phép
//!     hủy từ ngoài.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use futures_util::StreamExt;

use crate::api::AppState;
use crate::config;
use crate::connectors::{self, SourceRel};
use crate::connectors::ExtractSpec;
use crate::db::{run_status, Db, FlowRow, RunCreate};
use crate::flow::{self, FlowDef, SourceStep};
use crate::sync::{self, LandParams, SyncMode};

/// Cờ hủy poll giữa batch/step.
pub type CancelToken = Arc<AtomicBool>;

/// Registry token hủy theo run_id — cho phép cancel một run đang chạy từ ngoài.
pub type CancelRegistry = Arc<Mutex<HashMap<String, CancelToken>>>;

/// Registry hủy rỗng mới (dùng trong AppState boot + test).
pub fn new_cancel_registry() -> CancelRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Kết quả enqueue (§6.5): backpressure và flow-busy là kết quả nghiệp vụ, không lỗi.
#[derive(Debug, PartialEq)]
pub enum EnqueueOutcome {
    Created(String),
    FlowBusy,
    Backpressure,
}

/// Trần tổng run active trước khi từ chối enqueue (§6.5). Rộng rãi so với
/// max_concurrent (queue đệm), tối thiểu 16.
fn queue_capacity(db: &Db) -> i64 {
    (db.setting_i64("max_concurrent", 2) * 16).max(16)
}

/// Enqueue với backpressure mặc định theo settings.
pub fn enqueue(db: &Db, flow_id: &str, trigger: &str) -> Result<EnqueueOutcome> {
    enqueue_with_cap(db, flow_id, trigger, queue_capacity(db))
}

/// Enqueue với cap tường minh (test tiêm cap nhỏ). Tổng queued+running ≥ cap →
/// Backpressure; còn lại theo `run_create`.
pub fn enqueue_with_cap(db: &Db, flow_id: &str, trigger: &str, cap: i64) -> Result<EnqueueOutcome> {
    if db.runs_active_count()? >= cap {
        return Ok(EnqueueOutcome::Backpressure);
    }
    Ok(match db.run_create(flow_id, trigger)? {
        RunCreate::Created(id) => EnqueueOutcome::Created(id),
        RunCreate::FlowBusy => EnqueueOutcome::FlowBusy,
    })
}

/// Yêu cầu hủy một run đang chạy (set token; execute loop poll giữa batch/step).
pub fn request_cancel(reg: &CancelRegistry, run_id: &str) -> bool {
    if let Some(tok) = reg.lock().unwrap().get(run_id) {
        tok.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// spawn — poller + watchdog
// ---------------------------------------------------------------------------

/// Khởi động runner nền. Dùng `state.cancels` làm registry (api/mcp cancel chung).
pub fn spawn(state: AppState) -> CancelRegistry {
    let reg = state.cancels.clone();
    spawn_poller(state.clone(), reg.clone());
    spawn_scheduler(state.clone());
    spawn_watchdog(state);
    reg
}

fn spawn_poller(state: AppState, reg: CancelRegistry) {
    tokio::spawn(async move {
        let max = state.db.setting_i64("max_concurrent", 2).clamp(1, 8) as usize;
        let sem = Arc::new(tokio::sync::Semaphore::new(max));
        let mut ticker = tokio::time::interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;
            let queued = match state.db.run_list_queued(50) {
                Ok(q) => q,
                Err(e) => {
                    eprintln!("lakehouse runner: liệt kê queued lỗi: {e}");
                    continue;
                }
            };
            for run_id in queued {
                let permit = match sem.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => break, // hết slot — chờ tick sau
                };
                let db = state.db.clone();
                let reg2 = reg.clone();
                let hub = state.hub.clone();
                let root = config::lake_dir();
                tokio::spawn(async move {
                    let _permit = permit; // RAII slot — release cả khi panic
                    let cancel: CancelToken = Arc::new(AtomicBool::new(false));
                    let _guard = RegGuard::insert(&reg2, &run_id, cancel.clone());
                    if let Err(e) =
                        execute_run_at_hub(&root, &db, &run_id, cancel, Some(&hub)).await
                    {
                        eprintln!("lakehouse runner: run {run_id} lỗi: {e}");
                    }
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// scheduler — self-schedule (§6.6)
// ---------------------------------------------------------------------------

/// Loop tick 30s: mỗi flow enabled có `schedule` đến hạn → enqueue trigger 'schedule'
/// rồi cập nhật `last_scheduled_at` (persist SQLite → sống sót restart). Unique index
/// `ux_run_flow_active` tự chặn chồng run nếu flow chạy chậm hơn chu kỳ.
fn spawn_scheduler(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        loop {
            ticker.tick().await;
            if let Err(e) = scheduler_tick(&state.db) {
                eprintln!("lakehouse scheduler lỗi: {e}");
            }
        }
    });
}

/// Một tick scheduler ở thời điểm hiện tại. Xem `scheduler_tick_at`.
pub fn scheduler_tick(db: &Db) -> Result<Vec<String>> {
    scheduler_tick_at(db, chrono::Utc::now().naive_utc())
}

/// Một tick scheduler ở mốc `now` (tiêm được để test). Trả danh sách flow_id đã enqueue
/// (Created). Flow đến hạn nhưng đang chạy (FlowBusy) / queue đầy (Backpressure) vẫn
/// nhích `last_scheduled_at` để không đọng slot cũ — lần sau vẫn theo chu kỳ.
pub fn scheduler_tick_at(db: &Db, now: chrono::NaiveDateTime) -> Result<Vec<String>> {
    let ts = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let mut fired = Vec::new();
    for f in db.flow_list()? {
        if !f.enabled {
            continue;
        }
        let Some(raw) = f.schedule.as_deref() else {
            continue;
        };
        let Ok(sch) = serde_json::from_str::<flow::Schedule>(raw) else {
            // schedule JSON hỏng — bỏ qua flow này thay vì làm chết cả tick.
            continue;
        };
        if !schedule_due(&sch, f.last_scheduled_at.as_deref(), now) {
            continue;
        }
        match enqueue(db, &f.id, crate::db::trigger::SCHEDULE) {
            Ok(EnqueueOutcome::Created(_)) => fired.push(f.id.clone()),
            Ok(_) => {} // FlowBusy/Backpressure: vẫn nhích watermark lịch bên dưới
            Err(e) => {
                eprintln!("scheduler enqueue flow '{}' lỗi: {e}", f.id);
                continue;
            }
        }
        db.flow_set_last_scheduled(&f.id, &ts)?;
    }
    Ok(fired)
}

/// Đến hạn chưa (§6.6). Thuần — không I/O.
///   * `every_minutes`: chưa chạy bao giờ → đến hạn; ngược lại `now ≥ last + N phút`.
///   * `daily_at HH:MM`: đến hạn nếu `now ≥ slot hôm nay` VÀ chưa chạy slot đó
///     (`last < slot hôm nay`). daily_at hỏng → không bao giờ đến hạn (không panic).
pub fn schedule_due(
    sch: &flow::Schedule,
    last_scheduled_at: Option<&str>,
    now: chrono::NaiveDateTime,
) -> bool {
    let last = last_scheduled_at
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok());
    match sch {
        flow::Schedule::Every { every_minutes } => {
            let m = (*every_minutes).max(1);
            match last {
                None => true,
                Some(l) => now >= l + chrono::Duration::minutes(m),
            }
        }
        flow::Schedule::Daily { daily_at } => {
            let Some((h, mi)) = flow::parse_hhmm(daily_at) else {
                return false;
            };
            let Some(slot) = now.date().and_hms_opt(h, mi, 0) else {
                return false;
            };
            if now < slot {
                return false;
            }
            match last {
                None => true,
                Some(l) => l < slot,
            }
        }
    }
}

fn spawn_watchdog(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(60));
        loop {
            ticker.tick().await;
            if let Err(e) = watchdog_tick(&state.db) {
                eprintln!("lakehouse watchdog lỗi: {e}");
            }
        }
    });
}

/// Watchdog (§6.5): running kẹt >60' → failed; queued bỏ rơi >24h → cancelled.
pub fn watchdog_tick(db: &Db) -> Result<(usize, usize)> {
    let fmt = "%Y-%m-%d %H:%M:%S";
    let running_cutoff = (chrono::Utc::now() - chrono::Duration::minutes(60))
        .format(fmt)
        .to_string();
    let queued_cutoff = (chrono::Utc::now() - chrono::Duration::hours(24))
        .format(fmt)
        .to_string();
    let failed = db.run_fail_stuck_running(&running_cutoff, "watchdog: running kẹt >60 phút")?;
    let cancelled = db.run_cancel_stale_queued(&queued_cutoff, "watchdog: queued bỏ rơi >24 giờ")?;
    Ok((failed, cancelled))
}

/// RAII gỡ token khỏi registry khi run kết thúc (kể cả panic).
struct RegGuard {
    reg: CancelRegistry,
    run_id: String,
}
impl RegGuard {
    fn insert(reg: &CancelRegistry, run_id: &str, tok: CancelToken) -> RegGuard {
        reg.lock().unwrap().insert(run_id.to_string(), tok);
        RegGuard {
            reg: reg.clone(),
            run_id: run_id.to_string(),
        }
    }
}
impl Drop for RegGuard {
    fn drop(&mut self) {
        self.reg.lock().unwrap().remove(&self.run_id);
    }
}

// ---------------------------------------------------------------------------
// execute
// ---------------------------------------------------------------------------

/// Chạy một run end-to-end (không phát sự kiện — CHỈ dùng bởi test; runtime đi qua
/// `execute_run_at_hub` để phát WS). `#[cfg(test)]` nên không cảnh báo dead-code ở bin.
#[cfg(test)]
pub async fn execute_run_at(
    root: &Path,
    db: &Db,
    run_id: &str,
    cancel: CancelToken,
) -> Result<()> {
    execute_run_at_hub(root, db, run_id, cancel, None).await
}

/// Chạy một run end-to-end dưới gốc lake `root`. Claim nguyên tử trước (run đã bị
/// claim/terminal → no-op Ok). Status cuối qua guarded write. `hub` (nếu có) nhận
/// `run:status` mỗi lần đổi trạng thái và `dataset:updated` sau mỗi source land.
pub async fn execute_run_at_hub(
    root: &Path,
    db: &Db,
    run_id: &str,
    cancel: CancelToken,
    hub: Option<&crate::dashws::DashHub>,
) -> Result<()> {
    if !db.run_claim(run_id)? {
        // Đã bị worker khác claim, hoặc không còn 'queued' (terminal). Không phải lỗi.
        return Ok(());
    }
    let run = db
        .run_get(run_id)?
        .ok_or_else(|| anyhow!("run {run_id} biến mất sau claim"))?;
    if let Some(h) = hub {
        h.emit_run_status(run_id, &run.flow_id, run_status::RUNNING);
    }
    let flow_row = match db.flow_get(&run.flow_id)? {
        Some(f) => f,
        None => {
            let msg = format!("flow '{}' không tồn tại", run.flow_id);
            db.run_log_append(run_id, "error", None, &msg).ok();
            db.run_update_status_guarded(run_id, run_status::FAILED, Some(&msg))?;
            if let Some(h) = hub {
                h.emit_run_status(run_id, &run.flow_id, run_status::FAILED);
            }
            return Ok(());
        }
    };

    db.run_log_append(run_id, "info", None, &format!("bắt đầu flow '{}'", run.flow_id))
        .ok();
    let result = run_flow(root, db, run_id, &flow_row, &cancel, hub).await;

    let final_status = match result {
        Ok(()) => {
            db.run_update_status_guarded(run_id, run_status::SUCCESS, None)?;
            db.run_log_append(run_id, "info", None, "hoàn tất").ok();
            run_status::SUCCESS
        }
        Err(e) => {
            let msg = e.to_string();
            if cancel.load(Ordering::SeqCst) {
                db.run_update_status_guarded(run_id, run_status::CANCELLED, Some("đã hủy"))?;
                db.run_log_append(run_id, "warn", None, "đã hủy").ok();
                run_status::CANCELLED
            } else {
                db.run_update_status_guarded(run_id, run_status::FAILED, Some(&msg))?;
                db.run_log_append(run_id, "error", None, &msg).ok();
                run_status::FAILED
            }
        }
    };
    if let Some(h) = hub {
        h.emit_run_status(run_id, &flow_row.id, final_status);
    }
    Ok(())
}

async fn run_flow(
    root: &Path,
    db: &Db,
    run_id: &str,
    flow_row: &FlowRow,
    cancel: &CancelToken,
    hub: Option<&crate::dashws::DashHub>,
) -> Result<()> {
    let def = flow::parse(&flow_row.def)?;
    flow::validate(&def).map_err(|errs| {
        let joined = errs
            .iter()
            .map(|e| format!("[{}] {}: {}", e.step_id, e.field, e.message))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow!("flow không hợp lệ: {joined}")
    })?;
    let order = flow::derive_dag(&def).map_err(|errs| {
        anyhow!("DAG lỗi: {}", errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; "))
    })?;

    for step_id in order {
        if cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("hủy trước step '{step_id}'"));
        }
        if let Some(src) = def.sources.iter().find(|s| s.id == step_id) {
            if let Err(e) =
                execute_source(root, db, run_id, flow_row, &def, src, cancel, hub).await
            {
                db.step_run_upsert(run_id, &step_id, "failed", 0, 0, Some(&e.to_string()))
                    .ok();
                return Err(e);
            }
        } else if let Some(t) = def.transforms.iter().find(|t| t.id == step_id) {
            // Transform (§6.2): full / incremental_by_time.
            match crate::transform::execute_transform(root, db, run_id, flow_row, &def, t).await {
                Ok(rows) => {
                    db.run_log_append(
                        run_id,
                        "info",
                        Some(&step_id),
                        &format!("transform '{}' land {rows} dòng", t.kind),
                    )
                    .ok();
                    if let Some(h) = hub {
                        let (ns, name) = crate::flow::transform_target(t);
                        if let Ok(Some(d)) = db.dataset_get(&ns, &name) {
                            h.emit_dataset_updated(&ns, &name, d.current_schema_version, d.row_count);
                        }
                    }
                }
                Err(e) => {
                    db.step_run_upsert(run_id, &step_id, "failed", 0, 0, Some(&e.to_string()))
                        .ok();
                    return Err(e);
                }
            }
        } else if let Some(e) = def.exports.iter().find(|e| e.id == step_id) {
            // Export step: file (csv/json/parquet) HOẶC DB-load qua connection (§5 Phase 5).
            match execute_export(root, db, run_id, &def, e).await {
                Ok((msg, rows)) => {
                    db.step_run_upsert(run_id, &step_id, "success", rows, rows, None).ok();
                    db.run_log_append(run_id, "info", Some(&step_id), &msg).ok();
                }
                Err(err) => {
                    db.step_run_upsert(run_id, &step_id, "failed", 0, 0, Some(&err.to_string()))
                        .ok();
                    return Err(err);
                }
            }
        } else {
            db.run_log_append(
                run_id,
                "warn",
                Some(&step_id),
                &format!("step '{step_id}' không rõ loại — bỏ qua"),
            )
            .ok();
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
/// Chạy một export step (§12/§5). Hai nhánh (validate ép đúng một):
///   * `connection` + `table` → **DB-load** ra database ngoài (§5): đọc toàn bộ dataset
///     input → `connector.load` theo LoadMode (full_refresh/append/upsert).
///   * `format` → ghi file csv/json/parquet dưới exports/.
/// `input` là step id (source/transform); resolve ra (namespace, dataset) đích.
/// Trả `(thông điệp log, số dòng ghi)`.
async fn execute_export(
    root: &Path,
    db: &Db,
    _run_id: &str,
    def: &flow::FlowDef,
    step: &crate::flow::ExportStep,
) -> Result<(String, i64)> {
    // input là step id → (ns, dataset) đích của step đó.
    let (ns, name) = resolve_step_target(def, &step.input)
        .ok_or_else(|| anyhow!("export input '{}' không phải step trong flow", step.input))?;

    // ---- Nhánh DB-load (connection + table) ----
    if step.connection.as_deref().is_some_and(|c| !c.trim().is_empty()) {
        let table = step
            .table
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| anyhow!("export step '{}' thiếu table cho DB-load", step.id))?;
        let conn = db
            .connection_get(step.connection.as_deref().unwrap())?
            .ok_or_else(|| anyhow!("connection '{}' không tồn tại", step.connection.as_deref().unwrap()))?;
        let mode = connectors::LoadMode::from_export(&step.mode, step.keys.clone())?;

        // Đọc TOÀN BỘ dataset input ra RecordBatch (không clamp).
        let sql = format!("SELECT * FROM \"{ns}\".\"{name}\"");
        let (_schema, batches) = crate::engine::collect_all_at(root, db, &sql).await?;

        let connector = connectors::connector_for(conn)?;
        let spec = connectors::LoadSpec {
            table: table.to_string(),
            mode,
            create_if_missing: true,
        };
        let rows = connector.load(spec, batches).await?;
        return Ok((
            format!(
                "DB-load '{ns}.{name}' → connection '{}' bảng '{table}' ({rows} dòng, mode {})",
                step.connection.as_deref().unwrap(),
                step.mode
            ),
            rows as i64,
        ));
    }

    // ---- Nhánh file (csv/json/parquet) ----
    let fmt_str = step
        .format
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("export step '{}' thiếu format", step.id))?;
    let format = crate::export::ExportFormat::parse(fmt_str)?;

    let rep = crate::export::export_dataset_at(
        &config::exports_dir(),
        root,
        db,
        &ns,
        &name,
        format,
        None,
    )
    .await?;
    Ok((
        format!(
            "export '{ns}.{name}' → {} ({} dòng, {} bytes)",
            rep.file, rep.rows, rep.bytes
        ),
        rep.rows,
    ))
}

/// (namespace, dataset) của một step id (source hoặc transform) trong flow.
fn resolve_step_target(def: &flow::FlowDef, step_id: &str) -> Option<(String, String)> {
    if let Some(s) = def.sources.iter().find(|s| s.id == step_id) {
        return Some(flow::source_target(s));
    }
    if let Some(t) = def.transforms.iter().find(|t| t.id == step_id) {
        return Some(flow::transform_target(t));
    }
    None
}

async fn execute_source(
    root: &Path,
    db: &Db,
    run_id: &str,
    flow_row: &FlowRow,
    _def: &FlowDef,
    src: &SourceStep,
    cancel: &CancelToken,
    hub: Option<&crate::dashws::DashHub>,
) -> Result<()> {
    let flow_id = &flow_row.id;
    let step_id = &src.id;
    db.step_run_upsert(run_id, step_id, "running", 0, 0, None)?;

    // Extract mode (§6.2): merge kéo incremental theo cursor; snapshot cần FULL extract
    // (chỉ full mới suy được delete ở nguồn).
    let extract_mode = match src.mode.as_str() {
        "full_refresh" => SyncMode::FullRefresh,
        "incremental_append" | "incremental_merge" => SyncMode::IncrementalAppend,
        "snapshot" => SyncMode::FullRefresh,
        other => return Err(anyhow!("mode '{other}' không hỗ trợ")),
    };
    let conn = db
        .connection_get(&src.connection)?
        .ok_or_else(|| anyhow!("connection '{}' không tồn tại", src.connection))?;

    // dataset + ownership (một dataset chỉ 1 flow ghi — §6.1).
    let (ns, name) = flow::source_target(src);
    let ds_id = db.dataset_upsert(&ns, &name, None, None, None)?;
    if !db.dataset_set_owner(ds_id, Some(flow_id))? {
        return Err(anyhow!("dataset {ns}.{name} đã thuộc flow khác"));
    }

    // merge dùng cursor để kéo incremental; snapshot không dùng cursor để extract.
    let extract_cursor_col = match src.mode.as_str() {
        "incremental_append" | "incremental_merge" => src.cursor.as_ref().map(|c| c.column.as_str()),
        _ => None,
    };
    let initial = src.cursor.as_ref().and_then(|c| c.initial.as_ref());
    let spec = sync::plan_extract(
        db,
        flow_id,
        step_id,
        source_rel(src),
        src.columns.clone(),
        extract_mode,
        extract_cursor_col,
        initial,
        ExtractSpec::DEFAULT_BATCH_ROWS,
    )?;

    // Extract → gom RecordBatch (poll cancel + heartbeat giữa batch).
    let connector = connectors::connector_for(conn)?;
    let mut stream = connector.extract(spec).await?;
    let mut batches = Vec::new();
    let mut rows_read: i64 = 0;
    while let Some(b) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            return Err(anyhow!("hủy khi extract step '{step_id}'"));
        }
        let b = b?;
        rows_read += b.num_rows() as i64;
        batches.push(b);
        db.run_touch(run_id).ok();
    }

    let dataset = db
        .dataset_get_by_id(ds_id)?
        .ok_or_else(|| anyhow!("dataset id {ds_id} biến mất"))?;
    let applied = apply_source(root, db, &dataset, run_id, flow_id, step_id, src, &batches)?;

    db.step_run_upsert(run_id, step_id, "success", rows_read, applied.rows_written, None)?;
    db.lineage_add(run_id, step_id, "out", ds_id, applied.schema_version)?;
    // Interval thô: [started_at run, now]. Đủ để skip-lookup nhận diện đã xong.
    let started = db.run_get(run_id)?.and_then(|r| r.started_at).unwrap_or_default();
    let ended = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    db.step_interval_upsert(
        flow_id,
        step_id,
        flow_row.def_version,
        &started,
        &ended,
        run_id,
        "success",
    )?;
    db.run_log_append(
        run_id,
        "info",
        Some(step_id),
        &format!("land {} dòng (đọc {rows_read})", applied.rows_written),
    )
    .ok();
    // Phát dataset:updated với row_count MỚI (đọc lại sau land để aggregate chuẩn).
    if let Some(h) = hub {
        let row_count = db
            .dataset_get_by_id(ds_id)?
            .map(|d| d.row_count)
            .unwrap_or(applied.rows_written);
        h.emit_dataset_updated(&ns, &name, applied.schema_version, row_count);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// backfill (§6.2 — một quy tắc duy nhất)
// ---------------------------------------------------------------------------

/// Kết quả backfill: step nào chạy, step nào skip, tổng interval + dòng.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BackfillOutcome {
    pub steps_run: Vec<String>,
    pub steps_skipped: Vec<String>,
    pub intervals_run: usize,
    pub rows_written: i64,
}

/// Backfill per-step (§6.2 quy tắc duy nhất):
///   * step idempotent theo thời gian (`incremental_by_time`) nhận range `[start,end)` —
///     chunk stateless, ghi `step_interval`, KHÔNG đụng watermark sống.
///   * transform `full` + source (full/append/merge/snapshot) = KHÔNG idempotent theo
///     range → **SKIP mặc định**.
///   * `rebuild:[step_id]` = full-refresh-equivalent: transform `full` chạy lại; source
///     merge/SCD2 rebuild **mất lịch sử** → đòi `confirm=true`, thiếu → lỗi.
#[allow(clippy::too_many_arguments)]
pub async fn backfill_run(
    root: &Path,
    db: &Db,
    flow_id: &str,
    start: &str,
    end: &str,
    steps: Option<&[String]>,
    rebuild: &[String],
    confirm: bool,
) -> Result<BackfillOutcome> {
    let flow_row = db
        .flow_get(flow_id)?
        .ok_or_else(|| anyhow!("flow '{flow_id}' không tồn tại"))?;
    let def = flow::parse(&flow_row.def)?;
    flow::validate(&def).map_err(|errs| {
        anyhow!(
            "flow không hợp lệ: {}",
            errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; ")
        )
    })?;
    let order = flow::derive_dag(&def)
        .map_err(|errs| anyhow!("DAG lỗi: {}", errs.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; ")))?;

    let start_dt = crate::transform::parse_boundary(start)
        .ok_or_else(|| anyhow!("start '{start}' không phải mốc thời gian hợp lệ"))?;
    let end_dt = crate::transform::parse_boundary(end)
        .ok_or_else(|| anyhow!("end '{end}' không phải mốc thời gian hợp lệ"))?;

    let want = |id: &str| steps.map(|s| s.iter().any(|x| x == id)).unwrap_or(true);
    let rebuilding = |id: &str| rebuild.iter().any(|x| x == id);

    let run_id = format!("backfill-{}", uuid::Uuid::now_v7());
    let mut out = BackfillOutcome::default();

    for step_id in order {
        if !want(&step_id) {
            continue;
        }
        if let Some(t) = def.transforms.iter().find(|t| t.id == step_id) {
            match t.kind.as_str() {
                "incremental_by_time" => {
                    let o = crate::transform::run_incremental_range(
                        root, db, &def, t, &run_id, flow_id, flow_row.def_version, start_dt, end_dt,
                    )
                    .await?;
                    out.intervals_run += o.intervals_run;
                    out.rows_written += o.rows_written;
                    out.steps_run.push(step_id);
                }
                _ => {
                    // Full transform: chỉ rebuild mới chạy (full-refresh-equivalent).
                    if rebuilding(&step_id) {
                        let a = crate::transform::run_full(root, db, &def, t, &run_id, flow_id).await?;
                        out.rows_written += a.rows_written;
                        out.steps_run.push(step_id);
                    } else {
                        out.steps_skipped.push(step_id);
                    }
                }
            }
        } else if let Some(src) = def.sources.iter().find(|s| s.id == step_id) {
            let is_stateful_merge = src.mode == "incremental_merge" || src.mode == "snapshot";
            if rebuilding(&step_id) {
                if is_stateful_merge && !confirm {
                    return Err(anyhow!(
                        "rebuild step '{step_id}' ({}) là full-refresh-equivalent — SCD2/merge \
                         rebuild MẤT LỊCH SỬ; đặt confirm=true để xác nhận",
                        src.mode
                    ));
                }
                // Rebuild source = chạy lại step nguồn (xử lý trạng thái nguồn hiện tại).
                let cancel: CancelToken = Arc::new(AtomicBool::new(false));
                execute_source(root, db, &run_id, &flow_row, &def, src, &cancel, None).await?;
                out.steps_run.push(step_id);
            } else {
                // Source không idempotent theo range → skip mặc định.
                out.steps_skipped.push(step_id);
            }
        } else {
            out.steps_skipped.push(step_id);
        }
    }
    Ok(out)
}

/// Dispatch apply theo mode (§6.2): full/append → apply_land; incremental_merge →
/// apply_merge (+ đẩy watermark); snapshot → apply_snapshot SCD2.
#[allow(clippy::too_many_arguments)]
fn apply_source(
    root: &Path,
    db: &Db,
    dataset: &crate::db::Dataset,
    run_id: &str,
    flow_id: &str,
    step_id: &str,
    src: &SourceStep,
    batches: &[datafusion::arrow::record_batch::RecordBatch],
) -> Result<sync::AppliedLand> {
    let schema_policy = src.schema_policy.as_ref();
    match src.mode.as_str() {
        "full_refresh" | "incremental_append" => {
            let mode = if src.mode == "full_refresh" {
                SyncMode::FullRefresh
            } else {
                SyncMode::IncrementalAppend
            };
            let cursor_col = src.cursor.as_ref().map(|c| c.column.as_str());
            sync::apply_land_at(
                root,
                LandParams { db, dataset, run_id, flow_id, step_id, mode, cursor_col, schema_policy },
                batches,
            )
        }
        "incremental_merge" => {
            let primary_key = src.primary_key.clone().unwrap_or_default();
            let partition_by = src
                .target
                .as_ref()
                .and_then(|t| t.partition_by.clone())
                .unwrap_or_default();
            let strategy = sync::MergeStrategy::from_str(src.strategy.as_deref().unwrap_or("delete_insert"));
            let cursor_col = src.cursor.as_ref().map(|c| c.column.as_str());
            let applied = sync::apply_merge_at(
                root,
                sync::MergeParams {
                    db,
                    dataset,
                    run_id,
                    flow_id,
                    step_id,
                    primary_key: &primary_key,
                    partition_by: &partition_by,
                    strategy,
                    cursor_col,
                    schema_policy,
                },
                batches,
            )?;
            // Đẩy watermark theo max cursor incoming (extract lần sau kéo từ đây).
            if let Some(col) = cursor_col {
                let plan = sync::prepare_incremental(batches, col, None, &std::collections::HashSet::new())?;
                if let Some(wm) = &plan.new_watermark {
                    let hashes = serde_json::to_string(&plan.new_boundary_hashes).ok();
                    db.stream_state_set_monotonic(flow_id, step_id, col, wm, hashes.as_deref())?;
                }
            }
            Ok(applied)
        }
        "snapshot" => {
            let primary_key = src.primary_key.clone().unwrap_or_default();
            let strategy = match src.strategy.as_deref().unwrap_or("check") {
                "timestamp" => sync::SnapshotStrategy::Timestamp(
                    src.cursor.as_ref().map(|c| c.column.clone()).unwrap_or_default(),
                ),
                _ => sync::SnapshotStrategy::Check(src.check_columns.clone().unwrap_or_default()),
            };
            let hard_deletes = sync::HardDeletes::from_str(src.hard_deletes.as_deref().unwrap_or("ignore"));
            sync::apply_snapshot_at(
                root,
                sync::SnapshotParams {
                    db,
                    dataset,
                    run_id,
                    primary_key: &primary_key,
                    strategy: &strategy,
                    hard_deletes,
                },
                batches,
            )
        }
        other => Err(anyhow!("mode '{other}' không hỗ trợ")),
    }
}

/// SourceRel từ source step: `table` "schema.name" tách theo dấu '.' đầu; hoặc `query`.
fn source_rel(src: &SourceStep) -> SourceRel {
    if let Some(q) = &src.query {
        return SourceRel::Query { sql: q.clone() };
    }
    let t = src.table.clone().unwrap_or_default();
    match t.split_once('.') {
        Some((schema, name)) if !schema.is_empty() && !name.is_empty() => SourceRel::Table {
            schema: Some(schema.to_string()),
            name: name.to_string(),
        },
        _ => SourceRel::Table {
            schema: None,
            name: t,
        },
    }
}

#[allow(dead_code)]
fn default_root() -> PathBuf {
    config::lake_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use serde_json::json;

    /// Tạo file SQLite nguồn với bảng events(id,label) + seed rows.
    fn seed_sqlite(path: &str, rows: &[(i64, &str)]) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS events (id INTEGER, label TEXT);")
            .unwrap();
        for (id, label) in rows {
            conn.execute("INSERT INTO events (id, label) VALUES (?1, ?2)", rusqlite::params![id, label])
                .unwrap();
        }
    }

    fn full_refresh_def() -> String {
        json!({
            "flow": "ev",
            "sources": [{
                "id": "events", "connection": "src", "table": "events",
                "mode": "full_refresh",
                "target": {"namespace": "raw", "dataset": "events"}
            }]
        })
        .to_string()
    }

    fn incremental_def() -> String {
        json!({
            "flow": "ev",
            "sources": [{
                "id": "events", "connection": "src", "table": "events",
                "mode": "incremental_append",
                "cursor": {"column": "id", "initial": 0},
                "target": {"namespace": "raw", "dataset": "events"}
            }]
        })
        .to_string()
    }

    async fn count_rows(root: &Path, db: &Db) -> i64 {
        let page = crate::engine::query_page_at(
            root,
            db,
            "SELECT COUNT(*) AS n FROM raw.events",
            None,
            None,
        )
        .await
        .unwrap();
        page.rows[0][0].as_i64().unwrap()
    }

    #[tokio::test]
    async fn full_refresh_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lake");
        let src_path = dir.path().join("src.sqlite");
        seed_sqlite(src_path.to_str().unwrap(), &[(1, "a"), (2, "b"), (3, "c")]);

        let db = Db::open_memory().unwrap();
        db.connection_add("src", "sqlite", src_path.to_str().unwrap()).unwrap();
        db.flow_upsert("ev", None, &full_refresh_def(), true, None).unwrap();

        let id = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            other => panic!("kỳ vọng Created, nhận {other:?}"),
        };
        let cancel: CancelToken = Arc::new(AtomicBool::new(false));
        execute_run_at(&root, &db, &id, cancel).await.unwrap();

        assert_eq!(db.run_get(&id).unwrap().unwrap().status, run_status::SUCCESS);
        assert_eq!(count_rows(&root, &db).await, 3);
    }

    #[tokio::test]
    async fn full_refresh_rerun_no_doubling() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lake");
        let src_path = dir.path().join("src.sqlite");
        seed_sqlite(src_path.to_str().unwrap(), &[(1, "a"), (2, "b"), (3, "c")]);

        let db = Db::open_memory().unwrap();
        db.connection_add("src", "sqlite", src_path.to_str().unwrap()).unwrap();
        db.flow_upsert("ev", None, &full_refresh_def(), true, None).unwrap();

        for _ in 0..2 {
            let id = match enqueue(&db, "ev", "manual").unwrap() {
                EnqueueOutcome::Created(id) => id,
                other => panic!("kỳ vọng Created, nhận {other:?}"),
            };
            let cancel: CancelToken = Arc::new(AtomicBool::new(false));
            execute_run_at(&root, &db, &id, cancel).await.unwrap();
        }
        // Full refresh swap: file cũ tombstone → vẫn 3 dòng, không nhân đôi.
        assert_eq!(count_rows(&root, &db).await, 3);
    }

    #[tokio::test]
    async fn incremental_append_advances_watermark() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lake");
        let src_path = dir.path().join("src.sqlite");
        let sp = src_path.to_str().unwrap().to_string();
        seed_sqlite(&sp, &[(1, "a"), (2, "b"), (3, "c")]);

        let db = Db::open_memory().unwrap();
        db.connection_add("src", "sqlite", &sp).unwrap();
        db.flow_upsert("ev", None, &incremental_def(), true, None).unwrap();

        // Run 1 → 3 dòng, watermark = "3".
        let id1 = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            o => panic!("{o:?}"),
        };
        execute_run_at(&root, &db, &id1, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        assert_eq!(count_rows(&root, &db).await, 3);
        let st = db.stream_state_get("ev", "events").unwrap().unwrap();
        assert_eq!(st.last_value.as_deref(), Some("3"));

        // Thêm 2 dòng nguồn (id 4,5).
        seed_sqlite(&sp, &[(4, "d"), (5, "e")]);
        let id2 = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            o => panic!("{o:?}"),
        };
        execute_run_at(&root, &db, &id2, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        // Closed-range >= 3 kéo lại row biên (3,"c") — dedupe không nhân đôi → 5 dòng.
        assert_eq!(count_rows(&root, &db).await, 5);
        let st = db.stream_state_get("ev", "events").unwrap().unwrap();
        assert_eq!(st.last_value.as_deref(), Some("5"), "watermark tiến");

        // Run 3 không thêm gì → vẫn 5 (boundary dedupe).
        let id3 = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            o => panic!("{o:?}"),
        };
        execute_run_at(&root, &db, &id3, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        assert_eq!(count_rows(&root, &db).await, 5);
    }

    #[tokio::test]
    async fn export_db_load_to_sqlite_end_to_end() {
        // Flow: source (sqlite events) → export DB-load ra sqlite target (mode append).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lake");
        let src_path = dir.path().join("src.sqlite");
        seed_sqlite(src_path.to_str().unwrap(), &[(1, "a"), (2, "b"), (3, "c")]);
        let dest_path = dir.path().join("dest.sqlite");
        let dp = dest_path.to_str().unwrap().to_string();

        let db = Db::open_memory().unwrap();
        db.connection_add("src", "sqlite", src_path.to_str().unwrap()).unwrap();
        db.connection_add("dst", "sqlite", &dp).unwrap();

        let def = json!({
            "flow": "ev",
            "sources": [{
                "id": "events", "connection": "src", "table": "events",
                "mode": "full_refresh",
                "target": {"namespace": "raw", "dataset": "events"}
            }],
            "exports": [{
                "id": "out", "input": "events",
                "connection": "dst", "table": "events_copy",
                "mode": "append"
            }]
        })
        .to_string();
        db.flow_upsert("ev", None, &def, true, None).unwrap();

        let id = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            o => panic!("{o:?}"),
        };
        execute_run_at(&root, &db, &id, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        assert_eq!(db.run_get(&id).unwrap().unwrap().status, run_status::SUCCESS);

        // Bảng đích tự tạo (create_if_missing) + đúng 3 dòng.
        let conn = rusqlite::Connection::open(&dp).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM events_copy", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3);

        // step_run 'out' success với rows_written = 3.
        let steps = db.step_runs_for(&id).unwrap();
        let out = steps.iter().find(|s| s.step_id == "out").expect("có step out");
        assert_eq!(out.status, "success");
        assert_eq!(out.rows_written, 3);
    }

    #[tokio::test]
    async fn claim_is_atomic_rerun_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lake");
        let src_path = dir.path().join("src.sqlite");
        seed_sqlite(src_path.to_str().unwrap(), &[(1, "a")]);
        let db = Db::open_memory().unwrap();
        db.connection_add("src", "sqlite", src_path.to_str().unwrap()).unwrap();
        db.flow_upsert("ev", None, &full_refresh_def(), true, None).unwrap();

        let id = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            o => panic!("{o:?}"),
        };
        execute_run_at(&root, &db, &id, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        assert_eq!(db.run_get(&id).unwrap().unwrap().status, run_status::SUCCESS);
        // Chạy lại cùng id: đã terminal → claim thất bại → no-op, không lỗi, không nhân đôi.
        execute_run_at(&root, &db, &id, Arc::new(AtomicBool::new(false)))
            .await
            .unwrap();
        assert_eq!(count_rows(&root, &db).await, 1);
    }

    #[tokio::test]
    async fn cancel_before_run_marks_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lake");
        let src_path = dir.path().join("src.sqlite");
        seed_sqlite(src_path.to_str().unwrap(), &[(1, "a"), (2, "b")]);
        let db = Db::open_memory().unwrap();
        db.connection_add("src", "sqlite", src_path.to_str().unwrap()).unwrap();
        db.flow_upsert("ev", None, &full_refresh_def(), true, None).unwrap();

        let id = match enqueue(&db, "ev", "manual").unwrap() {
            EnqueueOutcome::Created(id) => id,
            o => panic!("{o:?}"),
        };
        // Token đã set true trước khi chạy → bail cancelled ở check đầu.
        let cancel: CancelToken = Arc::new(AtomicBool::new(true));
        execute_run_at(&root, &db, &id, cancel).await.unwrap();
        assert_eq!(db.run_get(&id).unwrap().unwrap().status, run_status::CANCELLED);
    }

    #[test]
    fn enqueue_backpressure_and_flow_busy() {
        let db = Db::open_memory().unwrap();
        // cap = 2: hai flow khác nhau enqueue được, flow thứ ba bị Backpressure.
        assert!(matches!(
            enqueue_with_cap(&db, "f1", "manual", 2).unwrap(),
            EnqueueOutcome::Created(_)
        ));
        // Cùng flow f1 khi run trước còn active → FlowBusy.
        assert_eq!(enqueue_with_cap(&db, "f1", "manual", 2).unwrap(), EnqueueOutcome::FlowBusy);
        assert!(matches!(
            enqueue_with_cap(&db, "f2", "manual", 2).unwrap(),
            EnqueueOutcome::Created(_)
        ));
        // Đã đủ 2 active → f3 Backpressure.
        assert_eq!(enqueue_with_cap(&db, "f3", "manual", 2).unwrap(), EnqueueOutcome::Backpressure);
    }

    // ---- backfill (§6.2) ----

    #[tokio::test]
    async fn backfill_incremental_by_time_runs_range() {
        use datafusion::arrow::array::{Int64Array, StringArray};
        use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
        use datafusion::arrow::record_batch::RecordBatch;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = Db::open_memory().unwrap();

        // raw.events(day, val) nạp sẵn (source step sẽ bị SKIP khi backfill).
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("day", DataType::Utf8, true),
            Field::new("val", DataType::Int64, true),
        ]));
        let ev = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(vec!["2024-01-01", "2024-01-02", "2024-01-03"])),
                Arc::new(Int64Array::from(vec![5, 7, 3])),
            ],
        )
        .unwrap();
        let t = crate::ingest::IngestedTable {
            name: "events".into(),
            schema,
            batches: vec![ev],
            origin: "csv",
            note: "t".into(),
            rows: 3,
        };
        crate::lake::create_dataset_from_ingested_at(root, &db, "raw", "events", &t, "seed").unwrap();

        let def = json!({
            "flow": "ev",
            "sources": [{"id": "events", "connection": "c", "table": "t", "mode": "full_refresh",
                         "target": {"namespace": "raw", "dataset": "events"}}],
            "transforms": [{
                "id": "daily", "kind": "incremental_by_time",
                "time_column": "day", "interval": "day", "lookback": 0,
                "sql": "SELECT day, SUM(val) AS total FROM events WHERE day >= @start AND day < @end GROUP BY day",
                "target": {"namespace": "marts", "dataset": "daily"}
            }]
        })
        .to_string();
        db.flow_upsert("ev", None, &def, true, None).unwrap();

        let out = backfill_run(root, &db, "ev", "2024-01-01", "2024-01-04", None, &[], false)
            .await
            .unwrap();
        assert_eq!(out.intervals_run, 3, "3 ngày trong range");
        assert!(out.steps_run.contains(&"daily".to_string()));
        assert!(out.steps_skipped.contains(&"events".to_string()), "source SKIP mặc định");

        let page = crate::engine::query_page_at(root, &db, "SELECT COUNT(*) AS n FROM marts.daily", None, None)
            .await
            .unwrap();
        assert_eq!(page.rows[0][0].as_i64().unwrap(), 3);
    }

    #[tokio::test]
    async fn backfill_merge_skipped_and_rebuild_needs_confirm() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let def = json!({
            "flow": "mg",
            "sources": [{
                "id": "orders", "connection": "c", "table": "t",
                "mode": "incremental_merge",
                "cursor": {"column": "u", "initial": 0},
                "primary_key": ["id"], "merge_key": ["region"],
                "target": {"namespace": "raw", "dataset": "orders", "partition_by": ["region"]}
            }]
        })
        .to_string();
        db.flow_upsert("mg", None, &def, true, None).unwrap();

        // Mặc định: merge source SKIP.
        let out = backfill_run(root, &db, "mg", "2024-01-01", "2024-02-01", None, &[], false)
            .await
            .unwrap();
        assert!(out.steps_skipped.contains(&"orders".to_string()));
        assert!(out.steps_run.is_empty());

        // Rebuild merge không confirm → lỗi.
        let err = backfill_run(
            root, &db, "mg", "2024-01-01", "2024-02-01", None, &["orders".to_string()], false,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("confirm"), "rebuild merge cần confirm");
    }

    // ---- scheduler (§6.6) ----

    fn dt(s: &str) -> chrono::NaiveDateTime {
        chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn schedule_due_every_minutes() {
        let sch = flow::Schedule::Every { every_minutes: 15 };
        let now = dt("2024-01-01 12:00:00");
        // Chưa chạy bao giờ → đến hạn.
        assert!(schedule_due(&sch, None, now));
        // Mới chạy 5' trước → chưa.
        assert!(!schedule_due(&sch, Some("2024-01-01 11:55:00"), now));
        // Chạy 15' trước → đúng hạn.
        assert!(schedule_due(&sch, Some("2024-01-01 11:45:00"), now));
        // Chạy 20' trước → quá hạn, vẫn đến hạn.
        assert!(schedule_due(&sch, Some("2024-01-01 11:40:00"), now));
    }

    #[test]
    fn schedule_due_daily_at() {
        let sch = flow::Schedule::Daily { daily_at: "03:00".into() };
        // Trước slot hôm nay → chưa.
        assert!(!schedule_due(&sch, None, dt("2024-01-01 02:59:00")));
        // Qua slot, chưa chạy → đến hạn.
        assert!(schedule_due(&sch, None, dt("2024-01-01 03:00:00")));
        // Đã chạy slot hôm nay → không lặp trong ngày.
        assert!(!schedule_due(&sch, Some("2024-01-01 03:00:05"), dt("2024-01-01 09:00:00")));
        // Sang ngày mới, qua slot → đến hạn lại.
        assert!(schedule_due(&sch, Some("2024-01-01 03:00:05"), dt("2024-01-02 03:01:00")));
        // daily_at hỏng → không bao giờ đến hạn (không panic).
        let bad = flow::Schedule::Daily { daily_at: "25:99".into() };
        assert!(!schedule_due(&bad, None, dt("2024-01-01 12:00:00")));
    }

    #[test]
    fn scheduler_tick_enqueues_due_and_advances_watermark() {
        let db = Db::open_memory().unwrap();
        // Flow enabled + every_minutes 10, chưa chạy bao giờ.
        let def = json!({
            "flow": "sch",
            "sources": [{"id": "e", "connection": "c", "table": "t", "mode": "full_refresh"}],
            "schedule": {"every_minutes": 10}
        })
        .to_string();
        let sched = serde_json::to_string(&flow::Schedule::Every { every_minutes: 10 }).unwrap();
        db.flow_upsert("sch", None, &def, true, Some(&sched)).unwrap();

        let now = dt("2024-01-01 12:00:00");
        let fired = scheduler_tick_at(&db, now).unwrap();
        assert_eq!(fired, vec!["sch".to_string()]);
        // last_scheduled_at đã nhích → tick lại NGAY không enqueue nữa (chưa tới chu kỳ).
        let f = db.flow_get("sch").unwrap().unwrap();
        assert_eq!(f.last_scheduled_at.as_deref(), Some("2024-01-01 12:00:00"));
        // Run active → dù có tick lại cũng FlowBusy, không tạo run mới.
        let again = scheduler_tick_at(&db, dt("2024-01-01 12:20:00")).unwrap();
        assert!(again.is_empty(), "flow đang chạy → FlowBusy, không enqueue thêm");
    }

    #[test]
    fn scheduler_tick_skips_disabled_and_unscheduled() {
        let db = Db::open_memory().unwrap();
        // Disabled dù có schedule.
        let d1 = json!({"flow": "off", "sources": [{"id":"e","connection":"c","table":"t","mode":"full_refresh"}], "schedule": {"every_minutes": 1}}).to_string();
        db.flow_upsert("off", None, &d1, false, Some("{\"every_minutes\":1}")).unwrap();
        // Enabled nhưng không lịch.
        let d2 = json!({"flow": "manual", "sources": [{"id":"e","connection":"c","table":"t","mode":"full_refresh"}]}).to_string();
        db.flow_upsert("manual", None, &d2, true, None).unwrap();

        let fired = scheduler_tick_at(&db, dt("2024-01-01 12:00:00")).unwrap();
        assert!(fired.is_empty());
    }

    #[test]
    fn watchdog_flips_old_runs() {
        let db = Db::open_memory().unwrap();
        // Running kẹt (updated_at 2020) → failed.
        let stuck = match db.run_create("f1", "manual").unwrap() {
            RunCreate::Created(id) => id,
            _ => unreachable!(),
        };
        db.run_claim(&stuck).unwrap();
        db.with_conn(|c| {
            c.execute(
                "UPDATE run SET updated_at = '2020-01-01 00:00:00' WHERE id = ?1",
                rusqlite::params![stuck],
            )
        })
        .unwrap();
        let (failed, _cancelled) = watchdog_tick(&db).unwrap();
        assert_eq!(failed, 1);
        assert_eq!(db.run_get(&stuck).unwrap().unwrap().status, run_status::FAILED);
    }
}
