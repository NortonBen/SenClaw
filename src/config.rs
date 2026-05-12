//! Application configuration. Mirrors `src-old/config.ts`.
//!
//! Env vars are read once at process start via [`Config::from_env`]. Brand-
//! prefixed vars use `SENCLAW_*` (renamed from `SEMACLAW_*`); platform-level
//! vars (`TELEGRAM_BOT_TOKEN`, `FEISHU_APP_ID`, …) are unchanged.
//! Default paths live under `~/.senclaw/` and `~/senclaw/`.

use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub agent_folder: String,
}

#[derive(Debug, Clone)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    pub domain: String,
}

#[derive(Debug, Clone)]
pub struct QqConfig {
    pub app_id: String,
    pub app_secret: String,
    pub sandbox: bool,
}

#[derive(Debug, Clone)]
pub struct WechatConfig {
    pub enabled: bool,
    pub api_base_url: String,
    pub agent_folder: String,
}

#[derive(Debug, Clone)]
pub struct AdminConfig {
    pub telegram_user_id: String,
    pub feishu_open_id: String,
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub max_concurrent: u32,
    pub max_messages_per_group: u32,
}

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub interval_sec: u64,
    pub notify_max_delay_minutes: u64,
}

#[derive(Debug, Clone)]
pub struct PathsConfig {
    pub db_path: PathBuf,
    pub agents_dir: PathBuf,
    pub workspace_dir: PathBuf,
    pub global_config_path: PathBuf,
    pub dispatch_state_path: PathBuf,
    pub managed_skills_dir: PathBuf,
    pub managed_plugins_dir: PathBuf,
    pub wiki_dir: PathBuf,
    pub hooks_path: PathBuf,
    pub virtual_agents_dir: PathBuf,
    /// Optional bundled-skills dir; empty when unset (TS treats blank as disabled).
    pub bundled_skills_dir: Option<PathBuf>,
    pub workspace_templates_dir: PathBuf,
    /// Marketplace configuration path
    pub marketplace_config_path: PathBuf,
    /// Marketplace state path
    pub marketplace_state_path: PathBuf,
    /// Marketplace git clones directory
    pub marketplace_clones_dir: PathBuf,
    /// Local model storage (MLX weights, tokenizers, configs).
    pub local_models_dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProvider {
    None,
    Openai,
    Openrouter,
    Ollama,
    Local,
}

impl EmbeddingProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Openai => "openai",
            Self::Openrouter => "openrouter",
            Self::Ollama => "ollama",
            Self::Local => "local",
        }
    }

    fn parse(raw: &str) -> Self {
        match raw {
            "openai" => Self::Openai,
            "openrouter" => Self::Openrouter,
            "ollama" => Self::Ollama,
            "local" => Self::Local,
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiServerConfig {
    pub port: u16,
    pub ws_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    /// Timeout in seconds for MCP tool calls (default: 300 = 5 minutes)
    pub request_timeout_secs: u64,
    /// Watchdog check interval in seconds (default: 60 = 1 minute)
    pub watchdog_interval_secs: u64,
    /// Enable watchdog monitoring (default: true)
    pub watchdog_enabled: bool,
    /// Binary name or path for Litho (`deepwiki-rs`). Override with `SENCLAW_LITHO_BINARY`.
    pub litho_binary: String,
    /// Optional `--model-efficient` for Litho (`SENCLAW_LITHO_MODEL_EFFICIENT`).
    pub litho_model_efficient: String,
}

#[derive(Debug, Clone)]
pub struct MemoryConfig {
    pub embedding_provider: EmbeddingProvider,
    pub openai_api_key: String,
    pub openai_base_url: String,
    pub openai_model: String,
    pub openrouter_api_key: String,
    pub openrouter_base_url: String,
    pub openrouter_model: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub local_model_path: String,
    pub local_model: String,
    /// 0 means "use provider default" (see [`Config::resolve_dimensions`]).
    pub embedding_dimensions: u32,
    pub chunk_size: u32,
    pub chunk_overlap: u32,
    pub search_max_results: u32,
    pub search_min_score: f32,
    pub pre_retrieval: bool,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub hub_url: String,
    pub channel_id: String,
    pub encryption_key: String,
    pub access_token: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub admin: AdminConfig,
    pub agent: AgentConfig,
    pub scheduler: SchedulerConfig,
    pub paths: PathsConfig,
    pub memory: MemoryConfig,
    pub ui_server: UiServerConfig,
    pub mcp: McpConfig,
    pub ws_port: u16,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn env_or(key: &str, fallback: &str) -> String {
    env::var(key).unwrap_or_else(|_| fallback.to_owned())
}

fn env_bool(key: &str, fallback: bool) -> bool {
    match env::var(key) {
        Ok(v) => v == "true",
        Err(_) => fallback,
    }
}

fn env_int<T: std::str::FromStr>(key: &str, fallback: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(fallback)
}

fn env_path(key: &str, fallback: PathBuf) -> PathBuf {
    match env::var(key) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => fallback,
    }
}

impl Config {
    /// Read env vars (no `.env` loading — the binary entrypoint already calls
    /// `dotenvy::dotenv()`).
    pub fn from_env() -> Self {
        let h = home();
        let senclaw_home = h.join(".senclaw");
        let senclaw_data = h.join("senclaw");

        Self {
            telegram: TelegramConfig {
                bot_token: env_or("TELEGRAM_BOT_TOKEN", ""),
                agent_folder: env_or("TELEGRAM_AGENT_FOLDER", "main"),
            },
            admin: AdminConfig {
                telegram_user_id: env_or("ADMIN_TELEGRAM_USER_ID", ""),
                feishu_open_id: env_or("ADMIN_FEISHU_OPEN_ID", ""),
            },
            agent: AgentConfig {
                max_concurrent: env_int("MAX_CONCURRENT_AGENTS", 5),
                max_messages_per_group: env_int("MAX_MESSAGES_PER_GROUP", 100),
            },
            scheduler: SchedulerConfig {
                interval_sec: env_int("SCHEDULER_INTERVAL_SEC", 60),
                notify_max_delay_minutes: env_int("NOTIFY_MAX_DELAY_MINUTES", 30),
            },
            paths: PathsConfig {
                db_path: env_path("DB_PATH", senclaw_home.join("senclaw.db")),
                agents_dir: env_path("AGENTS_DIR", senclaw_data.join("agents")),
                workspace_dir: env_path("WORKSPACE_DIR", senclaw_data.join("workspace")),
                global_config_path: env_path(
                    "SENCLAW_CONFIG_PATH",
                    senclaw_home.join("config.json"),
                ),
                dispatch_state_path: env_path(
                    "SENCLAW_DISPATCH_STATE_PATH",
                    senclaw_home.join("dispatch-state.json"),
                ),
                managed_skills_dir: env_path(
                    "MANAGED_SKILLS_DIR",
                    senclaw_home.join("managed").join("skills"),
                ),
                managed_plugins_dir: env_path(
                    "MANAGED_PLUGINS_DIR",
                    senclaw_home.join("managed").join("plugins"),
                ),
                wiki_dir: env_path("WIKI_DIR", senclaw_data.join("wiki")),
                hooks_path: env_path("SENCLAW_HOOKS_PATH", senclaw_home.join("hooks.json")),
                virtual_agents_dir: env_path(
                    "SENCLAW_VIRTUAL_AGENTS_DIR",
                    senclaw_data.join("virtual-agents"),
                ),
                bundled_skills_dir: {
                    let raw = env::var("SENCLAW_BUNDLED_SKILLS_DIR").ok();
                    let resolved: Option<PathBuf> = match raw {
                        Some(ref v) if !v.trim().is_empty() => {
                            let p = PathBuf::from(v);
                            if p.exists() {
                                Some(p)
                            } else {
                                None
                            }
                        }
                        _ => {
                            // Fallback: <project>/skills/ (mirrors TS __dirname + ../skills)
                            let project_skills =
                                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills");
                            if project_skills.exists() {
                                Some(project_skills)
                            } else {
                                None
                            }
                        }
                    };
                    resolved
                },
                workspace_templates_dir: env_path(
                    "SENCLAW_WORKSPACE_TEMPLATES_DIR",
                    senclaw_data.join("workspace-templates"),
                ),
                marketplace_config_path: env_path(
                    "SENCLAW_MARKETPLACE_CONFIG_PATH",
                    senclaw_home.join("marketplace.json"),
                ),
                marketplace_state_path: env_path(
                    "SENCLAW_MARKETPLACE_STATE_PATH",
                    senclaw_home.join("marketplace-state.json"),
                ),
                marketplace_clones_dir: env_path(
                    "SENCLAW_MARKETPLACE_CLONES_DIR",
                    senclaw_home.join("marketplace"),
                ),
                local_models_dir: env_path(
                    "SENCLAW_LOCAL_MODELS_DIR",
                    senclaw_home.join("local-models"),
                ),
            },
            memory: MemoryConfig {
                embedding_provider: EmbeddingProvider::parse(&env_or(
                    "SENCLAW_EMBEDDING_PROVIDER",
                    "none",
                )),
                openai_api_key: env_or("SENCLAW_OPENAI_API_KEY", ""),
                openai_base_url: env_or("SENCLAW_OPENAI_BASE_URL", "https://api.openai.com/v1"),
                openai_model: env_or("SENCLAW_OPENAI_MODEL", "text-embedding-3-small"),
                openrouter_api_key: env_or("SENCLAW_OPENROUTER_API_KEY", ""),
                openrouter_base_url: env_or(
                    "SENCLAW_OPENROUTER_BASE_URL",
                    "https://openrouter.ai/api/v1",
                ),
                openrouter_model: env_or(
                    "SENCLAW_OPENROUTER_MODEL",
                    "openai/text-embedding-3-small",
                ),
                ollama_base_url: env_or("SENCLAW_OLLAMA_BASE_URL", "http://localhost:11434"),
                ollama_model: env_or("SENCLAW_OLLAMA_MODEL", "nomic-embed-text"),
                local_model_path: env_or("SENCLAW_LOCAL_MODEL_PATH", ""),
                local_model: env_or("SENCLAW_LOCAL_MODEL", ""),
                embedding_dimensions: env_int("SENCLAW_EMBEDDING_DIMENSIONS", 0),
                chunk_size: env_int("SENCLAW_CHUNK_SIZE", 400),
                chunk_overlap: env_int("SENCLAW_CHUNK_OVERLAP", 80),
                search_max_results: env_int("SENCLAW_SEARCH_MAX_RESULTS", 5),
                search_min_score: env_int("SENCLAW_SEARCH_MIN_SCORE", 0.5_f32),
                pre_retrieval: env_bool("SENCLAW_PRE_RETRIEVAL", false),
            },
            ui_server: UiServerConfig {
                port: env_int("SENCLAW_UI_PORT", 18788),
                ws_token: match env::var("SENCLAW_WS_TOKEN") {
                    Ok(v) if !v.trim().is_empty() => Some(v),
                    _ => None,
                },
            },
            mcp: McpConfig {
                request_timeout_secs: env_int("SENCLAW_MCP_REQUEST_TIMEOUT_SECS", 300),
                watchdog_interval_secs: env_int("SENCLAW_MCP_WATCHDOG_INTERVAL_SECS", 60),
                watchdog_enabled: env_bool("SENCLAW_MCP_WATCHDOG_ENABLED", true),
                litho_binary: env_or("SENCLAW_LITHO_BINARY", "deepwiki-rs"),
                litho_model_efficient: env_or("SENCLAW_LITHO_MODEL_EFFICIENT", ""),
            },
            ws_port: env_int("SENCLAW_WS_PORT", 18789),
        }
    }

    /// Resolve embedding vector dimensions for a given provider, honoring the
    /// user override when present. Mirrors `resolveDimensions` in db.ts.
    pub fn resolve_dimensions(provider: EmbeddingProvider, configured: u32) -> u32 {
        if configured > 0 {
            return configured;
        }
        match provider {
            EmbeddingProvider::Local => 384,
            _ => 1536,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_provider_parses() {
        assert_eq!(
            EmbeddingProvider::parse("openai"),
            EmbeddingProvider::Openai
        );
        assert_eq!(EmbeddingProvider::parse("local"), EmbeddingProvider::Local);
        assert_eq!(EmbeddingProvider::parse("garbage"), EmbeddingProvider::None);
    }

    #[test]
    fn resolve_dimensions_uses_override_when_positive() {
        assert_eq!(
            Config::resolve_dimensions(EmbeddingProvider::Openai, 3072),
            3072
        );
    }

    #[test]
    fn resolve_dimensions_local_default_384() {
        assert_eq!(Config::resolve_dimensions(EmbeddingProvider::Local, 0), 384);
    }

    #[test]
    fn resolve_dimensions_other_default_1536() {
        assert_eq!(
            Config::resolve_dimensions(EmbeddingProvider::Openai, 0),
            1536
        );
        assert_eq!(
            Config::resolve_dimensions(EmbeddingProvider::Ollama, 0),
            1536
        );
    }
}
