//! Wiki manager — personal knowledge base backed by git.
//! Mirrors `src-old/wiki/WikiManager.ts`.
//!
//! Responsibilities:
//!   - Wiki dir initialization (git init + base structure)
//!   - File read/write with YAML frontmatter auto-maintenance
//!   - Auto git commit on changes
//!   - Directory tree scanning
//!   - Title search (filename / H1 / tags)
//!   - Stats (by category, by tag, recent files)

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

// ===== Types =====

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Frontmatter {
    pub created: String,
    pub updated: String,
    pub tags: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirNode {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<DirNode>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<Frontmatter>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Dir,
    File,
}

#[derive(Debug, Clone)]
pub struct WikiDoc {
    pub path: String,
    pub content: String,
    pub frontmatter: Frontmatter,
    pub git_log: Vec<GitCommit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub path: String,
    pub title: String,
    pub tags: Vec<String>,
    pub updated: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WikiStats {
    #[serde(rename = "totalFiles")]
    pub total_files: usize,
    #[serde(rename = "totalDirs")]
    pub total_dirs: usize,
    #[serde(rename = "byCategory")]
    pub by_category: Vec<CategoryStat>,
    #[serde(rename = "byTag")]
    pub by_tag: Vec<TagStat>,
    #[serde(rename = "recentFiles")]
    pub recent_files: Vec<RecentFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryStat {
    pub dir: String,
    pub count: usize,
    #[serde(rename = "lastUpdated")]
    pub last_updated: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagStat {
    pub tag: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentFile {
    pub path: String,
    pub title: String,
    pub updated: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitCommit {
    pub hash: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagEntry {
    pub name: String,
    pub count: usize,
}

// ===== WikiManager =====

pub struct WikiManager {
    wiki_dir: PathBuf,
    excluded: HashSet<String>,
}

impl WikiManager {
    pub fn new(wiki_dir: PathBuf) -> Self {
        let mut excluded = HashSet::new();
        excluded.insert(".git".to_string());
        excluded.insert("node_modules".to_string());
        excluded.insert(".DS_Store".to_string());
        Self {
            wiki_dir,
            excluded,
        }
    }

    /// Initialize git repo + base directory structure on first use.
    pub async fn ensure_init(&self) -> Result<()> {
        let git_dir = self.wiki_dir.join(".git");
        if git_dir.exists() {
            return Ok(());
        }

        fs::create_dir_all(self.wiki_dir.join("inbox"))?;

        self.git(&["init"]).await?;
        self.git(&["config", "user.name", "semaclaw"]).await?;
        self.git(&["config", "user.email", "semaclaw@local"]).await?;

        fs::write(self.wiki_dir.join(".gitignore"), ".DS_Store\n*.swp\n")?;

        let readme = concat!(
            "# Wiki\n",
            "\n",
            "Personal knowledge base maintained by SemaClaw.\n",
            "\n",
            "## Directory structure\n",
            "\n",
            "- `inbox/` — Agent staging area; put files here when category is unclear\n",
            "\n",
            "## Remote backup (optional)\n",
            "\n",
            "```bash\n",
            "cd ~/semaclaw/wiki\n",
            "git remote add origin git@github.com:user/my-wiki.git\n",
            "git push -u origin main\n",
            "```\n",
        );
        fs::write(self.wiki_dir.join("README.md"), readme)?;

        self.git(&["add", "-A"]).await?;
        self.git(&["commit", "-m", "wiki: initial commit"]).await?;

        tracing::info!(
            "[WikiManager] Initialized wiki at {:?}",
            self.wiki_dir
        );
        Ok(())
    }

    /// Get directory tree (dirs first, files include frontmatter).
    pub fn get_tree(&self) -> Result<Vec<DirNode>> {
        self.scan_dir(&self.wiki_dir.clone(), "")
    }

    /// Read document: content + frontmatter + git history.
    pub fn read_file(&self, rel_path: &str) -> Result<WikiDoc> {
        let abs_path = self.safe_path(rel_path)?;
        let content = fs::read_to_string(&abs_path)?;
        let fm = Self::parse_frontmatter(&content).0;
        let git_log = self.get_history(rel_path, Some(10))?;
        Ok(WikiDoc {
            path: rel_path.to_string(),
            content,
            frontmatter: fm,
            git_log,
        })
    }

    /// Write document (create or update).
    /// Auto-injects/updates frontmatter (created/updated/tags/source).
    /// Auto git commit.
    pub async fn write_file(
        &self,
        rel_path: &str,
        content: &str,
        source: Option<&str>,
        tags: Option<&[String]>,
        commit_msg: Option<&str>,
    ) -> Result<()> {
        let abs_path = self.safe_path(rel_path)?;
        let is_new = !abs_path.exists();
        let now = chrono::Utc::now().to_rfc3339();

        let (existing_fm, _) = Self::parse_frontmatter(content);
        let use_existing = !existing_fm.created.is_empty();
        let fm = Frontmatter {
            created: if is_new {
                now.clone()
            } else if use_existing {
                existing_fm.created.clone()
            } else {
                now.clone()
            },
            updated: now,
            tags: tags
                .map(|t| t.to_vec())
                .or_else(|| {
                    if existing_fm.tags.is_empty() {
                        None
                    } else {
                        Some(existing_fm.tags.clone())
                    }
                })
                .unwrap_or_default(),
            source: source
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    if existing_fm.source.is_empty() {
                        "manual".to_string()
                    } else {
                        existing_fm.source
                    }
                }),
        };

        let final_content = Self::inject_frontmatter(content, &fm);
        if let Some(parent) = abs_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs_path, &final_content)?;

        let action = if is_new { "add" } else { "edit" };
        let default_msg = format!("wiki: {action} {rel_path}");
        let cmsg = commit_msg.unwrap_or(&default_msg);
        self.git_commit(cmsg, Some(&[rel_path])).await?;
        Ok(())
    }

    /// Title search: scan all .md files, match by filename / H1 title / tags.
    /// When query is empty, returns all documents (for tag filtering).
    pub fn search(
        &self,
        query: &str,
        filter_tags: Option<&[String]>,
        limit: Option<usize>,
    ) -> Result<Vec<SearchResult>> {
        let limit = limit.unwrap_or(20);
        let query_lower = query.to_lowercase();
        let filter_tags = filter_tags.unwrap_or(&[]);
        let mut results: Vec<SearchResult> = Vec::new();

        for (rel_path, content) in self.collect_md_files() {
            let (fm, _) = Self::parse_frontmatter(&content);
            let title = Self::extract_title(&content, &rel_path);
            let title_lower = title.to_lowercase();
            let filename_lower = Path::new(&rel_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let tags_lower: Vec<String> = fm.tags.iter().map(|t| t.to_lowercase()).collect();

            if !filter_tags.is_empty()
                && !filter_tags
                    .iter()
                    .any(|t| tags_lower.contains(&t.to_lowercase()))
            {
                continue;
            }

            let matches = query.is_empty()
                || filename_lower.contains(&query_lower)
                || title_lower.contains(&query_lower)
                || tags_lower.iter().any(|t| t.contains(&query_lower));

            if matches {
                results.push(SearchResult {
                    path: rel_path,
                    title,
                    tags: fm.tags.clone(),
                    updated: fm.updated.clone(),
                });
            }
        }

        results.sort_by(|a, b| b.updated.cmp(&a.updated));
        Ok(results.into_iter().take(limit).collect())
    }

    /// Stats: file count by category, tag distribution, recent files.
    pub fn get_stats(&self) -> Result<WikiStats> {
        let mut by_category: HashMap<String, (usize, String)> = HashMap::new();
        let mut by_tag: HashMap<String, usize> = HashMap::new();
        let mut all_files: Vec<(String, String, String)> = Vec::new(); // (path, title, updated)

        for (rel_path, content) in self.collect_md_files() {
            let (fm, _) = Self::parse_frontmatter(&content);
            let title = Self::extract_title(&content, &rel_path);
            let updated = fm.updated.clone();

            let top_dir = rel_path
                .split('/')
                .next()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "(root)".to_string());
            let cat = by_category
                .entry(top_dir)
                .or_insert((0, String::new()));
            cat.0 += 1;
            if updated > cat.1 {
                cat.1 = updated.clone();
            }

            for tag in &fm.tags {
                *by_tag.entry(tag.clone()).or_insert(0) += 1;
            }

            all_files.push((rel_path, title, updated));
        }

        all_files.sort_by(|a, b| b.2.cmp(&a.2));

        let mut cat_stats: Vec<CategoryStat> = by_category
            .into_iter()
            .map(|(dir, (count, last_updated))| CategoryStat {
                dir,
                count,
                last_updated,
            })
            .collect();
        cat_stats.sort_by(|a, b| b.count.cmp(&a.count));

        let mut tag_stats: Vec<TagStat> = by_tag
            .into_iter()
            .map(|(tag, count)| TagStat { tag, count })
            .collect();
        tag_stats.sort_by(|a, b| b.count.cmp(&a.count));

        Ok(WikiStats {
            total_files: all_files.len(),
            total_dirs: self.count_dirs(&self.wiki_dir.clone()),
            by_category: cat_stats,
            by_tag: tag_stats,
            recent_files: all_files
                .into_iter()
                .take(10)
                .map(|(path, title, updated)| RecentFile {
                    path,
                    title,
                    updated,
                })
                .collect(),
        })
    }

    /// Git history for a file.
    pub fn get_history(&self, rel_path: &str, limit: Option<usize>) -> Result<Vec<GitCommit>> {
        let limit = limit.unwrap_or(10);
        let safe_rel = rel_path.replace('"', "\\\"");
        let output = Command::new("git")
            .args(["log", "--pretty=format:%H|%ai|%s", &format!("-n{limit}"), "--", &safe_rel])
            .current_dir(&self.wiki_dir)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let commits: Vec<GitCommit> = stdout
                    .lines()
                    .filter(|l| !l.is_empty())
                    .filter_map(|line| {
                        let mut parts = line.splitn(3, '|');
                        Some(GitCommit {
                            hash: parts.next()?.to_string(),
                            date: parts.next()?.to_string(),
                            message: parts.next()?.to_string(),
                        })
                    })
                    .collect();
                Ok(commits)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// All tags with occurrence counts.
    pub fn get_tags(&self) -> Vec<TagEntry> {
        let mut by_tag: HashMap<String, usize> = HashMap::new();
        for (_rel_path, content) in self.collect_md_files() {
            let (fm, _) = Self::parse_frontmatter(&content);
            for tag in &fm.tags {
                *by_tag.entry(tag.clone()).or_insert(0) += 1;
            }
        }
        let mut entries: Vec<TagEntry> = by_tag
            .into_iter()
            .map(|(name, count)| TagEntry { name, count })
            .collect();
        entries.sort_by(|a, b| b.count.cmp(&a.count));
        entries
    }

    /// Create a directory (with .gitkeep so git tracks it).
    pub async fn mkdir(&self, rel_path: &str) -> Result<()> {
        let abs_path = self.safe_path(rel_path)?;
        fs::create_dir_all(&abs_path)?;
        let keep_file = abs_path.join(".gitkeep");
        if !keep_file.exists() {
            fs::write(&keep_file, "")?;
            self.git_commit(
                &format!("wiki: mkdir {rel_path}"),
                Some(&[&format!("{rel_path}/.gitkeep")]),
            )
            .await?;
        }
        Ok(())
    }

    /// Delete an empty directory (fails if not empty).
    pub async fn delete_empty_dir(&self, rel_path: &str) -> Result<()> {
        let abs_path = self.safe_path(rel_path)?;
        let entries: Vec<_> = fs::read_dir(&abs_path)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() != ".gitkeep")
            .collect();
        if !entries.is_empty() {
            bail!("Directory not empty: {rel_path}");
        }
        fs::remove_dir_all(&abs_path)?;
        self.git_commit(&format!("wiki: rmdir {rel_path}"), None)
            .await?;
        Ok(())
    }

    /// Plain-text tree representation (for CLI / Agent).
    pub fn tree_text(&self) -> Result<String> {
        let nodes = self.get_tree()?;
        let mut lines: Vec<String> = Vec::new();
        Self::render_tree(&nodes, "", &mut lines);
        Ok(lines.join("\n"))
    }

    // ===== Private helpers =====

    fn scan_dir(&self, abs_dir: &Path, rel_base: &str) -> Result<Vec<DirNode>> {
        let entries = match fs::read_dir(abs_dir) {
            Ok(e) => e,
            Err(_) => return Ok(Vec::new()),
        };

        let mut nodes: Vec<DirNode> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if self.excluded.contains(&name_str) || name_str.starts_with('.') {
                continue;
            }
            let rel_path = if rel_base.is_empty() {
                name_str.clone()
            } else {
                format!("{rel_base}/{name_str}")
            };

            if entry.file_type().map_or(false, |t| t.is_dir()) {
                let children = self.scan_dir(&entry.path(), &rel_path)?;
                nodes.push(DirNode {
                    name: name_str,
                    path: rel_path,
                    node_type: NodeType::Dir,
                    children: Some(children),
                    frontmatter: None,
                });
            } else if entry.file_type().map_or(false, |t| t.is_file())
                && name_str.ends_with(".md")
            {
                let fm = fs::read_to_string(entry.path())
                    .ok()
                    .map(|c| Self::parse_frontmatter(&c).0);
                nodes.push(DirNode {
                    name: name_str,
                    path: rel_path,
                    node_type: NodeType::File,
                    children: None,
                    frontmatter: fm,
                });
            }
        }

        nodes.sort_by(|a, b| match (&a.node_type, &b.node_type) {
            (NodeType::Dir, NodeType::File) => std::cmp::Ordering::Less,
            (NodeType::File, NodeType::Dir) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        Ok(nodes)
    }

    fn collect_md_files(&self) -> Vec<(String, String)> {
        // (rel_path, content)
        let mut files = Vec::new();
        self.collect_md_files_recursive("", &mut files);
        files
    }

    fn collect_md_files_recursive(&self, rel_base: &str, files: &mut Vec<(String, String)>) {
        let abs_dir = if rel_base.is_empty() {
            self.wiki_dir.clone()
        } else {
            self.wiki_dir.join(rel_base)
        };
        let entries = match fs::read_dir(&abs_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if self.excluded.contains(&name_str) || name_str.starts_with('.') {
                continue;
            }
            let child_rel = if rel_base.is_empty() {
                name_str.clone()
            } else {
                format!("{rel_base}/{name_str}")
            };
            if entry.file_type().map_or(false, |t| t.is_dir()) {
                self.collect_md_files_recursive(&child_rel, files);
            } else if name_str.ends_with(".md") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    files.push((child_rel, content));
                }
            }
        }
    }

    fn count_dirs(&self, dir: &Path) -> usize {
        let mut count = 0;
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return 0,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();
            if self.excluded.contains(name_str.as_str()) || name_str.starts_with('.') {
                continue;
            }
            if entry.file_type().map_or(false, |t| t.is_dir()) {
                count += 1;
                count += self.count_dirs(&entry.path());
            }
        }
        count
    }

    async fn git_commit(&self, message: &str, files: Option<&[&str]>) -> Result<()> {
        if let Some(files) = files {
            for f in files {
                let _ = Command::new("git")
                    .args(["add", "--", f])
                    .current_dir(&self.wiki_dir)
                    .output();
            }
        } else {
            let _ = Command::new("git")
                .args(["add", "-A"])
                .current_dir(&self.wiki_dir)
                .output();
        }

        let output = Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(&self.wiki_dir)
            .output()?;

        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            let msg = stderr.to_string();
            if !msg.contains("nothing to commit") && !msg.contains("nothing added") {
                tracing::warn!(
                    "[WikiManager] git commit warning: {}",
                    &msg[..msg.len().min(200)]
                );
            }
        }
        Ok(())
    }

    async fn git(&self, args: &[&str]) -> Result<()> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.wiki_dir)
            .output()
            .context("git command failed")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git {} failed: {}", args.join(" "), stderr);
        }
        Ok(())
    }

    // ===== Frontmatter parsing =====

    fn parse_frontmatter(content: &str) -> (Frontmatter, String) {
        let default_fm = Frontmatter {
            created: String::new(),
            updated: String::new(),
            tags: Vec::new(),
            source: "manual".to_string(),
        };
        if !content.starts_with("---") {
            return (default_fm, content.to_string());
        }
        let end = content[3..].find("\n---");
        let end = match end {
            Some(i) => i,
            None => return (default_fm, content.to_string()),
        };
        let yaml_block = &content[4..3 + end];
        let body = content[3 + end + 4..].trim_start_matches('\n').to_string();

        let mut fm = default_fm.clone();
        for line in yaml_block.lines() {
            let colon = match line.find(':') {
                Some(i) => i,
                None => continue,
            };
            let key = line[..colon].trim();
            let val = line[colon + 1..].trim();

            match key {
                "created" | "updated" | "source" => {
                    let val = val.to_string();
                    match key {
                        "created" => fm.created = val,
                        "updated" => fm.updated = val,
                        "source" => fm.source = val,
                        _ => {}
                    }
                }
                "tags" => {
                    let tag_str = val.trim_start_matches('[').trim_end_matches(']');
                    fm.tags = tag_str
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
        (fm, body)
    }

    fn inject_frontmatter(content: &str, fm: &Frontmatter) -> String {
        let (_, body) = Self::parse_frontmatter(content);
        let tags = if fm.tags.is_empty() {
            "[]".to_string()
        } else {
            format!("[{}]", fm.tags.join(", "))
        };
        format!(
            "---\ncreated: {}\nupdated: {}\ntags: {tags}\nsource: {}\n---\n\n{body}",
            fm.created, fm.updated, fm.source,
        )
    }

    fn extract_title(content: &str, rel_path: &str) -> String {
        let (_, body) = Self::parse_frontmatter(content);
        let re = Regex::new(r"^#\s+(.+)").unwrap();
        if let Some(cap) = re.captures(&body) {
            return cap[1].trim().to_string();
        }
        Path::new(rel_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(rel_path)
            .to_string()
    }

    fn safe_path(&self, rel_path: &str) -> Result<PathBuf> {
        // Reject paths containing .. as a simple security measure.
        // A more thorough check uses canonicalize, but that fails on
        // non-existent paths.
        if rel_path.contains("..") {
            bail!("Path traversal detected: {rel_path}");
        }
        let abs = self.wiki_dir.join(rel_path);
        // If the path already exists, canonicalize both for symlink-safe comparison
        if abs.exists() || self.wiki_dir.exists() {
            if let (Ok(wiki_canon), Ok(abs_canon)) =
                (self.wiki_dir.canonicalize(), abs.canonicalize())
            {
                if !abs_canon.starts_with(&wiki_canon) {
                    bail!("Path traversal detected: {rel_path}");
                }
            }
        }
        Ok(abs)
    }

    fn render_tree(nodes: &[DirNode], indent: &str, lines: &mut Vec<String>) {
        for node in nodes {
            if node.node_type == NodeType::Dir {
                lines.push(format!("{indent}{}/", node.name));
                if let Some(ref children) = node.children {
                    Self::render_tree(children, &format!("{indent}  "), lines);
                }
            } else {
                lines.push(format!("{indent}{}", node.name));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager(tmp: &Path) -> WikiManager {
        WikiManager::new(tmp.join("wiki"))
    }

    #[test]
    fn test_parse_frontmatter_basic() {
        let content = "---\ncreated: 2024-01-01\nupdated: 2024-01-02\ntags: [rust, wiki]\nsource: agent\n---\n\n# Title\n\nBody text.";
        let (fm, body) = WikiManager::parse_frontmatter(content);
        assert_eq!(fm.created, "2024-01-01");
        assert_eq!(fm.updated, "2024-01-02");
        assert_eq!(fm.tags, vec!["rust", "wiki"]);
        assert_eq!(fm.source, "agent");
        assert_eq!(body, "# Title\n\nBody text.");
    }

    #[test]
    fn test_parse_frontmatter_no_fm() {
        let content = "# No frontmatter\n\nJust content.";
        let (fm, body) = WikiManager::parse_frontmatter(content);
        assert_eq!(fm.source, "manual");
        assert!(fm.tags.is_empty());
        assert_eq!(body, content);
    }

    #[test]
    fn test_inject_frontmatter() {
        let content = "# Old title\n\nOld body.";
        let fm = Frontmatter {
            created: "2024-01-01".to_string(),
            updated: "2024-01-02".to_string(),
            tags: vec!["demo".to_string()],
            source: "agent".to_string(),
        };
        let result = WikiManager::inject_frontmatter(content, &fm);
        assert!(result.starts_with("---\n"));
        assert!(result.contains("created: 2024-01-01"));
        assert!(result.contains("tags: [demo]"));
        assert!(result.contains("# Old title"));
    }

    #[test]
    fn test_extract_title() {
        let content = "---\ntitle: ignored\n---\n\n# The Real Title\n\nBody.";
        assert_eq!(
            WikiManager::extract_title(content, "some/path.md"),
            "The Real Title"
        );
    }

    #[test]
    fn test_extract_title_fallback() {
        let content = "No H1 here.";
        assert_eq!(
            WikiManager::extract_title(content, "some/my-page.md"),
            "my-page"
        );
    }

    #[test]
    fn test_safe_path_traversal_blocked() {
        let tmp = std::env::temp_dir().join(format!("wiki-test-{}", uuid::Uuid::new_v4()));
        let mgr = make_manager(&tmp);
        fs::create_dir_all(&mgr.wiki_dir).unwrap();

        assert!(mgr.safe_path("subdir/file.md").is_ok());
        assert!(mgr.safe_path("../outside.md").is_err());

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_tree_text_empty() {
        let tmp = std::env::temp_dir().join(format!("wiki-empty-{}", uuid::Uuid::new_v4()));
        let mgr = make_manager(&tmp);
        fs::create_dir_all(&mgr.wiki_dir).unwrap();
        let text = mgr.tree_text().unwrap();
        assert_eq!(text, "");
        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_render_tree() {
        let nodes = vec![
            DirNode {
                name: "inbox".to_string(),
                path: "inbox".to_string(),
                node_type: NodeType::Dir,
                children: Some(vec![DirNode {
                    name: "note.md".to_string(),
                    path: "inbox/note.md".to_string(),
                    node_type: NodeType::File,
                    children: None,
                    frontmatter: None,
                }]),
                frontmatter: None,
            },
            DirNode {
                name: "README.md".to_string(),
                path: "README.md".to_string(),
                node_type: NodeType::File,
                children: None,
                frontmatter: None,
            },
        ];
        let mut lines = Vec::new();
        WikiManager::render_tree(&nodes, "", &mut lines);
        assert_eq!(lines, vec!["inbox/", "  note.md", "README.md"]);
    }
}
