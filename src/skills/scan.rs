//! Skills directory scanner. Mirrors `src-old/skills/scan.ts`.
//!
//! Scans all skill sources (bundled, global-compat, global-sema, clawhub-managed)
//! and returns deduplicated skill entries sorted by name.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    /// Source label: bundled / global-compat / global-sema / clawhub-managed
    pub source: String,
    /// Absolute path to the skill directory
    pub dir: PathBuf,
    /// Absolute path to SKILL.md
    pub file_path: PathBuf,
}

pub struct SourceDef {
    pub dir: PathBuf,
    pub source: String,
}

/// Build the set of source directories to scan.
pub fn get_source_defs(config: &Config) -> Vec<SourceDef> {
    let mut defs: Vec<SourceDef> = Vec::new();

    if let Some(ref bundled) = config.paths.bundled_skills_dir {
        if !bundled.as_os_str().is_empty() {
            defs.push(SourceDef {
                dir: bundled.clone(),
                source: "bundled".to_string(),
            });
        }
    }

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    defs.push(SourceDef {
        dir: home.join(".claude").join("skills"),
        source: "global-compat".to_string(),
    });
    defs.push(SourceDef {
        dir: home.join(".sema").join("skills"),
        source: "global-sema".to_string(),
    });
    defs.push(SourceDef {
        dir: config.paths.managed_skills_dir.clone(),
        source: "clawhub-managed".to_string(),
    });

    defs
}

fn find_skill_md(dir: &Path) -> Option<PathBuf> {
    for name in &["SKILL.md", "skill.md", "Skill.md"] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn parse_frontmatter(content: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    if !content.starts_with("---") {
        return result;
    }
    let end = content[3..].find("\n---");
    let end = match end {
        Some(i) => i,
        None => return result,
    };
    let fm = &content[4..3 + end];
    for line in fm.lines() {
        if let Some(col) = line.find(':') {
            let key = line[..col].trim().to_string();
            let val = line[col + 1..].trim().trim_matches(|c| c == '"' || c == '\'').to_string();
            if !key.is_empty() {
                result.insert(key, val);
            }
        }
    }
    result
}

/// Scan a single source directory for skills.
pub fn scan_source(def: &SourceDef) -> Vec<SkillEntry> {
    if !def.dir.exists() {
        return Vec::new();
    }

    let mut entries: Vec<SkillEntry> = Vec::new();
    let items = match fs::read_dir(&def.dir) {
        Ok(i) => i,
        Err(_) => return entries,
    };

    for item in items.flatten() {
        let name = item.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let full_path = item.path();
        if !full_path.is_dir() {
            continue;
        }

        let skill_md = match find_skill_md(&full_path) {
            Some(p) => p,
            None => continue,
        };

        let content = match fs::read_to_string(&skill_md) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let fm = parse_frontmatter(&content);
        if fm.get("name").is_none() && fm.get("description").is_none() {
            continue;
        }

        let entry_name = fm
            .get("name")
            .cloned()
            .unwrap_or_else(|| name.to_string_lossy().to_string());
        entries.push(SkillEntry {
            name: entry_name,
            description: fm.get("description").cloned().unwrap_or_default(),
            version: fm.get("version").cloned(),
            source: def.source.clone(),
            dir: full_path,
            file_path: skill_md,
        });
    }

    entries
}

/// Scan all sources, deduplicate by name (later sources override earlier ones),
/// and return sorted results.
pub fn load_all_local_skills(config: &Config) -> Vec<SkillEntry> {
    let sources = get_source_defs(config);
    let mut map: HashMap<String, SkillEntry> = HashMap::new();
    for def in &sources {
        for entry in scan_source(def) {
            map.insert(entry.name.clone(), entry);
        }
    }
    let mut entries: Vec<SkillEntry> = map.into_values().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = "---\nname: test-skill\ndescription: A test skill\n---\n\n# Body\n";
        let fm = parse_frontmatter(content);
        assert_eq!(fm.get("name").map(|s| s.as_str()), Some("test-skill"));
        assert_eq!(
            fm.get("description").map(|s| s.as_str()),
            Some("A test skill")
        );
    }

    #[test]
    fn test_parse_frontmatter_no_fm() {
        let content = "# No frontmatter\n\nJust content.";
        let fm = parse_frontmatter(content);
        assert!(fm.is_empty());
    }

    #[test]
    fn test_scan_empty_dir() {
        let tmp = std::env::temp_dir().join(format!("empty-skills-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let def = SourceDef {
            dir: tmp.clone(),
            source: "test".to_string(),
        };
        let entries = scan_source(&def);
        assert!(entries.is_empty());
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_scan_source_with_skill() {
        let tmp = std::env::temp_dir().join(format!("test-skills-scan-{}", uuid::Uuid::new_v4()));
        let skill_dir = tmp.join("my-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: my-skill\ndescription: Does things\n---\n\n# My Skill\n",
        )
        .unwrap();

        let def = SourceDef {
            dir: tmp.clone(),
            source: "test".to_string(),
        };
        let entries = scan_source(&def);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my-skill");
        assert_eq!(entries[0].description, "Does things");
        assert_eq!(entries[0].source, "test");

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_scan_skips_hidden() {
        let tmp = std::env::temp_dir().join(format!("test-skills-hidden-{}", uuid::Uuid::new_v4()));
        let hidden_dir = tmp.join(".hidden-skill");
        fs::create_dir_all(&hidden_dir).unwrap();
        fs::write(
            hidden_dir.join("SKILL.md"),
            "---\nname: hidden\ndescription: Hidden\n---\n\n# Hidden\n",
        )
        .unwrap();

        let def = SourceDef {
            dir: tmp.clone(),
            source: "test".to_string(),
        };
        let entries = scan_source(&def);
        assert!(entries.is_empty());

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_scan_skips_files() {
        let tmp = std::env::temp_dir().join(format!("test-skills-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("not-a-skill.md"), "just a file").unwrap();

        let def = SourceDef {
            dir: tmp.clone(),
            source: "test".to_string(),
        };
        let entries = scan_source(&def);
        assert!(entries.is_empty());

        fs::remove_dir_all(&tmp).ok();
    }
}
