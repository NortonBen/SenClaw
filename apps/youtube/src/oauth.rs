//! OAuth 2.0 (Installed-App / loopback) for the YouTube Data API — the ONLY path
//! for owner-level comment moderation (`comments.setModerationStatus`:
//! heldForReview / rejected / banAuthor), which InnerTube can't do.
//!
//! The user creates a **Desktop-app** OAuth client in Google Cloud Console and
//! pastes its id/secret; consent runs in the browser via a loopback redirect.
//! Client secret + tokens live ONLY in the app's local DB and are never logged.

use crate::db::Db;
use serde_json::{json, Value};
use std::time::Duration;

const SCOPE: &str = "https://www.googleapis.com/auth/youtube.force-ssl";
const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}

/// The app's own HTTP port (daemon-assigned). A Desktop-app OAuth client permits
/// any loopback port, so this can vary per launch.
fn app_port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(4491)
}
fn redirect_uri() -> String {
    format!("http://127.0.0.1:{}/api/oauth/callback", app_port())
}

pub fn set_config(db: &Db, client_id: &str, client_secret: &str) -> Result<(), String> {
    db.set_kv(
        "oauth_config",
        &json!({ "clientId": client_id, "clientSecret": client_secret }),
    )
    .map_err(|e| e.to_string())
}

fn config(db: &Db) -> Option<(String, String)> {
    let v = db.get_kv("oauth_config").ok().flatten()?;
    Some((
        v.get("clientId")?.as_str()?.to_string(),
        v.get("clientSecret")?.as_str()?.to_string(),
    ))
}

/// Configuration / authorization state (no secrets leak out).
pub fn status(db: &Db) -> Value {
    let configured = config(db).is_some();
    let toks = db.get_kv("oauth_tokens").ok().flatten();
    let authorized = toks
        .as_ref()
        .and_then(|t| t.get("refreshToken"))
        .and_then(|x| x.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let expires_at = toks
        .as_ref()
        .and_then(|t| t.get("expiresAt"))
        .and_then(|x| x.as_i64());
    json!({
        "configured": configured,
        "authorized": authorized,
        "expiresAt": expires_at,
        "redirectUri": redirect_uri(),
        "scope": SCOPE,
        "identity": db.get_kv("oauth_identity").ok().flatten(),
    })
}

/// Parse the signed-in channel identity from a `channels.list?mine=true` response.
fn parse_identity(v: &Value) -> Value {
    let item = &v["items"][0];
    let sn = &item["snippet"];
    json!({
        "channelId": item.get("id").and_then(|x| x.as_str()).unwrap_or(""),
        "title": sn.get("title").and_then(|x| x.as_str()).unwrap_or(""),
        "thumbnail": sn.pointer("/thumbnails/default/url").and_then(|x| x.as_str()).unwrap_or(""),
    })
}

/// Who is signed in — the authorized channel's title/id/avatar. Caches into the DB.
pub async fn whoami(db: &Db) -> Result<Value, String> {
    let token = access_token(db).await?;
    let resp = reqwest::Client::new()
        .get("https://www.googleapis.com/youtube/v3/channels?part=snippet&mine=true")
        .bearer_auth(token)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "channels.list {code}: {}",
            body.chars().take(200).collect::<String>()
        ));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let identity = parse_identity(&v);
    let _ = db.set_kv("oauth_identity", &identity);
    Ok(identity)
}

/// Sign out: drop tokens + cached identity.
pub fn logout(db: &Db) {
    let _ = db.del_kv("oauth_tokens");
    let _ = db.del_kv("oauth_identity");
}

/// The consent URL to open in the browser (offline access so we get a refresh token).
pub fn auth_url(db: &Db) -> Result<String, String> {
    let (client_id, _) = config(db).ok_or("chưa cấu hình OAuth client (client_id/secret)")?;
    Ok(format!(
        "{AUTH_ENDPOINT}?response_type=code&client_id={}&redirect_uri={}&scope={}&access_type=offline&prompt=consent",
        enc(&client_id),
        enc(&redirect_uri()),
        enc(SCOPE),
    ))
}

/// Exchange the consent `code` for tokens and store them.
pub async fn exchange_code(db: &Db, code: &str) -> Result<(), String> {
    let (cid, secret) = config(db).ok_or("chưa cấu hình OAuth client")?;
    let ru = redirect_uri();
    let resp = reqwest::Client::new()
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("code", code),
            ("client_id", &cid),
            ("client_secret", &secret),
            ("redirect_uri", &ru),
            ("grant_type", "authorization_code"),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    store_tokens(db, &v, None)
}

fn store_tokens(db: &Db, v: &Value, keep_refresh: Option<String>) -> Result<(), String> {
    let access = v
        .get("access_token")
        .and_then(|x| x.as_str())
        .ok_or_else(|| {
            let err = v
                .get("error_description")
                .or_else(|| v.get("error"))
                .and_then(|x| x.as_str());
            format!(
                "token exchange thất bại: {}",
                err.unwrap_or("(no access_token)")
            )
        })?;
    let expires_in = v.get("expires_in").and_then(|x| x.as_i64()).unwrap_or(3600);
    let refresh = v
        .get("refresh_token")
        .and_then(|x| x.as_str())
        .map(String::from)
        .or(keep_refresh); // refresh_token is only returned on first consent
    let expires_at = crate::api::now() + expires_in - 60; // 60s safety margin
    db.set_kv(
        "oauth_tokens",
        &json!({ "accessToken": access, "refreshToken": refresh, "expiresAt": expires_at }),
    )
    .map_err(|e| e.to_string())
}

/// A valid access token, refreshing via the refresh_token when expired.
async fn access_token(db: &Db) -> Result<String, String> {
    let toks = db
        .get_kv("oauth_tokens")
        .ok()
        .flatten()
        .ok_or("chưa uỷ quyền OAuth")?;
    let access = toks
        .get("accessToken")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    let expires_at = toks.get("expiresAt").and_then(|x| x.as_i64()).unwrap_or(0);
    if !access.is_empty() && crate::api::now() < expires_at {
        return Ok(access.to_string());
    }
    // Refresh.
    let refresh = toks
        .get("refreshToken")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("token hết hạn và không có refresh_token — hãy uỷ quyền lại")?
        .to_string();
    let (cid, secret) = config(db).ok_or("chưa cấu hình OAuth client")?;
    let resp = reqwest::Client::new()
        .post(TOKEN_ENDPOINT)
        .form(&[
            ("client_id", cid.as_str()),
            ("client_secret", secret.as_str()),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    store_tokens(db, &v, Some(refresh))?;
    v.get("access_token")
        .and_then(|x| x.as_str())
        .map(String::from)
        .ok_or_else(|| "refresh không trả về access_token".to_string())
}

/// Build + validate the `setModerationStatus` request URL (pure → testable). The
/// `banAuthor` rule (only with `rejected`) is enforced here so we never send a
/// request Google would 400 with `banWithoutReject`.
pub fn moderation_url(comment_id: &str, status: &str, ban: bool) -> Result<String, String> {
    if comment_id.trim().is_empty() {
        return Err("thiếu comment_id".into());
    }
    if !matches!(status, "heldForReview" | "published" | "rejected") {
        return Err("status phải là heldForReview | published | rejected".into());
    }
    if ban && status != "rejected" {
        return Err(
            "banAuthor chỉ hợp lệ khi status=rejected (nếu không → banWithoutReject 400)".into(),
        );
    }
    Ok(format!(
        "https://www.googleapis.com/youtube/v3/comments/setModerationStatus?id={}&moderationStatus={}{}",
        enc(comment_id),
        status,
        if ban { "&banAuthor=true" } else { "" }
    ))
}

/// Owner-level moderation via the Data API. Needs a prior OAuth authorization.
pub async fn moderate(db: &Db, comment_id: &str, status: &str, ban: bool) -> Result<Value, String> {
    let url = moderation_url(comment_id, status, ban)?;
    let token = access_token(db).await?;
    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(token)
        .header("content-length", "0")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let code = resp.status();
    if code.is_success() {
        db.log(
            "moderate",
            &format!("{status}:{comment_id}"),
            crate::api::now(),
        );
        Ok(
            json!({ "ok": true, "commentId": comment_id, "moderationStatus": status, "banned": ban }),
        )
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!(
            "YouTube Data API {code}: {}",
            body.chars().take(300).collect::<String>()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_db() -> Db {
        let p: PathBuf = std::env::temp_dir().join(format!("yt-oauth-{}.db", crate::db::new_id()));
        Db::open(&p).unwrap()
    }

    #[test]
    fn status_and_auth_url() {
        let db = tmp_db();
        assert_eq!(status(&db)["configured"], false);
        assert!(auth_url(&db).is_err(), "no client yet");

        set_config(&db, "CID.apps.googleusercontent.com", "SECRET").unwrap();
        let st = status(&db);
        assert_eq!(st["configured"], true);
        assert_eq!(st["authorized"], false);

        let url = auth_url(&db).unwrap();
        assert!(url.contains("client_id=CID.apps.googleusercontent.com"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("youtube.force-ssl")); // scope, percent-encoded ':' but keyword survives
        assert!(url.contains("%2Fcallback"));
    }

    #[test]
    fn moderation_url_rules() {
        assert!(moderation_url("cid", "heldForReview", false)
            .unwrap()
            .contains("moderationStatus=heldForReview"));
        assert!(moderation_url("cid", "rejected", true)
            .unwrap()
            .contains("banAuthor=true"));
        // ban only with rejected
        assert!(moderation_url("cid", "published", true)
            .unwrap_err()
            .contains("banWithoutReject"));
        // invalid status
        assert!(moderation_url("cid", "nuke", false).is_err());
        // missing id
        assert!(moderation_url("", "published", false).is_err());
    }

    #[test]
    fn parses_channel_identity() {
        let resp = json!({ "items": [{
            "id": "UC123",
            "snippet": { "title": "Kênh của tôi", "thumbnails": { "default": { "url": "https://i/avatar.jpg" } } }
        }]});
        let id = parse_identity(&resp);
        assert_eq!(id["channelId"], "UC123");
        assert_eq!(id["title"], "Kênh của tôi");
        assert_eq!(id["thumbnail"], "https://i/avatar.jpg");
    }

    #[test]
    fn logout_clears_tokens_and_identity() {
        let db = tmp_db();
        set_config(&db, "c", "s").unwrap();
        store_tokens(
            &db,
            &json!({ "access_token": "AT", "refresh_token": "RT", "expires_in": 3600 }),
            None,
        )
        .unwrap();
        db.set_kv("oauth_identity", &json!({ "title": "X" }))
            .unwrap();
        assert_eq!(status(&db)["authorized"], true);
        logout(&db);
        assert_eq!(status(&db)["authorized"], false);
        assert!(status(&db)["identity"].is_null());
    }

    #[test]
    fn store_and_status_authorized() {
        let db = tmp_db();
        set_config(&db, "c", "s").unwrap();
        store_tokens(
            &db,
            &json!({ "access_token": "AT", "refresh_token": "RT", "expires_in": 3600 }),
            None,
        )
        .unwrap();
        let st = status(&db);
        assert_eq!(st["authorized"], true);
        // A refresh response WITHOUT a refresh_token keeps the old one.
        store_tokens(
            &db,
            &json!({ "access_token": "AT2", "expires_in": 3600 }),
            Some("RT".into()),
        )
        .unwrap();
        let toks = db.get_kv("oauth_tokens").unwrap().unwrap();
        assert_eq!(toks["refreshToken"], "RT");
        assert_eq!(toks["accessToken"], "AT2");
    }
}
