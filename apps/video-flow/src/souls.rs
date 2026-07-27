//! Souls — the app's own sub-agent system prompts, port of the soul-loading
//! part of `internal/agent/context/manager.go`. One markdown file per agent
//! type under `souls/`, YAML frontmatter stripped, editable at runtime via
//! `PUT /api/agents/{type}/soul`.

use std::path::PathBuf;

/// Canonical soul filename for an agent type (Go: CanonicalSoulBasename).
pub fn canonical_basename(agent_type: &str) -> String {
    match agent_type {
        "image" => "image-gen.md".to_string(),
        "video" => "video-gen.md".to_string(),
        "audio" => "audio-gen.md".to_string(),
        t => format!("{}.md", t.replace('_', "-")),
    }
}

fn read_candidates(agent_type: &str) -> Vec<String> {
    let mut v = vec![canonical_basename(agent_type)];
    let hyphen = format!("{}.md", agent_type.replace('_', "-"));
    let underscore = format!("{}.md", agent_type.replace('-', "_"));
    for c in [hyphen, underscore, format!("{agent_type}.md")] {
        if !v.contains(&c) {
            v.push(c);
        }
    }
    v
}

/// Load an agent's soul, frontmatter stripped. Empty string when absent.
pub fn load(souls_dir: &PathBuf, agent_type: &str) -> String {
    for cand in read_candidates(agent_type) {
        let p = souls_dir.join(&cand);
        if let Ok(raw) = std::fs::read_to_string(&p) {
            let body = strip_frontmatter(&raw);
            if !body.trim().is_empty() {
                return body;
            }
        }
    }
    String::new()
}

/// Overwrite an agent's soul at its canonical path.
pub fn write(souls_dir: &PathBuf, agent_type: &str, content: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(souls_dir)?;
    let p = souls_dir.join(canonical_basename(agent_type));
    std::fs::write(&p, content)?;
    Ok(p)
}

/// Soul if non-empty, else the in-code default (Go: SystemPromptOrDefault).
pub fn or_default(soul: &str, fallback: &str) -> String {
    if soul.trim().is_empty() {
        fallback.to_string()
    } else {
        soul.to_string()
    }
}

pub fn strip_frontmatter(raw: &str) -> String {
    let t = raw.trim_start();
    if let Some(rest) = t.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.trim_start_matches(['\n', '\r']).to_string();
        }
    }
    raw.to_string()
}

/// Read the raw soul file (frontmatter INCLUDED) for the editor UI.
pub fn load_raw(souls_dir: &PathBuf, agent_type: &str) -> String {
    for cand in read_candidates(agent_type) {
        if let Ok(raw) = std::fs::read_to_string(souls_dir.join(&cand)) {
            return raw;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names() {
        assert_eq!(canonical_basename("image"), "image-gen.md");
        assert_eq!(canonical_basename("video"), "video-gen.md");
        assert_eq!(canonical_basename("audio"), "audio-gen.md");
        assert_eq!(canonical_basename("script_parser"), "script-parser.md");
        assert_eq!(canonical_basename("critic"), "critic.md");
    }

    #[test]
    fn frontmatter_stripped() {
        let raw = "---\nname: x\ndescription: y\n---\n\nBody here";
        assert_eq!(strip_frontmatter(raw), "Body here");
        assert_eq!(strip_frontmatter("no fm"), "no fm");
    }
}
