//! `telegram-poll` — start a run from Telegram updates fetched by long polling.
//!
//! Replaces the old `telegram-hook` node. A webhook needs a public HTTPS URL
//! registered with Telegram; polling only needs the bot token, so a flow works
//! from a laptop or a box behind NAT with nothing exposed.
//!
//! The poll loop lives in a spawned task keyed by `(chain_id, node)` — same
//! shape as `schedule` — so `stop` really cancels it and two chains polling two
//! bots keep separate loops.
//!
//! The update offset is persisted through `StateStore`, so restarting the app
//! resumes where it left off instead of replaying (or losing) the backlog.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::services::Services;
use crate::engine::spec::{Category, Emitter, RuleSpec, SourceCtx, SourceRule};
use crate::engine::types::{ChainId, PORT_ERROR};
use crate::rules::TaskMap;

const DEFAULT_API_BASE: &str = "https://api.telegram.org";
/// Telegram caps long polling at 50 seconds.
const MAX_TIMEOUT_S: u64 = 50;
const DEFAULT_TIMEOUT_S: u64 = 30;
/// Room for the round trip on top of the long-poll window.
const HTTP_SLACK_S: u64 = 20;
const MAX_BACKOFF_S: u64 = 60;
/// Floor between two empty polls. Telegram holds the connection for `timeout`
/// seconds, so this normally never fires — it only stops the loop spinning when
/// the window is 0 or something in front of Telegram answers immediately.
const MIN_IDLE: Duration = Duration::from_millis(1000);
/// `StateStore` scope holding the next `offset` to ask Telegram for.
const OFFSET_SCOPE: &str = "telegram-offset";

/// The live poll loops, one per deployed node.
pub fn tasks() -> &'static TaskMap {
    static T: std::sync::OnceLock<TaskMap> = std::sync::OnceLock::new();
    T.get_or_init(TaskMap::new)
}

/// The `meta` every polled update carries.
pub fn meta_for(update_id: i64) -> Value {
    json!({ "_event": "telegram", "_source": "poll", "updateId": update_id })
}

/// A bot token is `<digits>:<secret>`. Checked so a URL token pasted over from
/// the old webhook node fails at save time, not silently at 3am.
fn token_looks_like_bot_token(token: &str) -> bool {
    match token.split_once(':') {
        Some((id, secret)) => {
            !id.is_empty()
                && id.chars().all(|c| c.is_ascii_digit())
                && secret.len() >= 8
                && !secret.contains(char::is_whitespace)
        }
        None => false,
    }
}

/// `"message, callback_query"` or `["message"]` → the list Telegram wants.
/// An empty value means "every update type except `chat_member`", Telegram's
/// own default.
fn parse_allowed(raw: Option<&Value>) -> Result<Vec<String>, String> {
    let items: Vec<String> = match raw {
        None | Some(Value::Null) => vec![],
        Some(Value::String(s)) => s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(_) => {
            return Err(
                "Loại update phải là chuỗi ngăn cách bởi dấu phẩy, hoặc mảng chuỗi.".to_string(),
            )
        }
    };
    for it in &items {
        if !it
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        {
            return Err(format!(
                "Loại update `{it}` không hợp lệ. Ví dụ: `message`, `edited_message`, \
                 `callback_query`, `my_chat_member`."
            ));
        }
    }
    Ok(items)
}

fn clamp_timeout(raw: u64) -> u64 {
    raw.min(MAX_TIMEOUT_S)
}

/// Telegram's own wording, unwrapped from the envelope.
fn describe(body: &Value) -> String {
    body.get("description")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| body.to_string())
}

/// Seconds Telegram asks us to wait after a 429.
fn retry_after(body: &Value) -> Option<u64> {
    body.get("parameters")
        .and_then(|p| p.get("retry_after"))
        .and_then(|v| v.as_u64())
}

struct Poller {
    http: reqwest::Client,
    base: String,
    token: String,
    timeout_s: u64,
    allowed: Vec<String>,
    svc: Arc<Services>,
    emitter: Emitter,
    chain_id: ChainId,
    node: String,
}

/// A failed API call, with Telegram's own "wait this long" when it sent one.
struct ApiError {
    message: String,
    retry_after: Option<u64>,
}

/// What one `getUpdates` call produced.
enum Poll {
    Updates(Vec<Value>),
    /// Recoverable: report it, wait, keep polling. `Some(secs)` is Telegram's
    /// own `retry_after`; `None` means use the growing backoff.
    Failed(String, Option<u64>),
}

impl Poller {
    fn url(&self, method: &str) -> String {
        format!("{}/bot{}/{method}", self.base, self.token)
    }

    async fn call(
        &self,
        method: &str,
        payload: Value,
        timeout: Duration,
    ) -> Result<Value, ApiError> {
        let fail = |m: String| ApiError {
            message: m,
            retry_after: None,
        };
        let resp = self
            .http
            .post(self.url(method))
            .json(&payload)
            .timeout(timeout)
            .send()
            .await
            .map_err(|e| fail(format!("Không gọi được Telegram API: {e}")))?;
        let status = resp.status();
        let body: Value = match resp.text().await {
            Ok(t) => serde_json::from_str(&t).unwrap_or_else(|_| json!({ "raw": t })),
            Err(e) => return Err(fail(format!("Không đọc được phản hồi Telegram: {e}"))),
        };
        let ok = status.is_success() && body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        if ok {
            return Ok(body.get("result").cloned().unwrap_or(Value::Null));
        }
        let description = describe(&body);
        // Telegram refuses getUpdates while a webhook is registered. Say what to
        // do instead of passing its English through.
        if status.as_u16() == 409 || description.contains("webhook is active") {
            return Err(fail(format!(
                "Bot đang gắn webhook nên Telegram từ chối polling ({description}). \
                 Bật `Xoá webhook khi khởi động` trong node, hoặc gọi deleteWebhook thủ công."
            )));
        }
        Err(ApiError {
            message: format!("Telegram trả về HTTP {}: {description}", status.as_u16()),
            retry_after: retry_after(&body),
        })
    }

    /// One `getUpdates`. `offset` of `None` means "whatever Telegram has".
    async fn get_updates(&self, offset: Option<i64>, timeout_s: u64, limit: Option<u64>) -> Poll {
        let mut payload = json!({ "timeout": timeout_s });
        if let Some(o) = offset {
            payload["offset"] = json!(o);
        }
        if let Some(l) = limit {
            payload["limit"] = json!(l);
        }
        if !self.allowed.is_empty() {
            payload["allowed_updates"] = json!(self.allowed);
        }
        match self
            .call(
                "getUpdates",
                payload,
                Duration::from_secs(timeout_s + HTTP_SLACK_S),
            )
            .await
        {
            Ok(Value::Array(a)) => Poll::Updates(a),
            Ok(_) => Poll::Updates(vec![]),
            Err(e) => Poll::Failed(e.message, e.retry_after),
        }
    }

    fn log(&self, level: &str, msg: impl Into<String>) {
        self.svc
            .log
            .write(self.chain_id, None, level, Some(&self.node), msg.into());
    }

    fn save_offset(&self, offset: i64) {
        self.svc
            .state
            .set(self.chain_id, &self.node, OFFSET_SCOPE, &json!(offset));
    }

    /// A poll failure is a message on `error`, not just a log line — a flow can
    /// route it to a notification. The old webhook node had no way to say
    /// "Telegram is unreachable" at all.
    async fn report(&self, msg: &str) {
        self.log("warn", msg.to_string());
        self.emitter
            .emit(
                PORT_ERROR,
                json!({ "error": msg }),
                json!({ "_event": "telegram", "_source": "poll" }),
            )
            .await;
    }
}

pub struct TelegramPollSource {
    spec: RuleSpec,
}

pub fn source() -> Arc<dyn SourceRule> {
    Arc::new(TelegramPollSource::new())
}

impl TelegramPollSource {
    fn new() -> Self {
        let spec = RuleSpec::builder("telegram-poll", "Telegram Polling", Category::Source)
            .desc("Tự lấy update từ Telegram bằng long polling — không cần URL công khai.")
            .icon("🤖")
            .color("#13c2c2")
            .schema(json!({
                "type": "object",
                "required": ["botToken"],
                "properties": {
                    "botToken": {
                        "type": "string",
                        "title": "Bot token",
                        "ui": "password",
                        "placeholder": "123456:ABC-DEF...",
                        "description": "Token của bot lấy từ @BotFather. Không phải chuỗi bí mật trong URL webhook."
                    },
                    "timeout": {
                        "type": "number",
                        "title": "Thời gian chờ mỗi lần hỏi (giây)",
                        "default": DEFAULT_TIMEOUT_S,
                        "minimum": 0,
                        "maximum": MAX_TIMEOUT_S,
                        "description": "Long polling: giữ kết nối tối đa bấy nhiêu giây để chờ update. Tối đa 50."
                    },
                    "allowedUpdates": {
                        "type": "string",
                        "title": "Loại update",
                        "placeholder": "message, callback_query",
                        "description": "Ngăn cách bởi dấu phẩy. Bỏ trống = lấy mặc định của Telegram."
                    },
                    "dropPending": {
                        "type": "boolean",
                        "title": "Bỏ qua update tồn đọng",
                        "default": true,
                        "description": "Khi bật node, nhảy tới update mới nhất thay vì chạy lại hàng đợi cũ."
                    },
                    "deleteWebhook": {
                        "type": "boolean",
                        "title": "Xoá webhook khi khởi động",
                        "default": true,
                        "description": "Telegram không cho polling khi bot còn gắn webhook. Bật để tự gỡ webhook (không xoá update đang chờ)."
                    },
                    "apiBase": {
                        "type": "string",
                        "title": "API base (chỉ để kiểm thử)",
                        "default": DEFAULT_API_BASE,
                        "description": "Chỉ đổi khi cần trỏ vào máy chủ giả trong kiểm thử."
                    }
                }
            }))
            .doc(
                "Node tự gọi `getUpdates` theo vòng lặp long polling — **không cần** URL \
                 công khai, không cần `setWebhook`, chạy được sau NAT.\n\n\
                 - Mỗi update là một lần chạy mới; toàn bộ update đi vào `data` \
                   (ví dụ `message.text`, `message.chat.id`).\n\
                 - `meta` gồm `{ \"_event\": \"telegram\", \"_source\": \"poll\", \"updateId\": ... }`.\n\
                 - Vị trí đọc (`offset`) được lưu lại, nên khởi động lại app không đọc trùng \
                   và cũng không mất update. Xoá state của chain là đọc lại từ đầu hàng đợi.\n\
                 - Telegram lỗi / mất mạng: message đi ra cổng `error` kèm mô tả, vòng lặp tự \
                   thử lại với thời gian giãn dần (1s → 60s). Gặp 429 thì đợi đúng \
                   `retry_after` Telegram yêu cầu.\n\
                 - Một bot chỉ nên có **một** node polling. Hai node cùng bot token sẽ giành \
                   update của nhau (Telegram trả lỗi 409).",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl SourceRule for TelegramPollSource {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("botToken").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => {
                if !token_looks_like_bot_token(s.trim()) {
                    out.push(
                        "Bot token phải có dạng `123456:ABC-DEF...` lấy từ @BotFather.".to_string(),
                    );
                }
            }
            _ => out.push("Thiếu Bot token.".to_string()),
        }
        if let Err(e) = parse_allowed(config.get("allowedUpdates")) {
            out.push(e);
        }
        if let Some(v) = config.get("timeout") {
            let secs = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                Value::Null => None,
                _ => Some(f64::NAN),
            };
            match secs {
                Some(s) if (0.0..=MAX_TIMEOUT_S as f64).contains(&s) => {}
                None => {}
                _ => out.push(format!(
                    "Thời gian chờ phải từ 0 đến {MAX_TIMEOUT_S} giây (giới hạn của Telegram)."
                )),
            }
        }
        out
    }

    async fn start(&self, ctx: SourceCtx) -> Result<(), String> {
        let Some(token) = ctx.cfg_str("botToken") else {
            return Err("Thiếu Bot token.".to_string());
        };
        let token = token.trim().to_string();
        if !token_looks_like_bot_token(&token) {
            return Err(
                "Bot token phải có dạng `123456:ABC-DEF...` lấy từ @BotFather.".to_string(),
            );
        }
        let allowed = parse_allowed(ctx.config.get("allowedUpdates"))?;
        let timeout_s = clamp_timeout(ctx.cfg_u64_or("timeout", DEFAULT_TIMEOUT_S));
        let drop_pending = ctx.cfg_bool("dropPending", true);
        let delete_webhook = ctx.cfg_bool("deleteWebhook", true);
        let base = ctx
            .cfg_str("apiBase")
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string())
            .trim_end_matches('/')
            .to_string();

        let saved_offset = ctx
            .svc
            .state
            .get(ctx.chain_id, &ctx.node, OFFSET_SCOPE)
            .and_then(|v| v.as_i64());

        let poller = Poller {
            http: ctx.svc.http.clone(),
            base,
            token,
            timeout_s,
            allowed,
            svc: ctx.svc.clone(),
            emitter: ctx.emitter.clone(),
            chain_id: ctx.chain_id,
            node: ctx.node.clone(),
        };

        let handle = tokio::spawn(async move {
            if delete_webhook {
                // Keeps pending updates: they are exactly what polling is about
                // to read.
                if let Err(e) = poller
                    .call(
                        "deleteWebhook",
                        json!({ "drop_pending_updates": false }),
                        Duration::from_secs(20),
                    )
                    .await
                {
                    poller.log("warn", format!("không gỡ được webhook cũ: {}", e.message));
                } else {
                    poller.log("info", "đã gỡ webhook cũ, chuyển sang polling");
                }
            }

            let mut offset = saved_offset;
            if drop_pending && offset.is_none() {
                // `offset = -1` returns only the newest pending update, which
                // gives us a cursor past the backlog without replaying it.
                if let Poll::Updates(u) = poller.get_updates(Some(-1), 0, Some(1)).await {
                    if let Some(id) = u.last().and_then(|x| x["update_id"].as_i64()) {
                        offset = Some(id + 1);
                        poller.save_offset(id + 1);
                        poller.log("info", "bỏ qua các update tồn đọng");
                    }
                }
            }

            let mut backoff = 1u64;
            let mut saved = saved_offset;
            loop {
                let started = tokio::time::Instant::now();
                match poller.get_updates(offset, poller.timeout_s, None).await {
                    Poll::Updates(updates) => {
                        backoff = 1;
                        let empty = updates.is_empty();
                        for u in updates {
                            let Some(id) = u.get("update_id").and_then(|v| v.as_i64()) else {
                                continue;
                            };
                            // Advance before emitting: a slow flow must never
                            // cause the same update to be fetched twice.
                            offset = Some(offset.unwrap_or(id + 1).max(id + 1));
                            poller.emitter.emit("out", u, meta_for(id)).await;
                        }
                        if offset != saved {
                            if let Some(o) = offset {
                                poller.save_offset(o);
                                saved = offset;
                            }
                        }
                        // `timeout: 0`, or a proxy that ignores the long-poll
                        // window, would otherwise spin this loop flat out.
                        if empty {
                            let elapsed = started.elapsed();
                            if elapsed < MIN_IDLE {
                                tokio::time::sleep(MIN_IDLE - elapsed).await;
                            }
                        }
                    }
                    Poll::Failed(msg, retry_after) => {
                        poller.report(&msg).await;
                        match retry_after {
                            // Telegram said how long to wait; obey it exactly.
                            Some(sec) => {
                                tokio::time::sleep(Duration::from_secs(sec.clamp(1, MAX_BACKOFF_S)))
                                    .await
                            }
                            None => {
                                tokio::time::sleep(Duration::from_secs(backoff)).await;
                                backoff = (backoff * 2).min(MAX_BACKOFF_S);
                            }
                        }
                    }
                }
            }
        });
        tasks().insert(ctx.chain_id, &ctx.node, handle);

        // The bot token is a credential — never log it.
        ctx.log(
            "info",
            format!("telegram polling đã bật (long-poll {timeout_s}s)"),
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
    use crate::engine::spec::Ingress;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn source_ctx(
        chain_id: ChainId,
        node: &str,
        config: Value,
    ) -> (SourceCtx, tokio::sync::mpsc::Receiver<Ingress>, Arc<Db>) {
        let db = Arc::new(Db::open(":memory:").expect("in-memory db"));
        let _ = db.create_chain(chain_id, "test", "");
        let svc = Arc::new(Services::new(db.clone(), EventBus::new()));
        let (tx, rx) = tokio::sync::mpsc::channel(16);
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
            db,
        )
    }

    /// Stands in for `api.telegram.org`: answers every request with the next
    /// queued body (the last one repeats) and reports what it was asked.
    async fn fake_telegram(bodies: Vec<&str>) -> (String, tokio::sync::mpsc::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let bodies: Vec<String> = bodies.into_iter().map(|b| b.to_string()).collect();
        tokio::spawn(async move {
            let mut n = 0usize;
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let read = match sock.read(&mut chunk).await {
                        Ok(0) | Err(_) => break,
                        Ok(k) => k,
                    };
                    buf.extend_from_slice(&chunk[..read]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(head) = text.find("\r\n\r\n") {
                        let len: usize = text
                            .to_lowercase()
                            .split("content-length:")
                            .nth(1)
                            .and_then(|s| s.split("\r\n").next())
                            .and_then(|s| s.trim().parse().ok())
                            .unwrap_or(0);
                        if buf.len() >= head + 4 + len {
                            break;
                        }
                    }
                }
                let req = String::from_utf8_lossy(&buf).to_string();
                let _ = tx.send(req).await;
                let body = bodies
                    .get(n)
                    .or_else(|| bodies.last())
                    .cloned()
                    .unwrap_or_else(|| r#"{"ok":true,"result":[]}"#.to_string());
                n += 1;
                let status = if body.contains("\"ok\":false") {
                    400
                } else {
                    200
                };
                let resp = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://{addr}"), rx)
    }

    fn config(base: &str, extra: Value) -> Value {
        let mut c = json!({
            "botToken": "123456:ABCDEFGH-ijklmnop",
            "apiBase": base,
            "timeout": 0,
            "dropPending": false,
            "deleteWebhook": false,
        });
        for (k, v) in extra.as_object().cloned().unwrap_or_default() {
            c[k] = v;
        }
        c
    }

    async fn next_request(rx: &mut tokio::sync::mpsc::Receiver<String>, what: &str) -> String {
        tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .unwrap_or_else(|_| panic!("phải có request `{what}`"))
            .expect("kênh còn mở")
    }

    #[test]
    fn a_url_token_is_not_accepted_as_a_bot_token() {
        // The old `telegram-hook` node took a URL secret; pasting it here has
        // to fail loudly.
        assert!(!token_looks_like_bot_token("chuoi-bi-mat-trong-url"));
        assert!(!token_looks_like_bot_token("abc:12345678"));
        assert!(!token_looks_like_bot_token("123456:short"));
        assert!(!token_looks_like_bot_token("123456:has space here"));
        assert!(token_looks_like_bot_token("123456:ABCDEFGH-ijklmnop"));
    }

    #[test]
    fn allowed_updates_accepts_a_csv_or_a_list() {
        assert_eq!(
            parse_allowed(Some(&json!("message, callback_query"))).unwrap(),
            vec!["message", "callback_query"]
        );
        assert_eq!(
            parse_allowed(Some(&json!(["message"]))).unwrap(),
            vec!["message"]
        );
        assert!(parse_allowed(None).unwrap().is_empty());
        assert!(parse_allowed(Some(&json!(""))).unwrap().is_empty());
        assert!(parse_allowed(Some(&json!("Tin nhắn"))).is_err());
        assert!(parse_allowed(Some(&json!(7))).is_err());
    }

    #[test]
    fn the_long_poll_window_is_capped_at_telegrams_limit() {
        assert_eq!(clamp_timeout(30), 30);
        assert_eq!(clamp_timeout(600), MAX_TIMEOUT_S);
    }

    #[test]
    fn validate_reports_token_timeout_and_update_type_problems() {
        let s = TelegramPollSource::new();
        assert!(!s.validate(&json!({})).is_empty());
        assert!(!s
            .validate(&json!({ "botToken": "khong-phai-token" }))
            .is_empty());
        assert!(!s
            .validate(&json!({ "botToken": "123456:ABCDEFGH-ij", "timeout": 120 }))
            .is_empty());
        assert!(!s
            .validate(&json!({ "botToken": "123456:ABCDEFGH-ij", "allowedUpdates": "Tin nhắn" }))
            .is_empty());
        assert!(s
            .validate(&json!({
                "botToken": "123456:ABCDEFGH-ij",
                "timeout": 30,
                "allowedUpdates": "message, callback_query"
            }))
            .is_empty());
    }

    #[tokio::test]
    async fn start_refuses_a_bad_token_without_spawning_anything() {
        let s = TelegramPollSource::new();
        let (c, _rx, _db) = source_ctx(41, "n1", json!({}));
        assert!(s.start(c).await.is_err());

        let (c, mut rx, _db) = source_ctx(42, "n1", json!({ "botToken": "chuoi-bi-mat" }));
        assert!(s.start(c).await.unwrap_err().contains("@BotFather"));
        // Nothing was spawned, so nothing can reach the engine.
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(rx.try_recv().is_err(), "không được phát gì khi start lỗi");
    }

    #[tokio::test]
    async fn every_update_becomes_its_own_run_and_the_offset_advances() {
        let (base, mut reqs) = fake_telegram(vec![
            r#"{"ok":true,"result":[{"update_id":10,"message":{"text":"xin chào","chat":{"id":7}}},{"update_id":11,"message":{"text":"ok"}}]}"#,
            r#"{"ok":true,"result":[]}"#,
        ])
        .await;
        let s = TelegramPollSource::new();
        let (c, mut rx, db) = source_ctx(
            43,
            "n1",
            config(&base, json!({ "allowedUpdates": "message" })),
        );
        s.start(c).await.unwrap();

        let first = tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .expect("phải phát trong 4 giây")
            .expect("kênh còn mở");
        assert_eq!(first.chain_id, 43);
        assert_eq!(first.node, "n1");
        assert_eq!(first.port, "out");
        assert_eq!(first.data["message"]["text"], "xin chào");
        assert_eq!(first.meta["_event"], "telegram");
        assert_eq!(first.meta["_source"], "poll");
        assert_eq!(first.meta["updateId"], 10);

        let second = rx.recv().await.unwrap();
        assert_eq!(second.data["update_id"], 11);

        let req1 = next_request(&mut reqs, "getUpdates").await;
        assert!(
            req1.starts_with("POST /bot123456:ABCDEFGH-ijklmnop/getUpdates "),
            "{req1}"
        );
        assert!(req1.contains("\"allowed_updates\":[\"message\"]"), "{req1}");
        assert!(
            !req1.contains("\"offset\""),
            "lần đầu chưa có offset: {req1}"
        );

        // The next call asks for 12 — one past the last update seen.
        let req2 = next_request(&mut reqs, "getUpdates lần 2").await;
        assert!(req2.contains("\"offset\":12"), "{req2}");
        assert_eq!(db.state_get(43, "n1", OFFSET_SCOPE), Some(json!(12)));

        s.stop(43, "n1").await;
    }

    /// A restart must not replay what the previous process already handled.
    #[tokio::test]
    async fn a_saved_offset_is_resumed_on_start() {
        let (base, mut reqs) = fake_telegram(vec![r#"{"ok":true,"result":[]}"#]).await;
        let s = TelegramPollSource::new();
        let (c, _rx, db) = source_ctx(44, "n1", config(&base, json!({ "dropPending": true })));
        db.state_set(44, "n1", OFFSET_SCOPE, &json!(99));
        s.start(c).await.unwrap();

        let req = next_request(&mut reqs, "getUpdates").await;
        assert!(req.contains("\"offset\":99"), "{req}");
        // `dropPending` must not throw away a resumed cursor.
        assert!(!req.contains("\"offset\":-1"), "{req}");
        s.stop(44, "n1").await;
    }

    #[tokio::test]
    async fn drop_pending_skips_the_backlog_before_the_first_real_poll() {
        let (base, mut reqs) = fake_telegram(vec![
            r#"{"ok":true,"result":[{"update_id":500,"message":{"text":"cũ"}}]}"#,
            r#"{"ok":true,"result":[]}"#,
        ])
        .await;
        let s = TelegramPollSource::new();
        let (c, mut rx, _db) = source_ctx(45, "n1", config(&base, json!({ "dropPending": true })));
        s.start(c).await.unwrap();

        let probe = next_request(&mut reqs, "probe").await;
        assert!(probe.contains("\"offset\":-1"), "{probe}");
        assert!(probe.contains("\"limit\":1"), "{probe}");

        let real = next_request(&mut reqs, "getUpdates").await;
        assert!(real.contains("\"offset\":501"), "{real}");
        assert!(rx.try_recv().is_err(), "update tồn đọng không được chạy");
        s.stop(45, "n1").await;
    }

    /// Telegram holds an empty long poll open for `timeout` seconds, but a
    /// window of 0 — or anything in front of Telegram that answers at once —
    /// used to turn the loop into a busy wait.
    #[tokio::test]
    async fn an_instantly_empty_poll_does_not_spin_the_loop() {
        let (base, mut reqs) = fake_telegram(vec![r#"{"ok":true,"result":[]}"#]).await;
        let s = TelegramPollSource::new();
        let (c, _rx, _db) = source_ctx(50, "n1", config(&base, json!({ "timeout": 0 })));
        s.start(c).await.unwrap();

        tokio::time::sleep(Duration::from_millis(1500)).await;
        s.stop(50, "n1").await;
        let mut calls = 0;
        while reqs.try_recv().is_ok() {
            calls += 1;
        }
        assert!(
            (1..=3).contains(&calls),
            "1,5 giây phải chỉ hỏi vài lần, không phải {calls}"
        );
    }

    #[tokio::test]
    async fn a_telegram_error_goes_out_the_error_port_and_the_loop_survives() {
        let (base, _reqs) = fake_telegram(vec![
            r#"{"ok":false,"description":"Unauthorized"}"#,
            r#"{"ok":true,"result":[{"update_id":1,"message":{"text":"lại chạy"}}]}"#,
            r#"{"ok":true,"result":[]}"#,
        ])
        .await;
        let s = TelegramPollSource::new();
        let (c, mut rx, _db) = source_ctx(46, "n1", config(&base, json!({})));
        s.start(c).await.unwrap();

        let err = tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .expect("lỗi phải đi ra cổng error")
            .unwrap();
        assert_eq!(err.port, PORT_ERROR);
        assert!(err.data["error"].as_str().unwrap().contains("Unauthorized"));

        // Backoff is 1s after the first failure, so the recovery lands inside 4s.
        let ok = tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .expect("vòng lặp phải chạy tiếp")
            .unwrap();
        assert_eq!(ok.port, "out");
        assert_eq!(ok.data["update_id"], 1);
        s.stop(46, "n1").await;
    }

    /// Telegram refuses getUpdates while a webhook is registered — the whole
    /// reason this node replaces `telegram-hook`, so the message has to name it.
    #[tokio::test]
    async fn a_webhook_conflict_explains_itself() {
        let (base, _reqs) = fake_telegram(vec![
            r#"{"ok":false,"description":"Conflict: can't use getUpdates method while webhook is active"}"#,
        ])
        .await;
        let s = TelegramPollSource::new();
        let (c, mut rx, _db) = source_ctx(47, "n1", config(&base, json!({})));
        s.start(c).await.unwrap();

        let err = tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .expect("phải báo lỗi")
            .unwrap();
        let text = err.data["error"].as_str().unwrap().to_string();
        assert!(text.contains("webhook"), "{text}");
        s.stop(47, "n1").await;
    }

    #[tokio::test]
    async fn delete_webhook_runs_first_when_enabled() {
        let (base, mut reqs) = fake_telegram(vec![
            r#"{"ok":true,"result":true}"#,
            r#"{"ok":true,"result":[]}"#,
        ])
        .await;
        let s = TelegramPollSource::new();
        let (c, _rx, _db) = source_ctx(48, "n1", config(&base, json!({ "deleteWebhook": true })));
        s.start(c).await.unwrap();

        let first = next_request(&mut reqs, "deleteWebhook").await;
        assert!(first.contains("/deleteWebhook"), "{first}");
        assert!(first.contains("\"drop_pending_updates\":false"), "{first}");
        let second = next_request(&mut reqs, "getUpdates").await;
        assert!(second.contains("/getUpdates"), "{second}");
        s.stop(48, "n1").await;
    }

    #[tokio::test]
    async fn stop_cancels_the_loop_and_the_token_never_reaches_the_log() {
        let (base, _reqs) = fake_telegram(vec![
            r#"{"ok":true,"result":[{"update_id":3,"message":{"text":"một"}}]}"#,
            r#"{"ok":true,"result":[{"update_id":4,"message":{"text":"hai"}}]}"#,
            r#"{"ok":true,"result":[]}"#,
        ])
        .await;
        let s = TelegramPollSource::new();
        let (c, mut rx, db) = source_ctx(49, "n1", config(&base, json!({})));
        s.start(c).await.unwrap();

        tokio::time::timeout(Duration::from_secs(4), rx.recv())
            .await
            .expect("phải phát")
            .unwrap();
        s.stop(49, "n1").await;
        while rx.try_recv().is_ok() {}
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(rx.try_recv().is_err(), "stop phải huỷ hẳn vòng lặp");

        for l in db.list_logs(49, 20).unwrap() {
            assert!(
                !l.message.contains("ABCDEFGH-ijklmnop"),
                "bot token lọt vào log: {}",
                l.message
            );
        }
    }

    #[test]
    fn meta_names_the_event_the_source_and_the_update() {
        assert_eq!(
            meta_for(5),
            json!({ "_event": "telegram", "_source": "poll", "updateId": 5 })
        );
    }
}
