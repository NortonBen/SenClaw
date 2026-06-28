//! Filesystem path helpers shared across the daemon.

use std::path::PathBuf;

/// Expand a leading `~` to the user's home directory.
///
/// The New Chat folder picker (Web UI) only captures the *string* the user
/// typed — it deliberately never touches the filesystem itself. That means a
/// `~/projects/foo` path arrives here verbatim, and the backend is responsible
/// for resolving it before opening the workspace. Anything that isn't a bare
/// `~` / `~/...` prefix is returned unchanged.
pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tilde_slash() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~/projects/foo"), home.join("projects/foo"));
        }
    }

    #[test]
    fn expands_bare_tilde() {
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expand_tilde("~"), home);
        }
    }

    #[test]
    fn leaves_absolute_unchanged() {
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn does_not_expand_tilde_user() {
        // `~bob/x` is a different user's home — we don't resolve it, leave verbatim.
        assert_eq!(expand_tilde("~bob/x"), PathBuf::from("~bob/x"));
    }
}
