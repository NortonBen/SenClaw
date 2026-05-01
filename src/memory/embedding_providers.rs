//! Embedding provider implementations. Mirrors `src-old/memory/embedding-providers.ts`.
//!
//! Four providers: OpenAI (batch of 8), OpenRouter (single), Ollama (single),
//! Local (ONNX Runtime via fastembed — enable with `--features local-embed`).

use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::sleep;

use super::embedding::EmbeddingProvider;

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 3;

fn jitter_ms(attempt: u32) -> Duration {
    // rand::random is Send-safe (doesn't hold Rng across calls)
    let base = 1000u64 * 2u64.pow(attempt);
    let jitter = rand::random::<u64>() % 1000;
    Duration::from_millis(base + jitter)
}

fn build_client() -> Result<Client> {
    Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .context("build reqwest client")
}

// ===== OpenAI =====

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, base_url: String) -> Self {
        Self { client: build_client().expect("reqwest"), api_key, base_url }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAiProvider {
    fn name(&self) -> &str { "openai" }

    fn model(&self) -> &str { "text-embedding-3-small" }

    fn dimensions(&self) -> u32 { 1536 }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut all: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for batch in texts.chunks(8) {
            let result = Self::call_api(&self.client, &self.api_key, &self.base_url, batch).await?;
            all.extend(result);
        }
        Ok(all)
    }
}

#[derive(Deserialize)]
struct OpenAiResponse {
    data: Vec<OpenAiEmbeddingData>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

impl OpenAiProvider {
    async fn call_api(
        client: &Client,
        api_key: &str,
        base_url: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", base_url.trim_end_matches('/'));
        for attempt in 0..MAX_RETRIES {
            let res = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({
                    "model": "text-embedding-3-small",
                    "input": texts,
                }))
                .send()
                .await;

            match res {
                Ok(r) if r.status().is_success() => {
                    let body: OpenAiResponse = r.json().await
                        .context("parse OpenAI embedding response")?;
                    let mut sorted = body.data;
                    sorted.sort_by_key(|d| d.index);
                    return Ok(sorted.into_iter().map(|d| d.embedding).collect());
                }
                Ok(r) => {
                    let status = r.status();
                    let body = r.text().await.unwrap_or_default();
                    if attempt < MAX_RETRIES - 1 {
                        sleep(jitter_ms(attempt)).await;
                        continue;
                    }
                    bail!("OpenAI API {status}: {body}");
                }
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        sleep(jitter_ms(attempt))
                        .await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
        bail!("unreachable");
    }
}

// ===== OpenRouter =====

pub struct OpenRouterProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    dims: std::sync::Mutex<u32>,
}

impl OpenRouterProvider {
    pub fn new(api_key: String, base_url: String, model: String) -> Self {
        Self {
            client: build_client().expect("reqwest"),
            api_key,
            base_url,
            model,
            dims: std::sync::Mutex::new(1536),
        }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenRouterProvider {
    fn name(&self) -> &str { "openrouter" }

    fn model(&self) -> &str { &self.model }

    fn dimensions(&self) -> u32 { *self.dims.lock().unwrap() }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            let vec = Self::embed_single(&self.client, &self.api_key, &self.base_url, &self.model, t).await?;
            if out.is_empty() && !vec.is_empty() {
                *self.dims.lock().unwrap() = vec.len() as u32;
            }
            out.push(vec);
        }
        Ok(out)
    }
}

impl OpenRouterProvider {
    async fn embed_single(
        client: &Client,
        api_key: &str,
        base_url: &str,
        model: &str,
        text: &str,
    ) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", base_url.trim_end_matches('/'));
        for attempt in 0..MAX_RETRIES {
            let res = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({ "model": model, "input": text }))
                .send()
                .await;

            match res {
                Ok(r) if r.status().is_success() => {
                    let body: OpenAiResponse = r.json().await
                        .context("parse OpenRouter embedding response")?;
                    return Ok(body.data.first().map(|d| d.embedding.clone()).unwrap_or_default());
                }
                Ok(r) => {
                    let status = r.status();
                    let body = r.text().await.unwrap_or_default();
                    if attempt < MAX_RETRIES - 1 {
                        sleep(jitter_ms(attempt)).await;
                        continue;
                    }
                    bail!("OpenRouter API {status}: {body}");
                }
                Err(e) => {
                    if attempt < MAX_RETRIES - 1 {
                        sleep(jitter_ms(attempt))
                        .await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
        bail!("unreachable");
    }
}

// ===== Ollama =====

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
    dims: std::sync::Mutex<u32>,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            client: build_client().expect("reqwest"),
            base_url,
            model: normalize_ollama_model(&model),
            dims: std::sync::Mutex::new(1536),
        }
    }
}

#[derive(Deserialize)]
struct OllamaResponse {
    embedding: Option<Vec<f32>>,
}

#[async_trait::async_trait]
impl EmbeddingProvider for OllamaProvider {
    fn name(&self) -> &str { "ollama" }

    fn model(&self) -> &str { &self.model }

    fn dimensions(&self) -> u32 { *self.dims.lock().unwrap() }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
            let res = self.client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({ "model": self.model, "prompt": t }))
                .send()
                .await
                .context("ollama embeddings request")?;

            if !res.status().is_success() {
                let status = res.status();
                let body = res.text().await.unwrap_or_default();
                bail!("ollama embeddings failed: {status} {body}");
            }

            let data: OllamaResponse = res.json().await.context("parse ollama response")?;
            let vec = data.embedding.unwrap_or_default();
            if out.is_empty() && !vec.is_empty() {
                *self.dims.lock().unwrap() = vec.len() as u32;
            }
            out.push(vec);
        }
        Ok(out)
    }
}

fn normalize_ollama_model(model: &str) -> String {
    let t = model.trim();
    if t.is_empty() { return "nomic-embed-text".into(); }
    if let Some(stripped) = t.strip_prefix("ollama/") { return stripped.to_owned(); }
    if regex::Regex::new(r"(?i)^(text-embedding-3|text-embedding-ada|embedding.*openai)")
        .unwrap()
        .is_match(t)
    {
        return "nomic-embed-text".into();
    }
    t.to_owned()
}

// ===== Local (ONNX Runtime via fastembed) =====
//
// Enabled with `--features local-embed`. Without the feature the provider
// compiles but returns a helpful error at embed() time.
//
// Supported model names (SENCLAW_LOCAL_MODEL):
//   paraphrase-multilingual-MiniLM-L12-v2 (default, 384-dim, multilingual)
//   all-MiniLM-L6-v2, all-MiniLM-L12-v2
//   bge-small-en-v1.5, bge-base-en-v1.5, bge-large-en-v1.5
//   multilingual-e5-small, multilingual-e5-base, multilingual-e5-large
//
// Custom local path (SENCLAW_LOCAL_MODEL_PATH):
//   Directory with model.onnx (or onnx/model.onnx) + tokenizer.json.
//   When set, SENCLAW_LOCAL_MODEL is still used for the dimensions() hint.
//
// Models from HuggingFace Hub are cached in ~/.senclaw/models/ on first use.
// Dimensions are read from fastembed model metadata (TextEmbedding::get_model_info)
// for known models; unknown/custom models fall back to a name-based heuristic.

pub struct LocalProvider {
    model: String,
    model_path: Option<String>,
    /// Pre-computed dimensions — avoids loading the model just for schema setup.
    dims: u32,
    /// Lazily initialised ONNX session. The field only exists when the feature
    /// is enabled so the crate compiles without fastembed in the dep graph.
    #[cfg(feature = "local-embed")]
    engine: std::sync::Arc<
        tokio::sync::OnceCell<std::sync::Arc<fastembed::TextEmbedding>>,
    >,
}

impl LocalProvider {
    pub fn new(model: Option<String>, model_path: Option<String>) -> Self {
        let model = model.unwrap_or_else(|| "paraphrase-multilingual-MiniLM-L12-v2".into());

        // For known HF-hub models, query fastembed's metadata for the real
        // dimension count.  Falls back to a name heuristic for custom paths or
        // unrecognised names.
        #[cfg(feature = "local-embed")]
        let dims = {
            if model_path.is_none() {
                // Extract dim inside the closure so we don't return a reference
                // to the locally-owned `em` (get_model_info lifetime is tied to &em).
                local_onnx::resolve_model(&model)
                    .ok()
                    .and_then(|em| {
                        fastembed::TextEmbedding::get_model_info(&em)
                            .ok()
                            .map(|info| info.dim as u32)
                    })
                    .unwrap_or_else(|| local_dims_hint(&model))
            } else {
                local_dims_hint(&model)
            }
        };
        #[cfg(not(feature = "local-embed"))]
        let dims = local_dims_hint(&model);

        Self {
            dims,
            model,
            model_path,
            #[cfg(feature = "local-embed")]
            engine: std::sync::Arc::new(tokio::sync::OnceCell::new()),
        }
    }
}

/// Name-based dimension heuristic used when fastembed metadata is unavailable.
/// large → 1024, base → 768, everything else (small/MiniLM) → 384.
fn local_dims_hint(model: &str) -> u32 {
    let m = model.to_lowercase();
    if m.contains("large") { 1024 } else if m.contains("base") { 768 } else { 384 }
}

#[async_trait::async_trait]
impl EmbeddingProvider for LocalProvider {
    fn name(&self) -> &str { "local" }
    fn model(&self) -> &str { &self.model }
    fn dimensions(&self) -> u32 { self.dims }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        embed_local(self, texts).await
    }
}

// ── feature-gated embed implementation ───────────────────────────────────────

#[cfg(feature = "local-embed")]
async fn embed_local(p: &LocalProvider, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    // Initialise the ONNX session lazily on first call.
    // spawn_blocking keeps the tokio thread pool free while the model loads.
    let engine = p
        .engine
        .get_or_try_init(|| {
            let model = p.model.clone();
            let path = p.model_path.clone();
            async move {
                let te = tokio::task::spawn_blocking(move || {
                    local_onnx::init_engine(&model, path.as_deref())
                })
                .await
                .context("spawn_blocking panicked during model init")??;
                Ok::<_, anyhow::Error>(std::sync::Arc::new(te))
            }
        })
        .await?;

    let engine = std::sync::Arc::clone(engine);
    let texts = texts.to_vec();
    // Run inference on a blocking thread — ONNX Runtime is CPU-bound.
    tokio::task::spawn_blocking(move || {
        engine.embed(texts, None).map_err(|e| anyhow::anyhow!("{e}"))
    })
    .await
    .context("spawn_blocking panicked during embed")?
}

#[cfg(not(feature = "local-embed"))]
async fn embed_local(_p: &LocalProvider, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
    bail!(
        "Local embedding provider requires the 'local-embed' feature.\n\
         Rebuild with: cargo build --features local-embed\n\
         Or use the ollama / openai provider instead."
    );
}

// ── fastembed internals (compiled only when local-embed feature is on) ────────

#[cfg(feature = "local-embed")]
pub(super) mod local_onnx {
    use anyhow::{bail, Context, Result};
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    use std::path::Path;

    pub fn init_engine(model: &str, model_path: Option<&str>) -> Result<TextEmbedding> {
        if let Some(path) = model_path {
            init_from_path(path)
        } else {
            init_from_hub(model)
        }
    }

    // ── HuggingFace Hub (auto-download on first run) ──────────────────────

    fn init_from_hub(model_name: &str) -> Result<TextEmbedding> {
        let em = resolve_model(model_name)?;
        let cache_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".senclaw")
            .join("models");
        let opts = InitOptions::new(em)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(true);
        tracing::info!("[LocalEmbed] Loading '{model_name}' (downloads on first run to ~/.senclaw/models/)");
        TextEmbedding::try_new(opts).map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// Map a model name string (with or without "org/" prefix) to the
    /// fastembed `EmbeddingModel` enum variant.  Exposed as `pub(super)` so
    /// `LocalProvider::new` can call `get_model_info` for the real dims.
    pub fn resolve_model(name: &str) -> Result<EmbeddingModel> {
        let tail = name.to_lowercase();
        let tail = tail.rsplit('/').next().unwrap_or(&tail);

        if tail.contains("paraphrase-multilingual-minilm-l12") {
            return Ok(EmbeddingModel::ParaphraseMLMiniLML12V2);
        }
        if tail.contains("all-minilm-l6") {
            return Ok(EmbeddingModel::AllMiniLML6V2);
        }
        if tail.contains("all-minilm-l12") {
            return Ok(EmbeddingModel::AllMiniLML12V2);
        }
        if tail.contains("bge-small-en") {
            return Ok(EmbeddingModel::BGESmallENV15);
        }
        if tail.contains("bge-base-en") {
            return Ok(EmbeddingModel::BGEBaseENV15);
        }
        if tail.contains("bge-large-en") {
            return Ok(EmbeddingModel::BGELargeENV15);
        }
        if tail.contains("multilingual-e5-small") || tail == "e5-small" {
            return Ok(EmbeddingModel::MultilingualE5Small);
        }
        if tail.contains("multilingual-e5-base") || tail == "e5-base" {
            return Ok(EmbeddingModel::MultilingualE5Base);
        }
        if tail.contains("multilingual-e5-large") || tail == "e5-large" {
            return Ok(EmbeddingModel::MultilingualE5Large);
        }
        bail!(
            "Unknown local model: '{name}'.\n\
             Supported: paraphrase-multilingual-MiniLM-L12-v2, all-MiniLM-L6-v2, \
             all-MiniLM-L12-v2, bge-small/base/large-en-v1.5, \
             multilingual-e5-small/base/large.\n\
             To use a custom model set SENCLAW_LOCAL_MODEL_PATH to a directory \
             with model.onnx + tokenizer.json."
        )
    }

    // ── Local directory (pre-downloaded / custom model) ───────────────────

    fn init_from_path(path: &str) -> Result<TextEmbedding> {
        use fastembed::{InitOptionsUserDefined, TokenizerFiles, UserDefinedEmbeddingModel};
        use std::fs;

        let dir = Path::new(path);
        if !dir.exists() {
            bail!("SENCLAW_LOCAL_MODEL_PATH does not exist: '{path}'");
        }

        // Support both HF hub layout (onnx/model.onnx) and flat layout.
        let onnx_path = if dir.join("onnx/model.onnx").exists() {
            dir.join("onnx/model.onnx")
        } else if dir.join("model.onnx").exists() {
            dir.join("model.onnx")
        } else {
            bail!("No ONNX model found in '{path}'. Expected 'model.onnx' or 'onnx/model.onnx'.");
        };

        let onnx_file = fs::read(&onnx_path)
            .with_context(|| format!("Cannot read {}", onnx_path.display()))?;

        let read_opt = |name: &str| -> Vec<u8> { fs::read(dir.join(name)).unwrap_or_default() };

        let tokenizer_files = TokenizerFiles {
            tokenizer_file: fs::read(dir.join("tokenizer.json"))
                .with_context(|| format!("Missing tokenizer.json in '{path}'"))?,
            config_file: read_opt("config.json"),
            special_tokens_map_file: read_opt("special_tokens_map.json"),
            tokenizer_config_file: read_opt("tokenizer_config.json"),
        };

        let user_model = UserDefinedEmbeddingModel::new(onnx_file, tokenizer_files);

        tracing::info!("[LocalEmbed] Loading model from '{path}'");
        // Use InitOptionsUserDefined::default() — matches rig-fastembed reference impl.
        TextEmbedding::try_new_from_user_defined(user_model, InitOptionsUserDefined::default())
            .map_err(|e| anyhow::anyhow!("Failed to load model from '{path}': {e}"))
    }
}
