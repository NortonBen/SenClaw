//! Platform channels — the **official-API** integration surface.
//!
//! Each platform module owns the server-to-server, ToS-sanctioned path (mainly
//! posting). The higher-risk read/DM operations that have no official API live
//! in `crate::web_ops` and go through the shared Chrome extension instead.
//!
//! The official paths are deliberately **inert stubs** until the operator wires
//! real credentials: they document the exact endpoint/scope but do not guess at
//! a versioned contract we cannot verify. This mirrors the finished, unit-tested
//! scaffold at `apps/crm/src/channels/tiktok.rs`.

pub mod sign;

pub mod facebook;
pub mod instagram;
pub mod threads;
pub mod tiktok;
pub mod x;
pub mod youtube;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Facebook,
    Tiktok,
    X,
    Instagram,
    Threads,
    Youtube,
}

/// How a capability is carried out for a given platform. Mirrors the
/// `capabilities` block each extension adapter declares — see
/// `extension/adapters/*.js`. Kept in lockstep by a test below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Handled here in Rust against the platform's official API.
    Official,
    /// Extension replays an internal request with captured credentials.
    Replay,
    /// Like Replay, but the request must be signed by page JS (TikTok).
    PageSign,
    /// Extension drives the on-page UI.
    Dom,
    /// The platform has no path for this at all.
    None,
}

impl Capability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Capability::Official => "official",
            Capability::Replay => "replay",
            Capability::PageSign => "page-sign",
            Capability::Dom => "dom",
            Capability::None => "none",
        }
    }
}

impl Platform {
    /// The four capabilities every adapter declares, in a stable order.
    pub const CAPS: [&'static str; 4] = ["post", "dm", "search", "browse"];

    /// What this platform can do, and how. **Authoritative in Rust** — the app
    /// must not send a user chasing an extension for something the platform
    /// simply does not have (e.g. Threads/TikTok/YouTube have no DM at all).
    pub fn capability(&self, cap: &str) -> Capability {
        use Capability::*;
        match (self, cap) {
            (Platform::Facebook, "post") => Official, // Page feed; personal profile would be Dom
            (Platform::Facebook, "dm") => Dom,        // personal Messenger
            (Platform::Facebook, "search") => Replay,
            (Platform::Facebook, "browse") => Replay,

            (Platform::X, "post") => Official,
            (Platform::X, "dm") => Replay,
            (Platform::X, "search") => Replay,
            (Platform::X, "browse") => Replay,

            (Platform::Threads, "post") => Official,
            (Platform::Threads, "dm") => None, // Threads has no DM
            (Platform::Threads, "search") => Official, // keyword-search API
            (Platform::Threads, "browse") => Replay,

            (Platform::Instagram, "post") => Official,
            (Platform::Instagram, "dm") => Replay,
            (Platform::Instagram, "search") => Replay,
            (Platform::Instagram, "browse") => Replay,

            (Platform::Tiktok, "post") => Official,
            (Platform::Tiktok, "dm") => None, // no third-party DM API
            (Platform::Tiktok, "search") => PageSign,
            (Platform::Tiktok, "browse") => PageSign,

            (Platform::Youtube, "post") => Official,
            (Platform::Youtube, "dm") => None, // YouTube has no DM
            (Platform::Youtube, "search") => Official,
            (Platform::Youtube, "browse") => Replay,

            _ => None,
        }
    }

    /// Human-readable reason a capability is unavailable, for honest errors.
    pub fn unsupported_reason(&self, cap: &str) -> String {
        let what = match cap {
            "dm" => "nhắn tin",
            "search" => "tìm kiếm",
            "browse" => "duyệt nội dung",
            "post" => "đăng bài",
            other => other,
        };
        format!(
            "{} không hỗ trợ {what} — {}",
            self.as_str(),
            self.official_note()
        )
    }

    /// Every platform this app accepts. Keep in lockstep with the extension's
    /// adapter registry (`extension/adapters/*.js`) — guarded by a test below.
    pub const ALL: [Platform; 6] = [
        Platform::Facebook,
        Platform::Tiktok,
        Platform::X,
        Platform::Instagram,
        Platform::Threads,
        Platform::Youtube,
    ];

    pub fn parse(s: &str) -> Option<Platform> {
        match s.trim().to_lowercase().as_str() {
            "facebook" | "fb" => Some(Platform::Facebook),
            "tiktok" | "tt" => Some(Platform::Tiktok),
            "x" | "twitter" => Some(Platform::X),
            "instagram" | "ig" | "insta" => Some(Platform::Instagram),
            "threads" | "th" => Some(Platform::Threads),
            "youtube" | "yt" => Some(Platform::Youtube),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Facebook => "facebook",
            Platform::Tiktok => "tiktok",
            Platform::X => "x",
            Platform::Instagram => "instagram",
            Platform::Threads => "threads",
            Platform::Youtube => "youtube",
        }
    }

    /// One-line honest note on what the official API allows for this platform.
    pub fn official_note(&self) -> &'static str {
        match self {
            Platform::Facebook => {
                "Graph API: đăng bài lên Page được (cần Page access token). DM cá nhân/nhóm KHÔNG có; chỉ Page inbox qua Messenger Platform."
            }
            Platform::Tiktok => {
                "Content Posting API: đăng video/ảnh được (scope video.publish, cần app được duyệt, ~15–25/ngày). DM bên thứ ba KHÔNG có (Business Messaging chỉ cho TikTok Shop, chặn US/EU/UK). Tìm kiếm/feed người khác: không có API."
            }
            Platform::X => {
                "API v2: đăng tweet được (tier trả phí). DM được nhưng giới hạn/tốn phí. Tìm kiếm cần tier Pro trở lên."
            }
            Platform::Instagram => {
                "Graph API (IG Business/Creator): đăng ảnh/reel được. DM chỉ qua Messenger Platform cho tài khoản Business. Duyệt feed người khác: không có API."
            }
            Platform::Threads => {
                "Threads API (Meta): đăng text/ảnh + reply được (~250 bài/24h), có API tìm kiếm theo từ khoá. KHÔNG có DM. Token đúc từ tài khoản IG liên kết."
            }
            Platform::Youtube => {
                "Data API v3: upload video, đọc/ghi comment, tìm kiếm được (tốn quota, mặc định 10k đơn vị/ngày). Không có DM."
            }
        }
    }
}

/// Post through the platform's official API. Returns a reference id on success.
///
/// Facebook (Page feed) and X (v2 tweet) are fully wired — a real
/// `official_config` makes them post for real. TikTok/Instagram/YouTube require
/// a media file + multi-step upload, so they stay documented stubs until that
/// media pipeline lands.
/// Whether `cfg` actually carries the official-API credentials for `platform`
/// (so `official_post` can hit the network). A config that only holds a captured
/// `web_session` (from the extension login) is NOT official config — Facebook
/// needs a real Page `page_id` + `access_token`; other platforms need at least
/// one non-`web_session` key.
pub fn official_configured(platform: Platform, cfg: &serde_json::Value) -> bool {
    let has = |k: &str| cfg.get(k).and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
    match platform {
        Platform::Facebook => has("page_id") && has("access_token"),
        _ => cfg.as_object().map(|m| m.keys().any(|k| k != "web_session")).unwrap_or(false),
    }
}

pub async fn official_post(
    platform: Platform,
    cfg: &serde_json::Value,
    text: &str,
) -> Result<String, String> {
    match platform {
        Platform::Facebook => facebook::official_post(cfg, text).await,
        Platform::Tiktok => tiktok::official_post(cfg, text),
        Platform::X => x::official_post(cfg, text).await,
        Platform::Instagram => instagram::official_post(cfg, text),
        Platform::Threads => threads::official_post(cfg, text).await,
        Platform::Youtube => youtube::official_post(cfg, text),
    }
}

/// Search through the platform's official API. Only called for platforms whose
/// `capability("search") == Official` (Threads, YouTube) — everything else goes
/// through the extension instead.
pub async fn official_search(
    platform: Platform,
    cfg: &serde_json::Value,
    query: &str,
) -> Result<serde_json::Value, String> {
    match platform {
        Platform::Threads => threads::official_search(cfg, query).await,
        Platform::Youtube => youtube::official_search(cfg, query).await,
        other => Err(format!(
            "{} không dùng tìm kiếm qua API chính thức",
            other.as_str()
        )),
    }
}

/// Shared reqwest client for the official-API paths.
pub(crate) fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("SenClaw-Social/0.1")
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the exact drift that shipped once: the Rust side accepted
    /// `youtube` while the extension's adapter registry had no youtube adapter,
    /// so any web op for it hit a null adapter. Every `Platform` MUST have a
    /// matching `src/adapters/<id>.ts` declaring `id: "<as_str>"`, and that file
    /// must be imported by the registry (`src/adapters/index.ts`).
    #[test]
    fn every_platform_has_an_extension_adapter() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("extension/src/adapters");
        let index = std::fs::read_to_string(dir.join("index.ts")).expect("read adapters/index.ts");

        for p in Platform::ALL {
            let id = p.as_str();
            let file = dir.join(format!("{id}.ts"));
            let src = std::fs::read_to_string(&file)
                .unwrap_or_else(|_| panic!("thiếu adapter {}", file.display()));
            assert!(
                src.contains(&format!("id: \"{id}\"")),
                "{id}.ts phải khai báo id: \"{id}\""
            );
            assert!(
                index.contains(&format!("./{id}")),
                "index.ts phải import adapter ./{id}"
            );
        }
    }

    /// The Rust capability table MUST equal what each extension adapter
    /// declares. Otherwise Rust could route a DM to a platform whose adapter
    /// says `dm: "none"` (the exact bug this guard was written for).
    #[test]
    fn rust_capability_table_matches_the_extension_adapters() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("extension/src/adapters");
        for p in Platform::ALL {
            let src = std::fs::read_to_string(dir.join(format!("{}.ts", p.as_str()))).unwrap();
            // Only look inside the capabilities block so a stray word elsewhere
            // in the file can't satisfy the match.
            let caps_block = src
                .split_once("capabilities: {")
                .and_then(|(_, rest)| rest.split_once("},"))
                .map(|(b, _)| b.to_string())
                .unwrap_or_else(|| panic!("{}.js thiếu khối capabilities", p.as_str()));

            for cap in Platform::CAPS {
                let needle = format!("{cap}: \"");
                let declared = caps_block
                    .split_once(&needle)
                    .and_then(|(_, rest)| rest.split_once('"'))
                    .map(|(v, _)| v.to_string())
                    .unwrap_or_else(|| panic!("{}.js thiếu capability '{cap}'", p.as_str()));
                assert_eq!(
                    p.capability(cap).as_str(),
                    declared,
                    "lệch capability '{cap}' cho {} (Rust vs adapter JS)",
                    p.as_str()
                );
            }
        }
    }

    #[test]
    fn platforms_without_dm_are_marked_none() {
        // The three that genuinely have no DM surface at all.
        for p in [Platform::Threads, Platform::Tiktok, Platform::Youtube] {
            assert_eq!(p.capability("dm"), Capability::None, "{}", p.as_str());
            assert!(p.unsupported_reason("dm").contains("không hỗ trợ nhắn tin"));
        }
        // …and the ones that do.
        for p in [Platform::Facebook, Platform::X, Platform::Instagram] {
            assert_ne!(p.capability("dm"), Capability::None, "{}", p.as_str());
        }
    }

    #[test]
    fn platform_parse_accepts_aliases() {
        assert_eq!(Platform::parse("FB"), Some(Platform::Facebook));
        assert_eq!(Platform::parse("twitter"), Some(Platform::X));
        assert_eq!(Platform::parse("insta"), Some(Platform::Instagram));
        assert_eq!(Platform::parse("threads"), Some(Platform::Threads));
        assert_eq!(Platform::parse("yt"), Some(Platform::Youtube));
        assert_eq!(Platform::parse("nope"), None);
    }
}
