//! Download worker pool. Jobs live in SQLite (`status='queued'`); a small
//! supervisor task claims them FIFO and runs up to `max_concurrent` at once.
//! Each job: re-resolve the link (CDN URLs expire within minutes, so stored
//! ones are never trusted), plan the files, stream them to `<file>.part`,
//! rename on success. Cancellation is a per-job [`AtomicBool`] checked between
//! chunks — a canceled job deletes its partial file.

use crate::db::{now_ts, Db};
use crate::tiktok::{self, Resolver};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;

/// Shared queue handle: wake signal + cancel flags of running jobs.
pub struct Queue {
    pub notify: tokio::sync::Notify,
    cancels: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    active: AtomicUsize,
}

impl Queue {
    pub fn new() -> Self {
        Self {
            notify: tokio::sync::Notify::new(),
            cancels: Mutex::new(HashMap::new()),
            active: AtomicUsize::new(0),
        }
    }

    pub fn wake(&self) {
        self.notify.notify_one();
    }

    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::SeqCst)
    }

    /// Flag a running job for cancellation. Returns false when no worker owns
    /// the id (then the caller cancels the queued row directly in the DB).
    pub fn request_cancel(&self, id: i64) -> bool {
        let map = self.cancels.lock().unwrap();
        match map.get(&id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    fn register(&self, id: i64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancels.lock().unwrap().insert(id, flag.clone());
        flag
    }

    fn unregister(&self, id: i64) {
        self.cancels.lock().unwrap().remove(&id);
    }
}

pub struct Ctx {
    pub db: Arc<Db>,
    pub http: reqwest::Client,
    pub resolver: Arc<Resolver>,
    pub queue: Arc<Queue>,
}

/// Supervisor loop — claims queued jobs while free slots exist. Runs forever;
/// spawn once from main. The periodic tick (not just `notify`) also rescues
/// jobs enqueued while every slot was busy.
pub async fn run_supervisor(ctx: Arc<Ctx>) {
    loop {
        let max: usize = ctx
            .db
            .setting("max_concurrent", "2")
            .parse()
            .map(|n: usize| n.clamp(1, 4))
            .unwrap_or(2);
        while ctx.queue.active_count() < max {
            let Some(id) = ctx.db.claim_next_queued() else {
                break;
            };
            ctx.queue.active.fetch_add(1, Ordering::SeqCst);
            let ctx2 = ctx.clone();
            tokio::spawn(async move {
                run_job(&ctx2, id).await;
                ctx2.queue.active.fetch_sub(1, Ordering::SeqCst);
                ctx2.queue.wake();
            });
        }
        tokio::select! {
            _ = ctx.queue.notify.notified() => {}
            _ = tokio::time::sleep(std::time::Duration::from_millis(1500)) => {}
        }
    }
}

async fn run_job(ctx: &Ctx, id: i64) {
    let flag = ctx.queue.register(id);
    let result = job_inner(ctx, id, &flag).await;
    ctx.queue.unregister(id);
    match result {
        Ok(files) => {
            ctx.db.log(
                "done",
                &format!("Tải xong {} file", files.len()),
                &id.to_string(),
            );
        }
        Err(e) if e.to_string() == CANCELED => {
            ctx.db.set_status(id, "canceled", "");
            ctx.db.log("canceled", "Đã hủy tải", &id.to_string());
        }
        Err(e) => {
            ctx.db.set_status(id, "error", &e.to_string());
            ctx.db.log("error", &e.to_string(), &id.to_string());
        }
    }
}

const CANCELED: &str = "__canceled__";

fn check(flag: &AtomicBool) -> anyhow::Result<()> {
    if flag.load(Ordering::SeqCst) {
        anyhow::bail!(CANCELED);
    }
    Ok(())
}

async fn job_inner(ctx: &Ctx, id: i64, flag: &AtomicBool) -> anyhow::Result<Vec<String>> {
    let row = ctx
        .db
        .get_download(id)
        .ok_or_else(|| anyhow::anyhow!("job #{id} biến mất khỏi DB"))?;
    let input_url = row["input_url"].as_str().unwrap_or("").to_string();
    let quality = row["quality"].as_str().unwrap_or("nowm").to_string();

    check(flag)?;
    let meta = ctx.resolver.resolve(&input_url).await?;
    ctx.db.apply_resolved(id, &meta);
    save_thumb(ctx, id, meta["cover_url"].as_str().unwrap_or("")).await;
    check(flag)?;

    let tpl = ctx.db.setting("filename_template", "{author}_{id}");
    let name = render_template(&tpl, &meta, &quality);
    let photo_audio = ctx.db.setting("photo_audio", "1") == "1";
    let (kind, plans) = tiktok::plan_files(&meta, &quality, &name, photo_audio)?;
    ctx.db.set_kind(id, &kind);

    let dir = PathBuf::from(ctx.db.setting("download_dir", ""));
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("không tạo được thư mục lưu {}: {e}", dir.display()))?;

    // Multi-file jobs land in one sub-folder; uniquify it (or the single file)
    // once so a re-download never overwrites an earlier copy.
    let multi = plans.len() > 1 || plans.iter().any(|p| p.rel.contains('/'));
    let unique_base = if multi {
        let d = unique_path(&dir.join(&name));
        std::fs::create_dir_all(&d)?;
        d
    } else {
        dir.clone()
    };

    ctx.db.set_status(id, "downloading", "");
    let mut total: i64 = plans.iter().map(|p| p.size_hint).sum();
    let mut downloaded: i64 = 0;
    let mut files: Vec<String> = Vec::new();

    for plan in &plans {
        check(flag)?;
        let target = if multi {
            // rel is "<name>/<part>" — inside the (already unique) folder only
            // the part matters.
            let part = plan.rel.split('/').next_back().unwrap_or(&plan.rel);
            unique_base.join(part)
        } else {
            unique_path(&dir.join(&plan.rel))
        };
        let n = fetch_one(
            ctx,
            id,
            &plan.url,
            &target,
            flag,
            &mut total,
            plan.size_hint,
            &mut downloaded,
        )
        .await;
        match n {
            Ok(()) => files.push(target.to_string_lossy().to_string()),
            Err(e) => {
                // Half-finished multi-file jobs keep what already arrived; the
                // partial current file is removed either way.
                let _ = tokio::fs::remove_file(path_part(&target)).await;
                if multi {
                    return Err(anyhow::anyhow!(
                        "tải được {}/{} file rồi lỗi: {e}",
                        files.len(),
                        plans.len()
                    ));
                }
                return Err(e);
            }
        }
    }

    if ctx.db.setting("save_meta_json", "0") == "1" {
        let meta_path = if multi {
            unique_base.join("metadata.json")
        } else {
            files
                .first()
                .map(|f| PathBuf::from(f).with_extension("json"))
                .unwrap_or_else(|| dir.join(format!("{name}.json")))
        };
        let doc = json!({
            "input_url": input_url,
            "quality": quality,
            "resolved_at": now_ts(),
            "meta": {
                "video_id": meta["video_id"], "kind": meta["kind"], "title": meta["title"],
                "author_id": meta["author_id"], "author_name": meta["author_name"],
                "duration": meta["duration"], "music_title": meta["music_title"],
                "stats": meta["stats"],
            },
        });
        let _ = std::fs::write(&meta_path, serde_json::to_vec_pretty(&doc).unwrap_or_default());
    }

    let shown_dir = if multi { &unique_base } else { &dir };
    ctx.db
        .finish_files(id, &shown_dir.to_string_lossy(), &files, downloaded.max(total));
    Ok(files)
}

/// Stream one URL to `target` (via `.part`), updating DB progress ~2×/second.
#[allow(clippy::too_many_arguments)]
async fn fetch_one(
    ctx: &Ctx,
    id: i64,
    url: &str,
    target: &Path,
    flag: &AtomicBool,
    total: &mut i64,
    size_hint: i64,
    downloaded: &mut i64,
) -> anyhow::Result<()> {
    let resp = ctx
        .http
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("không kết nối được CDN: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("CDN trả HTTP {} — link có thể đã hết hạn", resp.status());
    }
    if let Some(len) = resp.content_length() {
        // Replace the hint with the true length so the percent bar is honest.
        *total += len as i64 - size_hint;
    }
    let part = path_part(target);
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| anyhow::anyhow!("không ghi được file {}: {e}", part.display()))?;
    let mut stream = resp.bytes_stream();
    let mut last_write = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        // On cancel the caller removes the .part file.
        check(flag)?;
        let chunk = chunk.map_err(|e| anyhow::anyhow!("mạng rớt giữa chừng: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| anyhow::anyhow!("lỗi ghi đĩa: {e}"))?;
        *downloaded += chunk.len() as i64;
        if last_write.elapsed().as_millis() >= 400 {
            ctx.db.set_progress(id, *downloaded, *total);
            last_write = std::time::Instant::now();
        }
    }
    file.flush().await.ok();
    drop(file);
    tokio::fs::rename(&part, target)
        .await
        .map_err(|e| anyhow::anyhow!("không đổi tên file tải xong: {e}"))?;
    ctx.db.set_progress(id, *downloaded, *total);
    Ok(())
}

fn path_part(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Small cover JPEG in the app data dir → the history UI thumbnail. Purely
/// cosmetic, so every failure is swallowed.
async fn save_thumb(ctx: &Ctx, id: i64, cover_url: &str) {
    if cover_url.is_empty() {
        return;
    }
    let dir = crate::db::data_dir().join("thumbs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Ok(resp) = ctx.http.get(cover_url).send().await else {
        return;
    };
    if !resp.status().is_success() {
        return;
    }
    let Ok(bytes) = resp.bytes().await else { return };
    // Covers are ~30–100 KB; anything huge is not a thumbnail.
    if bytes.len() > 2_000_000 {
        return;
    }
    let _ = std::fs::write(dir.join(format!("{id}.jpg")), &bytes);
}

// ---- filename helpers ----

/// `{author} {id} {title} {date} {quality}` placeholders → sanitized filename
/// base (no extension). Falls back to the video id, then a timestamp, so the
/// result is never empty.
pub fn render_template(tpl: &str, meta: &Value, quality: &str) -> String {
    let s = |k: &str| meta[k].as_str().unwrap_or("").to_string();
    let date_ts = meta["stats"]["create_time"].as_i64().unwrap_or(0);
    let date = chrono::DateTime::from_timestamp(if date_ts > 0 { date_ts } else { now_ts() }, 0)
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_default();
    let raw = tpl
        .replace("{id}", &s("video_id"))
        .replace("{author}", &s("author_id"))
        .replace("{title}", &s("title"))
        .replace("{date}", &date)
        .replace("{quality}", quality);
    let mut name = sanitize_component(&raw);
    if name.is_empty() {
        name = sanitize_component(&s("video_id"));
    }
    if name.is_empty() {
        name = format!("tiktok_{}", now_ts());
    }
    name
}

/// Strip characters that break filenames (or shells), collapse runs of
/// whitespace to `_`, and cap length at a **char** boundary — titles are
/// Vietnamese/emoji-heavy, byte slicing would panic mid-codepoint.
pub fn sanitize_component(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '%' | '&' | '{' | '}'
            | '$' | '!' | '@' | '+' | '`' | '=' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let mut out = String::new();
    for part in cleaned.split_whitespace() {
        if !out.is_empty() {
            out.push('_');
        }
        out.push_str(part);
    }
    let out = out.trim_matches(['.', '_', ' ']).to_string();
    if out.chars().count() <= 80 {
        return out;
    }
    let cut: String = out.chars().take(80).collect();
    cut.trim_end_matches(['.', '_', ' ']).to_string()
}

/// `foo.mp4` exists → `foo_2.mp4`, `foo_3.mp4`… (same for extension-less
/// directories). Gives up after 999 and lets the OS error out.
pub fn unique_path(want: &Path) -> PathBuf {
    if !want.exists() {
        return want.to_path_buf();
    }
    let stem = want
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = want
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = want.parent().unwrap_or(Path::new("."));
    for i in 2..1000 {
        let cand = parent.join(format!("{stem}_{i}{ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    want.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_vietnamese_strips_dangerous() {
        assert_eq!(
            sanitize_component("Món ăn: đường phố / Sài Gòn?!"),
            "Món_ăn_đường_phố_Sài_Gòn"
        );
        assert_eq!(sanitize_component("...///:::"), "");
        // 200 ký tự đa byte — không được panic, cắt ở ranh giới ký tự.
        let long = "ăn".repeat(100);
        let cut = sanitize_component(&long);
        assert_eq!(cut.chars().count(), 80);
    }

    #[test]
    fn template_renders_and_falls_back() {
        let meta = serde_json::json!({
            "video_id": "123", "author_id": "user.x", "title": "Chào / mọi người",
            "stats": {"create_time": 1654632929}
        });
        assert_eq!(
            render_template("{author}_{id}", &meta, "nowm"),
            "user.x_123"
        );
        assert_eq!(
            render_template("{date}_{title}", &meta, "hd"),
            "20220607_Chào_mọi_người"
        );
        // Template rác → rơi về video id.
        assert_eq!(render_template("///", &meta, "nowm"), "123");
    }

    #[test]
    fn unique_path_appends_counter() {
        let dir = tempfile::tempdir().unwrap();
        let want = dir.path().join("clip.mp4");
        assert_eq!(unique_path(&want), want);
        std::fs::write(&want, b"x").unwrap();
        assert_eq!(unique_path(&want), dir.path().join("clip_2.mp4"));
        std::fs::write(dir.path().join("clip_2.mp4"), b"x").unwrap();
        assert_eq!(unique_path(&want), dir.path().join("clip_3.mp4"));
    }
}
