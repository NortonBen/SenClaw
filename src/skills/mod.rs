//! Skills loader and registry. Port targets: src-old/skills/*.ts

pub mod disabled;
pub mod expand;
pub mod scan;

use std::collections::HashMap;
use std::sync::RwLock;

use scan::SkillEntry;

/// Parsed skill metadata from YAML frontmatter.
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub when_to_use: Option<String>,
    pub model: Option<String>,
    pub max_thinking_tokens: Option<u32>,
    pub disable_model_invocation: bool,
    pub argument_hint: Option<String>,
    pub version: Option<String>,
}

/// A loaded skill with metadata, content, and source info.
#[derive(Debug, Clone)]
pub struct Skill {
    pub metadata: SkillMetadata,
    /// The markdown body (instructions).
    pub content: String,
    pub file_path: String,
    pub base_dir: String,
    /// Source: user / project / managed / workspace
    pub locate: String,
}

/// Thread-safe skill registry keyed by skill name.
#[derive(Debug)]
pub struct SkillRegistry {
    inner: RwLock<HashMap<String, Skill>>,
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl SkillRegistry {
    pub fn find(&self, name: &str) -> Option<Skill> {
        self.inner.read().unwrap().get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.inner.read().unwrap().keys().cloned().collect();
        names.sort();
        names
    }

    pub fn is_empty(&self) -> bool {
        self.inner.read().unwrap().is_empty()
    }

    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    /// Load skills from scanned entries into this registry.
    pub fn load_entries(&self, entries: &[SkillEntry]) {
        let map = build_skill_map(entries);
        *self.inner.write().unwrap() = map;
    }

    /// Build a registry from scanned `SkillEntry` values.
    pub fn from_entries(entries: &[SkillEntry]) -> Self {
        Self {
            inner: RwLock::new(build_skill_map(entries)),
        }
    }
}

fn build_skill_map(entries: &[SkillEntry]) -> HashMap<String, Skill> {
    entries
        .iter()
        .filter_map(|e| {
            let content = std::fs::read_to_string(&e.file_path).ok()?;
            let body = extract_body(&content);
            let meta = parse_skill_metadata(&content, &e.name, &e.description);
            Some((
                meta.name.clone(),
                Skill {
                    metadata: meta,
                    content: body,
                    file_path: e.file_path.to_string_lossy().to_string(),
                    base_dir: e.dir.to_string_lossy().to_string(),
                    locate: e.source.clone(),
                },
            ))
        })
        .collect()
}

/// Parse YAML-like frontmatter into `SkillMetadata`.
fn parse_skill_metadata(content: &str, default_name: &str, default_desc: &str) -> SkillMetadata {
    let mut name = default_name.to_string();
    let mut description = default_desc.to_string();
    let mut allowed_tools = Vec::new();
    let mut when_to_use = None;
    let mut model = None;
    let mut max_thinking_tokens = None;
    let mut disable_model_invocation = false;
    let mut argument_hint = None;
    let mut version = None;

    if let Some(fm) = extract_frontmatter(content) {
        for line in fm.lines() {
            let (key, val) = match line.split_once(':') {
                Some((k, v)) => (k.trim(), v.trim().trim_matches(|c| c == '"' || c == '\'')),
                None => continue,
            };
            match key {
                "name" => name = val.to_string(),
                "description" => description = val.to_string(),
                "allowed-tools" => {
                    allowed_tools = val
                        .split(|c: char| c.is_whitespace() || c == ',')
                        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\''))
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                }
                "when-to-use" => when_to_use = Some(val.to_string()),
                "model" => model = Some(val.to_string()),
                "max-thinking-tokens" => {
                    max_thinking_tokens = val.parse::<u32>().ok();
                }
                "disable-model-invocation" => {
                    disable_model_invocation = val.eq_ignore_ascii_case("true");
                }
                "argument-hint" => argument_hint = Some(val.to_string()),
                "version" => version = Some(val.to_string()),
                _ => {}
            }
        }
    }

    SkillMetadata {
        name,
        description,
        allowed_tools,
        when_to_use,
        model,
        max_thinking_tokens,
        disable_model_invocation,
        argument_hint,
        version,
    }
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

fn extract_body(content: &str) -> String {
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            return rest[end + 4..].trim().to_string();
        }
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_metadata() {
        let content = "---\nname: my-skill\ndescription: A test skill\nallowed-tools: Bash, Read\nversion: \"1.0\"\n---\n\n# Body\n";
        let meta = parse_skill_metadata(content, "fallback", "fallback desc");
        assert_eq!(meta.name, "my-skill");
        assert_eq!(meta.description, "A test skill");
        assert_eq!(meta.allowed_tools, vec!["Bash", "Read"]);
        assert_eq!(meta.version, Some("1.0".into()));
    }

    #[test]
    fn test_extract_body() {
        let content = "---\nname: s\ndescription: d\n---\n\n# Hello\nworld\n";
        let body = extract_body(content);
        assert_eq!(body, "# Hello\nworld");
    }

    #[test]
    fn test_registry_find() {
        let reg = SkillRegistry::from_entries(&[]);
        assert!(reg.find("nonexistent").is_none());
        assert!(reg.is_empty());
    }
}
