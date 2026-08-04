//! Hub store support — marketplace catalogs served over plain HTTP.
//!
//! A hub is a URL that serves a `marketplace.json` catalog (the Claude Code
//! marketplace format): an index of plugins, each pointing at a git repository.
//! Unlike a `git` source — which is one repo cloned wholesale and scanned for
//! plugin directories — a hub source is browsed first and installed per plugin:
//! only the plugins the user picks get cloned into the source directory.
//!
//! Layout under a hub source's `local_path`:
//!
//! ```text
//! <local_path>/catalog.json     cached copy of the remote marketplace.json
//! <local_path>/installed.json   manifest: plugin name → resolved dir + origin
//! <local_path>/repos/<plugin>/  git clone backing one installed plugin
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// The hub SenClaw ships with. Override per-install with `SENCLAW_HUB_URL`.
pub const DEFAULT_HUB_URL: &str = "https://hub-store.bacnd.com";

/// Filename a hub URL is assumed to serve when it points at a directory.
const CATALOG_FILE: &str = "marketplace.json";

const CATALOG_CACHE: &str = "catalog.json";
const INSTALLED_MANIFEST: &str = "installed.json";
const REPOS_DIR: &str = "repos";

// ── Catalog ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubOwner {
    pub name: Option<String>,
    pub url: Option<String>,
}

/// Where a catalog entry's code lives. Accepts both the shorthand
/// (`"source": "https://github.com/u/r"`) and the object form
/// (`"source": {"source": "github", "repo": "u/r"}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HubPluginSource {
    Url(String),
    Spec {
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        repo: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        branch: Option<String>,
        #[serde(default, rename = "ref")]
        git_ref: Option<String>,
    },
}

/// A git checkout to perform for one catalog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTarget {
    pub url: String,
    pub branch: String,
    /// Sub-directory inside the repo holding the plugin, when the catalog says so.
    pub subdir: Option<String>,
}

impl HubPluginSource {
    /// Resolve the entry into something `git_sync::clone_or_pull` can take.
    pub fn git_target(&self) -> Result<GitTarget> {
        match self {
            Self::Url(s) => {
                let s = s.trim();
                if is_git_url(s) {
                    Ok(GitTarget {
                        url: s.to_string(),
                        branch: "main".to_string(),
                        subdir: None,
                    })
                } else if let Some(repo) = as_owner_repo(s) {
                    Ok(GitTarget {
                        url: github_url(&repo),
                        branch: "main".to_string(),
                        subdir: None,
                    })
                } else {
                    // A bare relative path only resolves inside a git-backed
                    // marketplace repo; an HTTP catalog has no tree to read.
                    bail!(
                        "plugin source {s:?} is a path relative to the marketplace repo, \
                         which an HTTP hub cannot resolve — add it as a git source instead"
                    )
                }
            }
            Self::Spec {
                source,
                repo,
                url,
                path,
                branch,
                git_ref,
            } => {
                let kind = source.as_deref().unwrap_or("git");
                let url = match kind {
                    "github" => {
                        let repo = repo.as_deref().or(url.as_deref()).ok_or_else(|| {
                            anyhow::anyhow!("github plugin source is missing `repo`")
                        })?;
                        if is_git_url(repo) {
                            repo.to_string()
                        } else {
                            github_url(repo)
                        }
                    }
                    "git" | "url" => url
                        .as_deref()
                        .or(repo.as_deref())
                        .ok_or_else(|| anyhow::anyhow!("git plugin source is missing `url`"))?
                        .to_string(),
                    other => bail!("unsupported plugin source type {other:?}"),
                };
                Ok(GitTarget {
                    url,
                    branch: branch
                        .clone()
                        .or_else(|| git_ref.clone())
                        .unwrap_or_else(|| "main".to_string()),
                    subdir: path.clone(),
                })
            }
        }
    }
}

fn is_git_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("git@")
}

/// `"owner/repo"` → `Some("owner/repo")`; anything else → None.
fn as_owner_repo(s: &str) -> Option<String> {
    let mut parts = s.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    if parts.next().is_some() || owner.is_empty() || repo.is_empty() || owner.starts_with('.') {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn github_url(repo: &str) -> String {
    format!("https://github.com/{}", repo.trim_end_matches(".git"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubPlugin {
    pub name: String,
    pub source: HubPluginSource,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<serde_json::Value>,
    #[serde(default)]
    pub keywords: Option<Vec<String>>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubCatalog {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub owner: Option<HubOwner>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub plugins: Vec<HubPlugin>,
}

impl HubCatalog {
    pub fn find(&self, name: &str) -> Option<&HubPlugin> {
        self.plugins.iter().find(|p| p.name == name)
    }
}

// ── Catalog fetch ────────────────────────────────────────────────────────────

/// Turn whatever the user typed into the URL of the catalog document.
/// `https://hub-store.bacnd.com` → `https://hub-store.bacnd.com/marketplace.json`.
pub fn normalize_catalog_url(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.ends_with(".json") {
        url.to_string()
    } else {
        format!("{url}/{CATALOG_FILE}")
    }
}

/// The site a catalog URL belongs to — what we show as the source's home.
pub fn catalog_home(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .trim_end_matches(CATALOG_FILE)
        .trim_end_matches('/')
        .to_string()
}

/// Blocking HTTP GET of a catalog. Call from a blocking context
/// (`spawn_blocking`) — reqwest's blocking client cannot run on a reactor thread.
pub fn fetch_catalog(url: &str) -> Result<HubCatalog> {
    let url = normalize_catalog_url(url);
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("senclaw/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;

    let res = client
        .get(&url)
        .send()
        .with_context(|| format!("failed to fetch hub catalog {url}"))?;
    let status = res.status();
    if !status.is_success() {
        bail!("hub catalog {url} returned HTTP {status}");
    }
    let body = res
        .text()
        .with_context(|| format!("failed to read hub catalog {url}"))?;
    serde_json::from_str(&body).with_context(|| format!("failed to parse hub catalog {url}"))
}

// ── On-disk state ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub name: String,
    /// Absolute path of the plugin directory (may be nested inside the clone).
    pub dir: String,
    #[serde(rename = "repoUrl")]
    pub repo_url: String,
    pub branch: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(rename = "installedAt")]
    pub installed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledFile {
    #[serde(default)]
    pub plugins: HashMap<String, InstalledPlugin>,
}

pub fn catalog_cache_path(local_path: &Path) -> PathBuf {
    local_path.join(CATALOG_CACHE)
}

pub fn installed_path(local_path: &Path) -> PathBuf {
    local_path.join(INSTALLED_MANIFEST)
}

pub fn repo_path(local_path: &Path, plugin: &str) -> PathBuf {
    local_path.join(REPOS_DIR).join(sanitize_name(plugin))
}

/// Keep a catalog-supplied name from escaping the source directory.
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['-', '.'])
        .to_string()
}

pub fn read_catalog_cache(local_path: &Path) -> Option<HubCatalog> {
    let raw = std::fs::read_to_string(catalog_cache_path(local_path)).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn write_catalog_cache(local_path: &Path, catalog: &HubCatalog) -> Result<()> {
    std::fs::create_dir_all(local_path)
        .with_context(|| format!("failed to create {local_path:?}"))?;
    std::fs::write(
        catalog_cache_path(local_path),
        serde_json::to_string_pretty(catalog)? + "\n",
    )
    .context("failed to cache hub catalog")
}

pub fn read_installed(local_path: &Path) -> InstalledFile {
    std::fs::read_to_string(installed_path(local_path))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn write_installed(local_path: &Path, file: &InstalledFile) -> Result<()> {
    std::fs::create_dir_all(local_path)
        .with_context(|| format!("failed to create {local_path:?}"))?;
    std::fs::write(
        installed_path(local_path),
        serde_json::to_string_pretty(file)? + "\n",
    )
    .context("failed to write hub install manifest")
}

// ── Plugin directory resolution ──────────────────────────────────────────────

/// A directory is a plugin when it carries plugin.json, in either the flat
/// layout or the Claude Code `.claude-plugin/` layout.
pub fn is_plugin_dir(dir: &Path) -> bool {
    plugin_json_path(dir).is_some()
}

/// Path of a directory's plugin.json, checking both accepted locations.
pub fn plugin_json_path(dir: &Path) -> Option<PathBuf> {
    let flat = dir.join("plugin.json");
    if flat.is_file() {
        return Some(flat);
    }
    let nested = dir.join(".claude-plugin").join("plugin.json");
    if nested.is_file() {
        return Some(nested);
    }
    None
}

/// Find the plugin directory inside a freshly cloned repo. Catalog entries
/// range from "the repo *is* the plugin" to "the plugin is one of many
/// directories in a monorepo", so try the cheap guesses before walking.
pub fn resolve_plugin_dir(
    repo_root: &Path,
    plugin_name: &str,
    subdir: Option<&str>,
) -> Option<PathBuf> {
    if let Some(sub) = subdir {
        let explicit = repo_root.join(sub);
        // An explicit path wins even without plugin.json — the catalog said so.
        if explicit.is_dir() {
            return Some(explicit);
        }
    }

    let guesses = [
        repo_root.to_path_buf(),
        repo_root.join(plugin_name),
        repo_root.join("plugins").join(plugin_name),
    ];
    for dir in guesses {
        if is_plugin_dir(&dir) {
            return Some(dir);
        }
    }

    find_named_plugin_dir(repo_root, plugin_name, 3)
}

fn find_named_plugin_dir(root: &Path, plugin_name: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(root).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if name == plugin_name && is_plugin_dir(&path) {
            return Some(path);
        }
        subdirs.push(path);
    }
    subdirs
        .into_iter()
        .find_map(|dir| find_named_plugin_dir(&dir, plugin_name, depth - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_catalog_urls() {
        assert_eq!(
            normalize_catalog_url("https://hub-store.bacnd.com"),
            "https://hub-store.bacnd.com/marketplace.json"
        );
        assert_eq!(
            normalize_catalog_url("https://hub-store.bacnd.com/"),
            "https://hub-store.bacnd.com/marketplace.json"
        );
        assert_eq!(
            normalize_catalog_url("https://hub-store.bacnd.com/marketplace.json"),
            "https://hub-store.bacnd.com/marketplace.json"
        );
        assert_eq!(
            catalog_home("https://hub-store.bacnd.com/marketplace.json"),
            "https://hub-store.bacnd.com"
        );
    }

    #[test]
    fn parses_the_shipped_catalog_shape() {
        let raw = r#"{
            "name": "senclaw",
            "owner": { "name": "senclaw hub", "url": "https://hub-store.bacnd.com" },
            "description": "Skills, plugins, workflows and apps.",
            "plugins": [
                {
                    "name": "qodo-skills",
                    "source": { "source": "github", "repo": "qodo-ai/qodo-skills" },
                    "description": "Shift-left code review skills.",
                    "version": "0.6.1",
                    "author": { "name": "Qodo.ai" },
                    "keywords": ["code-review"],
                    "category": "development"
                }
            ]
        }"#;
        let catalog: HubCatalog = serde_json::from_str(raw).unwrap();
        assert_eq!(catalog.plugins.len(), 1);
        let target = catalog
            .find("qodo-skills")
            .unwrap()
            .source
            .git_target()
            .unwrap();
        assert_eq!(
            target,
            GitTarget {
                url: "https://github.com/qodo-ai/qodo-skills".into(),
                branch: "main".into(),
                subdir: None,
            }
        );
    }

    #[test]
    fn accepts_shorthand_and_git_sources() {
        let shorthand: HubPluginSource = serde_json::from_str(r#""owner/repo""#).unwrap();
        assert_eq!(
            shorthand.git_target().unwrap().url,
            "https://github.com/owner/repo"
        );

        let direct: HubPluginSource =
            serde_json::from_str(r#""https://git.example.com/a/b.git""#).unwrap();
        assert_eq!(
            direct.git_target().unwrap().url,
            "https://git.example.com/a/b.git"
        );

        let spec: HubPluginSource = serde_json::from_str(
            r#"{"source":"git","url":"https://x/y.git","path":"pkgs/p","branch":"dev"}"#,
        )
        .unwrap();
        let target = spec.git_target().unwrap();
        assert_eq!(target.branch, "dev");
        assert_eq!(target.subdir.as_deref(), Some("pkgs/p"));
    }

    #[test]
    fn rejects_repo_relative_sources() {
        let rel: HubPluginSource = serde_json::from_str(r#""./plugins/local""#).unwrap();
        assert!(rel.git_target().is_err());
    }

    #[test]
    fn sanitizes_plugin_names_into_paths() {
        assert_eq!(sanitize_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(sanitize_name("code-modernization"), "code-modernization");
    }

    #[test]
    fn resolves_plugin_dir_layouts() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = temp.path();

        // Flat: repo root is the plugin.
        std::fs::write(root.join("plugin.json"), "{}").unwrap();
        assert_eq!(resolve_plugin_dir(root, "anything", None).unwrap(), root);

        // Monorepo: .claude-plugin/plugin.json in a nested directory.
        let nested = root.join("packages").join("code-modernization");
        std::fs::create_dir_all(nested.join(".claude-plugin")).unwrap();
        std::fs::write(nested.join(".claude-plugin").join("plugin.json"), "{}").unwrap();
        std::fs::remove_file(root.join("plugin.json")).unwrap();
        assert_eq!(
            resolve_plugin_dir(root, "code-modernization", None).unwrap(),
            nested
        );
    }
}
