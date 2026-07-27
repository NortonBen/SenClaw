//! `schedule` — fire on a cron expression, in a named timezone.
//!
//! The ticking loop lives in a spawned task keyed by `(chain_id, node)`, so two
//! chains that both contain a `schedule` node keep separate timers and `stop`
//! can actually cancel one. The Go scheduler keyed its cron entries by node id
//! alone, so deploying a second chain silently replaced the first one's timer.

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use chrono_tz::Tz;
use cron::Schedule;
use serde_json::{json, Value};

use crate::engine::spec::{Category, RuleSpec, SourceCtx, SourceRule};
use crate::engine::types::ChainId;
use crate::rules::TaskMap;

const DEFAULT_TZ: &str = "Asia/Ho_Chi_Minh";

/// The live tickers, one per deployed node.
pub fn tasks() -> &'static TaskMap {
    static T: std::sync::OnceLock<TaskMap> = std::sync::OnceLock::new();
    T.get_or_init(TaskMap::new)
}

/// The `cron` crate wants `giây phút giờ ngày tháng thứ [năm]`, i.e. 6–7 fields.
/// A classic 5-field crontab line is accepted by prepending the seconds field.
pub fn normalize_cron(raw: &str) -> String {
    let expr = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if expr.starts_with('@') {
        return expr; // @hourly and friends are parsed as-is
    }
    if expr.split(' ').filter(|s| !s.is_empty()).count() == 5 {
        return format!("0 {expr}");
    }
    expr
}

fn parse_cron(raw: &str) -> Result<Schedule, String> {
    let expr = normalize_cron(raw);
    if expr.is_empty() {
        return Err("Thiếu biểu thức cron.".to_string());
    }
    Schedule::from_str(&expr).map_err(|e| {
        format!(
            "Cron `{expr}` không hợp lệ ({e}). Cú pháp: `giây phút giờ ngày tháng thứ`. \
             Nhập 5 trường kiểu crontab cũng được, hệ thống tự thêm `0 ` cho giây."
        )
    })
}

fn parse_tz(raw: &str) -> Result<Tz, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Ok(Tz::UTC);
    }
    Tz::from_str(name)
        .map_err(|_| format!("Múi giờ `{name}` không hợp lệ. Ví dụ: `Asia/Ho_Chi_Minh`, `UTC`."))
}

pub struct ScheduleSource {
    spec: RuleSpec,
}

pub fn source() -> Arc<dyn SourceRule> {
    Arc::new(ScheduleSource::new())
}

impl ScheduleSource {
    fn new() -> Self {
        let spec = RuleSpec::builder("schedule", "Hẹn giờ", Category::Source)
            .desc("Chạy chain theo lịch cron trong múi giờ đã chọn.")
            .icon("🗓️")
            .color("#52c41a")
            .schema(json!({
                "type": "object",
                "required": ["cron"],
                "properties": {
                    "cron": {
                        "type": "string",
                        "title": "Biểu thức cron",
                        "placeholder": "0 */5 * * * *",
                        "description": "6 trường: giây phút giờ ngày tháng thứ. Nhập 5 trường cũng được, giây sẽ là 0."
                    },
                    "timezone": {
                        "type": "string",
                        "title": "Múi giờ",
                        "default": DEFAULT_TZ,
                        "placeholder": "Asia/Ho_Chi_Minh"
                    },
                    "payload": {
                        "type": "object",
                        "title": "Dữ liệu gửi kèm",
                        "ui": "code",
                        "default": {},
                        "description": "Object JSON đi ra cổng `out`, kèm thêm `ts` và `iso`."
                    }
                }
            }))
            .doc(
                "Mỗi lần đến hạn là một lần chạy mới.\n\n\
                 - Cron 6 trường: `giây phút giờ ngày tháng thứ` — `0 */5 * * * *` là mỗi 5 phút.\n\
                 - Nhập 5 trường kiểu crontab (`*/5 * * * *`) cũng được, hệ thống tự thêm \
                   `0 ` cho giây.\n\
                 - Dữ liệu ra: `payload` cộng `ts` (giây Unix) và `iso` (RFC3339 theo múi giờ).\n\n\
                 Bộ đếm giờ gắn theo (chain, node) nên hai chain cùng có node hẹn giờ \
                 không đè lịch của nhau.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl SourceRule for ScheduleSource {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("cron").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => {
                if let Err(e) = parse_cron(s) {
                    out.push(e);
                }
            }
            _ => out.push("Thiếu biểu thức cron.".to_string()),
        }
        if let Some(tz) = config.get("timezone").and_then(|v| v.as_str()) {
            if let Err(e) = parse_tz(tz) {
                out.push(e);
            }
        }
        match config.get("payload") {
            None | Some(Value::Null) | Some(Value::Object(_)) => {}
            Some(_) => out.push("Dữ liệu gửi kèm phải là một object JSON.".to_string()),
        }
        out
    }

    async fn start(&self, ctx: SourceCtx) -> Result<(), String> {
        let Some(cron_raw) = ctx.cfg_str("cron") else {
            return Err("Thiếu biểu thức cron.".to_string());
        };
        let schedule = parse_cron(&cron_raw)?;
        let tz = parse_tz(
            &ctx.cfg_str("timezone")
                .unwrap_or_else(|| DEFAULT_TZ.to_string()),
        )?;

        let payload = match ctx.config.get("payload") {
            Some(v @ Value::Object(_)) => v.clone(),
            _ => json!({}),
        };
        let emitter = ctx.emitter.clone();
        let node = ctx.node.clone();
        let chain_id = ctx.chain_id;

        let handle = tokio::spawn(async move {
            let mut cursor = Utc::now().with_timezone(&tz);
            loop {
                // `after` yields times strictly later than the cursor, so the
                // cursor always advances and a late tick cannot spin.
                let Some(next) = schedule.after(&cursor).next() else {
                    break;
                };
                let wait = next
                    .clone()
                    .signed_duration_since(Utc::now().with_timezone(&tz));
                if let Ok(d) = wait.to_std() {
                    tokio::time::sleep(d).await;
                }
                let mut data = payload.clone();
                if let Some(map) = data.as_object_mut() {
                    map.insert("ts".to_string(), json!(next.timestamp()));
                    map.insert("iso".to_string(), json!(next.to_rfc3339()));
                }
                emitter.emit_out(data).await;
                cursor = next;
            }
        });
        tasks().insert(chain_id, &node, handle);

        ctx.log(
            "info",
            format!(
                "hẹn giờ `{}` ({}) đã bật",
                normalize_cron(&cron_raw),
                tz.name()
            ),
        );
        Ok(())
    }

    async fn stop(&self, chain_id: ChainId, node: &str) {
        tasks().remove(chain_id, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::engine::services::{EventBus, Services};
    use crate::engine::spec::{Emitter, Ingress};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn source_ctx(
        chain_id: ChainId,
        node: &str,
        config: Value,
    ) -> (SourceCtx, tokio::sync::mpsc::Receiver<Ingress>) {
        let db = Arc::new(Db::open(":memory:").expect("in-memory db"));
        let _ = db.create_chain(chain_id, "test", "");
        let svc = Arc::new(Services::new(db, EventBus::new()));
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let emitter = Emitter {
            tx,
            chain_id,
            node: node.to_string(),
        };
        (
            SourceCtx {
                chain_id,
                node: node.to_string(),
                config,
                svc,
                emitter,
            },
            rx,
        )
    }

    #[test]
    fn a_five_field_crontab_line_gains_a_seconds_field() {
        assert_eq!(normalize_cron("*/5 * * * *"), "0 */5 * * * *");
        assert_eq!(normalize_cron("  30   9 * * 1  "), "0 30 9 * * 1");
        // Already 6 (or 7) fields: untouched.
        assert_eq!(normalize_cron("0 0 9 * * *"), "0 0 9 * * *");
        assert_eq!(normalize_cron("0 0 9 * * * 2030"), "0 0 9 * * * 2030");
        assert_eq!(normalize_cron("@hourly"), "@hourly");
    }

    #[test]
    fn both_five_and_six_field_expressions_parse() {
        assert!(parse_cron("*/5 * * * *").is_ok());
        assert!(parse_cron("0 0 9 * * *").is_ok());
        assert!(parse_cron("@hourly").is_ok());
    }

    #[test]
    fn a_broken_cron_says_what_the_format_is() {
        let err = parse_cron("mỗi 5 phút").unwrap_err();
        assert!(err.contains("giây phút giờ"), "{err}");
        assert!(parse_cron("* *").is_err());
    }

    #[test]
    fn an_unknown_timezone_is_rejected() {
        assert!(parse_tz("Asia/Ho_Chi_Minh").is_ok());
        assert!(parse_tz("UTC").is_ok());
        let err = parse_tz("Asia/Saigon_City").unwrap_err();
        assert!(err.contains("không hợp lệ"), "{err}");
    }

    #[test]
    fn validate_reports_cron_timezone_and_payload_problems() {
        let s = ScheduleSource::new();
        assert!(!s.validate(&json!({})).is_empty());
        assert!(!s.validate(&json!({ "cron": "không phải cron" })).is_empty());
        assert!(!s
            .validate(&json!({ "cron": "* * * * *", "timezone": "Mars/Olympus" }))
            .is_empty());
        assert!(!s
            .validate(&json!({ "cron": "* * * * *", "payload": "xin chào" }))
            .is_empty());
        assert!(s
            .validate(&json!({
                "cron": "0 */5 * * * *",
                "timezone": "Asia/Ho_Chi_Minh",
                "payload": { "kind": "tick" }
            }))
            .is_empty());
    }

    #[tokio::test]
    async fn start_refuses_a_bad_cron_without_spawning_anything() {
        let s = ScheduleSource::new();
        let (c, _rx) = source_ctx(21, "n1", json!({ "cron": "@@@" }));
        assert!(s.start(c).await.is_err());
    }

    #[tokio::test]
    async fn a_ticking_schedule_emits_payload_plus_ts_and_iso_then_stops() {
        let s = ScheduleSource::new();
        let (c, mut rx) = source_ctx(
            22,
            "n1",
            json!({ "cron": "* * * * * *", "timezone": "Asia/Ho_Chi_Minh", "payload": { "kind": "tick" } }),
        );
        s.start(c).await.unwrap();

        let ing = tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .expect("phải phát trong vòng 4 giây")
            .expect("kênh còn mở");
        assert_eq!(ing.node, "n1");
        assert_eq!(ing.chain_id, 22);
        assert_eq!(ing.data["kind"], "tick");
        assert!(ing.data["ts"].as_i64().unwrap() > 1_700_000_000);
        assert!(ing.data["iso"].as_str().unwrap().contains("+07:00"));

        s.stop(22, "n1").await;
        while rx.try_recv().is_ok() {} // drain anything already in flight
        tokio::time::sleep(Duration::from_millis(2100)).await;
        assert!(rx.try_recv().is_err(), "stop phải huỷ hẳn bộ đếm giờ");
    }

    #[tokio::test]
    async fn the_task_map_is_keyed_by_chain_and_node_and_remove_aborts() {
        static FIRED_A: AtomicBool = AtomicBool::new(false);
        static FIRED_B: AtomicBool = AtomicBool::new(false);

        tasks().insert(
            9001,
            "same-node",
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                FIRED_A.store(true, Ordering::SeqCst);
            }),
        );
        // Same node id, different chain: must NOT replace the first entry.
        tasks().insert(
            9002,
            "same-node",
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                FIRED_B.store(true, Ordering::SeqCst);
            }),
        );

        tasks().remove(9001, "same-node");
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(!FIRED_A.load(Ordering::SeqCst), "task đã bị huỷ");
        assert!(FIRED_B.load(Ordering::SeqCst), "chain khác không bị đè");

        tasks().remove(9002, "same-node");
    }
}
