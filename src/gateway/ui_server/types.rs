use serde::{Deserialize, Serialize};

// ===== AdminPermissionsConfig =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminPermissionsConfig {
    #[serde(rename = "skipMainAgentPermissions")]
    pub skip_main_agent_permissions: bool,
    #[serde(rename = "skipAllAgentsPermissions")]
    pub skip_all_agents_permissions: bool,
}

impl Default for AdminPermissionsConfig {
    fn default() -> Self {
        Self {
            skip_main_agent_permissions: false,
            skip_all_agents_permissions: false,
        }
    }
}

// ===== MIME types =====

pub(crate) const MIME: &[(&str, &str)] = &[
    (".html", "text/html; charset=utf-8"),
    (".js", "application/javascript"),
    (".mjs", "application/javascript"),
    (".css", "text/css"),
    (".svg", "image/svg+xml"),
    (".png", "image/png"),
    (".ico", "image/x-icon"),
    (".json", "application/json"),
    (".woff2", "font/woff2"),
    (".woff", "font/woff"),
    (".ttf", "font/ttf"),
];

pub(crate) fn path_to_mime(path: &str) -> &'static str {
    if let Some(pos) = path.rfind('.') {
        let ext = &path[pos..];
        for (e, m) in MIME {
            if *e == ext {
                return m;
            }
        }
    }
    "application/octet-stream"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_lookup() {
        assert_eq!(path_to_mime("index.html"), "text/html; charset=utf-8");
        assert_eq!(path_to_mime("app.js"), "application/javascript");
        assert_eq!(path_to_mime("style.css"), "text/css");
        assert_eq!(path_to_mime("unknown.xyz"), "application/octet-stream");
    }

    #[test]
    fn test_admin_perms_default() {
        let cfg = AdminPermissionsConfig::default();
        assert!(!cfg.skip_main_agent_permissions);
        assert!(!cfg.skip_all_agents_permissions);
    }
}
