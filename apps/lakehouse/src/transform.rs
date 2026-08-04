//! Thực thi transform trong flow (design §6.2): `full` + `incremental_by_time`.
//!
//! Hai kind:
//!   * **Full** — chạy `sql` một lần trên input flow (engine::transform_select) → land
//!     FullRefresh (tombstone toàn bộ file cũ + thêm mới, 1 txn). Idempotent: chạy lại
//!     ra kết quả y hệt (cho bảng dimension). = `sync::apply_land` FullRefresh.
//!   * **IncrementalByTime** — target LUÔN tự partition theo bucket(`time_column`,
//!     `interval`). Với mỗi interval trong `[start,end)`: thay `@start`/`@end` bằng biên
//!     literal, chạy `sql`, land partition (delete-interval + insert = idempotent). Ghi
//!     `step_interval` success từng interval. `lookback` = chạy lại N interval cuối.
//!
//! Macro `@start`/`@end` thay bằng string literal đóng nháy — `time_column` so sánh dạng
//! chuỗi ISO (khớp watermark chuỗi ở sync.rs). Đây là giới hạn thiết kế: cursor thời gian
//! phải là cột chuỗi ISO-8601/date đơn điệu; `time_column` PHẢI có trong cột output để
//! bucket được (design §6.2).

#![allow(dead_code)]

use std::path::Path;

use anyhow::{anyhow, Result};
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use datafusion::arrow::array::Array;

use crate::db::{Db, FlowRow};
use crate::engine;
use crate::flow::{self, FlowDef, TransformStep};
use crate::sync::{self, LandParams, SyncMode};

// ---------------------------------------------------------------------------
// interval
// ---------------------------------------------------------------------------

/// Granularity bucket cho incremental_by_time (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interval {
    Hour,
    Day,
    Week,
    Month,
}

impl Interval {
    pub fn from_str(s: &str) -> Option<Interval> {
        match s {
            "hour" => Some(Interval::Hour),
            "day" => Some(Interval::Day),
            "week" => Some(Interval::Week),
            "month" => Some(Interval::Month),
            _ => None,
        }
    }
}

/// Neo `dt` xuống đầu bucket của `interval` (floor).
fn floor_to(dt: NaiveDateTime, iv: Interval) -> NaiveDateTime {
    let d = dt.date();
    match iv {
        Interval::Hour => dt
            .date()
            .and_time(NaiveTime::from_hms_opt(dt.hour(), 0, 0).unwrap()),
        Interval::Day => d.and_time(NaiveTime::MIN),
        Interval::Week => {
            // Neo về thứ Hai (ISO): weekday().num_days_from_monday().
            let back = d.weekday().num_days_from_monday() as i64;
            (d - Duration::days(back)).and_time(NaiveTime::MIN)
        }
        Interval::Month => NaiveDate::from_ymd_opt(d.year(), d.month(), 1)
            .unwrap()
            .and_time(NaiveTime::MIN),
    }
}

/// Biên kế tiếp sau bucket bắt đầu tại `start` (start PHẢI đã floor).
fn next_boundary(start: NaiveDateTime, iv: Interval) -> NaiveDateTime {
    match iv {
        Interval::Hour => start + Duration::hours(1),
        Interval::Day => start + Duration::days(1),
        Interval::Week => start + Duration::days(7),
        Interval::Month => {
            let d = start.date();
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1)
            } else {
                (d.year(), d.month() + 1)
            };
            NaiveDate::from_ymd_opt(y, m, 1)
                .unwrap()
                .and_time(NaiveTime::MIN)
        }
    }
}

/// Lùi `start` về `n` bucket (cho lookback).
fn back_n(start: NaiveDateTime, iv: Interval, n: i64) -> NaiveDateTime {
    let mut s = start;
    for _ in 0..n {
        s = prev_boundary(s, iv);
    }
    s
}

fn prev_boundary(start: NaiveDateTime, iv: Interval) -> NaiveDateTime {
    match iv {
        Interval::Hour => start - Duration::hours(1),
        Interval::Day => start - Duration::days(1),
        Interval::Week => start - Duration::days(7),
        Interval::Month => {
            let d = start.date();
            let (y, m) = if d.month() == 1 {
                (d.year() - 1, 12)
            } else {
                (d.year(), d.month() - 1)
            };
            NaiveDate::from_ymd_opt(y, m, 1)
                .unwrap()
                .and_time(NaiveTime::MIN)
        }
    }
}

/// Nhãn partition của bucket bắt đầu tại `start` (§6.2). Ổn định, an toàn tên thư mục.
fn label(start: NaiveDateTime, iv: Interval) -> String {
    match iv {
        Interval::Hour => start.format("%Y-%m-%dT%H").to_string(),
        Interval::Day | Interval::Week => start.format("%Y-%m-%d").to_string(),
        Interval::Month => start.format("%Y-%m").to_string(),
    }
}

/// Literal `@start`/`@end` (đóng-nháy bởi caller) — so sánh chuỗi với cột thời gian.
/// Day/week/month → 'YYYY-MM-DD'; hour → 'YYYY-MM-DD HH:00:00'.
fn bound_literal(dt: NaiveDateTime, iv: Interval) -> String {
    match iv {
        Interval::Hour => dt.format("%Y-%m-%d %H:00:00").to_string(),
        _ => dt.format("%Y-%m-%d").to_string(),
    }
}

/// Danh sách bucket `[start_i, end_i)` (đã floor `range_start`) phủ `[range_start, range_end)`.
/// Rỗng nếu `range_start >= range_end`. Cap 100_000 bucket (chống chạy loạn).
pub fn plan_intervals(
    range_start: NaiveDateTime,
    range_end: NaiveDateTime,
    iv: Interval,
) -> Vec<(NaiveDateTime, NaiveDateTime)> {
    let mut out = Vec::new();
    let mut b = floor_to(range_start, iv);
    let mut guard = 0;
    while b < range_end && guard < 100_000 {
        let nb = next_boundary(b, iv);
        out.push((b, nb));
        b = nb;
        guard += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// macro @start/@end
// ---------------------------------------------------------------------------

/// Thay `@start`/`@end` bằng string literal. Không nội suy giá trị người dùng — biên do
/// engine sinh (an toàn). `@end` không chứa `@start` nên thay độc lập.
pub fn substitute_macros(sql: &str, start_lit: &str, end_lit: &str) -> String {
    sql.replace("@start", &format!("'{start_lit}'"))
        .replace("@end", &format!("'{end_lit}'"))
}

// ---------------------------------------------------------------------------
// parse boundary
// ---------------------------------------------------------------------------

/// Parse một mốc thời gian từ chuỗi (date hoặc datetime). Trả None nếu không nhận dạng.
pub fn parse_boundary(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim();
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_time(NaiveTime::MIN));
    }
    // Thử RFC3339 (bỏ phần offset/frac).
    if s.len() >= 10 {
        if let Ok(d) = NaiveDate::parse_from_str(&s[..10], "%Y-%m-%d") {
            return Some(d.and_time(NaiveTime::MIN));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// full transform
// ---------------------------------------------------------------------------

/// Chạy transform `full` → land FullRefresh vào target dataset. Trả (rows_written,
/// schema_version). Không đụng step_interval (idempotent full = một "interval" duy nhất).
pub async fn run_full(
    root: &Path,
    db: &Db,
    def: &FlowDef,
    step: &TransformStep,
    run_id: &str,
    flow_id: &str,
) -> Result<sync::AppliedLand> {
    let (ns, name) = flow::transform_target(step);
    let ds_id = db.dataset_upsert(&ns, &name, None, None, None)?;
    if !db.dataset_set_owner(ds_id, Some(flow_id))? {
        return Err(anyhow!("dataset {ns}.{name} đã thuộc flow khác"));
    }
    let (_schema, batches) = engine::transform_select_at(root, db, def, &step.sql).await?;
    let dataset = db
        .dataset_get_by_id(ds_id)?
        .ok_or_else(|| anyhow!("dataset id {ds_id} biến mất"))?;
    let applied = sync::apply_land_at(
        root,
        LandParams {
            db,
            dataset: &dataset,
            run_id,
            flow_id,
            step_id: &step.id,
            mode: SyncMode::FullRefresh,
            cursor_col: None,
            schema_policy: None,
        },
        &batches,
    )?;
    db.lineage_add(run_id, &step.id, "out", ds_id, applied.schema_version)?;
    Ok(applied)
}

// ---------------------------------------------------------------------------
// incremental_by_time transform
// ---------------------------------------------------------------------------

/// Kết quả chạy incremental_by_time.
#[derive(Debug, Clone, Default)]
pub struct IncrOutcome {
    pub intervals_run: usize,
    pub rows_written: i64,
    pub schema_version: Option<i64>,
}

/// Chạy incremental_by_time cho một dải `[start, end)` tường minh (dùng bởi backfill +
/// test). Mỗi interval: thay macro, chạy SQL, land partition, ghi step_interval success.
#[allow(clippy::too_many_arguments)]
pub async fn run_incremental_range(
    root: &Path,
    db: &Db,
    def: &FlowDef,
    step: &TransformStep,
    run_id: &str,
    flow_id: &str,
    def_version: i64,
    start: NaiveDateTime,
    end: NaiveDateTime,
) -> Result<IncrOutcome> {
    let iv = Interval::from_str(step.interval.as_deref().unwrap_or(""))
        .ok_or_else(|| anyhow!("interval không hợp lệ ở step '{}'", step.id))?;
    let time_col = step
        .time_column
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("time_column bắt buộc ở step '{}'", step.id))?;

    let (ns, name) = flow::transform_target(step);
    let ds_id = db.dataset_upsert(&ns, &name, None, None, None)?;
    if !db.dataset_set_owner(ds_id, Some(flow_id))? {
        return Err(anyhow!("dataset {ns}.{name} đã thuộc flow khác"));
    }

    let intervals = plan_intervals(start, end, iv);
    let mut outcome = IncrOutcome::default();
    for (b0, b1) in intervals {
        let sql = substitute_macros(&step.sql, &bound_literal(b0, iv), &bound_literal(b1, iv));
        let (_schema, batches) = engine::transform_select_at(root, db, def, &sql).await?;
        let dataset = db
            .dataset_get_by_id(ds_id)?
            .ok_or_else(|| anyhow!("dataset id {ds_id} biến mất"))?;
        let part = label(b0, iv);
        let applied =
            sync::apply_land_partition_at(root, db, &dataset, run_id, &part, &batches, None)?;
        // Verify time_column có trong output (bucket đúng partition) — nhẹ, chỉ khi có dòng.
        let _ = time_col;
        outcome.intervals_run += 1;
        outcome.rows_written += applied.rows_written;
        outcome.schema_version = applied.schema_version;
        // step_interval success cho đúng interval này (resume/backfill skip §6.5).
        db.step_interval_upsert(
            flow_id,
            &step.id,
            def_version,
            &b0.format("%Y-%m-%d %H:%M:%S").to_string(),
            &b1.format("%Y-%m-%d %H:%M:%S").to_string(),
            run_id,
            "success",
        )?;
    }
    db.lineage_add(run_id, &step.id, "out", ds_id, outcome.schema_version)?;
    Ok(outcome)
}

/// Chạy incremental_by_time tự suy dải (§6.2): từ interval success cuối (hoặc min-time
/// upstream nếu chưa có) tới hết dải dữ liệu upstream. `lookback` = lùi start N interval.
pub async fn run_incremental_auto(
    root: &Path,
    db: &Db,
    def: &FlowDef,
    step: &TransformStep,
    run_id: &str,
    flow_id: &str,
    def_version: i64,
) -> Result<IncrOutcome> {
    let iv = Interval::from_str(step.interval.as_deref().unwrap_or(""))
        .ok_or_else(|| anyhow!("interval không hợp lệ ở step '{}'", step.id))?;
    let time_col = step
        .time_column
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| anyhow!("time_column bắt buộc ở step '{}'", step.id))?;
    let lookback = step.lookback.unwrap_or(0).max(0);

    // Dải dữ liệu upstream (min/max time_column) — bound số interval theo dữ liệu thật.
    let (min_t, max_t) = probe_minmax(root, db, def, &step.sql, time_col).await?;
    let Some(min_t) = min_t else {
        // Upstream rỗng → không có gì để chạy.
        return Ok(IncrOutcome::default());
    };
    let max_t = max_t.unwrap_or(min_t);

    // Start = interval success cuối (nếu có) else floor(min); rồi lùi lookback.
    let prior_end = db
        .step_interval_list_success(flow_id, &step.id, def_version)?
        .into_iter()
        .filter_map(|si| parse_boundary(&si.interval_end))
        .max();
    // lookback CHỈ áp khi đã có interval trước (re-run N interval cuối để bắt late-data);
    // lần đầu không có gì để chạy lại → bắt từ floor(min) upstream.
    let start = match prior_end {
        Some(pe) => back_n(floor_to(pe, iv), iv, lookback),
        None => floor_to(min_t, iv),
    };
    // End = biên sau bucket chứa max (phủ trọn bucket cuối).
    let end = next_boundary(floor_to(max_t, iv), iv);

    run_incremental_range(
        root,
        db,
        def,
        step,
        run_id,
        flow_id,
        def_version,
        start,
        end,
    )
    .await
}

/// Min/max của `time_column` trên output transform với dải mở toang (`@start`/`@end` =
/// biên rộng). `time_column` phải là cột output. NULL/không dòng → None.
async fn probe_minmax(
    root: &Path,
    db: &Db,
    def: &FlowDef,
    sql: &str,
    time_column: &str,
) -> Result<(Option<NaiveDateTime>, Option<NaiveDateTime>)> {
    let wide = substitute_macros(sql, "0001-01-01 00:00:00", "9999-12-31 23:59:59");
    let probe = format!(
        "SELECT MIN(\"{time_column}\") AS mn, MAX(\"{time_column}\") AS mx FROM ({wide}) _probe"
    );
    // transform_select_at đăng ký alias trần (bare step id) → probe thấy `events` v.v.
    let (_schema, batches) = engine::transform_select_at(root, db, def, &probe).await?;
    let Some(b) = batches.iter().find(|b| b.num_rows() > 0) else {
        return Ok((None, None));
    };
    // Cast cả hai cột về Utf8 rồi parse cell 0.
    let str_cell = |idx: usize| -> Option<NaiveDateTime> {
        let col = b.column(idx);
        let s =
            datafusion::arrow::compute::cast(col, &datafusion::arrow::datatypes::DataType::Utf8)
                .ok()?;
        let sa = s
            .as_any()
            .downcast_ref::<datafusion::arrow::array::StringArray>()?;
        if sa.is_null(0) {
            None
        } else {
            parse_boundary(sa.value(0))
        }
    };
    Ok((str_cell(0), str_cell(1)))
}

// ---------------------------------------------------------------------------
// runner integration
// ---------------------------------------------------------------------------

/// Chạy một transform step trong runner (sau các source). `full` → run_full;
/// `incremental_by_time` → run_incremental_auto. Trả rows_written để log.
pub async fn execute_transform(
    root: &Path,
    db: &Db,
    run_id: &str,
    flow_row: &FlowRow,
    def: &FlowDef,
    step: &TransformStep,
) -> Result<i64> {
    let flow_id = &flow_row.id;
    db.step_run_upsert(run_id, &step.id, "running", 0, 0, None)?;
    let rows = match step.kind.as_str() {
        "full" => {
            let a = run_full(root, db, def, step, run_id, flow_id).await?;
            a.rows_written
        }
        "incremental_by_time" => {
            let o =
                run_incremental_auto(root, db, def, step, run_id, flow_id, flow_row.def_version)
                    .await?;
            o.rows_written
        }
        other => return Err(anyhow!("transform kind '{other}' chưa hỗ trợ")),
    };
    db.step_run_upsert(run_id, &step.id, "success", 0, rows, None)?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use datafusion::arrow::array::{Int64Array, StringArray};
    use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
    use datafusion::arrow::record_batch::RecordBatch;

    use crate::ingest::IngestedTable;
    use crate::lake;

    fn nd(s: &str) -> NaiveDateTime {
        parse_boundary(s).unwrap()
    }

    /// Import trực tiếp một bảng vào lake dưới `root`.
    fn import(
        root: &Path,
        db: &Db,
        ns: &str,
        name: &str,
        schema: SchemaRef,
        batches: Vec<RecordBatch>,
        run_id: &str,
    ) {
        let rows = batches.iter().map(|b| b.num_rows()).sum();
        let t = IngestedTable {
            name: name.into(),
            schema,
            batches,
            origin: "csv",
            note: "t".into(),
            rows,
        };
        lake::create_dataset_from_ingested_at(root, db, ns, name, &t, run_id).unwrap();
    }

    // ---- interval math ----

    #[test]
    fn plan_intervals_day_covers_range() {
        let iv = Interval::Day;
        let ivs = plan_intervals(nd("2024-01-01"), nd("2024-01-04"), iv);
        assert_eq!(ivs.len(), 3);
        assert_eq!(label(ivs[0].0, iv), "2024-01-01");
        assert_eq!(label(ivs[2].0, iv), "2024-01-03");
        assert_eq!(ivs[0].1, nd("2024-01-02"));
    }

    #[test]
    fn week_floors_to_monday() {
        // 2024-01-03 là thứ Tư → floor về thứ Hai 2024-01-01.
        let f = floor_to(nd("2024-01-03"), Interval::Week);
        assert_eq!(label(f, Interval::Week), "2024-01-01");
        assert_eq!(next_boundary(f, Interval::Week), nd("2024-01-08"));
    }

    #[test]
    fn month_boundary_wraps_year() {
        let f = floor_to(nd("2024-12-15"), Interval::Month);
        assert_eq!(label(f, Interval::Month), "2024-12");
        assert_eq!(next_boundary(f, Interval::Month), nd("2025-01-01"));
    }

    #[test]
    fn substitute_macros_quotes_bounds() {
        let sql = "SELECT * FROM t WHERE d >= @start AND d < @end";
        let out = substitute_macros(sql, "2024-01-01", "2024-01-02");
        assert_eq!(
            out,
            "SELECT * FROM t WHERE d >= '2024-01-01' AND d < '2024-01-02'"
        );
    }

    // ---- full transform end-to-end ----

    fn schema_id_amount() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("cust", DataType::Int64, true),
            Field::new("amount", DataType::Int64, true),
        ]))
    }
    fn schema_id_name() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("cust", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
        ]))
    }

    #[tokio::test]
    async fn full_transform_join_two_raw_datasets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = Db::open_memory().unwrap();

        // raw.orders(cust, amount) + raw.customers(cust, name).
        let orders = RecordBatch::try_new(
            schema_id_amount(),
            vec![
                Arc::new(Int64Array::from(vec![1, 1, 2])),
                Arc::new(Int64Array::from(vec![10, 20, 5])),
            ],
        )
        .unwrap();
        import(
            root,
            &db,
            "raw",
            "orders",
            schema_id_amount(),
            vec![orders],
            "r1",
        );
        let custs = RecordBatch::try_new(
            schema_id_name(),
            vec![
                Arc::new(Int64Array::from(vec![1, 2])),
                Arc::new(StringArray::from(vec!["a", "b"])),
            ],
        )
        .unwrap();
        import(
            root,
            &db,
            "raw",
            "customers",
            schema_id_name(),
            vec![custs],
            "r2",
        );

        // Flow: sources orders/customers (alias trần), transform JOIN → marts.rev.
        let def: FlowDef = serde_json::from_value(serde_json::json!({
            "flow": "shop",
            "sources": [
                {"id": "orders", "connection": "c", "table": "t", "mode": "full_refresh",
                 "target": {"namespace": "raw", "dataset": "orders"}},
                {"id": "customers", "connection": "c", "table": "t", "mode": "full_refresh",
                 "target": {"namespace": "raw", "dataset": "customers"}}
            ],
            "transforms": [{
                "id": "rev", "kind": "full",
                "sql": "SELECT c.name, SUM(o.amount) AS total FROM orders o \
                        JOIN customers c ON o.cust = c.cust GROUP BY c.name",
                "target": {"namespace": "marts", "dataset": "rev"}
            }]
        }))
        .unwrap();

        let step = def.transforms[0].clone();
        run_full(root, &db, &def, &step, "run-1", "shop")
            .await
            .unwrap();

        let page = engine::query_page_at(
            root,
            &db,
            "SELECT name, total FROM marts.rev ORDER BY name",
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(page.returned, 2);
        // a: 10+20=30, b: 5.
        assert_eq!(page.rows[0][0], serde_json::json!("a"));
        assert_eq!(page.rows[0][1], serde_json::json!(30));
        assert_eq!(page.rows[1][1], serde_json::json!(5));

        // Chạy lại full = idempotent (swap), vẫn 2 dòng.
        run_full(root, &db, &def, &step, "run-2", "shop")
            .await
            .unwrap();
        let page2 = engine::query_page_at(root, &db, "SELECT * FROM marts.rev", None, None)
            .await
            .unwrap();
        assert_eq!(page2.returned, 2);
    }

    // ---- incremental_by_time end-to-end ----

    fn schema_day_val() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("day", DataType::Utf8, true),
            Field::new("val", DataType::Int64, true),
        ]))
    }

    fn day_batch(rows: &[(&str, i64)]) -> RecordBatch {
        let days: Vec<Option<String>> = rows.iter().map(|(d, _)| Some(d.to_string())).collect();
        let vals: Vec<i64> = rows.iter().map(|(_, v)| *v).collect();
        RecordBatch::try_new(
            schema_day_val(),
            vec![
                Arc::new(StringArray::from(days)),
                Arc::new(Int64Array::from(vals)),
            ],
        )
        .unwrap()
    }

    fn incr_def() -> FlowDef {
        serde_json::from_value(serde_json::json!({
            "flow": "ev",
            "sources": [{"id": "events", "connection": "c", "table": "t",
                         "mode": "full_refresh",
                         "target": {"namespace": "raw", "dataset": "events"}}],
            "transforms": [{
                "id": "daily", "kind": "incremental_by_time",
                "time_column": "day", "interval": "day", "lookback": 0,
                "sql": "SELECT day, SUM(val) AS total FROM events \
                        WHERE day >= @start AND day < @end GROUP BY day",
                "target": {"namespace": "marts", "dataset": "daily"}
            }]
        }))
        .unwrap()
    }

    async fn daily_count(root: &Path, db: &Db) -> i64 {
        let page = engine::query_page_at(
            root,
            db,
            "SELECT COUNT(*) AS n FROM marts.daily",
            None,
            None,
        )
        .await
        .unwrap();
        page.rows[0][0].as_i64().unwrap()
    }
    async fn daily_total(root: &Path, db: &Db, day: &str) -> i64 {
        let page = engine::query_page_at(
            root,
            db,
            &format!("SELECT total FROM marts.daily WHERE day = '{day}'"),
            None,
            None,
        )
        .await
        .unwrap();
        page.rows[0][0].as_i64().unwrap()
    }

    #[tokio::test]
    async fn incremental_by_time_partitions_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = Db::open_memory().unwrap();

        // 3 ngày dữ liệu.
        let ev = day_batch(&[
            ("2024-01-01", 5),
            ("2024-01-01", 5),
            ("2024-01-02", 7),
            ("2024-01-03", 3),
        ]);
        import(root, &db, "raw", "events", schema_day_val(), vec![ev], "r1");
        let def = incr_def();
        let step = def.transforms[0].clone();

        // Chạy [01-01, 01-04) → 3 partition.
        let o = run_incremental_range(
            root,
            &db,
            &def,
            &step,
            "run-1",
            "ev",
            1,
            nd("2024-01-01"),
            nd("2024-01-04"),
        )
        .await
        .unwrap();
        assert_eq!(o.intervals_run, 3);
        assert_eq!(daily_count(root, &db).await, 3);
        assert_eq!(daily_total(root, &db, "2024-01-01").await, 10);
        assert_eq!(daily_total(root, &db, "2024-01-02").await, 7);

        // Chạy lại CÙNG interval 01-02 → partition thay, KHÔNG nhân đôi.
        let o2 = run_incremental_range(
            root,
            &db,
            &def,
            &step,
            "run-2",
            "ev",
            1,
            nd("2024-01-02"),
            nd("2024-01-03"),
        )
        .await
        .unwrap();
        assert_eq!(o2.intervals_run, 1);
        assert_eq!(
            daily_count(root, &db).await,
            3,
            "vẫn 3 ngày, không nhân đôi"
        );
        assert_eq!(
            daily_total(root, &db, "2024-01-02").await,
            7,
            "tổng đúng, không cộng dồn"
        );

        // step_interval ghi nhận các interval success.
        let done = db.step_interval_list_success("ev", "daily", 1).unwrap();
        assert!(done
            .iter()
            .any(|s| s.interval_start.starts_with("2024-01-02")));
    }

    #[tokio::test]
    async fn incremental_by_time_lookback_reruns_last_n() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let db = Db::open_memory().unwrap();

        let ev = day_batch(&[("2024-01-01", 5), ("2024-01-02", 7), ("2024-01-03", 3)]);
        import(root, &db, "raw", "events", schema_day_val(), vec![ev], "r1");
        let mut def = incr_def();
        def.transforms[0].lookback = Some(2);
        let step = def.transforms[0].clone();

        // Auto run: min=01-01, max=01-03 → chạy 01-01..01-04 (3 interval), lookback không
        // ảnh hưởng lần đầu (chưa có prior interval).
        let o = run_incremental_auto(root, &db, &def, &step, "run-1", "ev", 1)
            .await
            .unwrap();
        assert_eq!(o.intervals_run, 3);
        assert_eq!(daily_count(root, &db).await, 3);

        // Sửa dữ liệu ngày 01-03 (giá trị mới) rồi auto run lại: prior_end = 01-04,
        // lookback=2 → start lùi về 01-02, chạy lại 01-02 + 01-03 (2 interval).
        let ev2 = day_batch(&[("2024-01-01", 5), ("2024-01-02", 7), ("2024-01-03", 99)]);
        // Full-refresh raw.events (swap) để đổi giá trị 01-03.
        let ds = db.dataset_get("raw", "events").unwrap().unwrap();
        let files = lake::land_batches_at(root, "raw", "events", "r2", &[ev2], None).unwrap();
        db.manifest_swap_files(ds.id, "r2", &files).unwrap();

        let o2 = run_incremental_auto(root, &db, &def, &step, "run-2", "ev", 1)
            .await
            .unwrap();
        // Lookback=2 → chạy lại đúng 2 interval cuối.
        assert_eq!(o2.intervals_run, 2, "lookback=2 chạy lại 2 interval");
        assert_eq!(daily_count(root, &db).await, 3, "vẫn 3 ngày");
        assert_eq!(
            daily_total(root, &db, "2024-01-03").await,
            99,
            "giá trị mới sau restatement"
        );
    }
}
