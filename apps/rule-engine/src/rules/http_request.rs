//! `http-request` — call an external HTTP endpoint.
//!
//! The Go original conflated the two kinds of failure: a non-2xx response was
//! tagged `Type=success` yet routed down the error branch, and `maping.go`
//! merged both branches into one target anyway. Here the three cases are three
//! ports: `success` (2xx), `failed` (the server answered, but not 2xx) and
//! `error` (nothing was answered — DNS, TLS, timeout, bad config).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::Method;
use serde_json::{json, Map, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const METHODS: [&str; 5] = ["GET", "POST", "PUT", "PATCH", "DELETE"];
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

pub struct HttpRequestRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(HttpRequestRule::new())
}

fn parse_method(raw: &str) -> Option<Method> {
    match raw {
        "GET" => Some(Method::GET),
        "POST" => Some(Method::POST),
        "PUT" => Some(Method::PUT),
        "PATCH" => Some(Method::PATCH),
        "DELETE" => Some(Method::DELETE),
        _ => None,
    }
}

/// The UI stores the body either as raw text or, in the code editor, as JSON.
fn body_template(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::String(_) | Value::Null => None,
        other => Some(other.to_string()),
    }
}

impl HttpRequestRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("http-request", "HTTP Request", Category::Sink)
            .desc("Gọi một API HTTP bên ngoài rồi phát kết quả đi tiếp.")
            .icon("🌐")
            .color("#13c2c2")
            .outputs(vec![
                PortSpec::new("success", "success")
                    .color("#52c41a")
                    .desc("HTTP 2xx"),
                PortSpec::new("failed", "failed")
                    .color("#fa8c16")
                    .desc("Máy chủ trả lời nhưng mã trạng thái không phải 2xx"),
            ])
            .schema(json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "method": {
                        "type": "string",
                        "title": "Phương thức",
                        "ui": "select",
                        "enum": METHODS,
                        "default": "GET"
                    },
                    "url": {
                        "type": "string",
                        "title": "URL",
                        "placeholder": "https://api.example.com/devices/${device_id}",
                        "description": "Có thể chèn dữ liệu bằng ${field} hoặc ${a.b.c}."
                    },
                    "headers": {
                        "type": "object",
                        "title": "Headers",
                        "ui": "keyvalue",
                        "default": {},
                        "description": "Mỗi giá trị header cũng được thay ${...}."
                    },
                    "body": {
                        "type": "string",
                        "title": "Body",
                        "ui": "textarea",
                        "placeholder": "{\"temp\": ${temperature}}",
                        "description": "Bỏ trống nếu không gửi body. Tự đặt Content-Type: application/json khi body là JSON hợp lệ."
                    },
                    "timeoutMs": {
                        "type": "integer",
                        "title": "Timeout (ms)",
                        "default": DEFAULT_TIMEOUT_MS,
                        "minimum": 100
                    },
                    "parseJson": {
                        "type": "boolean",
                        "title": "Tự phân tích JSON",
                        "default": true,
                        "description": "Bật: body phản hồi thành object/array. Tắt: giữ nguyên chuỗi."
                    }
                }
            }))
            .doc(
                "Gọi HTTP rồi phát `{ status, body, headers }`.\n\n\
                 - `success`: mã 2xx\n\
                 - `failed`: máy chủ trả lời với mã khác 2xx (vẫn có `status` và `body` để xử lý tiếp)\n\
                 - `error`: không gọi được (timeout, DNS, TLS, cấu hình sai)\n\n\
                 Bản Go gộp `failed` vào `error` và làm mất `status`, nên không thể \
                 phân biệt \"API từ chối\" với \"không gọi được API\".",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for HttpRequestRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("url").and_then(|v| v.as_str()) {
            None | Some("") => out.push("Thiếu URL.".to_string()),
            Some(_) => {}
        }
        if let Some(m) = config.get("method").and_then(|v| v.as_str()) {
            if !m.trim().is_empty() && parse_method(m.trim().to_uppercase().as_str()).is_none() {
                out.push(format!(
                    "Phương thức `{m}` không hợp lệ. Chọn: {}.",
                    METHODS.join(", ")
                ));
            }
        }
        match config.get("headers") {
            None | Some(Value::Null) | Some(Value::Object(_)) => {}
            Some(_) => out.push("`headers` phải là object dạng {tên: giá trị}.".to_string()),
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let method_raw = ctx.cfg_str_or("method", "GET").trim().to_uppercase();
        let Some(method) = parse_method(&method_raw) else {
            return ctx.fail_config(format!(
                "Phương thức `{method_raw}` không hợp lệ. Chọn: {}.",
                METHODS.join(", ")
            ));
        };
        let Some(url_tpl) = ctx.cfg_str("url") else {
            return ctx.fail_config("Thiếu URL.");
        };
        let url = daq::interpolate(&url_tpl, &msg.data, &msg.meta);
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ctx.fail_config(format!(
                "URL phải bắt đầu bằng http:// hoặc https:// (đang là `{url}`)."
            ));
        }

        let timeout = ctx
            .cfg_u64_or("timeoutMs", DEFAULT_TIMEOUT_MS)
            .clamp(100, 600_000);
        let parse_json = ctx.cfg_bool("parseJson", true);

        let mut req = ctx
            .svc
            .http
            .request(method, &url)
            .timeout(Duration::from_millis(timeout));

        let mut has_content_type = false;
        match ctx.cfg("headers") {
            None => {}
            Some(Value::Object(map)) => {
                for (k, v) in map {
                    let raw = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let value = daq::interpolate(&raw, &msg.data, &msg.meta);
                    let Ok(name) = HeaderName::from_bytes(k.as_bytes()) else {
                        return ctx.fail_config(format!("Tên header không hợp lệ: `{k}`."));
                    };
                    let Ok(val) = HeaderValue::from_str(&value) else {
                        return ctx
                            .fail_config(format!("Giá trị header `{k}` chứa ký tự không hợp lệ."));
                    };
                    if name == CONTENT_TYPE {
                        has_content_type = true;
                    }
                    req = req.header(name, val);
                }
            }
            Some(_) => return ctx.fail_config("`headers` phải là object dạng {tên: giá trị}."),
        }

        if let Some(tpl) = ctx.cfg("body").and_then(body_template) {
            let body = daq::interpolate(&tpl, &msg.data, &msg.meta);
            // Guessing the content type only when the user did not set one keeps
            // an explicit `text/plain` from being overwritten.
            if !has_content_type && serde_json::from_str::<Value>(&body).is_ok() {
                req = req.header(CONTENT_TYPE, "application/json");
            }
            req = req.body(body);
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return ctx.fail_runtime(format!("Gọi HTTP thất bại: {e}")),
        };

        let status = resp.status().as_u16();
        let mut headers = Map::new();
        for (k, v) in resp.headers().iter() {
            headers.insert(
                k.as_str().to_string(),
                json!(v.to_str().unwrap_or_default()),
            );
        }
        let text = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ctx.fail_runtime(format!("Không đọc được phản hồi: {e}")),
        };
        let body = if parse_json {
            serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!(text))
        } else {
            json!(text)
        };

        let out = json!({ "status": status, "body": body, "headers": Value::Object(headers) });
        let port = if (200..300).contains(&status) {
            "success"
        } else {
            "failed"
        };
        Outcome::port(port, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot HTTP server. Returns the base URL and a channel carrying the
    /// raw request text, so tests exercise the real client instead of a mock.
    async fn one_shot(status: u16, body: &str) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let body = body.to_string();
        tokio::spawn(async move {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let req = read_request(&mut sock).await;
            let _ = tx.send(req);
            let resp = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
        (format!("http://{addr}"), rx)
    }

    async fn read_request(sock: &mut tokio::net::TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = match sock.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
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
        String::from_utf8_lossy(&buf).to_string()
    }

    #[tokio::test]
    async fn a_2xx_response_goes_out_the_success_port() {
        let (base, _rx) = one_shot(200, r#"{"ok":true,"n":7}"#).await;
        let r = HttpRequestRule::new();
        let c = ctx("http-request", json!({ "url": format!("{base}/ping") }));
        let (port, data) = one(r.handle(&c, msg(json!({}))).await);
        assert_eq!(port, "success");
        assert_eq!(data["status"], 200);
        assert_eq!(data["body"]["n"], 7);
        assert!(data["headers"]["content-type"]
            .as_str()
            .unwrap()
            .contains("json"));
    }

    /// The Go rule sent this down the error branch with the status thrown away.
    #[tokio::test]
    async fn a_non_2xx_response_goes_out_failed_and_keeps_the_status() {
        let (base, _rx) = one_shot(500, r#"{"error":"boom"}"#).await;
        let r = HttpRequestRule::new();
        let c = ctx("http-request", json!({ "url": base }));
        let (port, data) = one(r.handle(&c, msg(json!({}))).await);
        assert_eq!(port, "failed");
        assert_eq!(data["status"], 500);
        assert_eq!(data["body"]["error"], "boom");
    }

    #[tokio::test]
    async fn url_headers_and_body_are_interpolated() {
        let (base, rx) = one_shot(200, "{}").await;
        let r = HttpRequestRule::new();
        let c = ctx(
            "http-request",
            json!({
                "method": "post",
                "url": format!("{base}/dev/${{device_id}}"),
                "headers": { "x-token": "t-${token}" },
                "body": "{\"temp\": ${temperature}}"
            }),
        );
        let out = r
            .handle(
                &c,
                msg(json!({ "device_id": "d1", "token": "abc", "temperature": 31.5 })),
            )
            .await;
        assert_eq!(one(out).0, "success");

        let req = rx.await.unwrap();
        assert!(req.starts_with("POST /dev/d1 "), "{req}");
        assert!(req.to_lowercase().contains("x-token: t-abc"), "{req}");
        assert!(req.contains(r#"{"temp": 31.5}"#), "{req}");
        assert!(
            req.to_lowercase()
                .contains("content-type: application/json"),
            "{req}"
        );
    }

    #[tokio::test]
    async fn parse_json_off_keeps_the_body_as_text() {
        let (base, _rx) = one_shot(200, r#"{"a":1}"#).await;
        let r = HttpRequestRule::new();
        let c = ctx("http-request", json!({ "url": base, "parseJson": false }));
        let (_, data) = one(r.handle(&c, msg(json!({}))).await);
        assert_eq!(data["body"], r#"{"a":1}"#);
    }

    #[tokio::test]
    async fn a_transport_failure_is_a_fail_not_a_failed_port() {
        let r = HttpRequestRule::new();
        // Port 1 on loopback refuses connections everywhere we run tests.
        let c = ctx(
            "http-request",
            json!({ "url": "http://127.0.0.1:1/", "timeoutMs": 500 }),
        );
        let err = failure(r.handle(&c, msg(json!({}))).await);
        assert!(err.contains("Gọi HTTP thất bại"), "{err}");
    }

    #[tokio::test]
    async fn bad_config_fails_before_any_request() {
        let r = HttpRequestRule::new();
        let c = ctx("http-request", json!({}));
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("Thiếu URL"));

        let c = ctx(
            "http-request",
            json!({ "url": "https://x/", "method": "FETCH" }),
        );
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("không hợp lệ"));

        let c = ctx("http-request", json!({ "url": "ftp://x/" }));
        assert!(failure(r.handle(&c, msg(json!({}))).await).contains("http://"));
    }

    #[test]
    fn validate_rejects_missing_url_bad_method_and_non_object_headers() {
        let r = HttpRequestRule::new();
        assert!(!r.validate(&json!({})).is_empty());
        assert!(!r
            .validate(&json!({ "url": "https://x/", "method": "TRACE" }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "url": "https://x/", "headers": "a=b" }))
            .is_empty());
        assert!(r
            .validate(&json!({ "url": "https://x/", "method": "post", "headers": {} }))
            .is_empty());
    }

    #[test]
    fn success_failed_and_error_are_three_distinct_ports() {
        let r = HttpRequestRule::new();
        assert!(r.spec().has_output("success"));
        assert!(r.spec().has_output("failed"));
        assert!(r.spec().has_output("error"));
        assert_eq!(
            r.spec().output("success").unwrap().arity,
            crate::engine::spec::PortArity::Many
        );
    }
}
