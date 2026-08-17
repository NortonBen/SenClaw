//! Where local model weights live, and what is already there.
//!
//! The root is **`~/.senclaw/local-models/`**, the same directory the daemon has
//! always used, handed to an app in `SENCLAW_LOCAL_MODELS_DIR`. It is not under
//! `space-app-data/`, and moving it there would be the single most expensive
//! mistake available here: the directory is measured in tens of gigabytes on a
//! machine that has been using local models, and relocating it means every user
//! re-downloads all of it.
//!
//! Two apps share this root — `mlx-lm` and `candle` — because a checkpoint is a
//! checkpoint. Which engine can *run* one is a question the app answers; where
//! it is stored is not.

use std::path::{Path, PathBuf};

/// Env var the daemon injects with the shared model root.
pub const ENV_MODELS_DIR: &str = "SENCLAW_LOCAL_MODELS_DIR";

/// The model root: `$SENCLAW_LOCAL_MODELS_DIR`, else `~/.senclaw/local-models`.
///
/// The fallback matters for `cargo run` outside SenClaw — and it must be the
/// *same* path the daemon uses, or a developer's app would quietly build a
/// second copy of the model library.
pub fn models_root() -> PathBuf {
    if let Ok(p) = std::env::var(ENV_MODELS_DIR) {
        let p = p.trim();
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("local-models")
}

/// `org/repo` → `org__repo`, the on-disk directory name.
///
/// `/` is the only character folded. A model id is validated by
/// [`normalize_hf_id`] before it reaches here, so this is a naming rule rather
/// than a sanitizer — but it is reversible, which [`dirname_to_id`] depends on.
pub fn safe_dirname(model_id: &str) -> String {
    model_id.replace('/', "__")
}

/// The inverse of [`safe_dirname`], for listing what is installed.
///
/// `None` for a directory name that was not produced by it: `hf-cache`, an
/// editor's leftovers, anything a user dropped in by hand. Guessing would put a
/// non-model in the picker.
pub fn dirname_to_id(name: &str) -> Option<String> {
    if !name.contains("__") {
        return None;
    }
    let id = name.replacen("__", "/", 1);
    if id.split('/').count() == 2 && !id.starts_with('/') && !id.ends_with('/') {
        Some(id)
    } else {
        None
    }
}

/// Where one model's files live.
pub fn model_dir(model_id: &str) -> PathBuf {
    models_root().join(safe_dirname(model_id))
}

/// Accept the shapes people actually paste, and reject the ones that would
/// escape the model root.
///
/// A HuggingFace id is pasted as often from the address bar as typed, so the URL
/// forms are normalised rather than refused. The `..` and `\` checks are the
/// point of the function: `safe_dirname` only folds `/`, so without them an id
/// of `../../etc/x` would write outside the root.
pub fn normalize_hf_id(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty model id".into());
    }
    let stripped = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("huggingface.co/")
        .trim_start_matches("hf.co/")
        .trim_end_matches('/');
    let parts: Vec<&str> = stripped.split('/').collect();
    if parts.len() < 2 {
        return Err(format!("expected `org/repo` form, got `{s}`"));
    }
    let (org, repo) = (parts[0], parts[1]);
    if org.is_empty() || repo.is_empty() {
        return Err(format!("invalid `org/repo` in `{s}`"));
    }
    for seg in [org, repo] {
        if seg.contains("..") || seg.contains('\\') {
            return Err(format!("unsafe path segment in `{s}`"));
        }
    }
    Ok(format!("{org}/{repo}"))
}

/// Does this directory hold a usable checkpoint?
///
/// A config plus at least one weight shard. Both halves are needed: a directory
/// with `config.json` and no weights is an interrupted download, and reporting
/// it as installed sends the user to a model that fails to load.
pub fn is_installed(dir: &Path) -> bool {
    if !dir.join("config.json").exists() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        name.ends_with(".safetensors") || name.ends_with(".gguf") || name.ends_with(".bin")
    })
}

/// Does this checkpoint ship weights either engine can actually read?
///
/// Both read **safetensors only**. A repo whose weights are `pytorch_model.bin`
/// passes [`is_installed`] — it is a complete download of a real model — but
/// neither engine can load it, and listing it produces a model that appears in
/// the picker and fails when it is selected. `state-spaces/mamba2-370m` is the
/// case this was written for: a supported architecture, shipped only as a
/// PyTorch pickle.
///
/// Separate from `is_installed` on purpose. "Is the download complete" and "can
/// this engine run it" are different questions, and the management screen shows
/// the first while the model picker filters on the second.
pub fn has_safetensors(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().ends_with(".safetensors"))
}

/// One installed model, as the management UI sees it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledModel {
    pub id: String,
    pub dir: PathBuf,
    /// Total bytes on disk. Shown so a user clearing space can see which of
    /// these is the 14 GB one.
    pub size_bytes: u64,
    /// From the checkpoint's own `config.json`, when it says.
    pub architecture: Option<String>,
    pub context_length: Option<u32>,
}

/// Everything installed under the model root, sorted by id.
pub fn list_installed() -> Vec<InstalledModel> {
    let root = models_root();
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<InstalledModel> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let dir = e.path();
            if !dir.is_dir() {
                return None;
            }
            let id = dirname_to_id(&e.file_name().to_string_lossy())?;
            if !is_installed(&dir) {
                return None;
            }
            let (architecture, context_length) = read_config_summary(&dir);
            Some(InstalledModel {
                id,
                size_bytes: dir_size(&dir),
                architecture,
                context_length,
                dir,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// `(architecture, context_length)` out of a checkpoint's `config.json`.
pub fn read_config_summary(dir: &Path) -> (Option<String>, Option<u32>) {
    let Ok(raw) = std::fs::read_to_string(dir.join("config.json")) else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return (None, None);
    };
    let arch = v["architectures"][0]
        .as_str()
        .or_else(|| v["model_type"].as_str())
        .map(str::to_string);
    // `text_config` is where multimodal checkpoints (Gemma 4) keep the language
    // model's own window; the top-level one describes the wrapper.
    let ctx = v["text_config"]["max_position_embeddings"]
        .as_u64()
        .or_else(|| v["max_position_embeddings"].as_u64())
        .map(|n| n as u32);
    (arch, ctx)
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => 0,
        })
        .sum()
}

/// Delete a model's directory.
pub fn remove(model_id: &str) -> std::io::Result<()> {
    let dir = model_dir(model_id);
    // Never let a bad id turn this into an unbounded delete. `model_dir` is
    // built from `safe_dirname`, so the result is always exactly one level under
    // the root — anything else means the id skipped normalization.
    if dir.parent() != Some(models_root().as_path()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "model path escapes the model root",
        ));
    }
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shapes_people_actually_paste_are_accepted() {
        for raw in [
            "mlx-community/gemma-4-e2b-it-4bit",
            "  mlx-community/gemma-4-e2b-it-4bit  ",
            "https://huggingface.co/mlx-community/gemma-4-e2b-it-4bit",
            "hf.co/mlx-community/gemma-4-e2b-it-4bit/",
            // Extra path segments (a /tree/main URL) are dropped.
            "https://huggingface.co/mlx-community/gemma-4-e2b-it-4bit/tree/main",
        ] {
            assert_eq!(
                normalize_hf_id(raw).unwrap(),
                "mlx-community/gemma-4-e2b-it-4bit",
                "failed on {raw}"
            );
        }
    }

    /// `safe_dirname` folds only `/`, so these are what stands between a model
    /// id and a write outside the model root.
    #[test]
    fn a_traversal_id_is_refused() {
        for raw in ["../../etc/passwd", "org/../..", "..\\x/y", "org/re\\po", "org", "", "/", "a/"]
        {
            assert!(normalize_hf_id(raw).is_err(), "`{raw}` must be refused");
        }
    }

    #[test]
    fn the_directory_name_round_trips() {
        let id = "mlx-community/gemma-4-e2b-it-4bit";
        assert_eq!(dirname_to_id(&safe_dirname(id)).as_deref(), Some(id));
    }

    /// Only names this module produced are read back as models — the model root
    /// also holds `hf-cache`, and a stray directory must not become a picker
    /// entry.
    #[test]
    fn a_foreign_directory_name_is_not_a_model() {
        for name in ["hf-cache", ".DS_Store", "notes", "a__b__c__d"] {
            let back = dirname_to_id(name);
            assert!(
                back.is_none() || back.as_deref() == Some("a/b__c__d"),
                "`{name}` produced {back:?}"
            );
        }
        assert_eq!(dirname_to_id("hf-cache"), None);
    }

    #[test]
    fn a_config_without_weights_is_an_interrupted_download_not_a_model() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        assert!(!is_installed(dir.path()), "config alone is not a checkpoint");

        std::fs::write(dir.path().join("model.safetensors"), b"x").unwrap();
        assert!(is_installed(dir.path()));
    }

    /// A real case: `state-spaces/mamba2-370m` is a supported architecture
    /// shipped only as a PyTorch pickle. Complete download, unloadable model.
    #[test]
    fn a_pytorch_only_checkpoint_is_installed_but_not_runnable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        std::fs::write(dir.path().join("pytorch_model.bin"), b"x").unwrap();

        assert!(is_installed(dir.path()), "the download is complete");
        assert!(!has_safetensors(dir.path()), "but no engine here can read it");

        std::fs::write(dir.path().join("model.safetensors"), b"x").unwrap();
        assert!(has_safetensors(dir.path()));
    }

    #[test]
    fn sharded_safetensors_count() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("model-00001-of-00004.safetensors"), b"x").unwrap();
        assert!(has_safetensors(dir.path()));
    }

    #[test]
    fn a_multimodal_checkpoint_reports_the_text_models_window() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.json"),
            r#"{"model_type":"gemma4","max_position_embeddings":4096,
                "text_config":{"max_position_embeddings":131072}}"#,
        )
        .unwrap();
        let (arch, ctx) = read_config_summary(dir.path());
        assert_eq!(arch.as_deref(), Some("gemma4"));
        assert_eq!(ctx, Some(131_072), "the wrapper's window is not the model's");
    }
}
