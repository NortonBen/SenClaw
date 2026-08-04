//! Background analysis worker.
//!
//! Analysing a segment takes minutes, far longer than an HTTP request should
//! hold open, so the REST and MCP layers create a job row, spawn this, and
//! return the job id immediately. Progress is polled or pushed over the
//! dashboard socket.

use crate::db::{CloneConfig, Project};
use crate::gemini::{self, GenerateRequest, VideoPart};
use crate::prompts;
use crate::scenes;
use crate::state::Core;
use anyhow::{bail, Context, Result};
use base64::Engine;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Analyse from the beginning, discarding any existing scenes.
    Start,
    /// Analyse the segment after the last one held.
    Continue,
    /// Drop the last scene and regenerate it from the segment before.
    Regenerate,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_lowercase().as_str() {
            "start" | "" => Some(Mode::Start),
            "continue" => Some(Mode::Continue),
            "regenerate" | "redo" => Some(Mode::Regenerate),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Start => "start",
            Mode::Continue => "continue",
            Mode::Regenerate => "regenerate",
        }
    }
}

/// Which scene number the model should resume after, for a given mode.
///
/// `Regenerate` resumes from the scene *before* the last one so the model
/// produces a fresh take on the segment being replaced — resuming from the last
/// one would generate the segment after it instead.
pub fn resume_point(mode: Mode, scene_numbers: &[i64]) -> i64 {
    match mode {
        Mode::Start => 0,
        Mode::Continue => scene_numbers.last().copied().unwrap_or(0),
        Mode::Regenerate => {
            if scene_numbers.len() >= 2 {
                scene_numbers[scene_numbers.len() - 2]
            } else {
                0
            }
        }
    }
}

/// Create the job row and spawn the worker. Returns the job id.
pub fn start(core: &Arc<Core>, project: &Project, mode: Mode, cfg: CloneConfig) -> Result<i64> {
    let Some(guard) = core.try_claim(project.id) else {
        bail!("dự án này đang chạy một lượt phân tích khác — chờ nó xong đã");
    };

    let existing = core.db.scenes(project.id)?;
    let numbers: Vec<i64> = existing
        .iter()
        .filter_map(|s| scenes::scene_number(&s.json))
        .collect();
    let from = resume_point(mode, &numbers);

    let temperature = prompts::temperature_for(&cfg);
    let job_id = core
        .db
        .create_job(project.id, mode.as_str(), from, &cfg.model, temperature)?;

    core.dash.emit(
        "job:started",
        json!({ "job_id": job_id, "project_id": project.id, "kind": mode.as_str() }),
    );

    let core2 = core.clone();
    let project2 = project.clone();
    tokio::spawn(async move {
        // The guard is moved in so the slot is released however this ends,
        // including a panic inside the worker.
        let _guard = guard;
        if let Err(e) = run(&core2, &project2, mode, cfg, job_id, from).await {
            let msg = scenes::truncate_chars(&format!("{e:#}"), 600);
            let _ = core2.db.fail_job(job_id, &msg);
            core2.dash.emit(
                "job:failed",
                json!({ "job_id": job_id, "project_id": project2.id, "error": msg }),
            );
        }
    });

    Ok(job_id)
}

async fn run(
    core: &Arc<Core>,
    project: &Project,
    mode: Mode,
    cfg: CloneConfig,
    job_id: i64,
    from: i64,
) -> Result<()> {
    core.db.set_job_status(job_id, "processing")?;

    let api_key = core.db.gemini_api_key();
    if api_key.trim().is_empty() {
        bail!("chưa có Gemini API key — mở Cài đặt của Video Cloner để nhập key");
    }

    let video = attach_video(core, project, &api_key).await?;

    let char_image = if project.char_image_path.is_empty() {
        None
    } else {
        match tokio::fs::read(&project.char_image_path).await {
            Ok(bytes) => Some((
                project.char_image_mime.clone(),
                base64::engine::general_purpose::STANDARD.encode(&bytes),
            )),
            // A missing reference image should not sink the whole run; the
            // prompt simply falls back to the text description.
            Err(_) => None,
        }
    };

    let system = prompts::system_instruction(&cfg);
    let prompt = prompts::user_prompt(&cfg, from, char_image.is_some());

    let raw = gemini::generate(GenerateRequest {
        api_key: &api_key,
        model: &cfg.model,
        system: &system,
        prompt: &prompt,
        temperature: prompts::temperature_for(&cfg),
        video: &video,
        char_image,
    })
    .await?;

    let parsed = scenes::parse_scenes(&raw);
    if parsed.is_empty() {
        bail!(
            "Gemini trả lời nhưng không có scene JSON nào đọc được. Trích đoạn: {}",
            scenes::truncate_chars(raw.trim(), 300)
        );
    }

    // Only mutate the stored scenes once the model has actually produced
    // something usable — a failed run must never destroy earlier work.
    //
    // Both destructive modes take a restore point first: the scenes about to be
    // dropped cost minutes of model time and cannot be reproduced exactly, since
    // every run samples at a non-zero temperature.
    match mode {
        Mode::Start => {
            core.db.snapshot(
                project.id,
                "analyze_start",
                "trước khi phân tích lại từ đầu",
            )?;
            core.db.clear_scenes(project.id)?;
        }
        Mode::Regenerate => {
            core.db.snapshot(
                project.id,
                "analyze_regenerate",
                "trước khi làm lại đoạn cuối",
            )?;
            core.db.delete_last_scene(project.id)?;
        }
        Mode::Continue => {}
    }

    let added = core.db.append_scenes(project.id, &parsed, job_id)?;
    // Stored in full: a run that parses badly can only be diagnosed from the
    // exact text the model returned.
    core.db.finish_job(job_id, added, &raw)?;

    core.dash.emit(
        "job:completed",
        json!({
            "job_id": job_id,
            "project_id": project.id,
            "scenes_added": added,
            "total_scenes": core.db.scene_count(project.id).unwrap_or(0),
        }),
    );
    Ok(())
}

/// Get the video into a form Gemini can read, reusing a previous upload when
/// one is still valid.
async fn attach_video(core: &Arc<Core>, project: &Project, api_key: &str) -> Result<VideoPart> {
    let path = Path::new(&project.video_path);
    if !path.exists() {
        bail!(
            "không tìm thấy file video của dự án ({}) — hãy tải lại video",
            project.video_path
        );
    }

    if !gemini::needs_files_api(project.video_size as u64) {
        return gemini::read_inline(path, &project.video_mime).await;
    }

    if !project.file_uri.is_empty() && gemini::is_file_uri_fresh(&project.file_uri_at) {
        return Ok(VideoPart::Remote {
            mime: project.video_mime.clone(),
            uri: project.file_uri.clone(),
        });
    }

    core.dash.emit(
        "video:uploading",
        json!({ "project_id": project.id, "size": project.video_size }),
    );

    let uri = gemini::upload_file(api_key, path, &project.video_mime, &project.video_filename)
        .await
        .context("tải video lên Gemini Files API")?;

    core.db.set_file_uri(project.id, &uri)?;
    Ok(VideoPart::Remote {
        mime: project.video_mime.clone(),
        uri,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_always_resumes_from_the_beginning() {
        assert_eq!(resume_point(Mode::Start, &[1, 2, 3]), 0);
    }

    #[test]
    fn continue_resumes_after_the_last_scene() {
        assert_eq!(resume_point(Mode::Continue, &[1, 2, 3]), 3);
        assert_eq!(resume_point(Mode::Continue, &[]), 0);
    }

    #[test]
    fn regenerate_resumes_before_the_last_scene_so_it_is_replaced() {
        assert_eq!(resume_point(Mode::Regenerate, &[1, 2, 3]), 2);
    }

    #[test]
    fn regenerating_the_only_scene_starts_over() {
        assert_eq!(resume_point(Mode::Regenerate, &[1]), 0);
        assert_eq!(resume_point(Mode::Regenerate, &[]), 0);
    }

    #[test]
    fn mode_parses_the_names_the_api_accepts() {
        assert_eq!(Mode::parse("start"), Some(Mode::Start));
        assert_eq!(Mode::parse(""), Some(Mode::Start));
        assert_eq!(Mode::parse("Continue"), Some(Mode::Continue));
        assert_eq!(Mode::parse("redo"), Some(Mode::Regenerate));
        assert_eq!(Mode::parse("nonsense"), None);
    }
}
