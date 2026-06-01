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
    /// Cognitive memory database — separate SQLite file from the main
    /// `db_path` so the user can wipe the cognitive graph (which is
    /// rebuildable from sources like SOUL.md / user chat) without
    /// touching irreplaceable data (channel messages, scheduled tasks).
    /// Defaults to a sibling `senclaw_cognitive.db` next to `db_path`.
    pub cognitive_db_path: PathBuf,
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
    /// Whisper ASR model storage, separate from LLM/local-model storage.
    pub whisper_models_dir: PathBuf,
    /// TTS (Text-to-Speech) model storage — separate from LLM and Whisper storage.
    /// Default: `~/.senclaw/tts-models`. Override with `SENCLAW_TTS_MODELS_DIR`.
    pub tts_models_dir: PathBuf,
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
    /// Auto-cognify each user message into the cognitive graph on arrival.
    /// Fires in a background `tokio::spawn` so it never adds latency to the
    /// agent's reply. When disabled, the cognitive graph only grows via
    /// explicit `cog_add` / `cog_cognify` MCP tool calls.
    pub cognitive_reflection: bool,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub hub_url: String,
    pub channel_id: String,
    pub encryption_key: String,
    pub access_token: String,
}

/// Governance knobs for the cognitive memory layer.
///
/// Backstory: cognify runs an LLM per user message (via P14 auto-reflection)
/// + per CogAdd call. On a local model with thinking enabled (Qwen3, R1
/// family) one extraction can emit 2000+ tokens (mostly `<think>` reasoning)
/// and tie up the runtime for over a minute. Without limits, a busy chat can
/// queue 5+ concurrent cognify calls and saturate the engine. The knobs
/// below cap input size, total in-flight cognify calls, and output bytes so
/// the cognitive layer can't drown out the foreground agent.
#[derive(Debug, Clone)]
pub struct CognitiveConfig {
    /// Master switch. When false, cognify is short-circuited everywhere
    /// (CogAdd → ok with `llm_skipped`, reflection → no-op, SOUL ingest →
    /// chunk-only). Useful for slow local models where the cognitive layer
    /// would just queue forever.
    pub enabled: bool,
    /// Maximum cognify calls allowed to run at once. Acquired via semaphore
    /// in `CognitiveSystem::cognify`. When the cap is hit, new calls block
    /// until a permit frees. 1 = strictly serial.
    pub max_concurrent: usize,
    /// Hard cap on bytes the cognify LLM can stream back. Local-MLX
    /// adapters close the receiver once exceeded — Qwen3 `<think>` blocks
    /// that run away get cut at this byte budget instead of decoding to
    /// `eos`. The JSON we actually care about is < 1 KB; default 8 KB
    /// leaves ample headroom for reasoning preamble.
    pub max_output_chars: usize,
    /// Reflection (auto-cognify on user messages, P14) skips messages
    /// shorter than this — short ack/yes/no has no facts to extract.
    pub reflect_min_chars: usize,
    /// Reflection skips messages longer than this — a 10 KB paste would
    /// blow up the prompt token count. Caller can still CogAdd manually.
    pub reflect_max_chars: usize,
    /// Minimum interval between reflection calls for the same agent.
    /// Defends against busy-chat storms. 0 = no cooldown.
    pub reflect_cooldown_ms: u64,
    /// Cadence for the periodic maintenance sweep (cleanup junk +
    /// merge duplicate entities). `0` disables the sweep entirely; the
    /// user can still trigger it manually from the Settings UI.
    pub maintenance_interval_hours: u64,
}

impl Default for CognitiveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            // Local LLMs don't parallelise well; 1 = serial cognify is the
            // safe default. Remote APIs can crank this up via env.
            max_concurrent: 1,
            // ~8 KB ≈ 2K tokens — enough for a `<think>` preamble + JSON.
            max_output_chars: 8 * 1024,
            reflect_min_chars: 20,
            // ~2 KB ≈ 500 tokens — anything longer is a paste, not a
            // sentence; user can still CogAdd it explicitly.
            reflect_max_chars: 2000,
            // 2 s cooldown lets multi-line replies in quick succession
            // queue without firing 5 cognifies back-to-back.
            reflect_cooldown_ms: 2000,
            // Daily maintenance. Cheap on small graphs; bigger ones can
            // raise the cadence via the Settings UI.
            maintenance_interval_hours: 24,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub telegram: TelegramConfig,
    pub admin: AdminConfig,
    pub agent: AgentConfig,
    pub scheduler: SchedulerConfig,
    pub paths: PathsConfig,
    pub memory: MemoryConfig,
    pub cognitive: CognitiveConfig,
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
                cognitive_db_path: env_path(
                    "COGNITIVE_DB_PATH",
                    senclaw_home.join("senclaw_cognitive.db"),
                ),
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
                whisper_models_dir: env_path(
                    "SENCLAW_WHISPER_MODELS_DIR",
                    senclaw_home.join("whisper-models"),
                ),
                tts_models_dir: env_path("SENCLAW_TTS_MODELS_DIR", senclaw_home.join("tts-models")),
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
                cognitive_reflection: env_bool("SENCLAW_COGNITIVE_REFLECTION", true),
            },
            cognitive: CognitiveConfig {
                enabled: env_bool("SENCLAW_COGNITIVE_ENABLED", true),
                max_concurrent: env_int::<usize>("SENCLAW_COGNITIVE_MAX_CONCURRENT", 1).max(1),
                max_output_chars: env_int::<usize>("SENCLAW_COGNITIVE_MAX_OUTPUT_CHARS", 8 * 1024)
                    .max(256),
                reflect_min_chars: env_int::<usize>("SENCLAW_COGNITIVE_REFLECT_MIN_CHARS", 20),
                reflect_max_chars: env_int::<usize>("SENCLAW_COGNITIVE_REFLECT_MAX_CHARS", 2000)
                    .max(100),
                reflect_cooldown_ms: env_int::<u64>("SENCLAW_COGNITIVE_REFLECT_COOLDOWN_MS", 2000),
                maintenance_interval_hours: env_int::<u64>(
                    "SENCLAW_COGNITIVE_MAINTENANCE_HOURS",
                    24,
                ),
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

    /// Layer the persisted Settings → Embedding UI choices on top of the
    /// env-derived defaults. Env still wins when set explicitly; this only
    /// fills in `memory.*` fields the user has chosen in the UI.
    ///
    /// **Call this at daemon boot** (`run_daemon`) — it's the bridge between
    /// the persisted JSON config and the in-memory `Config`. Without it the
    /// Settings page silently writes a file the daemon never reads.
    pub fn apply_persisted_overrides(&mut self, global_config_path: &std::path::Path) {
        // Embedding provider + per-provider credentials.
        if let Some(ec) = crate::gateway::group_manager::load_embedding_config(global_config_path) {
            // Provider — only override when the user actually picked one.
            let parsed = EmbeddingProvider::parse(&ec.provider);
            if parsed != EmbeddingProvider::None || !ec.provider.is_empty() && ec.provider != "none"
            {
                self.memory.embedding_provider = parsed;
            }

            // Per-provider fields. `skip_serializing_if = "String::is_empty"`
            // on the source struct means empty strings *are* meaningful here
            // — they mean "use env default", so we only patch when non-empty.
            if !ec.api_key.is_empty() {
                match parsed {
                    EmbeddingProvider::Openai => self.memory.openai_api_key = ec.api_key.clone(),
                    EmbeddingProvider::Openrouter => {
                        self.memory.openrouter_api_key = ec.api_key.clone()
                    }
                    _ => {}
                }
            }
            if !ec.base_url.is_empty() {
                match parsed {
                    EmbeddingProvider::Openai => self.memory.openai_base_url = ec.base_url.clone(),
                    EmbeddingProvider::Openrouter => {
                        self.memory.openrouter_base_url = ec.base_url.clone()
                    }
                    EmbeddingProvider::Ollama => self.memory.ollama_base_url = ec.base_url.clone(),
                    _ => {}
                }
            }
            if !ec.model_name.is_empty() {
                match parsed {
                    EmbeddingProvider::Openai => self.memory.openai_model = ec.model_name.clone(),
                    EmbeddingProvider::Openrouter => {
                        self.memory.openrouter_model = ec.model_name.clone()
                    }
                    EmbeddingProvider::Ollama => self.memory.ollama_model = ec.model_name.clone(),
                    EmbeddingProvider::Local => self.memory.local_model = ec.model_name.clone(),
                    _ => {}
                }
            }
            if !ec.model_path.is_empty() && parsed == EmbeddingProvider::Local {
                self.memory.local_model_path = ec.model_path.clone();
            }
            if let Some(d) = ec.dimensions {
                if d > 0 {
                    self.memory.embedding_dimensions = d;
                }
            }
        }

        // Cognitive governance knobs. Same precedence as embedding: env
        // wins when explicitly set; UI fills in everything else.
        if let Some(cc) = crate::gateway::group_manager::load_cognitive_config(global_config_path) {
            if let Some(v) = cc.enabled {
                self.cognitive.enabled = v;
            }
            if let Some(v) = cc.max_concurrent {
                self.cognitive.max_concurrent = v.max(1);
            }
            if let Some(v) = cc.max_output_chars {
                self.cognitive.max_output_chars = v.max(256);
            }
            if let Some(v) = cc.reflect_min_chars {
                self.cognitive.reflect_min_chars = v;
            }
            if let Some(v) = cc.reflect_max_chars {
                self.cognitive.reflect_max_chars = v.max(100);
            }
            if let Some(v) = cc.reflect_cooldown_ms {
                self.cognitive.reflect_cooldown_ms = v;
            }
            if let Some(v) = cc.auto_reflection {
                self.memory.cognitive_reflection = v;
            }
            if let Some(v) = cc.maintenance_interval_hours {
                self.cognitive.maintenance_interval_hours = v;
            }
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
