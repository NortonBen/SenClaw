//! Virtual agent persona registry. Mirrors `src-old/agent/PersonaRegistry.ts`.
//!
//! Scans markdown files with YAML-like frontmatter from the virtual agents directory.
//! Includes a polling-based background watcher for hot-reload (1.5s interval, matching
//! the TS `fs.watch` behavior).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tokio::task::JoinHandle;

/// Parsed persona configuration from a `.md` file.
#[derive(Debug, Clone)]
pub struct PersonaConfig {
    pub name: String,
    pub description: String,
    /// `None` = use default toolset.
    pub tools: Option<Vec<String>>,
    /// Reserved, not currently active.
    pub model: Option<String>,
    pub max_concurrent: u32,
    /// Body text after the frontmatter block.
    pub system_prompt: String,
    pub file_path: PathBuf,
}

const MAX_SYSTEM_PROMPT_LENGTH: usize = 8000;

pub struct PersonaRegistry {
    dir: PathBuf,
    personas: HashMap<String, PersonaConfig>,
}

impl PersonaRegistry {
    pub fn new(dir: PathBuf) -> Self {
        let mut reg = Self {
            dir,
            personas: HashMap::new(),
        };
        reg.load_all();
        reg
    }

    pub fn get(&self, name: &str) -> Option<&PersonaConfig> {
        self.personas.get(name)
    }

    pub fn list(&self) -> Vec<&PersonaConfig> {
        self.personas.values().collect()
    }

    /// Rescan the directory and refresh in-memory cache.
    pub fn reload(&mut self) {
        self.load_all();
    }

    /// Spawn a background task that watches the virtual agents directory for
    /// `.md` file changes and auto-reloads. Uses polling at the same 1.5 s
    /// interval as [`crate::memory::manager::MemoryManager`].
    pub fn spawn_watcher(this: Arc<Mutex<Self>>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let dir = {
                let guard = this.lock().unwrap();
                guard.dir.clone()
            };
            if !dir.is_dir() {
                tracing::warn!(
                    "[PersonaRegistry] Watcher: directory not found, skipping: {}",
                    dir.display()
                );
                return;
            }

            // Seed initial mtimes
            let mut last_mtimes: HashMap<PathBuf, SystemTime> = HashMap::new();
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(true, |e| e != "md") {
                        continue;
                    }
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Ok(mtime) = meta.modified() {
                            last_mtimes.insert(path, mtime);
                        }
                    }
                }
            }

            let mut interval = tokio::time::interval(std::time::Duration::from_millis(1500));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                interval.tick().await;

                let mut changed = false;
                // Check for new/modified files
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.extension().map_or(true, |e| e != "md") {
                            continue;
                        }
                        if let Ok(meta) = fs::metadata(&path) {
                            if let Ok(mtime) = meta.modified() {
                                let prev = last_mtimes.get(&path).copied();
                                if prev != Some(mtime) {
                                    changed = true;
                                    last_mtimes.insert(path, mtime);
                                }
                            }
                        }
                    }
                }
                // Check for deleted files
                let mut deleted = Vec::new();
                for path in last_mtimes.keys() {
                    if !path.exists() {
                        deleted.push(path.clone());
                        changed = true;
                    }
                }
                for path in &deleted {
                    last_mtimes.remove(path);
                }

                if changed {
                    tracing::info!("[PersonaRegistry] File change detected, reloading...");
                    if let Ok(mut guard) = this.lock() {
                        guard.load_all();
                    }
                }
            }
        })
    }

    // ===== Internal =====

    fn load_all(&mut self) {
        if !self.dir.is_dir() {
            tracing::warn!(
                "[PersonaRegistry] Directory not found, skipping: {}",
                self.dir.display()
            );
            return;
        }

        let entries = match fs::read_dir(&self.dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("[PersonaRegistry] Failed to read dir: {e}");
                return;
            }
        };

        let mut new_map: HashMap<String, PersonaConfig> = HashMap::new();

        for entry in entries.flatten() {
            let file_path = entry.path();
            if file_path.extension().map_or(true, |e| e != "md") {
                continue;
            }
            match parse_file(&file_path) {
                Ok(Some(mut persona)) => {
                    let key = persona.name.clone();
                    if new_map.contains_key(&key) {
                        let file_base = file_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        if new_map.contains_key(file_base) {
                            tracing::warn!(
                                "[PersonaRegistry] Duplicate name \"{}\" and filename \"{}\" both taken, skipping {}",
                                persona.name,
                                file_base,
                                file_path.display()
                            );
                            continue;
                        }
                        tracing::warn!(
                            "[PersonaRegistry] Duplicate name \"{}\", falling back to filename \"{}\"",
                            persona.name,
                            file_base
                        );
                        persona.name = file_base.to_string();
                    }
                    new_map.insert(persona.name.clone(), persona);
                }
                Ok(None) => { /* parse failure already logged */ }
                Err(e) => {
                    tracing::warn!(
                        "[PersonaRegistry] Failed to parse {}: {e}",
                        file_path.display()
                    );
                }
            }
        }

        let keys: Vec<&str> = new_map.keys().map(|s| s.as_str()).collect();
        tracing::info!(
            "[PersonaRegistry] Loaded {} persona(s): {}",
            new_map.len(),
            keys.join(", ")
        );
        self.personas = new_map;
    }
}

// ===== File parsing =====

fn parse_file(file_path: &Path) -> Result<Option<PersonaConfig>, anyhow::Error> {
    let raw = fs::read_to_string(file_path)?;
    let lines: Vec<&str> = raw.lines().collect();

    // Find frontmatter boundaries
    let mut fm_start: Option<usize> = None;
    let mut fm_end: Option<usize> = None;

    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "---" {
            if fm_start.is_none() {
                fm_start = Some(i);
            } else {
                fm_end = Some(i);
                break;
            }
        }
    }

    let (fm_start, fm_end) = match (fm_start, fm_end) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            tracing::warn!(
                "[PersonaRegistry] No frontmatter found in {}",
                file_path.file_name().unwrap_or_default().to_string_lossy()
            );
            return Ok(None);
        }
    };

    // Parse frontmatter key: value pairs
    let mut fm: HashMap<String, String> = HashMap::new();
    for i in (fm_start + 1)..fm_end {
        let line = lines[i].trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(colon_idx) = line.find(':') {
            let key = line[..colon_idx].trim().to_string();
            let value = line[colon_idx + 1..].trim().to_string();
            fm.insert(key, value);
        }
    }

    let file_name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let name = fm.remove("name").unwrap_or_else(|| file_name.to_string());

    let description = match fm.remove("description") {
        Some(d) if !d.is_empty() => d,
        _ => {
            tracing::warn!(
                "[PersonaRegistry] Missing required field \"description\" in {}",
                file_path.file_name().unwrap_or_default().to_string_lossy()
            );
            return Ok(None);
        }
    };

    let tools = fm.remove("tools").map(|t| {
        t.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    });

    let model = fm.remove("model").filter(|m| !m.is_empty());

    let max_concurrent = fm
        .remove("max_concurrent")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);

    // Everything after second --- is the system prompt
    let mut system_prompt = lines[fm_end + 1..].join("\n").trim().to_string();
    if system_prompt.len() > MAX_SYSTEM_PROMPT_LENGTH {
        tracing::warn!(
            "[PersonaRegistry] \"{name}\" systemPrompt too long ({} chars), truncating to {MAX_SYSTEM_PROMPT_LENGTH}",
            system_prompt.len()
        );
        system_prompt = system_prompt.chars().take(MAX_SYSTEM_PROMPT_LENGTH).collect();
    }

    Ok(Some(PersonaConfig {
        name,
        description,
        tools,
        model,
        max_concurrent,
        system_prompt,
        file_path: file_path.to_path_buf(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_persona() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("coder.md");
        let content = "\
---
name: Coder
description: Writes and reviews code
tools: Read,Write,Edit,Bash
max_concurrent: 3
---

You are an expert programmer. Write clean, idiomatic code.
";
        fs::write(&file_path, content).unwrap();

        let persona = parse_file(&file_path).unwrap().unwrap();
        assert_eq!(persona.name, "Coder");
        assert_eq!(persona.description, "Writes and reviews code");
        assert_eq!(
            persona.tools.unwrap(),
            vec!["Read", "Write", "Edit", "Bash"]
        );
        assert_eq!(persona.max_concurrent, 3);
        assert!(persona.system_prompt.contains("expert programmer"));
    }

    #[test]
    fn name_fallback_to_filename() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("helper.md");
        fs::write(
            &file_path,
            "\
---
description: A helpful assistant
tools: Read
---

Be helpful.
",
        )
        .unwrap();

        let persona = parse_file(&file_path).unwrap().unwrap();
        assert_eq!(persona.name, "helper");
        assert_eq!(persona.description, "A helpful assistant");
        assert_eq!(persona.tools.unwrap(), vec!["Read"]);
    }

    #[test]
    fn missing_description_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("bad.md");
        fs::write(
            &file_path,
            "\
---
name: Bad
tools: Bash
---

No description here.
",
        )
        .unwrap();

        assert!(parse_file(&file_path).unwrap().is_none());
    }

    #[test]
    fn no_frontmatter_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("plain.md");
        fs::write(&file_path, "Just plain text, no frontmatter.").unwrap();

        assert!(parse_file(&file_path).unwrap().is_none());
    }

    #[test]
    fn defaults_when_fields_missing() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("minimal.md");
        fs::write(
            &file_path,
            "\
---
description: Minimal persona
---

Hello world.
",
        )
        .unwrap();

        let persona = parse_file(&file_path).unwrap().unwrap();
        assert_eq!(persona.name, "minimal");
        assert_eq!(persona.max_concurrent, 5);
        assert!(persona.tools.is_none());
        assert!(persona.model.is_none());
    }

    #[test]
    fn truncates_long_system_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("verbose.md");
        let long_body = "x".repeat(9000);
        fs::write(
            &file_path,
            format!(
                "\
---
description: Verbose
---

{long_body}
"
            ),
        )
        .unwrap();

        let persona = parse_file(&file_path).unwrap().unwrap();
        assert_eq!(persona.system_prompt.chars().count(), MAX_SYSTEM_PROMPT_LENGTH);
    }

    #[test]
    fn registry_loads_and_lists() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("alpha.md"),
            "\
---
description: First persona
---

Body A.
",
        )
        .unwrap();
        fs::write(
            dir.path().join("beta.md"),
            "\
---
name: Beta
description: Second persona
tools: Bash
max_concurrent: 2
---

Body B.
",
        )
        .unwrap();

        let reg = PersonaRegistry::new(dir.path().to_path_buf());
        assert_eq!(reg.list().len(), 2);
        assert!(reg.get("alpha").is_some());
        assert_eq!(reg.get("Beta").unwrap().max_concurrent, 2);

        // Reload after adding a new file
        fs::write(
            dir.path().join("gamma.md"),
            "\
---
description: Third
---

Body C.
",
        )
        .unwrap();
        // Create a new registry pointing to same dir
        let mut reg2 = PersonaRegistry::new(dir.path().to_path_buf());
        assert_eq!(reg2.list().len(), 3);
        // Or use reload
        reg2.reload();
        assert_eq!(reg2.list().len(), 3);
    }
}
