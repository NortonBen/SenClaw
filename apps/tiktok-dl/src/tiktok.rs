//! TikTok link resolver. Talks to the public tikwm.com endpoint (the same
//! service the desktop downloaders use for HD / no-watermark variants) and
//! normalizes its response into one flat `meta` JSON shape the rest of the app
//! shares. No TikTok login, no cookies — only public posts resolve.
//!
//! tikwm free tier allows ~1 request/second, so every call goes through a
//! global gate that spaces requests ≥ [`GATE_MS`] apart; concurrent workers
//! simply queue on the gate. Cloudflare occasionally challenges this endpoint
//! (it always serves HTML in that case) — surfaced as a clear retryable error,
//! never a parse panic.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::time::Instant;

const API: &str = "https://www.tikwm.com/api/";
const USER_POSTS: &str = "https://www.tikwm.com/api/user/posts";
const GATE_MS: u64 = 1600;

/// Browser-shaped UA — the endpoint (and the TikTok CDN the files live on)
/// serve bot UAs a Cloudflare challenge instead of JSON/bytes.
pub const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
                      (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .expect("build http client")
}

pub struct Resolver {
    http: reqwest::Client,
    gate: tokio::sync::Mutex<Instant>,
}

impl Resolver {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            gate: tokio::sync::Mutex::new(Instant::now() - Duration::from_secs(5)),
        }
    }

    /// Space calls out to the rate limit. Holding the lock across the sleep is
    /// deliberate: it makes concurrent callers line up single-file.
    async fn wait_gate(&self) {
        let mut last = self.gate.lock().await;
        let elapsed = last.elapsed();
        let min = Duration::from_millis(GATE_MS);
        if elapsed < min {
            tokio::time::sleep(min - elapsed).await;
        }
        *last = Instant::now();
    }

    async fn call(&self, endpoint: &str, form: &[(&str, &str)]) -> Result<Value> {
        let mut last_err = anyhow!("resolver: chưa gọi");
        for attempt in 0..4 {
            if attempt > 0 {
                // Grows 2s → 4s → 6s; the per-call gate below adds its own
                // spacing on top, so parallel workers cannot re-trip the limit.
                tokio::time::sleep(Duration::from_millis(2000 * attempt)).await;
            }
            self.wait_gate().await;
            let resp = self
                .http
                .post(endpoint)
                .header("Referer", "https://www.tikwm.com/")
                .form(form)
                .send()
                .await;
            let body = match resp {
                Ok(r) => r.text().await.unwrap_or_default(),
                Err(e) => {
                    last_err = anyhow!("không gọi được máy chủ phân giải: {e}");
                    continue;
                }
            };
            if body.trim_start().starts_with('<') {
                last_err = anyhow!(
                    "máy chủ phân giải đang bị Cloudflare chặn tạm thời — thử lại sau ít phút"
                );
                continue;
            }
            let v: Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(_) => {
                    last_err = anyhow!("máy chủ phân giải trả dữ liệu lạ");
                    continue;
                }
            };
            if v["code"].as_i64() != Some(0) {
                let msg = v["msg"].as_str().unwrap_or("lỗi không rõ");
                // Rate-limit responses ("Free Api Limit: 1 request/second…")
                // are transient — back off and retry. Everything else ("Url
                // parsing is failed" = link sai / video riêng tư / đã xoá)
                // cannot improve on retry, so fail fast.
                if msg.to_ascii_lowercase().contains("limit") {
                    last_err = anyhow!("máy chủ phân giải đang giới hạn tần suất — thử lại sau");
                    continue;
                }
                bail!("link không phân giải được ({msg}) — kiểm tra link có đúng và video còn công khai không");
            }
            return Ok(v["data"].clone());
        }
        Err(last_err)
    }

    /// Resolve one post URL → flat `meta` (shape documented in [`normalize`]).
    pub async fn resolve(&self, url: &str) -> Result<Value> {
        let url = url.trim();
        if url.is_empty() {
            bail!("thiếu link");
        }
        let data = self.call(API, &[("url", url), ("hd", "1")]).await?;
        Ok(normalize(&data))
    }

    /// Newest posts of one profile — best effort: tikwm sits this endpoint
    /// behind stricter Cloudflare rules than `/api/`, so from some networks it
    /// is simply unavailable. Callers get a clean error and the UI suggests
    /// pasting links instead (same fallback the original desktop app had).
    pub async fn user_posts(&self, unique_id: &str, count: i64, cursor: &str) -> Result<Value> {
        let uid = unique_id.trim().trim_start_matches('@');
        if uid.is_empty() {
            bail!("thiếu unique_id (tên tài khoản, ví dụ @tiktok)");
        }
        let count = count.clamp(1, 34).to_string();
        let cursor = if cursor.is_empty() { "0" } else { cursor };
        let data = self
            .call(
                USER_POSTS,
                &[("unique_id", uid), ("count", &count), ("cursor", cursor)],
            )
            .await?;
        let videos: Vec<Value> = data["videos"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|v| {
                let id = v["video_id"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v["video_id"].to_string());
                json!({
                    "video_id": id,
                    "url": format!("https://www.tiktok.com/@{uid}/video/{id}"),
                    "title": v["title"].as_str().unwrap_or(""),
                    "duration": v["duration"].as_i64().unwrap_or(0),
                    "size": v["size"].as_i64().unwrap_or(0),
                    "cover": v["cover"].as_str().unwrap_or(""),
                    "is_images": v["images"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
                    "play_count": v["play_count"].as_i64().unwrap_or(0),
                    "create_time": v["create_time"].as_i64().unwrap_or(0),
                })
            })
            .collect();
        Ok(json!({
            "unique_id": uid,
            "videos": videos,
            "cursor": data["cursor"].to_string(),
            "has_more": data["hasMore"].as_bool().unwrap_or(false),
        }))
    }
}

/// tikwm `data` → the one flat meta shape stored in the DB and shown in the UI.
/// `kind` is `images` for photo-mode posts (the `images` array is present and
/// non-empty), `video` otherwise.
fn normalize(d: &Value) -> Value {
    let images: Vec<String> = d["images"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let kind = if images.is_empty() { "video" } else { "images" };
    let s = |k: &str| d[k].as_str().unwrap_or("").to_string();
    let n = |k: &str| d[k].as_i64().unwrap_or(0);
    json!({
        "video_id": s("id"),
        "kind": kind,
        "title": s("title"),
        "region": s("region"),
        "duration": n("duration"),
        "cover_url": if s("origin_cover").is_empty() { s("cover") } else { s("origin_cover") },
        "author_id": d["author"]["unique_id"].as_str().unwrap_or(""),
        "author_name": d["author"]["nickname"].as_str().unwrap_or(""),
        "author_avatar": d["author"]["avatar"].as_str().unwrap_or(""),
        "play": s("play"),
        "wmplay": s("wmplay"),
        "hdplay": s("hdplay"),
        "size": n("size"),
        "wm_size": n("wm_size"),
        "hd_size": n("hd_size"),
        "music_url": s("music"),
        "music_title": d["music_info"]["title"].as_str().unwrap_or(""),
        "images": images,
        "stats": {
            "play_count": n("play_count"),
            "digg_count": n("digg_count"),
            "comment_count": n("comment_count"),
            "share_count": n("share_count"),
            "download_count": n("download_count"),
            "collect_count": n("collect_count"),
            "create_time": n("create_time"),
            "region": s("region"),
        },
    })
}

/// One file the worker has to fetch. `rel` is the path relative to the job's
/// target (either `<name>.<ext>` or `<name>/<part>` for multi-file posts).
#[derive(Debug, PartialEq)]
pub struct FilePlan {
    pub url: String,
    pub rel: String,
    /// Size hint from the resolver (0 = unknown) — progress bars only.
    pub size_hint: i64,
}

/// Decide what to download for `meta` at `quality`. Returns `(kind, plans)`;
/// `name` is the sanitized base filename (no extension).
pub fn plan_files(
    meta: &Value,
    quality: &str,
    name: &str,
    photo_audio: bool,
) -> Result<(String, Vec<FilePlan>)> {
    let s = |k: &str| meta[k].as_str().unwrap_or("").to_string();
    let images: Vec<String> = meta["images"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    if quality == "audio" {
        let url = s("music_url");
        if url.is_empty() {
            bail!("post này không tách được nhạc nền");
        }
        return Ok((
            "audio".into(),
            vec![FilePlan {
                url,
                rel: format!("{name}.mp3"),
                size_hint: 0,
            }],
        ));
    }
    if quality == "avatar" {
        let url = s("author_avatar");
        if url.is_empty() {
            bail!("không lấy được ảnh đại diện của tác giả");
        }
        return Ok((
            "avatar".into(),
            vec![FilePlan {
                url,
                rel: format!("{name}.jpg"),
                size_hint: 0,
            }],
        ));
    }

    // Photo-mode post: quality nowm/hd/wm all mean "the images themselves"
    // (TikTok photo posts have no watermarked/HD variants).
    if !images.is_empty() {
        let mut plans: Vec<FilePlan> = images
            .iter()
            .enumerate()
            .map(|(i, url)| FilePlan {
                url: url.clone(),
                rel: format!("{name}/{:02}.jpg", i + 1),
                size_hint: 0,
            })
            .collect();
        if photo_audio && !s("music_url").is_empty() {
            plans.push(FilePlan {
                url: s("music_url"),
                rel: format!("{name}/nhac-nen.mp3"),
                size_hint: 0,
            });
        }
        return Ok(("images".into(), plans));
    }

    // Plain video. Fall through the variants so a missing HD link degrades to
    // the ordinary no-watermark file instead of failing the job.
    let (url, size) = match quality {
        "hd" => [
            (s("hdplay"), meta["hd_size"].as_i64().unwrap_or(0)),
            (s("play"), meta["size"].as_i64().unwrap_or(0)),
            (s("wmplay"), meta["wm_size"].as_i64().unwrap_or(0)),
        ]
        .into_iter()
        .find(|(u, _)| !u.is_empty())
        .unwrap_or_default(),
        "wm" => [
            (s("wmplay"), meta["wm_size"].as_i64().unwrap_or(0)),
            (s("play"), meta["size"].as_i64().unwrap_or(0)),
        ]
        .into_iter()
        .find(|(u, _)| !u.is_empty())
        .unwrap_or_default(),
        _ => [
            (s("play"), meta["size"].as_i64().unwrap_or(0)),
            (s("hdplay"), meta["hd_size"].as_i64().unwrap_or(0)),
            (s("wmplay"), meta["wm_size"].as_i64().unwrap_or(0)),
        ]
        .into_iter()
        .find(|(u, _)| !u.is_empty())
        .unwrap_or_default(),
    };
    if url.is_empty() {
        bail!("post không có link video tải được");
    }
    Ok((
        "video".into(),
        vec![FilePlan {
            url,
            rel: format!("{name}.mp4"),
            size_hint: size,
        }],
    ))
}

/// Pull every TikTok/Douyin link out of a free-form text blob (one per line,
/// comma-separated, mixed with prose — anything). Scheme-less `vm.tiktok.com/…`
/// forms are accepted too. Order-preserving dedup.
pub fn extract_urls(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |u: String| {
        let u = u.trim_end_matches(['.', ',', ';', ')', ']', '}', '>', '"', '\'']).to_string();
        if is_supported_url(&u) && !out.contains(&u) {
            out.push(u);
        }
    };
    for raw in text.split(|c: char| c.is_whitespace() || c == ',' || c == ';') {
        let tok = raw.trim().trim_matches(['"', '\'', '(', ')', '<', '>', '[', ']']);
        if tok.is_empty() {
            continue;
        }
        if let Some(pos) = tok.find("http://").or_else(|| tok.find("https://")) {
            push(tok[pos..].to_string());
        } else if tok.starts_with("vm.tiktok.com/")
            || tok.starts_with("vt.tiktok.com/")
            || tok.starts_with("www.tiktok.com/")
            || tok.starts_with("m.tiktok.com/")
            || tok.starts_with("tiktok.com/")
        {
            push(format!("https://{tok}"));
        }
    }
    out
}

pub fn is_supported_url(u: &str) -> bool {
    let Some(host) = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
    else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == "tiktok.com"
        || host.ends_with(".tiktok.com")
        || host == "douyin.com"
        || host.ends_with(".douyin.com")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_video_meta() -> Value {
        normalize(&json!({
            "id": "7106594312292453675",
            "title": "how many frogs did you find?",
            "region": "US",
            "duration": 24,
            "cover": "https://cdn/cover.jpeg",
            "origin_cover": "https://cdn/origin.jpeg",
            "play": "https://cdn/play.mp4",
            "wmplay": "https://cdn/wm.mp4",
            "hdplay": "https://cdn/hd.mp4",
            "size": 100, "wm_size": 110, "hd_size": 200,
            "music": "https://cdn/music.mp3",
            "music_info": {"title": "original sound"},
            "play_count": 5,
            "author": {"unique_id": "tiktok", "nickname": "TikTok", "avatar": "https://cdn/ava.jpg"}
        }))
    }

    #[test]
    fn normalize_flattens_the_tikwm_shape() {
        let m = sample_video_meta();
        assert_eq!(m["kind"], "video");
        assert_eq!(m["video_id"], "7106594312292453675");
        assert_eq!(m["author_id"], "tiktok");
        assert_eq!(m["cover_url"], "https://cdn/origin.jpeg");
        assert_eq!(m["stats"]["play_count"], 5);
    }

    #[test]
    fn photo_posts_become_kind_images() {
        let m = normalize(&json!({
            "id": "1", "title": "album",
            "images": ["https://cdn/1.jpg", "https://cdn/2.jpg"],
            "music": "https://cdn/m.mp3",
            "author": {"unique_id": "a"}
        }));
        assert_eq!(m["kind"], "images");
        let (kind, plans) = plan_files(&m, "nowm", "album", true).unwrap();
        assert_eq!(kind, "images");
        assert_eq!(plans.len(), 3, "2 ảnh + nhạc nền");
        assert_eq!(plans[0].rel, "album/01.jpg");
        assert_eq!(plans[2].rel, "album/nhac-nen.mp3");
        let (_, no_audio) = plan_files(&m, "nowm", "album", false).unwrap();
        assert_eq!(no_audio.len(), 2);
    }

    #[test]
    fn quality_fallback_chain() {
        let m = sample_video_meta();
        let (_, p) = plan_files(&m, "hd", "v", true).unwrap();
        assert_eq!(p[0].url, "https://cdn/hd.mp4");
        assert_eq!(p[0].size_hint, 200);

        // Missing hdplay → hd falls back to the plain no-watermark file.
        let mut d = sample_video_meta();
        d["hdplay"] = json!("");
        let (_, p) = plan_files(&d, "hd", "v", true).unwrap();
        assert_eq!(p[0].url, "https://cdn/play.mp4");

        let (_, p) = plan_files(&m, "wm", "v", true).unwrap();
        assert_eq!(p[0].url, "https://cdn/wm.mp4");
        let (kind, p) = plan_files(&m, "audio", "v", true).unwrap();
        assert_eq!((kind.as_str(), p[0].rel.as_str()), ("audio", "v.mp3"));
        let (kind, p) = plan_files(&m, "avatar", "v", true).unwrap();
        assert_eq!((kind.as_str(), p[0].rel.as_str()), ("avatar", "v.jpg"));
    }

    #[test]
    fn extract_urls_from_messy_text() {
        let text = r#"
            xem cái này https://www.tiktok.com/@tiktok/video/7106594312292453675?is_from_webapp=1
            vm.tiktok.com/ZSAbCdEf/ , https://vt.tiktok.com/ZSxyz123/
            (https://www.tiktok.com/@user/photo/123456789)
            https://example.com/khong-phai-tiktok
            https://www.tiktok.com/@tiktok/video/7106594312292453675?is_from_webapp=1
        "#;
        let urls = extract_urls(text);
        assert_eq!(
            urls,
            vec![
                "https://www.tiktok.com/@tiktok/video/7106594312292453675?is_from_webapp=1",
                "https://vm.tiktok.com/ZSAbCdEf/",
                "https://vt.tiktok.com/ZSxyz123/",
                "https://www.tiktok.com/@user/photo/123456789",
            ],
            "lấy đúng link tiktok, bỏ link lạ, dedup"
        );
    }

    #[test]
    fn supported_url_rejects_lookalikes() {
        assert!(is_supported_url("https://vm.tiktok.com/x"));
        assert!(is_supported_url("https://v.douyin.com/abc"));
        assert!(!is_supported_url("https://tiktok.com.evil.vn/x"));
        assert!(!is_supported_url("ftp://tiktok.com/x"));
    }
}
