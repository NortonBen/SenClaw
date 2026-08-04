//! Sync modes Phase 2 (design §6.2): `full_refresh` + `incremental_append`.
//!
//! Hai việc:
//!   * `plan_extract` — dựng `ExtractSpec` cho connector: full = không cursor;
//!     incremental = đọc watermark (`stream_state`) làm biên dưới closed-range `>=`.
//!   * `apply_land` — land batch → commit manifest theo mode: full = swap nguyên tử
//!     (tombstone hết file cũ + thêm mới, 1 txn — `db.manifest_swap_files`);
//!     incremental = chỉ thêm file, rồi đẩy watermark (`stream_state_set_monotonic`).
//!
//! **Dedupe biên** (§6.2): closed-range `>=` kéo lại các row có `cursor == watermark`
//! đã nạp lần trước. `prepare_incremental` lọc chúng bằng `boundary_hashes` (hash toàn
//! bộ row) rồi tính watermark mới + tập hash biên mới. Hàm này THUẦN — test không cần DB.
//!
//! Watermark so sánh dạng CHUỖI (khớp `stream_state_set_monotonic` trong db.rs): hợp
//! với cursor ISO-8601/lexicographic đơn điệu. Cursor số không zero-pad có thể sai thứ
//! tự từ điển — giới hạn thiết kế đã ghi ở db.rs.

#![allow(dead_code)]

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use serde_json::Value;

use std::sync::Arc;

use datafusion::arrow::array::{Array, ArrayRef, BooleanArray, StringArray, UInt32Array};
use datafusion::arrow::compute::{cast, concat_batches, filter_record_batch, take};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;

use std::collections::HashMap;

use crate::connectors::{CursorOp, CursorPred, ExtractSpec, SourceRel};
use crate::db::{Dataset, Db};
use crate::lake;

/// Mode sync Phase 2. `incremental_merge`/`snapshot` = Phase 3 (không có ở đây).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    FullRefresh,
    IncrementalAppend,
}

impl SyncMode {
    /// Map chuỗi mode extract cơ bản. merge/snapshot dùng path riêng (apply_merge/apply_snapshot).
    pub fn from_flow_mode(s: &str) -> Option<SyncMode> {
        match s {
            "full_refresh" => Some(SyncMode::FullRefresh),
            "incremental_append" => Some(SyncMode::IncrementalAppend),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// plan_extract
// ---------------------------------------------------------------------------

/// Dựng `ExtractSpec`. Với `IncrementalAppend`, đọc watermark `stream_state` của
/// (flow, step) làm biên dưới; chưa có watermark → dùng `initial` (nếu có) — khớp
/// dlt `initial_value`; không có cả hai → không cursor (kéo hết lần đầu).
#[allow(clippy::too_many_arguments)]
pub fn plan_extract(
    db: &Db,
    flow_id: &str,
    step_id: &str,
    source: SourceRel,
    columns: Option<Vec<String>>,
    mode: SyncMode,
    cursor_col: Option<&str>,
    initial: Option<&Value>,
    batch_rows: usize,
) -> Result<ExtractSpec> {
    let cursor = match mode {
        SyncMode::FullRefresh => None,
        SyncMode::IncrementalAppend => {
            let col = cursor_col
                .ok_or_else(|| anyhow!("incremental_append cần cursor.column"))?
                .to_string();
            // Watermark sống > initial. Cả hai giữ dạng JSON Value để bind param.
            let from = match db.stream_state_get(flow_id, step_id)? {
                Some(st) => st.last_value.map(|v| parse_watermark(&v)),
                None => None,
            }
            .or_else(|| initial.cloned());
            from.map(|from| CursorPred {
                column: col,
                op: CursorOp::Ge, // closed-range; dedupe biên ở apply_land
                from,
                to: None,
            })
        }
    };

    Ok(ExtractSpec {
        source,
        columns,
        cursor,
        batch_rows: batch_rows.max(1),
    })
}

/// Watermark persist ở `stream_state.last_value` là TEXT. Khôi phục kiểu JSON hợp lý
/// để bind param đúng (số → Number, còn lại → String).
fn parse_watermark(s: &str) -> Value {
    if let Ok(i) = s.parse::<i64>() {
        Value::from(i)
    } else if let Ok(f) = s.parse::<f64>() {
        serde_json::Number::from_f64(f)
            .map(Value::Number)
            .unwrap_or_else(|| Value::from(s))
    } else {
        Value::from(s)
    }
}

// ---------------------------------------------------------------------------
// apply_land
// ---------------------------------------------------------------------------

/// Kết quả một lần land.
#[derive(Debug, Clone)]
pub struct AppliedLand {
    /// Số dòng thực land (sau dedupe biên).
    pub rows_written: i64,
    /// Số file mới thêm vào manifest.
    pub files: usize,
    /// Watermark mới (nếu incremental và có tiến).
    pub watermark: Option<String>,
    /// schema_version hiện hành sau land (cho lineage).
    pub schema_version: Option<i64>,
}

/// Tham số land (gom để tránh hàm quá nhiều đối số).
pub struct LandParams<'a> {
    pub db: &'a Db,
    pub dataset: &'a Dataset,
    pub run_id: &'a str,
    pub flow_id: &'a str,
    pub step_id: &'a str,
    pub mode: SyncMode,
    pub cursor_col: Option<&'a str>,
    /// Chính sách schema evolution (§6.4) — None = mặc định (evolve + variant).
    pub schema_policy: Option<&'a serde_json::Value>,
}

/// Commit batch vào lake dưới `config::lake_dir()`. Xem `apply_land_at` cho gốc tùy chọn.
pub fn apply_land(p: LandParams, batches: &[RecordBatch]) -> Result<AppliedLand> {
    apply_land_at(&crate::config::lake_dir(), p, batches)
}

pub fn apply_land_at(
    root: &std::path::Path,
    p: LandParams,
    batches: &[RecordBatch],
) -> Result<AppliedLand> {
    let ns = &p.dataset.namespace;
    let name = &p.dataset.name;
    let ds_id = p.dataset.id;

    // Schema evolution (§6.4): unify schema batch vs catalog, áp policy, bump version.
    // `landed` = batch đã ép về schema hiệu lực (cột biến thể chuyển sang col__v_text).
    let evo = evolve_schema(p.db, p.dataset, batches, p.schema_policy)?;
    let schema_version = evo.version;

    match p.mode {
        SyncMode::FullRefresh => {
            let landed = conform_land_batches(batches, &evo)?;
            let files = lake::land_batches_at(root, ns, name, p.run_id, &landed, None)?;
            let rows_written: i64 = files.iter().map(|f| f.row_count).sum();
            // Swap nguyên tử: file cũ tombstone + file mới active trong 1 txn.
            p.db.manifest_swap_files(ds_id, p.run_id, &files)?;
            Ok(AppliedLand {
                rows_written,
                files: files.len(),
                watermark: None,
                schema_version,
            })
        }
        SyncMode::IncrementalAppend => {
            let col = p
                .cursor_col
                .ok_or_else(|| anyhow!("incremental_append cần cursor.column"))?;
            // Trạng thái biên trước.
            let (prev_watermark, prev_hashes) = match p.db.stream_state_get(p.flow_id, p.step_id)? {
                Some(st) => {
                    let hashes = st
                        .boundary_hashes
                        .as_deref()
                        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
                        .unwrap_or_default()
                        .into_iter()
                        .collect::<HashSet<String>>();
                    (st.last_value, hashes)
                }
                None => (None, HashSet::new()),
            };

            // Dedupe biên trên batch GỐC (cursor col còn nguyên kiểu), rồi ép schema.
            let plan = prepare_incremental(batches, col, prev_watermark.as_deref(), &prev_hashes)?;
            let landed = conform_land_batches(&plan.batches, &evo)?;
            let files = lake::land_batches_at(root, ns, name, p.run_id, &landed, None)?;
            let rows_written: i64 = files.iter().map(|f| f.row_count).sum();
            p.db.manifest_add_files(ds_id, p.run_id, &files)?;

            // Đẩy watermark (chỉ tiến — predicate monotonic trong db.rs).
            let mut applied_wm = None;
            if let Some(wm) = &plan.new_watermark {
                let hashes_json = serde_json::to_string(&plan.new_boundary_hashes).ok();
                let advanced = p.db.stream_state_set_monotonic(
                    p.flow_id,
                    p.step_id,
                    col,
                    wm,
                    hashes_json.as_deref(),
                )?;
                if advanced {
                    applied_wm = Some(wm.clone());
                }
            }

            Ok(AppliedLand {
                rows_written,
                files: files.len(),
                watermark: applied_wm,
                schema_version,
            })
        }
    }
}

/// Land MỘT partition (§6.2 incremental_by_time): evolve schema như apply_land, ép
/// batch, ghi file partition, rồi `manifest_replace_partition` (tombstone file cũ của
/// partition + thêm mới, 1 txn) — "delete interval + insert" idempotent.
pub fn apply_land_partition_at(
    root: &std::path::Path,
    db: &Db,
    dataset: &Dataset,
    run_id: &str,
    part_label: &str,
    batches: &[RecordBatch],
    schema_policy: Option<&serde_json::Value>,
) -> Result<AppliedLand> {
    let ns = &dataset.namespace;
    let name = &dataset.name;
    let evo = evolve_schema(db, dataset, batches, schema_policy)?;
    let landed = conform_land_batches(batches, &evo)?;
    let files = lake::land_partition_at(root, ns, name, run_id, part_label, &landed)?;
    let rows_written: i64 = files.iter().map(|f| f.row_count).sum();
    db.manifest_replace_partition(dataset.id, run_id, part_label, &files)?;
    Ok(AppliedLand {
        rows_written,
        files: files.len(),
        watermark: None,
        schema_version: evo.version,
    })
}

// ---------------------------------------------------------------------------
// Schema evolution (§6.4) — thuần trên Arrow schema + policy
// ---------------------------------------------------------------------------

/// Ba nút chính sách khi cột MỚI xuất hiện ở nguồn (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewColPolicy {
    /// Thêm cột nullable vào catalog (mặc định) — file cũ đọc NULL.
    Evolve,
    /// Từ chối land (schema drift) — hard error.
    Freeze,
    /// Bỏ qua cột mới (không vào catalog) — dữ liệu cột đó bị loại.
    Discard,
}

/// Ba nút chính sách khi kiểu cột đổi KHÔNG lossless (§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypePolicy {
    /// Thêm cột phụ `col__v_text` (mặc định) — không fail load.
    Variant,
    /// Từ chối land — hard error.
    Freeze,
    /// Bỏ qua thay đổi kiểu — ép về kiểu cũ (cast fail → NULL).
    Discard,
}

/// Chính sách schema evolution đã phân giải (§6.4).
#[derive(Debug, Clone, Copy)]
pub struct SchemaPolicy {
    pub new_columns: NewColPolicy,
    pub type_change: TypePolicy,
}

impl Default for SchemaPolicy {
    fn default() -> Self {
        SchemaPolicy {
            new_columns: NewColPolicy::Evolve,
            type_change: TypePolicy::Variant,
        }
    }
}

impl SchemaPolicy {
    /// Đọc từ JSON `{new_columns, type_change}` (khai trong DSL). Key/giá trị lạ giữ mặc định.
    pub fn from_json(v: Option<&serde_json::Value>) -> Self {
        let mut p = SchemaPolicy::default();
        let Some(obj) = v.and_then(|v| v.as_object()) else {
            return p;
        };
        match obj.get("new_columns").and_then(|x| x.as_str()) {
            Some("evolve") => p.new_columns = NewColPolicy::Evolve,
            Some("freeze") => p.new_columns = NewColPolicy::Freeze,
            Some("discard") => p.new_columns = NewColPolicy::Discard,
            _ => {}
        }
        match obj.get("type_change").and_then(|x| x.as_str()) {
            Some("variant") => p.type_change = TypePolicy::Variant,
            Some("freeze") => p.type_change = TypePolicy::Freeze,
            Some("discard") => p.type_change = TypePolicy::Discard,
            _ => {}
        }
        p
    }
}

/// Kết quả evolve một lần land: schema hiệu lực + version + danh sách cột biến thể.
#[derive(Debug, Clone)]
pub struct SchemaEvo {
    /// schema_version hiện hành (đã bump nếu có thay đổi). None khi không land gì.
    pub version: Option<i64>,
    /// Schema catalog hiệu lực để ép batch land theo.
    pub effective: Option<SchemaRef>,
    /// Tên cột GỐC (base) đã chuyển sang biến thể — giá trị mới đưa vào `base__v_text`.
    pub variant_bases: HashSet<String>,
}

/// Hậu tố cột biến thể (§6.4).
const VARIANT_SUFFIX: &str = "__v_text";

/// Diff giữa schema catalog hiện hành và schema batch → schema hiệu lực + version.
/// Không có schema cũ (dataset mới) → set schema batch làm version 1. Trả `SchemaEvo`.
fn evolve_schema(
    db: &Db,
    dataset: &Dataset,
    batches: &[RecordBatch],
    policy_json: Option<&serde_json::Value>,
) -> Result<SchemaEvo> {
    let policy = SchemaPolicy::from_json(policy_json);
    let incoming: SchemaRef = match batches.first() {
        Some(b) => b.schema(),
        None => {
            // Không có dữ liệu land — giữ nguyên schema hiện hành (nếu có).
            let cur = db.schema_version_current(dataset.id)?;
            return Ok(SchemaEvo {
                version: cur.map(|s| s.version),
                effective: None,
                variant_bases: HashSet::new(),
            });
        }
    };

    let current = db.schema_version_current(dataset.id)?;
    let Some(cur) = current else {
        // Dataset mới: schema batch = version 1 (không biến đổi).
        let v =
            db.schema_version_add(dataset.id, &lake::schema_to_json(&incoming), Some("land"))?;
        return Ok(SchemaEvo {
            version: Some(v),
            effective: Some(incoming),
            variant_bases: HashSet::new(),
        });
    };

    let cur_schema = lake::schema_from_json(&cur.arrow_schema)?;
    let plan = unify_schema(&cur_schema, &incoming, policy)?;

    if !plan.changed {
        return Ok(SchemaEvo {
            version: Some(cur.version),
            effective: Some(cur_schema),
            variant_bases: plan.variant_bases,
        });
    }

    let v = db.schema_version_add(
        dataset.id,
        &lake::schema_to_json(&plan.target),
        Some(&plan.change_summary),
    )?;
    Ok(SchemaEvo {
        version: Some(v),
        effective: Some(plan.target),
        variant_bases: plan.variant_bases,
    })
}

struct UnifyPlan {
    target: SchemaRef,
    variant_bases: HashSet<String>,
    changed: bool,
    change_summary: String,
}

/// Unify schema hiện hành `cur` với `incoming` theo TÊN cột (§6.4). Trả schema hiệu lực.
/// Cột chỉ-có-ở-cur giữ nguyên (file mới land NULL). Cột mới ở incoming → theo
/// `new_columns` policy. Kiểu đổi lossless → widen; không lossless → theo `type_change`.
fn unify_schema(cur: &SchemaRef, incoming: &SchemaRef, policy: SchemaPolicy) -> Result<UnifyPlan> {
    let mut fields: Vec<Field> = cur.fields().iter().map(|f| f.as_ref().clone()).collect();
    let mut variant_bases: HashSet<String> = HashSet::new();
    let mut added = Vec::new();
    let mut widened = Vec::new();
    let mut variant = Vec::new();

    // Vị trí cột theo tên trong `fields` (cur), để cập nhật tại chỗ khi widen.
    let pos = |fields: &[Field], name: &str| fields.iter().position(|f| f.name() == name);

    for inf in incoming.fields() {
        let iname = inf.name();
        match cur.field_with_name(iname) {
            Ok(cf) => {
                let ctype = cf.data_type();
                let itype = inf.data_type();
                if ctype == itype {
                    continue; // không đổi
                }
                if can_widen(ctype, itype) {
                    // incoming rộng hơn → catalog nâng lên kiểu rộng (lossless).
                    if let Some(i) = pos(&fields, iname) {
                        fields[i] = Field::new(iname, itype.clone(), true);
                    }
                    widened.push(iname.to_string());
                } else if can_widen(itype, ctype) {
                    // incoming HẸP hơn kiểu catalog → giữ catalog, cast lên khi land. Không đổi.
                    continue;
                } else {
                    // Không tương thích → theo type_change policy.
                    match policy.type_change {
                        TypePolicy::Freeze => {
                            anyhow::bail!(
                                "schema drift: cột '{iname}' đổi kiểu {ctype:?}→{itype:?} \
                                 (type_change=freeze)"
                            );
                        }
                        TypePolicy::Discard => {
                            // Giữ kiểu cũ, cast fail → NULL lúc land. Không đổi schema.
                            continue;
                        }
                        TypePolicy::Variant => {
                            variant_bases.insert(iname.to_string());
                            let vt = format!("{iname}{VARIANT_SUFFIX}");
                            if pos(&fields, &vt).is_none() {
                                fields.push(Field::new(&vt, DataType::Utf8, true));
                            }
                            variant.push(iname.to_string());
                        }
                    }
                }
            }
            Err(_) => {
                // Cột mới ở incoming.
                match policy.new_columns {
                    NewColPolicy::Freeze => {
                        anyhow::bail!("schema drift: cột mới '{iname}' (new_columns=freeze)");
                    }
                    NewColPolicy::Discard => continue,
                    NewColPolicy::Evolve => {
                        // Thêm nullable — file cũ đọc NULL.
                        fields.push(Field::new(iname, inf.data_type().clone(), true));
                        added.push(iname.to_string());
                    }
                }
            }
        }
    }

    let changed = !added.is_empty() || !widened.is_empty() || !variant.is_empty();
    let mut summary = Vec::new();
    if !added.is_empty() {
        summary.push(format!("add:{}", added.join(",")));
    }
    if !widened.is_empty() {
        summary.push(format!("widen:{}", widened.join(",")));
    }
    if !variant.is_empty() {
        summary.push(format!("variant:{}", variant.join(",")));
    }

    Ok(UnifyPlan {
        target: Arc::new(Schema::new(fields)),
        variant_bases,
        changed,
        change_summary: if summary.is_empty() {
            "land".into()
        } else {
            summary.join("; ")
        },
    })
}

/// Tập cast lossless mà SchemaAdapter DF 54 làm được (§6.4): mở rộng int/uint theo
/// độ rộng + float32→float64. `from` là kiểu HẸP, `to` là kiểu RỘNG hơn.
fn can_widen(from: &DataType, to: &DataType) -> bool {
    let int_rank = |d: &DataType| -> Option<u8> {
        match d {
            DataType::Int8 => Some(1),
            DataType::Int16 => Some(2),
            DataType::Int32 => Some(3),
            DataType::Int64 => Some(4),
            _ => None,
        }
    };
    let uint_rank = |d: &DataType| -> Option<u8> {
        match d {
            DataType::UInt8 => Some(1),
            DataType::UInt16 => Some(2),
            DataType::UInt32 => Some(3),
            DataType::UInt64 => Some(4),
            _ => None,
        }
    };
    if let (Some(a), Some(b)) = (int_rank(from), int_rank(to)) {
        return b > a;
    }
    if let (Some(a), Some(b)) = (uint_rank(from), uint_rank(to)) {
        return b > a;
    }
    matches!((from, to), (DataType::Float32, DataType::Float64))
}

/// Ép danh sách batch về schema hiệu lực để land (§6.4). Cột biến thể: giá trị GỐC
/// (kiểu mới) đổ vào `base__v_text` (cast Utf8), còn cột `base` (kiểu cũ) land NULL.
fn conform_land_batches(batches: &[RecordBatch], evo: &SchemaEvo) -> Result<Vec<RecordBatch>> {
    let Some(target) = evo.effective.as_ref() else {
        return Ok(batches.to_vec());
    };
    let mut out = Vec::with_capacity(batches.len());
    for b in batches {
        out.push(conform_land_batch(b, target, &evo.variant_bases)?);
    }
    Ok(out)
}

fn conform_land_batch(
    batch: &RecordBatch,
    target: &SchemaRef,
    variant_bases: &HashSet<String>,
) -> Result<RecordBatch> {
    let n = batch.num_rows();
    let mut cols: Vec<ArrayRef> = Vec::with_capacity(target.fields().len());
    for field in target.fields() {
        let fname = field.name();
        // Cột biến thể phụ `base__v_text` — đổ text từ cột gốc `base` của batch.
        if let Some(base) = fname.strip_suffix(VARIANT_SUFFIX) {
            if variant_bases.contains(base) {
                let col = match batch.schema().index_of(base) {
                    Ok(i) => cast(batch.column(i), &DataType::Utf8)
                        .unwrap_or_else(|_| new_null(&DataType::Utf8, n)),
                    Err(_) => new_null(&DataType::Utf8, n),
                };
                cols.push(col);
                continue;
            }
        }
        // Cột GỐC của một biến thể → land NULL (giá trị đã dời sang __v_text).
        if variant_bases.contains(fname) && incoming_type_differs(batch, fname, field.data_type()) {
            cols.push(new_null(field.data_type(), n));
            continue;
        }
        // Cột thường: lấy từ batch (cast về kiểu target, fail → NULL) hoặc NULL nếu thiếu.
        let col = match batch.schema().index_of(fname) {
            Ok(i) => {
                let c = batch.column(i);
                if c.data_type() == field.data_type() {
                    c.clone()
                } else {
                    cast(c, field.data_type()).unwrap_or_else(|_| new_null(field.data_type(), n))
                }
            }
            Err(_) => new_null(field.data_type(), n),
        };
        cols.push(col);
    }
    RecordBatch::try_new(target.clone(), cols)
        .map_err(|e| anyhow!("ép batch land về schema hiệu lực thất bại: {e}"))
}

/// Batch có cột `name` với kiểu KHÁC `catalog_type` không (dùng để quyết định NULL-hoá
/// cột gốc của biến thể). Batch thiếu cột → false (không NULL-hoá).
fn incoming_type_differs(batch: &RecordBatch, name: &str, catalog_type: &DataType) -> bool {
    match batch.schema().index_of(name) {
        Ok(i) => batch.column(i).data_type() != catalog_type,
        Err(_) => false,
    }
}

fn new_null(dt: &DataType, n: usize) -> ArrayRef {
    datafusion::arrow::array::new_null_array(dt, n)
}

// ---------------------------------------------------------------------------
// dedupe biên — thuần, test không cần DB
// ---------------------------------------------------------------------------

/// Kết quả chuẩn bị incremental: batch đã lọc + watermark + tập hash biên mới.
#[derive(Debug, Clone)]
pub struct IncrementalPlan {
    pub batches: Vec<RecordBatch>,
    /// Watermark mới = max cursor của row GIỮ LẠI (dạng chuỗi). None nếu không có row.
    pub new_watermark: Option<String>,
    /// Hash các row GIỮ LẠI có `cursor == new_watermark` — dedupe biên lần sau.
    pub new_boundary_hashes: Vec<String>,
    pub rows_kept: usize,
}

/// Lọc row trùng biên rồi tính watermark/hash mới. Row bị bỏ ⇔ `cursor == prev_watermark`
/// VÀ hash toàn-row nằm trong `prev_hashes` (đã nạp lần trước). Row cursor NULL: giữ,
/// không tính vào watermark.
pub fn prepare_incremental(
    batches: &[RecordBatch],
    cursor_col: &str,
    prev_watermark: Option<&str>,
    prev_hashes: &HashSet<String>,
) -> Result<IncrementalPlan> {
    let mut kept_batches = Vec::new();
    let mut max_cursor: Option<String> = None;
    // (batch_idx, row_idx, hash) của row giữ lại có cursor — để gom hash biên sau.
    let mut kept_cursor_hash: Vec<(String, String)> = Vec::new();
    let mut rows_kept = 0usize;

    for batch in batches {
        if batch.num_rows() == 0 {
            continue;
        }
        let cur_idx = batch
            .schema()
            .index_of(cursor_col)
            .map_err(|_| anyhow!("cursor column '{cursor_col}' không có trong batch"))?;

        // Cột cursor + mọi cột → chuỗi để so sánh/hash (SQLite dataset nhỏ).
        let utf8_cols = batch_utf8_columns(batch)?;
        let cursor_str = &utf8_cols[cur_idx];

        let n = batch.num_rows();
        let mut mask = Vec::with_capacity(n);
        for r in 0..n {
            let cur_val = str_cell(cursor_str, r);
            let hash = row_hash(&utf8_cols, r);
            // Bỏ khi trùng biên với lần trước. So NUMERIC khi cả hai là số (đồng bộ
            // với cursor_gt bên dưới) — nếu không, cursor số "9" vs "12" bị so lexical.
            let is_boundary_dup = match (&cur_val, prev_watermark) {
                (Some(cv), Some(pw)) => {
                    cursor_eq(Some(cv.as_str()), Some(pw)) && prev_hashes.contains(&hash)
                }
                _ => false,
            };
            if is_boundary_dup {
                mask.push(false);
                continue;
            }
            mask.push(true);
            rows_kept += 1;
            if let Some(cv) = cur_val {
                // So NUMERIC (không lexical): id 9 < 10 < 12; lexical "9" > "12" làm
                // watermark tụt → re-pull vô hạn (BUG watermark lexical).
                if max_cursor
                    .as_deref()
                    .is_none_or(|m| cursor_gt(Some(cv.as_str()), Some(m)))
                {
                    max_cursor = Some(cv.clone());
                }
                kept_cursor_hash.push((cv, hash));
            }
        }
        let mask = BooleanArray::from(mask);
        let filtered = filter_record_batch(batch, &mask)
            .map_err(|e| anyhow!("lọc batch dedupe biên thất bại: {e}"))?;
        if filtered.num_rows() > 0 {
            kept_batches.push(filtered);
        }
    }

    // Hash biên mới = hash của row giữ lại có cursor == max.
    let new_boundary_hashes: Vec<String> = match &max_cursor {
        Some(mx) => {
            let mut set: Vec<String> = kept_cursor_hash
                .iter()
                .filter(|(cv, _)| cursor_eq(Some(cv.as_str()), Some(mx.as_str())))
                .map(|(_, h)| h.clone())
                .collect();
            set.sort();
            set.dedup();
            set
        }
        None => Vec::new(),
    };

    Ok(IncrementalPlan {
        batches: kept_batches,
        new_watermark: max_cursor,
        new_boundary_hashes,
        rows_kept,
    })
}

/// Ép mọi cột của batch về `StringArray` (cast; kiểu không cast được → cột toàn null).
fn batch_utf8_columns(batch: &RecordBatch) -> Result<Vec<StringArray>> {
    let mut out = Vec::with_capacity(batch.num_columns());
    for c in 0..batch.num_columns() {
        let col = batch.column(c);
        let s = if col.data_type() == &DataType::Utf8 {
            col.as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("cột Utf8 downcast lỗi"))?
                .clone()
        } else {
            let casted = cast(col, &DataType::Utf8)
                .map_err(|e| anyhow!("cast cột về Utf8 thất bại: {e}"))?;
            casted
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow!("cast Utf8 downcast lỗi"))?
                .clone()
        };
        out.push(s);
    }
    Ok(out)
}

fn str_cell(arr: &StringArray, i: usize) -> Option<String> {
    if arr.is_null(i) {
        None
    } else {
        Some(arr.value(i).to_string())
    }
}

/// Hash FNV-1a 64-bit toàn-row (mọi cột dạng chuỗi + phân tách cột + đánh dấu NULL) →
/// hex. KHÔNG phụ thuộc build (dev-hash ngẫu nhiên), ổn định qua các run persist.
fn row_hash(cols: &[StringArray], row: usize) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let fnv = |h: &mut u64, byte: u8| {
        *h ^= byte as u64;
        *h = h.wrapping_mul(0x100_0000_01b3);
    };
    for col in cols {
        if col.is_null(row) {
            fnv(&mut h, 0); // đánh dấu NULL, phân biệt với chuỗi rỗng
        } else {
            for b in col.value(row).as_bytes() {
                fnv(&mut h, *b);
            }
        }
        fnv(&mut h, 0x1f); // dấu phân tách cột
    }
    format!("{h:016x}")
}

// ---------------------------------------------------------------------------
// incremental_merge (§6.2) — DeleteInsert | Upsert | InsertOnly
// ---------------------------------------------------------------------------

/// Chiến lược merge (§6.2). `DeleteInsert` mặc định.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    /// Xóa (tombstone) mọi partition khớp + ghi lại từ nguồn (dedupe theo PK).
    DeleteInsert,
    /// Cập nhật theo PK: partition chứa PK cũ được ghi lại; PK đổi partition → rewrite cả hai.
    Upsert,
    /// Chỉ thêm PK CHƯA có (bỏ qua PK đã tồn tại) — append-only theo identity.
    InsertOnly,
}

impl MergeStrategy {
    pub fn from_str(s: &str) -> MergeStrategy {
        match s {
            "upsert" => MergeStrategy::Upsert,
            "insert_only" => MergeStrategy::InsertOnly,
            _ => MergeStrategy::DeleteInsert,
        }
    }
}

/// Tham số merge.
pub struct MergeParams<'a> {
    pub db: &'a Db,
    pub dataset: &'a Dataset,
    pub run_id: &'a str,
    pub flow_id: &'a str,
    pub step_id: &'a str,
    /// Identity dedupe/upsert (§6.2). Bắt buộc không rỗng (validate đảm bảo).
    pub primary_key: &'a [String],
    /// Cột partition (merge_key ⊆ partition_by). Rỗng ⇒ full-rewrite (allow_full_rewrite).
    pub partition_by: &'a [String],
    pub strategy: MergeStrategy,
    /// Cột recency: dedupe giữ row cursor lớn nhất (None ⇒ giữ row xuất hiện sau).
    pub cursor_col: Option<&'a str>,
    pub schema_policy: Option<&'a serde_json::Value>,
}

/// Nhãn sentinel khi không có partition_by (allow_full_rewrite): toàn dataset = 1 partition.
const FULL_REWRITE_PART: &str = "_all";

/// Thực thi incremental_merge (§6.2). Đọc file active của partition liên quan (prune theo
/// nhãn partition + PK), gộp batch mới, dedupe theo primary_key (giữ cursor lớn nhất),
/// rồi tombstone file partition cũ + add file mới trong 1 txn/partition. PK đổi giá trị
/// partition → rewrite CẢ partition cũ lẫn mới.
pub fn apply_merge_at(
    root: &std::path::Path,
    p: MergeParams,
    batches: &[RecordBatch],
) -> Result<AppliedLand> {
    let ds = p.dataset;
    let (ns, name, ds_id) = (&ds.namespace, &ds.name, ds.id);

    let evo = evolve_schema(p.db, ds, batches, p.schema_policy)?;
    let Some(target) = evo.effective.clone() else {
        // Không có dữ liệu vào — giữ nguyên trạng thái.
        return Ok(AppliedLand {
            rows_written: 0,
            files: 0,
            watermark: None,
            schema_version: evo.version,
        });
    };
    let landed: Vec<RecordBatch> = conform_land_batches(batches, &evo)?
        .into_iter()
        .filter(|b| b.num_rows() > 0)
        .collect();
    if landed.is_empty() {
        return Ok(AppliedLand {
            rows_written: 0,
            files: 0,
            watermark: None,
            schema_version: evo.version,
        });
    }
    let inc =
        concat_batches(&target, &landed).map_err(|e| anyhow!("gộp batch merge thất bại: {e}"))?;
    let inc_n = inc.num_rows();

    let inc_pk = key_columns(&inc, p.primary_key)?;
    let inc_part = partition_labels(&inc, p.partition_by, inc_n)?;
    let inc_cursor = match p.cursor_col {
        Some(c) => Some(col_utf8_vals(&inc, c)?),
        None => None,
    };

    // Dedupe incoming theo PK — giữ row cursor lớn nhất (>= để row sau thắng khi hoà).
    let mut best: HashMap<&str, usize> = HashMap::new();
    for r in 0..inc_n {
        let pk = inc_pk[r].as_str();
        match best.get(pk) {
            None => {
                best.insert(pk, r);
            }
            Some(&br) => {
                let replace = match &inc_cursor {
                    Some(cv) => cursor_ge(cv[r].as_deref(), cv[br].as_deref()),
                    None => true,
                };
                if replace {
                    best.insert(pk, r);
                }
            }
        }
    }
    let kept: std::collections::HashSet<usize> = best.values().copied().collect();
    let incoming_pks: std::collections::HashSet<&str> = inc_pk.iter().map(|s| s.as_str()).collect();

    let existing = read_active_by_partition(root, p.db, ds_id, &target)?;

    let mut rows_written = 0i64;
    let mut files = 0usize;

    if p.strategy == MergeStrategy::InsertOnly {
        // Chỉ thêm PK chưa tồn tại ở BẤT KỲ partition nào.
        let existing_pks: std::collections::HashSet<String> = existing
            .values()
            .map(|b| key_columns(b, p.primary_key))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();
        let mut part_rows: HashMap<String, Vec<u32>> = HashMap::new();
        for r in 0..inc_n {
            if !kept.contains(&r) || existing_pks.contains(&inc_pk[r]) {
                continue;
            }
            part_rows
                .entry(inc_part[r].clone())
                .or_default()
                .push(r as u32);
        }
        for (part, idx) in part_rows {
            let seg = take_rows(&inc, &idx)?;
            let f = lake::land_partition_at(root, ns, name, p.run_id, &part, &[seg])?;
            rows_written += f.iter().map(|x| x.row_count).sum::<i64>();
            files += f.len();
            p.db.manifest_add_files(ds_id, p.run_id, &f)?;
        }
        return Ok(AppliedLand {
            rows_written,
            files,
            watermark: None,
            schema_version: evo.version,
        });
    }

    // DeleteInsert / Upsert: rewrite các partition bị ảnh hưởng.
    let mut affected: std::collections::HashSet<String> = std::collections::HashSet::new();
    for r in 0..inc_n {
        if kept.contains(&r) {
            affected.insert(inc_part[r].clone());
        }
    }
    // Partition cũ chứa PK incoming (PK đổi partition) cũng phải rewrite (xóa bản cũ).
    let mut existing_pk_by_part: HashMap<String, Vec<String>> = HashMap::new();
    for (part, b) in &existing {
        let pks = key_columns(b, p.primary_key)?;
        if pks.iter().any(|k| incoming_pks.contains(k.as_str())) {
            affected.insert(part.clone());
        }
        existing_pk_by_part.insert(part.clone(), pks);
    }

    for part in affected {
        let mut segs: Vec<RecordBatch> = Vec::new();
        // Giữ row cũ trong partition không bị PK incoming đụng tới.
        if let Some(b) = existing.get(&part) {
            let pks = &existing_pk_by_part[&part];
            let mask: Vec<bool> = pks
                .iter()
                .map(|k| !incoming_pks.contains(k.as_str()))
                .collect();
            let keptb = filter_by(b, &mask)?;
            if keptb.num_rows() > 0 {
                segs.push(keptb);
            }
        }
        // Thêm row incoming (deduped) thuộc partition này.
        let mask: Vec<bool> = (0..inc_n)
            .map(|r| kept.contains(&r) && inc_part[r] == part)
            .collect();
        let incb = filter_by(&inc, &mask)?;
        if incb.num_rows() > 0 {
            segs.push(incb);
        }
        let f = lake::land_partition_at(root, ns, name, p.run_id, &part, &segs)?;
        rows_written += f.iter().map(|x| x.row_count).sum::<i64>();
        files += f.len();
        // Rewrite partition = 1 txn (tombstone file cũ + add file mới); segs rỗng ⇒ partition rỗng.
        p.db.manifest_replace_partition(ds_id, p.run_id, &part, &f)?;
    }

    Ok(AppliedLand {
        rows_written,
        files,
        watermark: None,
        schema_version: evo.version,
    })
}

// ---------------------------------------------------------------------------
// snapshot SCD2 (§6.2)
// ---------------------------------------------------------------------------

/// Chiến lược phát hiện thay đổi (§6.2).
#[derive(Debug, Clone)]
pub enum SnapshotStrategy {
    /// So `updated_at`: row đổi ⇔ giá trị cột này khác bản current.
    Timestamp(String),
    /// So hash các cột (rỗng ⇒ toàn bộ cột business).
    Check(Vec<String>),
}

/// Xử lý row nguồn biến mất (chỉ suy được từ full extract) (§6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardDeletes {
    /// Bỏ qua — bản current giữ nguyên.
    Ignore,
    /// Đóng bản current (đưa vào history, `_is_deleted=true`).
    Invalidate,
    /// Đóng bản cũ + thêm bản current mới đánh dấu đã xóa.
    NewRecord,
}

impl HardDeletes {
    pub fn from_str(s: &str) -> HardDeletes {
        match s {
            "invalidate" => HardDeletes::Invalidate,
            "new_record" => HardDeletes::NewRecord,
            _ => HardDeletes::Ignore,
        }
    }
}

/// Tham số snapshot SCD2.
pub struct SnapshotParams<'a> {
    pub db: &'a Db,
    pub dataset: &'a Dataset,
    pub run_id: &'a str,
    pub primary_key: &'a [String],
    pub strategy: &'a SnapshotStrategy,
    pub hard_deletes: HardDeletes,
}

/// Cột meta SCD2 (§6.2) — luôn ở CUỐI schema, sau cột business.
const SCD2_META: &[&str] = &[
    "_valid_from",
    "_valid_to",
    "_is_current",
    "_row_hash",
    "_is_deleted",
];
/// Nhãn partition SCD2: bản hiện hành vs lịch sử (partition theo `_is_current`).
const PART_CURRENT: &str = "current";
const PART_HISTORY: &str = "history";

/// Schema đích SCD2 = cột business + 5 cột meta.
fn scd2_target_schema(business: &SchemaRef) -> SchemaRef {
    let mut fields: Vec<Field> = business
        .fields()
        .iter()
        .map(|f| f.as_ref().clone())
        .collect();
    fields.push(Field::new("_valid_from", DataType::Utf8, true));
    fields.push(Field::new("_valid_to", DataType::Utf8, true));
    fields.push(Field::new("_is_current", DataType::Boolean, true));
    fields.push(Field::new("_row_hash", DataType::Utf8, true));
    fields.push(Field::new("_is_deleted", DataType::Boolean, true));
    Arc::new(Schema::new(fields))
}

/// Suy schema business (bỏ 5 cột meta cuối) từ schema đích SCD2.
fn business_from_target(target: &SchemaRef) -> SchemaRef {
    let n = target.fields().len().saturating_sub(SCD2_META.len());
    let fields: Vec<Field> = target
        .fields()
        .iter()
        .take(n)
        .map(|f| f.as_ref().clone())
        .collect();
    Arc::new(Schema::new(fields))
}

/// Thực thi snapshot SCD2 (§6.2). So nguồn (full extract) vs bản current: row đổi → đóng
/// bản cũ (`_valid_to=now`, `_is_current=false`, vào partition history) + thêm bản mới;
/// row mất theo `hard_deletes`. Partition `current` được ghi lại; row đóng APPEND vào
/// `history`. unique theo (`_row_hash`, `_valid_from`).
pub fn apply_snapshot_at(
    root: &std::path::Path,
    p: SnapshotParams,
    batches: &[RecordBatch],
) -> Result<AppliedLand> {
    let ds = p.dataset;
    let (ns, name, ds_id) = (&ds.namespace, &ds.name, ds.id);
    let now = now_ts();

    let src_nonempty: Vec<RecordBatch> = batches
        .iter()
        .filter(|b| b.num_rows() > 0)
        .cloned()
        .collect();

    // Schema business: từ nguồn nếu có, else suy từ schema đích hiện có.
    let business: SchemaRef = match src_nonempty.first() {
        Some(b) => b.schema(),
        None => match p.db.schema_version_current(ds_id)? {
            Some(sv) => business_from_target(&lake::schema_from_json(&sv.arrow_schema)?),
            None => {
                return Ok(AppliedLand {
                    rows_written: 0,
                    files: 0,
                    watermark: None,
                    schema_version: None,
                });
            }
        },
    };
    let target = scd2_target_schema(&business);

    // Đảm bảo schema_version catalog (SCD2 = schema đích, không dùng evolve_schema).
    let schema_version = match p.db.schema_version_current(ds_id)? {
        Some(sv) => sv.version,
        None => {
            p.db.schema_version_add(ds_id, &lake::schema_to_json(&target), Some("snapshot"))?
        }
    };

    // Nguồn (full extract) về schema business.
    let src = if src_nonempty.is_empty() {
        RecordBatch::new_empty(business.clone())
    } else {
        let conformed: Vec<RecordBatch> = src_nonempty
            .iter()
            .map(|b| conform_to(b, &business))
            .collect::<Result<_>>()?;
        concat_batches(&business, &conformed)
            .map_err(|e| anyhow!("gộp nguồn snapshot thất bại: {e}"))?
    };
    let src_n = src.num_rows();
    let src_pk = key_columns(&src, p.primary_key)?;
    let hash_cols: Vec<String> = match p.strategy {
        SnapshotStrategy::Timestamp(col) => vec![col.clone()],
        SnapshotStrategy::Check(cols) if cols.is_empty() => {
            business.fields().iter().map(|f| f.name().clone()).collect()
        }
        SnapshotStrategy::Check(cols) => cols.clone(),
    };
    let src_hash = hash_columns(&src, &hash_cols)?;
    // Full extract có thể trùng PK — giữ bản xuất hiện sau.
    let mut src_by_pk: HashMap<String, usize> = HashMap::new();
    for r in 0..src_n {
        src_by_pk.insert(src_pk[r].clone(), r);
    }

    // Bản current hiện có.
    let existing = read_active_by_partition(root, p.db, ds_id, &target)?;
    let cur = existing.get(PART_CURRENT).cloned();
    let cur_pk = match &cur {
        Some(b) => key_columns(b, p.primary_key)?,
        None => Vec::new(),
    };
    let cur_hash = match &cur {
        Some(b) => col_utf8_vals(b, "_row_hash")?,
        None => Vec::new(),
    };
    // Cờ đã-xóa của bản current (Boolean → "true"/"false"). Row đang deleted mà sống
    // lại ở nguồn phải reinstate, không được carry (BUG SCD2 reinstate kẹt deleted).
    let cur_deleted = match &cur {
        Some(b) => col_utf8_vals(b, "_is_deleted")?,
        None => Vec::new(),
    };
    let mut cur_map: HashMap<&str, usize> = HashMap::new();
    for (i, pk) in cur_pk.iter().enumerate() {
        cur_map.insert(pk.as_str(), i);
    }

    let mut carry: Vec<u32> = Vec::new(); // giữ nguyên bản current (target rows)
    let mut close: Vec<(u32, bool)> = Vec::new(); // đóng → history (is_deleted)
    let mut new_src: Vec<u32> = Vec::new(); // bản current mới từ nguồn
    let mut del_marker: Vec<u32> = Vec::new(); // marker đã-xóa (new_record) từ bản cũ

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (pk, &r) in &src_by_pk {
        seen.insert(pk.as_str());
        match cur_map.get(pk.as_str()) {
            None => new_src.push(r as u32),
            Some(&ci) => {
                let same =
                    cur_hash.get(ci).and_then(|x| x.as_deref()) == Some(src_hash[r].as_str());
                // Bản current đang đánh dấu đã-xóa mà row xuất hiện lại ⇒ coi như ĐÃ ĐỔI
                // (đóng bản deleted + thêm bản current mới sống lại), bất kể hash trùng.
                let was_deleted = cur_deleted.get(ci).and_then(|x| x.as_deref()) == Some("true");
                if same && !was_deleted {
                    carry.push(ci as u32);
                } else {
                    close.push((ci as u32, false));
                    new_src.push(r as u32);
                }
            }
        }
    }
    // Row biến mất ở nguồn.
    for (pk, &ci) in &cur_map {
        if seen.contains(*pk) {
            continue;
        }
        match p.hard_deletes {
            HardDeletes::Ignore => carry.push(ci as u32),
            HardDeletes::Invalidate => close.push((ci as u32, true)),
            HardDeletes::NewRecord => {
                close.push((ci as u32, false));
                del_marker.push(ci as u32);
            }
        }
    }

    // ---- partition current (ghi lại toàn bộ) ----
    let mut cur_segs: Vec<RecordBatch> = Vec::new();
    if let Some(b) = &cur {
        if !carry.is_empty() {
            cur_segs.push(take_rows(b, &carry)?);
        }
    }
    if !new_src.is_empty() {
        let bus = take_rows(&src, &new_src)?;
        let n = new_src.len();
        let hashes: Vec<Option<String>> = new_src
            .iter()
            .map(|&r| Some(src_hash[r as usize].clone()))
            .collect();
        let seg = append_meta(
            &bus,
            &target,
            &vec![Some(now.clone()); n],
            &vec![None; n],
            &vec![true; n],
            &hashes,
            &vec![false; n],
        )?;
        cur_segs.push(seg);
    }
    if !del_marker.is_empty() {
        if let Some(b) = &cur {
            let bus = project_cols(&take_rows(b, &del_marker)?, &business)?;
            let n = del_marker.len();
            let hashes: Vec<Option<String>> = del_marker
                .iter()
                .map(|&r| cur_hash[r as usize].clone())
                .collect();
            let seg = append_meta(
                &bus,
                &target,
                &vec![Some(now.clone()); n],
                &vec![None; n],
                &vec![true; n],
                &hashes,
                &vec![true; n],
            )?;
            cur_segs.push(seg);
        }
    }
    let mut rows_written = 0i64;
    let mut files = 0usize;
    let cur_files = lake::land_partition_at(root, ns, name, p.run_id, PART_CURRENT, &cur_segs)?;
    rows_written += cur_files.iter().map(|x| x.row_count).sum::<i64>();
    files += cur_files.len();
    p.db.manifest_replace_partition(ds_id, p.run_id, PART_CURRENT, &cur_files)?;

    // ---- partition history (append row đóng) ----
    if !close.is_empty() {
        if let Some(b) = &cur {
            let idx: Vec<u32> = close.iter().map(|(i, _)| *i).collect();
            let flags: Vec<bool> = close.iter().map(|(_, d)| *d).collect();
            let closed = close_history_rows(&take_rows(b, &idx)?, &target, &now, &flags)?;
            let hist_files =
                lake::land_partition_at(root, ns, name, p.run_id, PART_HISTORY, &[closed])?;
            rows_written += hist_files.iter().map(|x| x.row_count).sum::<i64>();
            files += hist_files.len();
            p.db.manifest_add_files(ds_id, p.run_id, &hist_files)?;
        }
    }

    Ok(AppliedLand {
        rows_written,
        files,
        watermark: None,
        schema_version: Some(schema_version),
    })
}

// ---------------------------------------------------------------------------
// helper Arrow dùng chung cho merge + snapshot
// ---------------------------------------------------------------------------

fn now_ts() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Cast một cột về `StringArray` (đã là Utf8 → clone).
fn cast_utf8(col: &ArrayRef) -> Result<StringArray> {
    let c = if col.data_type() == &DataType::Utf8 {
        col.clone()
    } else {
        cast(col, &DataType::Utf8).map_err(|e| anyhow!("cast Utf8 thất bại: {e}"))?
    };
    Ok(c.as_any()
        .downcast_ref::<StringArray>()
        .ok_or_else(|| anyhow!("downcast Utf8 thất bại"))?
        .clone())
}

/// Giá trị chuỗi từng dòng của cột `name` (NULL → None).
fn col_utf8_vals(batch: &RecordBatch, name: &str) -> Result<Vec<Option<String>>> {
    let idx = batch
        .schema()
        .index_of(name)
        .map_err(|_| anyhow!("cột '{name}' không có trong batch"))?;
    let a = cast_utf8(batch.column(idx))?;
    Ok((0..a.len())
        .map(|r| {
            if a.is_null(r) {
                None
            } else {
                Some(a.value(r).to_string())
            }
        })
        .collect())
}

/// Khóa nối (0x1f phân tách, 0x00 đánh dấu NULL) của một tập cột — cho từng dòng.
fn key_columns(batch: &RecordBatch, cols: &[String]) -> Result<Vec<String>> {
    let n = batch.num_rows();
    let mut arrs = Vec::with_capacity(cols.len());
    for c in cols {
        let idx = batch
            .schema()
            .index_of(c)
            .map_err(|_| anyhow!("cột khóa '{c}' không có trong batch"))?;
        arrs.push(cast_utf8(batch.column(idx))?);
    }
    let mut out = Vec::with_capacity(n);
    for r in 0..n {
        let mut s = String::new();
        for a in &arrs {
            if a.is_null(r) {
                s.push('\u{0}');
            } else {
                s.push_str(a.value(r));
            }
            s.push('\u{1f}');
        }
        out.push(s);
    }
    Ok(out)
}

/// Nhãn partition từng dòng theo `partition_by` (join '__'). Rỗng ⇒ full-rewrite sentinel.
fn partition_labels(batch: &RecordBatch, cols: &[String], n: usize) -> Result<Vec<String>> {
    if cols.is_empty() {
        return Ok(vec![FULL_REWRITE_PART.to_string(); n]);
    }
    let mut arrs = Vec::with_capacity(cols.len());
    for c in cols {
        let idx = batch
            .schema()
            .index_of(c)
            .map_err(|_| anyhow!("cột partition '{c}' không có trong batch"))?;
        arrs.push(cast_utf8(batch.column(idx))?);
    }
    let mut out = Vec::with_capacity(n);
    for r in 0..n {
        let parts: Vec<String> = arrs
            .iter()
            .map(|a| {
                if a.is_null(r) {
                    "__null__".to_string()
                } else {
                    a.value(r).to_string()
                }
            })
            .collect();
        out.push(parts.join("__"));
    }
    Ok(out)
}

/// So cursor: cả hai parse số → so số; else so chuỗi. None = nhỏ nhất. `a >= b`?
fn cursor_ge(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(p), Ok(q)) => p >= q,
            _ => x >= y,
        },
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => true,
    }
}

/// So cursor lớn hơn HẲN — cùng thứ tự numeric-or-string như cursor_ge. None = nhỏ nhất.
fn cursor_gt(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(p), Ok(q)) => p > q,
            _ => x > y,
        },
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// So cursor bằng nhau — cùng thứ tự numeric-or-string như cursor_ge (để "10" == "10.0"
/// và biên số không lệch với so lexical).
fn cursor_eq(a: Option<&str>, b: Option<&str>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(p), Ok(q)) => p == q,
            _ => x == y,
        },
        (None, None) => true,
        _ => false,
    }
}

/// Hash FNV toàn-dòng trên một tập cột (dùng row_hash sẵn có).
fn hash_columns(batch: &RecordBatch, cols: &[String]) -> Result<Vec<String>> {
    let mut arrs = Vec::with_capacity(cols.len());
    for c in cols {
        let idx = batch
            .schema()
            .index_of(c)
            .map_err(|_| anyhow!("cột hash '{c}' không có trong batch"))?;
        arrs.push(cast_utf8(batch.column(idx))?);
    }
    Ok((0..batch.num_rows()).map(|r| row_hash(&arrs, r)).collect())
}

/// Lọc dòng theo mask boolean.
fn filter_by(batch: &RecordBatch, mask: &[bool]) -> Result<RecordBatch> {
    let m = BooleanArray::from(mask.to_vec());
    filter_record_batch(batch, &m).map_err(|e| anyhow!("lọc batch thất bại: {e}"))
}

/// Chọn dòng theo chỉ số (arrow take mọi cột).
fn take_rows(batch: &RecordBatch, idx: &[u32]) -> Result<RecordBatch> {
    let indices = UInt32Array::from(idx.to_vec());
    let cols: Result<Vec<ArrayRef>> = batch
        .columns()
        .iter()
        .map(|c| take(c.as_ref(), &indices, None).map_err(|e| anyhow!("take thất bại: {e}")))
        .collect();
    RecordBatch::try_new(batch.schema(), cols?)
        .map_err(|e| anyhow!("take dựng batch thất bại: {e}"))
}

/// Ép batch về `target` theo TÊN cột (thiếu → NULL, kiểu khác → cast, fail → NULL).
fn conform_to(batch: &RecordBatch, target: &SchemaRef) -> Result<RecordBatch> {
    let n = batch.num_rows();
    let mut cols: Vec<ArrayRef> = Vec::with_capacity(target.fields().len());
    for field in target.fields() {
        let col = match batch.schema().index_of(field.name()) {
            Ok(i) => {
                let c = batch.column(i);
                if c.data_type() == field.data_type() {
                    c.clone()
                } else {
                    cast(c, field.data_type()).unwrap_or_else(|_| new_null(field.data_type(), n))
                }
            }
            Err(_) => new_null(field.data_type(), n),
        };
        cols.push(col);
    }
    RecordBatch::try_new(target.clone(), cols)
        .map_err(|e| anyhow!("ép batch về target thất bại: {e}"))
}

/// Chiếu batch xuống đúng tập cột của `schema` (theo tên, giữ nguyên mảng).
fn project_cols(batch: &RecordBatch, schema: &SchemaRef) -> Result<RecordBatch> {
    let mut cols = Vec::with_capacity(schema.fields().len());
    for f in schema.fields() {
        let i = batch
            .schema()
            .index_of(f.name())
            .map_err(|_| anyhow!("cột chiếu '{}' không có", f.name()))?;
        cols.push(batch.column(i).clone());
    }
    RecordBatch::try_new(schema.clone(), cols).map_err(|e| anyhow!("chiếu batch thất bại: {e}"))
}

/// Gắn 5 cột meta SCD2 vào một batch business để thành batch schema đích.
fn append_meta(
    business: &RecordBatch,
    target: &SchemaRef,
    valid_from: &[Option<String>],
    valid_to: &[Option<String>],
    is_current: &[bool],
    row_hash: &[Option<String>],
    is_deleted: &[bool],
) -> Result<RecordBatch> {
    let mut cols: Vec<ArrayRef> = business.columns().to_vec();
    cols.push(Arc::new(StringArray::from(valid_from.to_vec())));
    cols.push(Arc::new(StringArray::from(valid_to.to_vec())));
    cols.push(Arc::new(BooleanArray::from(is_current.to_vec())));
    cols.push(Arc::new(StringArray::from(row_hash.to_vec())));
    cols.push(Arc::new(BooleanArray::from(is_deleted.to_vec())));
    RecordBatch::try_new(target.clone(), cols)
        .map_err(|e| anyhow!("gắn cột meta SCD2 thất bại: {e}"))
}

/// Đóng các bản current (đã take về target): set `_valid_to=now`, `_is_current=false`,
/// `_is_deleted=flag`. Business + `_valid_from`/`_row_hash` giữ nguyên.
fn close_history_rows(
    rows: &RecordBatch,
    target: &SchemaRef,
    now: &str,
    is_deleted: &[bool],
) -> Result<RecordBatch> {
    let n = rows.num_rows();
    let mut cols: Vec<ArrayRef> = rows.columns().to_vec();
    let vt = target
        .index_of("_valid_to")
        .map_err(|e| anyhow!("thiếu _valid_to: {e}"))?;
    let ic = target
        .index_of("_is_current")
        .map_err(|e| anyhow!("thiếu _is_current: {e}"))?;
    let idl = target
        .index_of("_is_deleted")
        .map_err(|e| anyhow!("thiếu _is_deleted: {e}"))?;
    cols[vt] = Arc::new(StringArray::from(vec![Some(now.to_string()); n]));
    cols[ic] = Arc::new(BooleanArray::from(vec![false; n]));
    cols[idl] = Arc::new(BooleanArray::from(is_deleted.to_vec()));
    RecordBatch::try_new(target.clone(), cols)
        .map_err(|e| anyhow!("đóng bản lịch sử thất bại: {e}"))
}

/// Đọc mọi file active của dataset, ép về `schema`, gộp theo nhãn partition.
fn read_active_by_partition(
    root: &std::path::Path,
    db: &Db,
    ds_id: i64,
    schema: &SchemaRef,
) -> Result<HashMap<String, RecordBatch>> {
    let files = db.manifest_active_files(ds_id)?;
    let mut grouped: HashMap<String, Vec<RecordBatch>> = HashMap::new();
    for f in files {
        let abs = root.join(&f.path);
        let part = f.partition.clone().unwrap_or_default();
        for b in lake::read_parquet_file(&abs)? {
            grouped
                .entry(part.clone())
                .or_default()
                .push(conform_to(&b, schema)?);
        }
    }
    let mut out = HashMap::new();
    for (part, batches) in grouped {
        let cb = concat_batches(schema, &batches)
            .map_err(|e| anyhow!("gộp file partition '{part}' thất bại: {e}"))?;
        out.insert(part, cb);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use datafusion::arrow::array::{Int64Array, StringArray as SA};
    use datafusion::arrow::datatypes::{Field, Schema, SchemaRef};

    fn schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("label", DataType::Utf8, true),
        ]))
    }

    fn batch(ids: Vec<i64>, labels: Vec<&str>) -> RecordBatch {
        RecordBatch::try_new(
            schema(),
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(SA::from(labels.into_iter().map(Some).collect::<Vec<_>>())),
            ],
        )
        .unwrap()
    }

    #[test]
    fn first_run_keeps_all_and_sets_watermark() {
        let b = batch(vec![1, 2, 3], vec!["a", "b", "c"]);
        let plan = prepare_incremental(&[b], "id", None, &HashSet::new()).unwrap();
        assert_eq!(plan.rows_kept, 3);
        assert_eq!(plan.new_watermark.as_deref(), Some("3"));
        assert_eq!(plan.new_boundary_hashes.len(), 1, "1 row ở biên id=3");
    }

    #[test]
    fn boundary_row_deduped_not_doubled() {
        // Lần 1: id 1,2,3 → watermark "3", hash biên của row (3,"c").
        let b1 = batch(vec![1, 2, 3], vec!["a", "b", "c"]);
        let p1 = prepare_incremental(&[b1], "id", None, &HashSet::new()).unwrap();
        let prev_hashes: HashSet<String> = p1.new_boundary_hashes.into_iter().collect();

        // Lần 2: closed-range >= "3" kéo lại (3,"c") + thêm (4,"d"),(5,"e").
        let b2 = batch(vec![3, 4, 5], vec!["c", "d", "e"]);
        let p2 = prepare_incremental(&[b2], "id", Some("3"), &prev_hashes).unwrap();
        // Row (3,"c") trùng biên bị bỏ → chỉ 2 row mới.
        assert_eq!(p2.rows_kept, 2);
        assert_eq!(p2.new_watermark.as_deref(), Some("5"));
        let total: usize = p2.batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total, 2);

        // Lần 3: nguồn không đổi, >= "5" kéo lại (5,"e") — trùng biên, bỏ hết.
        let prev2: HashSet<String> = p2.new_boundary_hashes.into_iter().collect();
        let b3 = batch(vec![5], vec!["e"]);
        let p3 = prepare_incremental(&[b3], "id", Some("5"), &prev2).unwrap();
        assert_eq!(p3.rows_kept, 0);
        // Không row → watermark None (không đẩy state, giữ "5").
        assert_eq!(p3.new_watermark, None);
    }

    #[test]
    fn boundary_dup_only_when_hash_matches() {
        // Cùng cursor biên nhưng row KHÁC nội dung (label khác) → KHÔNG phải trùng, giữ.
        let b1 = batch(vec![3], vec!["c"]);
        let p1 = prepare_incremental(&[b1], "id", None, &HashSet::new()).unwrap();
        let prev: HashSet<String> = p1.new_boundary_hashes.into_iter().collect();

        let b2 = batch(vec![3, 3], vec!["c", "c2"]);
        let p2 = prepare_incremental(&[b2], "id", Some("3"), &prev).unwrap();
        // (3,"c") trùng → bỏ; (3,"c2") mới → giữ.
        assert_eq!(p2.rows_kept, 1);
        let total: usize = p2.batches.iter().map(|b| b.num_rows()).sum();
        assert_eq!(total, 1);
    }

    #[test]
    fn watermark_numeric_not_lexical_over_ten() {
        // BUG watermark lexical: ids 1..=12, lexical max = "9" ("9" > "12"); numeric = "12".
        let ids: Vec<i64> = (1..=12).collect();
        let labels: Vec<&str> = vec!["x"; 12];
        let b = batch(ids, labels);
        let p1 = prepare_incremental(&[b], "id", None, &HashSet::new()).unwrap();
        assert_eq!(
            p1.new_watermark.as_deref(),
            Some("12"),
            "watermark phải numeric max"
        );
        assert_eq!(p1.new_boundary_hashes.len(), 1, "biên đúng 1 row id=12");

        // Steady-state: connector chỉ fetch cursor >= "12" (numeric) → chỉ id=12; trùng
        // biên → 0 row re-append (trước fix watermark tụt về "9" gây re-pull 10,11,12).
        let prev: HashSet<String> = p1.new_boundary_hashes.into_iter().collect();
        let b2 = batch(vec![12], vec!["x"]);
        let p2 = prepare_incremental(&[b2], "id", Some("12"), &prev).unwrap();
        assert_eq!(p2.rows_kept, 0, "steady-state không re-append");
    }

    #[test]
    fn parse_watermark_types() {
        assert_eq!(parse_watermark("42"), Value::from(42));
        assert_eq!(parse_watermark("2024-01-01"), Value::from("2024-01-01"));
        assert!(parse_watermark("3.5").is_number());
    }

    // ---- schema evolution (§6.4) ----

    mod evo {
        use super::*;
        use datafusion::arrow::array::Int32Array;
        use datafusion::arrow::datatypes::DataType;
        use std::path::Path;

        fn ts_id_schema(id_ty: DataType) -> SchemaRef {
            Arc::new(Schema::new(vec![
                Field::new("ts", DataType::Utf8, true),
                Field::new("id", id_ty, true),
            ]))
        }

        /// Land một batch qua đường IncrementalAppend (giữ file cũ → đọc được mixed).
        fn land_incr(root: &Path, db: &Db, run_id: &str, batches: &[RecordBatch]) {
            let ds = db
                .dataset_get_by_id(db.dataset_upsert("raw", "t", None, None, None).unwrap())
                .unwrap()
                .unwrap();
            apply_land_at(
                root,
                LandParams {
                    db,
                    dataset: &ds,
                    run_id,
                    flow_id: "f",
                    step_id: "s",
                    mode: SyncMode::IncrementalAppend,
                    cursor_col: Some("ts"),
                    schema_policy: None,
                },
                batches,
            )
            .unwrap();
        }

        #[tokio::test]
        async fn add_columns_bumps_version_old_file_reads_null() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "t", None, None, None).unwrap();

            // v1: {ts, id}.
            let b1 = RecordBatch::try_new(
                ts_id_schema(DataType::Int64),
                vec![
                    Arc::new(SA::from(vec![Some("2024-01-01")])),
                    Arc::new(Int64Array::from(vec![1])),
                ],
            )
            .unwrap();
            land_incr(root, &db, "run-1", &[b1]);
            assert_eq!(
                db.schema_version_current(ds_id).unwrap().unwrap().version,
                1
            );

            // v2: {ts, id, extra} → AddColumns.
            let s2: SchemaRef = Arc::new(Schema::new(vec![
                Field::new("ts", DataType::Utf8, true),
                Field::new("id", DataType::Int64, true),
                Field::new("extra", DataType::Utf8, true),
            ]));
            let b2 = RecordBatch::try_new(
                s2,
                vec![
                    Arc::new(SA::from(vec![Some("2024-01-02")])),
                    Arc::new(Int64Array::from(vec![2])),
                    Arc::new(SA::from(vec![Some("x")])),
                ],
            )
            .unwrap();
            land_incr(root, &db, "run-2", &[b2]);
            assert_eq!(
                db.schema_version_current(ds_id).unwrap().unwrap().version,
                2
            );

            let page = crate::engine::query_page_at(
                root,
                &db,
                "SELECT ts, id, extra FROM raw.t ORDER BY ts",
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(page.returned, 2);
            // Row cũ (ts 01-01) thiếu extra → NULL; row mới có 'x'.
            assert_eq!(page.rows[0][2], Value::Null, "file cũ đọc NULL cột mới");
            assert_eq!(page.rows[1][2], Value::from("x"));
        }

        #[tokio::test]
        async fn widen_int32_to_int64() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "t", None, None, None).unwrap();

            let b1 = RecordBatch::try_new(
                ts_id_schema(DataType::Int32),
                vec![
                    Arc::new(SA::from(vec![Some("2024-01-01")])),
                    Arc::new(Int32Array::from(vec![10])),
                ],
            )
            .unwrap();
            land_incr(root, &db, "run-1", &[b1]);

            let b2 = RecordBatch::try_new(
                ts_id_schema(DataType::Int64),
                vec![
                    Arc::new(SA::from(vec![Some("2024-01-02")])),
                    Arc::new(Int64Array::from(vec![20])),
                ],
            )
            .unwrap();
            land_incr(root, &db, "run-2", &[b2]);

            // Catalog schema id giờ là int64.
            let sv = db.schema_version_current(ds_id).unwrap().unwrap();
            assert_eq!(sv.version, 2);
            let sch = crate::lake::schema_from_json(&sv.arrow_schema).unwrap();
            assert_eq!(
                sch.field_with_name("id").unwrap().data_type(),
                &DataType::Int64
            );

            let page = crate::engine::query_page_at(
                root,
                &db,
                "SELECT id FROM raw.t ORDER BY ts",
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(page.returned, 2);
            // File cũ int32 cast lên int64 khi đọc; số nguyên vẹn.
            assert_eq!(page.rows[0][0], Value::from(10));
            assert_eq!(page.rows[1][0], Value::from(20));
        }

        #[tokio::test]
        async fn variant_incompatible_type_goes_to_v_text() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "t", None, None, None).unwrap();

            // v1: val Int64.
            let s1: SchemaRef = Arc::new(Schema::new(vec![
                Field::new("ts", DataType::Utf8, true),
                Field::new("val", DataType::Int64, true),
            ]));
            let b1 = RecordBatch::try_new(
                s1,
                vec![
                    Arc::new(SA::from(vec![Some("2024-01-01")])),
                    Arc::new(Int64Array::from(vec![1])),
                ],
            )
            .unwrap();
            land_incr(root, &db, "run-1", &[b1]);

            // v2: val Utf8 (không tương thích) → variant val__v_text.
            let s2: SchemaRef = Arc::new(Schema::new(vec![
                Field::new("ts", DataType::Utf8, true),
                Field::new("val", DataType::Utf8, true),
            ]));
            let b2 = RecordBatch::try_new(
                s2,
                vec![
                    Arc::new(SA::from(vec![Some("2024-01-02")])),
                    Arc::new(SA::from(vec![Some("hello")])),
                ],
            )
            .unwrap();
            land_incr(root, &db, "run-2", &[b2]);

            let sv = db.schema_version_current(ds_id).unwrap().unwrap();
            assert_eq!(sv.version, 2);
            let sch = crate::lake::schema_from_json(&sv.arrow_schema).unwrap();
            assert!(
                sch.field_with_name("val__v_text").is_ok(),
                "có cột biến thể"
            );
            assert_eq!(
                sch.field_with_name("val").unwrap().data_type(),
                &DataType::Int64
            );

            let page = crate::engine::query_page_at(
                root,
                &db,
                "SELECT val, val__v_text FROM raw.t ORDER BY ts",
                None,
                None,
            )
            .await
            .unwrap();
            assert_eq!(page.returned, 2);
            // Row cũ: val=1, v_text NULL. Row mới: val NULL, v_text='hello'.
            assert_eq!(page.rows[0][0], Value::from(1));
            assert_eq!(page.rows[0][1], Value::Null);
            assert_eq!(page.rows[1][0], Value::Null);
            assert_eq!(page.rows[1][1], Value::from("hello"));
        }

        #[test]
        fn unify_freeze_rejects_new_column() {
            let cur: SchemaRef =
                Arc::new(Schema::new(vec![Field::new("a", DataType::Int64, true)]));
            let inc: SchemaRef = Arc::new(Schema::new(vec![
                Field::new("a", DataType::Int64, true),
                Field::new("b", DataType::Int64, true),
            ]));
            let policy = SchemaPolicy {
                new_columns: NewColPolicy::Freeze,
                type_change: TypePolicy::Variant,
            };
            assert!(unify_schema(&cur, &inc, policy).is_err());
        }

        #[test]
        fn can_widen_matrix() {
            assert!(can_widen(&DataType::Int32, &DataType::Int64));
            assert!(can_widen(&DataType::Int8, &DataType::Int32));
            assert!(can_widen(&DataType::Float32, &DataType::Float64));
            assert!(!can_widen(&DataType::Int64, &DataType::Int32)); // hẹp hơn
            assert!(!can_widen(&DataType::Int64, &DataType::Utf8)); // không tương thích
        }
    }

    // ---- incremental_merge + snapshot SCD2 (§6.2) ----

    mod merge_scd2 {
        use super::*;
        use datafusion::arrow::array::Int64Array;
        use std::path::Path;

        fn irv_schema() -> SchemaRef {
            Arc::new(Schema::new(vec![
                Field::new("id", DataType::Int64, true),
                Field::new("region", DataType::Utf8, true),
                Field::new("v", DataType::Int64, true),
            ]))
        }

        /// batch (id, region, v).
        fn irv(rows: &[(i64, &str, i64)]) -> RecordBatch {
            RecordBatch::try_new(
                irv_schema(),
                vec![
                    Arc::new(Int64Array::from(
                        rows.iter().map(|r| r.0).collect::<Vec<_>>(),
                    )),
                    Arc::new(SA::from(
                        rows.iter()
                            .map(|r| Some(r.1.to_string()))
                            .collect::<Vec<_>>(),
                    )),
                    Arc::new(Int64Array::from(
                        rows.iter().map(|r| r.2).collect::<Vec<_>>(),
                    )),
                ],
            )
            .unwrap()
        }

        fn merge(
            root: &Path,
            db: &Db,
            ds_id: i64,
            run: &str,
            strategy: MergeStrategy,
            batch: RecordBatch,
        ) {
            let ds = db.dataset_get_by_id(ds_id).unwrap().unwrap();
            apply_merge_at(
                root,
                MergeParams {
                    db,
                    dataset: &ds,
                    run_id: run,
                    flow_id: "f",
                    step_id: "s",
                    primary_key: &["id".to_string()],
                    partition_by: &["region".to_string()],
                    strategy,
                    cursor_col: Some("v"),
                    schema_policy: None,
                },
                &[batch],
            )
            .unwrap();
        }

        async fn rows_of(root: &Path, db: &Db, sql: &str) -> Vec<Vec<Value>> {
            crate::engine::query_page_at(root, db, sql, None, None)
                .await
                .unwrap()
                .rows
        }

        #[tokio::test]
        async fn delete_insert_rewrites_partition_no_dup() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "m", None, None, None).unwrap();

            merge(
                root,
                &db,
                ds_id,
                "r1",
                MergeStrategy::DeleteInsert,
                irv(&[(1, "east", 10), (2, "west", 20)]),
            );
            let n = rows_of(root, &db, "SELECT COUNT(*) AS n FROM raw.m").await;
            assert_eq!(n[0][0], Value::from(2));

            // Update id=1 (val mới) trong partition east.
            merge(
                root,
                &db,
                ds_id,
                "r2",
                MergeStrategy::DeleteInsert,
                irv(&[(1, "east", 11)]),
            );
            let all = rows_of(root, &db, "SELECT id, v FROM raw.m ORDER BY id").await;
            assert_eq!(all.len(), 2, "không nhân đôi");
            assert_eq!(all[0][0], Value::from(1));
            assert_eq!(all[0][1], Value::from(11), "id=1 val cập nhật");
            assert_eq!(all[1][0], Value::from(2));
            assert_eq!(all[1][1], Value::from(20), "west không đụng");
        }

        #[tokio::test]
        async fn pk_changes_partition_rewrites_both() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "m", None, None, None).unwrap();

            merge(
                root,
                &db,
                ds_id,
                "r1",
                MergeStrategy::DeleteInsert,
                irv(&[(1, "east", 10), (2, "west", 20)]),
            );
            // id=1 chuyển east → west.
            merge(
                root,
                &db,
                ds_id,
                "r2",
                MergeStrategy::Upsert,
                irv(&[(1, "west", 12)]),
            );

            // Không PK trùng: id=1 chỉ còn 1 dòng, ở west.
            let cnt = rows_of(
                root,
                &db,
                "SELECT id, COUNT(*) AS c FROM raw.m GROUP BY id ORDER BY id",
            )
            .await;
            assert_eq!(cnt.len(), 2);
            assert_eq!(
                cnt[0][1],
                Value::from(1),
                "id=1 chỉ 1 dòng (partition cũ đã rewrite)"
            );
            assert_eq!(cnt[1][1], Value::from(1));
            let reg = rows_of(root, &db, "SELECT region FROM raw.m WHERE id = 1").await;
            assert_eq!(reg[0][0], Value::from("west"));
        }

        #[tokio::test]
        async fn insert_only_skips_existing_pk() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "m", None, None, None).unwrap();

            merge(
                root,
                &db,
                ds_id,
                "r1",
                MergeStrategy::InsertOnly,
                irv(&[(1, "east", 10)]),
            );
            // id=1 đã có → bỏ; id=3 mới → thêm.
            merge(
                root,
                &db,
                ds_id,
                "r2",
                MergeStrategy::InsertOnly,
                irv(&[(1, "east", 99), (3, "east", 30)]),
            );
            let all = rows_of(root, &db, "SELECT id, v FROM raw.m ORDER BY id").await;
            assert_eq!(all.len(), 2);
            assert_eq!(
                all[0][1],
                Value::from(10),
                "id=1 giữ giá trị cũ (không upsert)"
            );
            assert_eq!(all[1][0], Value::from(3));
        }

        // ---- SCD2 ----

        fn dim_schema() -> SchemaRef {
            Arc::new(Schema::new(vec![
                Field::new("id", DataType::Int64, true),
                Field::new("name", DataType::Utf8, true),
                Field::new("updated_at", DataType::Utf8, true),
            ]))
        }
        fn dim(rows: &[(i64, &str, &str)]) -> RecordBatch {
            RecordBatch::try_new(
                dim_schema(),
                vec![
                    Arc::new(Int64Array::from(
                        rows.iter().map(|r| r.0).collect::<Vec<_>>(),
                    )),
                    Arc::new(SA::from(
                        rows.iter()
                            .map(|r| Some(r.1.to_string()))
                            .collect::<Vec<_>>(),
                    )),
                    Arc::new(SA::from(
                        rows.iter()
                            .map(|r| Some(r.2.to_string()))
                            .collect::<Vec<_>>(),
                    )),
                ],
            )
            .unwrap()
        }
        fn snapshot(root: &Path, db: &Db, ds_id: i64, run: &str, batch: RecordBatch) {
            let ds = db.dataset_get_by_id(ds_id).unwrap().unwrap();
            apply_snapshot_at(
                root,
                SnapshotParams {
                    db,
                    dataset: &ds,
                    run_id: run,
                    primary_key: &["id".to_string()],
                    strategy: &SnapshotStrategy::Timestamp("updated_at".to_string()),
                    hard_deletes: HardDeletes::Ignore,
                },
                &[batch],
            )
            .unwrap();
        }

        #[tokio::test]
        async fn scd2_timestamp_closes_changed_row() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "dim", None, None, None).unwrap();

            snapshot(
                root,
                &db,
                ds_id,
                "r1",
                dim(&[(1, "a", "t1"), (2, "b", "t1")]),
            );
            // id=1 đổi (updated_at t2), id=2 giữ nguyên.
            snapshot(
                root,
                &db,
                ds_id,
                "r2",
                dim(&[(1, "a2", "t2"), (2, "b", "t1")]),
            );

            // Current: 2 dòng (id1 mới + id2 carry).
            let cur = rows_of(
                root,
                &db,
                "SELECT id, name FROM raw.dim WHERE _is_current ORDER BY id",
            )
            .await;
            assert_eq!(cur.len(), 2);
            assert_eq!(cur[0][1], Value::from("a2"), "id=1 bản current mới");
            assert_eq!(cur[1][1], Value::from("b"));

            // History: 1 dòng (id1 cũ), _valid_to đã set.
            let hist = rows_of(
                root,
                &db,
                "SELECT id, name, _valid_to FROM raw.dim WHERE NOT _is_current ORDER BY id",
            )
            .await;
            assert_eq!(hist.len(), 1);
            assert_eq!(hist[0][1], Value::from("a"), "bản lịch sử giữ giá trị cũ");
            assert_ne!(hist[0][2], Value::Null, "_valid_to đã đóng");
        }

        #[tokio::test]
        async fn scd2_reinstate_hash_match_is_noop() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "dim", None, None, None).unwrap();

            snapshot(root, &db, ds_id, "r1", dim(&[(1, "a", "t1")]));
            // Chạy lại y hệt (hash trùng) → không tạo version mới.
            snapshot(root, &db, ds_id, "r2", dim(&[(1, "a", "t1")]));

            let cur = rows_of(
                root,
                &db,
                "SELECT COUNT(*) AS n FROM raw.dim WHERE _is_current",
            )
            .await;
            assert_eq!(cur[0][0], Value::from(1), "vẫn 1 bản current");
            let hist = rows_of(
                root,
                &db,
                "SELECT COUNT(*) AS n FROM raw.dim WHERE NOT _is_current",
            )
            .await;
            assert_eq!(
                hist[0][0],
                Value::from(0),
                "hash trùng → không đóng bản nào"
            );
        }

        #[tokio::test]
        async fn scd2_invalidate_hard_delete() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "dim", None, None, None).unwrap();

            let ds = db.dataset_get_by_id(ds_id).unwrap().unwrap();
            apply_snapshot_at(
                root,
                SnapshotParams {
                    db: &db,
                    dataset: &ds,
                    run_id: "r1",
                    primary_key: &["id".to_string()],
                    strategy: &SnapshotStrategy::Timestamp("updated_at".to_string()),
                    hard_deletes: HardDeletes::Invalidate,
                },
                &[dim(&[(1, "a", "t1"), (2, "b", "t1")])],
            )
            .unwrap();

            // id=2 biến mất ở nguồn → invalidate (đóng, đưa vào history).
            apply_snapshot_at(
                root,
                SnapshotParams {
                    db: &db,
                    dataset: &ds,
                    run_id: "r2",
                    primary_key: &["id".to_string()],
                    strategy: &SnapshotStrategy::Timestamp("updated_at".to_string()),
                    hard_deletes: HardDeletes::Invalidate,
                },
                &[dim(&[(1, "a", "t1")])],
            )
            .unwrap();

            let cur = rows_of(
                root,
                &db,
                "SELECT id FROM raw.dim WHERE _is_current ORDER BY id",
            )
            .await;
            assert_eq!(cur.len(), 1, "chỉ id=1 còn current");
            assert_eq!(cur[0][0], Value::from(1));
            let del = rows_of(
                root,
                &db,
                "SELECT id FROM raw.dim WHERE NOT _is_current AND _is_deleted",
            )
            .await;
            assert_eq!(del.len(), 1, "id=2 vào history, đánh dấu deleted");
        }

        #[tokio::test]
        async fn imported_then_unpartitioned_merge_no_double_count() {
            use crate::ingest::IngestedTable;
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();

            // Import → file lưu partition=NULL.
            let table = IngestedTable {
                name: "m".into(),
                schema: irv_schema(),
                batches: vec![irv(&[(1, "east", 10), (2, "west", 20)])],
                origin: "csv",
                note: String::new(),
                rows: 2,
            };
            crate::lake::create_dataset_from_ingested_at(root, &db, "raw", "m", &table, "r0")
                .unwrap();
            let ds_id = db.dataset_upsert("raw", "m", None, None, None).unwrap();
            let ds = db.dataset_get_by_id(ds_id).unwrap().unwrap();

            // Merge unpartitioned (partition_by rỗng) update id=1 → v=11.
            apply_merge_at(
                root,
                MergeParams {
                    db: &db,
                    dataset: &ds,
                    run_id: "r1",
                    flow_id: "f",
                    step_id: "s",
                    primary_key: &["id".to_string()],
                    partition_by: &[],
                    strategy: MergeStrategy::DeleteInsert,
                    cursor_col: Some("v"),
                    schema_policy: None,
                },
                &[irv(&[(1, "east", 11)])],
            )
            .unwrap();

            // File NULL-partition cũ phải bị tombstone → đúng 2 dòng, không double-count.
            let all = rows_of(root, &db, "SELECT id, v FROM raw.m ORDER BY id, v").await;
            assert_eq!(all.len(), 2, "NULL-partition không double-count");
            assert_eq!(
                all[0],
                vec![Value::from(1), Value::from(11)],
                "id=1 cập nhật v=11"
            );
            assert_eq!(
                all[1],
                vec![Value::from(2), Value::from(20)],
                "id=2 giữ nguyên"
            );
        }

        #[tokio::test]
        async fn scd2_new_record_reinstate_clears_deleted() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let db = Db::open_memory().unwrap();
            let ds_id = db.dataset_upsert("raw", "dim", None, None, None).unwrap();
            let ds = db.dataset_get_by_id(ds_id).unwrap().unwrap();

            let snap = |run: &str, batch: RecordBatch| {
                apply_snapshot_at(
                    root,
                    SnapshotParams {
                        db: &db,
                        dataset: &ds,
                        run_id: run,
                        primary_key: &["id".to_string()],
                        strategy: &SnapshotStrategy::Timestamp("updated_at".to_string()),
                        hard_deletes: HardDeletes::NewRecord,
                    },
                    &[batch],
                )
                .unwrap();
            };

            snap("r1", dim(&[(1, "a", "t1")])); // xuất hiện
            snap("r2", dim(&[])); // biến mất → NewRecord: current row đánh dấu deleted
            let del = rows_of(
                root,
                &db,
                "SELECT _is_deleted FROM raw.dim WHERE _is_current AND id = 1",
            )
            .await;
            assert_eq!(del.len(), 1);
            assert_eq!(del[0][0], Value::from(true), "r2: current row deleted");

            snap("r3", dim(&[(1, "a", "t1")])); // sống lại (hash trùng) → reinstate
            let cur = rows_of(
                root,
                &db,
                "SELECT _is_deleted FROM raw.dim WHERE _is_current AND id = 1",
            )
            .await;
            assert_eq!(cur.len(), 1, "vẫn 1 bản current");
            assert_eq!(
                cur[0][0],
                Value::from(false),
                "reinstate: _is_deleted phải false"
            );
        }
    }
}
