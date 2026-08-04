//! Google REST client: OAuth 2.0 (auth-code + refresh) and thin wrappers over
//! the Gmail, Calendar and Drive v3 HTTP APIs. No SDK — every call is a plain
//! reqwest request, so the whole surface is auditable in one file.
//!
//! Token lifecycle: `access_token()` returns a live token, refreshing through
//! the stored `refresh_token` when the saved one is expired (or on a 401 via
//! `authed()`'s single retry). Manually pasted tokens have no refresh_token —
//! they simply expire and the error says to reconnect.

use anyhow::{anyhow, bail, Result};
use base64::Engine;
use chrono::{DateTime, Local, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::{now, Db, Tokens};

pub const SCOPES: [&str; 5] = [
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/gmail.send",
    "https://www.googleapis.com/auth/calendar.events",
    "https://www.googleapis.com/auth/drive.file",
    "https://www.googleapis.com/auth/drive.readonly",
];

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const NOT_CONNECTED: &str = "Chưa kết nối Google — mở UI của app để kết nối (OAuth) hoặc dán access token, hoặc dùng gworkspace_set_settings với accessToken.";

fn enc(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}

/// Consent-screen URL for the auth-code flow.
pub fn auth_url(client_id: &str, redirect_uri: &str) -> String {
    format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent",
        enc(client_id),
        enc(redirect_uri),
        enc(&SCOPES.join(" ")),
    )
}

/// Shared handle passed to REST + MCP layers.
#[derive(Clone)]
pub struct Google {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
}

/// Pull a human message out of a Google error body (API or OAuth shape).
fn google_err(status: reqwest::StatusCode, body: &str) -> anyhow::Error {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["error"]["message"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            v["error"].as_str().map(|e| {
                let desc = v["error_description"].as_str().unwrap_or("");
                if desc.is_empty() {
                    e.to_string()
                } else {
                    format!("{e}: {desc}")
                }
            })
        })
        .unwrap_or_else(|| {
            let mut t = body.trim().to_string();
            // truncate on a char boundary — Google errors can carry UTF-8
            if t.len() > 300 {
                let mut cut = 300;
                while !t.is_char_boundary(cut) {
                    cut -= 1;
                }
                t.truncate(cut);
            }
            t
        });
    if status.as_u16() == 401 {
        anyhow!("Google trả 401 ({msg}) — token đã hết hạn hoặc bị thu hồi. Kết nối lại trong UI của app.")
    } else {
        anyhow!("Google API {status}: {msg}")
    }
}

impl Google {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            http: reqwest::Client::new(),
        }
    }

    // ---- OAuth ----

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<Value> {
        let res = self.http.post(TOKEN_URL).form(form).send().await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(google_err(status, &body));
        }
        Ok(serde_json::from_str(&body)?)
    }

    /// Exchange an auth code; keeps any prior refresh_token if Google omits it.
    pub async fn exchange_code(&self, code: &str, redirect_uri: &str) -> Result<Tokens> {
        let (client_id, client_secret) = self.credentials()?;
        let v = self
            .token_request(&[
                ("client_id", &client_id),
                ("client_secret", &client_secret),
                ("code", code),
                ("grant_type", "authorization_code"),
                ("redirect_uri", redirect_uri),
            ])
            .await?;
        let prior = self.db.tokens();
        let tokens = tokens_from_response(&v, &prior.refresh_token);
        self.db.save_tokens(&tokens)?;
        Ok(tokens)
    }

    fn credentials(&self) -> Result<(String, String)> {
        let id = self.db.client_id();
        let secret = self.db.client_secret();
        if id.is_empty() || secret.is_empty() {
            bail!("Chưa cấu hình Google Client ID / Client Secret — đặt trong Settings của app (hoặc gworkspace_set_settings).");
        }
        Ok((id, secret))
    }

    async fn refresh(&self, refresh_token: &str) -> Result<Tokens> {
        let (client_id, client_secret) = self.credentials()?;
        let v = self
            .token_request(&[
                ("client_id", &client_id),
                ("client_secret", &client_secret),
                ("refresh_token", refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .await?;
        let tokens = tokens_from_response(&v, refresh_token);
        self.db.save_tokens(&tokens)?;
        Ok(tokens)
    }

    /// A live access token: the saved one, or a refreshed one when expired.
    pub async fn access_token(&self) -> Result<String> {
        let t = self.db.tokens();
        if t.access_token.is_empty() {
            bail!(NOT_CONNECTED);
        }
        let expired = t.expires_at > 0 && t.expires_at < now() + 60;
        if expired && !t.refresh_token.is_empty() {
            return Ok(self.refresh(&t.refresh_token).await?.access_token);
        }
        Ok(t.access_token)
    }

    // ---- authenticated requests (single 401-retry via refresh) ----

    async fn authed(&self, build: impl Fn(&str) -> reqwest::RequestBuilder) -> Result<Value> {
        let token = self.access_token().await?;
        let res = build(&token).send().await?;
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        if status.as_u16() == 401 {
            let refresh_token = self.db.tokens().refresh_token;
            if !refresh_token.is_empty() {
                let fresh = self.refresh(&refresh_token).await?;
                let res = build(&fresh.access_token).send().await?;
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                if !status.is_success() {
                    return Err(google_err(status, &body));
                }
                return parse_json(&body);
            }
        }
        if !status.is_success() {
            return Err(google_err(status, &body));
        }
        parse_json(&body)
    }

    async fn get(&self, url: String) -> Result<Value> {
        self.authed(|tok| self.http.get(&url).bearer_auth(tok))
            .await
    }

    async fn post_json(&self, url: String, body: Value) -> Result<Value> {
        self.authed(|tok| self.http.post(&url).bearer_auth(tok).json(&body))
            .await
    }

    // ---- Gmail ----

    pub async fn list_emails(&self, max: u32, q: &str) -> Result<Value> {
        let mut url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults={}",
            max.clamp(1, 50)
        );
        if !q.is_empty() {
            url.push_str(&format!("&q={}", enc(q)));
        }
        let list = self.get(url).await?;
        let ids: Vec<String> = list["messages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let metas = futures_util::future::join_all(ids.iter().map(|id| {
            let url = format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{id}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date"
            );
            self.get(url)
        }))
        .await;

        let emails: Vec<Value> = metas
            .into_iter()
            .filter_map(|m| m.ok())
            .map(|m| {
                json!({
                    "id": m["id"],
                    "threadId": m["threadId"],
                    "subject": header(&m, "Subject"),
                    "from": header(&m, "From"),
                    "date": header(&m, "Date"),
                    "snippet": m["snippet"],
                })
            })
            .collect();
        Ok(json!({ "count": emails.len(), "emails": emails }))
    }

    pub async fn read_email(&self, id: &str) -> Result<Value> {
        let msg = self
            .get(format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
                enc(id)
            ))
            .await?;
        Ok(digest_message(&msg))
    }

    pub async fn send_email(&self, to: &str, subject: &str, body_html: &str) -> Result<Value> {
        let raw = build_mime(to, subject, body_html);
        self.post_json(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/send".into(),
            json!({ "raw": raw }),
        )
        .await
    }

    // ---- Calendar ----

    pub async fn list_events(&self, max: u32, days_ahead: u32) -> Result<Value> {
        let time_min = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin={}&maxResults={}&singleEvents=true&orderBy=startTime",
            enc(&time_min),
            max.clamp(1, 100),
        );
        if days_ahead > 0 {
            let time_max = (Utc::now() + chrono::Duration::days(days_ahead as i64))
                .to_rfc3339_opts(SecondsFormat::Secs, true);
            url.push_str(&format!("&timeMax={}", enc(&time_max)));
        }
        let v = self.get(url).await?;
        let events: Vec<Value> = v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|e| {
                        json!({
                            "id": e["id"],
                            "summary": e["summary"],
                            "description": e["description"],
                            "location": e["location"],
                            "start": e["start"]["dateTime"].as_str().or(e["start"]["date"].as_str()),
                            "end": e["end"]["dateTime"].as_str().or(e["end"]["date"].as_str()),
                            "htmlLink": e["htmlLink"],
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({ "count": events.len(), "events": events }))
    }

    pub async fn create_event(
        &self,
        summary: &str,
        description: &str,
        start: &str,
        end: &str,
    ) -> Result<Value> {
        let event = json!({
            "summary": summary,
            "description": description,
            "start": time_field(start)?,
            "end": time_field(end)?,
        });
        self.post_json(
            "https://www.googleapis.com/calendar/v3/calendars/primary/events".into(),
            event,
        )
        .await
    }

    // ---- Drive ----

    pub async fn list_files(&self, max: u32, q: &str) -> Result<Value> {
        let mut url = format!(
            "https://www.googleapis.com/drive/v3/files?pageSize={}&orderBy=modifiedTime%20desc&fields=files(id,name,mimeType,modifiedTime,size,webViewLink)",
            max.clamp(1, 100)
        );
        if !q.is_empty() {
            url.push_str(&format!("&q={}", enc(q)));
        }
        let v = self.get(url).await?;
        let files = v["files"].clone();
        Ok(json!({
            "count": files.as_array().map(|a| a.len()).unwrap_or(0),
            "files": files,
        }))
    }

    pub async fn upload_file(&self, name: &str, mime: &str, content: &str) -> Result<Value> {
        let mime = if mime.is_empty() { "text/plain" } else { mime };
        let (body, content_type) = multipart_related(name, mime, content.as_bytes());
        self.authed(|tok| {
            self.http
                .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&fields=id,name,webViewLink")
                .bearer_auth(tok)
                .header("content-type", content_type.clone())
                .body(body.clone())
        })
        .await
    }
}

// ---- pure helpers (unit-tested, no network) ----

fn parse_json(body: &str) -> Result<Value> {
    if body.trim().is_empty() {
        return Ok(json!({}));
    }
    Ok(serde_json::from_str(body)?)
}

/// Map a token-endpoint response to `Tokens`, keeping `fallback_refresh`
/// when Google omits refresh_token (it only sends it on the first consent).
fn tokens_from_response(v: &Value, fallback_refresh: &str) -> Tokens {
    let expires_in = v["expires_in"].as_i64().unwrap_or(3600);
    Tokens {
        access_token: v["access_token"].as_str().unwrap_or_default().to_string(),
        refresh_token: v["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(fallback_refresh)
            .to_string(),
        expires_at: now() + expires_in,
    }
}

fn header(msg: &Value, name: &str) -> Value {
    msg["payload"]["headers"]
        .as_array()
        .and_then(|hs| {
            hs.iter().find(|h| {
                h["name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
        })
        .map(|h| h["value"].clone())
        .unwrap_or(Value::Null)
}

fn b64url_decode(s: &str) -> Option<String> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.trim_end_matches('='))
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Walk a Gmail payload tree collecting text/plain, text/html and attachments.
fn walk_parts(part: &Value, text: &mut String, html: &mut String, atts: &mut Vec<Value>) {
    let mime = part["mimeType"].as_str().unwrap_or("");
    let filename = part["filename"].as_str().unwrap_or("");
    if !filename.is_empty() {
        atts.push(json!({
            "filename": filename,
            "mimeType": mime,
            "size": part["body"]["size"],
            "attachmentId": part["body"]["attachmentId"],
        }));
    } else if let Some(data) = part["body"]["data"].as_str() {
        if let Some(decoded) = b64url_decode(data) {
            if mime.starts_with("text/plain") {
                text.push_str(&decoded);
            } else if mime.starts_with("text/html") {
                html.push_str(&decoded);
            }
        }
    }
    if let Some(children) = part["parts"].as_array() {
        for child in children {
            walk_parts(child, text, html, atts);
        }
    }
}

/// Reduce a `format=full` Gmail message to the fields an agent needs.
fn digest_message(msg: &Value) -> Value {
    let mut text = String::new();
    let mut html = String::new();
    let mut atts = Vec::new();
    walk_parts(&msg["payload"], &mut text, &mut html, &mut atts);
    json!({
        "id": msg["id"],
        "threadId": msg["threadId"],
        "subject": header(msg, "Subject"),
        "from": header(msg, "From"),
        "to": header(msg, "To"),
        "date": header(msg, "Date"),
        "snippet": msg["snippet"],
        "bodyText": text,
        "bodyHtml": if text.is_empty() { Value::String(html) } else { Value::String(String::new()) },
        "attachments": atts,
        "labelIds": msg["labelIds"],
    })
}

/// RFC 2047 B-encoding for non-ASCII header values (Subject: tiếng Việt…).
fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        value.to_string()
    } else {
        format!(
            "=?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
        )
    }
}

/// Build the base64url `raw` payload Gmail's send endpoint expects.
fn build_mime(to: &str, subject: &str, body_html: &str) -> String {
    let message = format!(
        "To: {}\r\nSubject: {}\r\nMIME-Version: 1.0\r\nContent-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
        to,
        encode_header(subject),
        base64::engine::general_purpose::STANDARD.encode(body_html.as_bytes()),
    );
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(message.as_bytes())
}

/// Calendar start/end field from user input: RFC3339 stays as-is; a naive
/// "YYYY-MM-DDTHH:MM[:SS]" gets the server's local offset; a bare date becomes
/// an all-day field.
fn time_field(input: &str) -> Result<Value> {
    let s = input.trim();
    if DateTime::parse_from_rfc3339(s).is_ok() {
        return Ok(json!({ "dateTime": s }));
    }
    if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
        return Ok(json!({ "date": s }));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
            if let Some(local) = Local.from_local_datetime(&naive).single() {
                return Ok(json!({
                    "dateTime": local.to_rfc3339_opts(SecondsFormat::Secs, false)
                }));
            }
        }
    }
    bail!("Thời gian không hợp lệ: '{input}' — dùng RFC3339 (2026-07-30T15:00:00+07:00), 'YYYY-MM-DDTHH:MM' (giờ local) hoặc 'YYYY-MM-DD' (cả ngày).")
}

/// Google multipart/related upload body (reqwest's multipart is form-data,
/// which the Drive endpoint rejects — so the body is assembled by hand).
fn multipart_related(name: &str, mime: &str, content: &[u8]) -> (Vec<u8>, String) {
    let boundary = "senclaw_gws_boundary_7f3a";
    let metadata = json!({ "name": name, "mimeType": mime }).to_string();
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{metadata}\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("--{boundary}\r\nContent-Type: {mime}\r\n\r\n").as_bytes());
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--").as_bytes());
    (body, format!("multipart/related; boundary={boundary}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_url_carries_client_and_scopes() {
        let url = auth_url("my-client", "http://127.0.0.1:4310/api/auth/callback");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=my%2Dclient"));
        assert!(url.contains("gmail%2Ereadonly")); // NON_ALPHANUMERIC encodes '.' too
        assert!(url.contains("access_type=offline"));
        assert!(url
            .contains("redirect_uri=http%3A%2F%2F127%2E0%2E0%2E1%3A4310%2Fapi%2Fauth%2Fcallback"));
    }

    #[test]
    fn tokens_from_response_keeps_prior_refresh() {
        let v = json!({ "access_token": "at2", "expires_in": 100 });
        let t = tokens_from_response(&v, "prior-refresh");
        assert_eq!(t.access_token, "at2");
        assert_eq!(t.refresh_token, "prior-refresh");
        assert!(t.expires_at > now());

        let v2 = json!({ "access_token": "at3", "refresh_token": "fresh" });
        assert_eq!(tokens_from_response(&v2, "prior").refresh_token, "fresh");
    }

    #[test]
    fn mime_encodes_utf8_subject_and_body() {
        let raw = build_mime("a@b.c", "Xin chào đội ngũ", "<p>Nội dung tiếng Việt</p>");
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&raw)
            .unwrap();
        let msg = String::from_utf8(decoded).unwrap();
        assert!(msg.contains("To: a@b.c"));
        assert!(msg.contains("Subject: =?UTF-8?B?"));
        assert!(msg.contains("Content-Transfer-Encoding: base64"));
        // body round-trips
        let body_b64 = msg.split("\r\n\r\n").nth(1).unwrap();
        let body = base64::engine::general_purpose::STANDARD
            .decode(body_b64)
            .unwrap();
        assert_eq!(
            String::from_utf8(body).unwrap(),
            "<p>Nội dung tiếng Việt</p>"
        );
        // ASCII subject stays plain
        assert_eq!(encode_header("Hello"), "Hello");
    }

    #[test]
    fn time_field_variants() {
        assert_eq!(
            time_field("2026-07-30T15:00:00+07:00").unwrap()["dateTime"],
            "2026-07-30T15:00:00+07:00"
        );
        assert_eq!(time_field("2026-07-30").unwrap()["date"], "2026-07-30");
        let naive = time_field("2026-07-30T15:00").unwrap();
        let dt = naive["dateTime"].as_str().unwrap();
        assert!(dt.starts_with("2026-07-30T15:00:00"));
        assert!(
            DateTime::parse_from_rfc3339(dt).is_ok(),
            "local offset appended: {dt}"
        );
        assert!(time_field("mai 3h").is_err());
    }

    #[test]
    fn digest_extracts_nested_text_and_attachments() {
        let b64 = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        let msg = json!({
            "id": "m1",
            "snippet": "hi",
            "payload": {
                "mimeType": "multipart/mixed",
                "headers": [
                    { "name": "Subject", "value": "Báo cáo" },
                    { "name": "From", "value": "x@y.z" }
                ],
                "parts": [
                    {
                        "mimeType": "multipart/alternative",
                        "filename": "",
                        "parts": [
                            { "mimeType": "text/plain", "filename": "", "body": { "data": b64("nội dung thư") } },
                            { "mimeType": "text/html", "filename": "", "body": { "data": b64("<b>html</b>") } }
                        ]
                    },
                    { "mimeType": "application/pdf", "filename": "bc.pdf", "body": { "size": 999, "attachmentId": "att1" } }
                ]
            }
        });
        let d = digest_message(&msg);
        assert_eq!(d["subject"], "Báo cáo");
        assert_eq!(d["bodyText"], "nội dung thư");
        assert_eq!(d["bodyHtml"], ""); // text wins, html dropped to keep payload small
        assert_eq!(d["attachments"][0]["filename"], "bc.pdf");
    }

    #[test]
    fn multipart_body_shape() {
        let (body, ct) = multipart_related("a.txt", "text/plain", "xin chào".as_bytes());
        let s = String::from_utf8(body).unwrap();
        assert!(ct.starts_with("multipart/related; boundary="));
        assert!(s.contains(r#""name":"a.txt""#));
        assert!(s.contains("Content-Type: text/plain\r\n\r\nxin chào"));
        assert!(s.trim_end().ends_with("--"));
    }

    #[test]
    fn google_err_shapes() {
        let api = google_err(
            reqwest::StatusCode::FORBIDDEN,
            r#"{"error":{"message":"Insufficient Permission","status":"PERMISSION_DENIED"}}"#,
        );
        assert!(api.to_string().contains("Insufficient Permission"));
        let oauth = google_err(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_grant","error_description":"Bad Request"}"#,
        );
        assert!(oauth.to_string().contains("invalid_grant: Bad Request"));
        let e401 = google_err(reqwest::StatusCode::UNAUTHORIZED, "{}");
        assert!(e401.to_string().contains("Kết nối lại"));
    }
}
