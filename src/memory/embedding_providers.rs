//! Embedding provider implementations. Mirrors `src-old/memory/embedding-providers.ts`.
//!
//! Four providers: OpenAI (batch of 8), OpenRouter (single), Ollama (single),
//! Local (pure-Rust candle/BERT — enable with `--features local-embed`).

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
        Self {
            client: build_client().expect("reqwest"),
            api_key,
            base_url,
        }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        "text-embedding-3-small"
    }

    fn dimensions(&self) -> u32 {
        1536
    }

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
                    let body: OpenAiResponse =
                        r.json().await.context("parse OpenAI embedding response")?;
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
                        sleep(jitter_ms(attempt)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
        bail!("embedding failed after {MAX_RETRIES} retries");
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
    fn name(&self) -> &str {
        "openrouter"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> u32 {
        *self.dims.lock().unwrap()
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            let vec =
                Self::embed_single(&self.client, &self.api_key, &self.base_url, &self.model, t)
                    .await?;
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
                    let body: OpenAiResponse = r
                        .json()
                        .await
                        .context("parse OpenRouter embedding response")?;
                    return Ok(body
                        .data
                        .first()
                        .map(|d| d.embedding.clone())
                        .unwrap_or_default());
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
                        sleep(jitter_ms(attempt)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
        bail!("embedding failed after {MAX_RETRIES} retries");
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
    fn name(&self) -> &str {
        "ollama"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> u32 {
        *self.dims.lock().unwrap()
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            let url = format!("{}/api/embeddings", self.base_url.trim_end_matches('/'));
            let res = self
                .client
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
    if t.is_empty() {
        return "nomic-embed-text".into();
    }
    if let Some(stripped) = t.strip_prefix("ollama/") {
        return stripped.to_owned();
    }
    if regex::Regex::new(r"(?i)^(text-embedding-3|text-embedding-ada|embedding.*openai)")
        .unwrap()
        .is_match(t)
    {
        return "nomic-embed-text".into();
    }
    t.to_owned()
}

// ===== Local (pure-Rust candle/BERT) =====
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
//   Directory with model.safetensors + tokenizer.json + config.json.
//   When set, SENCLAW_LOCAL_MODEL is still used for the dimensions() hint.
//
// Models are downloaded from HuggingFace Hub and cached in ~/.senclaw/models/.
// Dimensions fall back to a name-based heuristic (large→1024, base→768, else→384).
//
// Apple Silicon: cargo build --features local-embed-metal

pub struct LocalProvider {
    model: String,
    model_path: Option<String>,
    dims: u32,
    #[cfg(feature = "local-embed")]
    engine: std::sync::Arc<tokio::sync::OnceCell<std::sync::Arc<local_candle::CandleEngine>>>,
}

impl LocalProvider {
    pub fn new(model: Option<String>, model_path: Option<String>) -> Self {
        let model = model.unwrap_or_else(|| "paraphrase-multilingual-MiniLM-L12-v2".into());
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

// ── feature-gated embed ───────────────────────────────────────────────────────

#[cfg(feature = "local-embed")]
async fn embed_local(p: &LocalProvider, texts: &[String]) -> Result<Vec<Vec<f32>>> {
    let engine = p
        .engine
        .get_or_try_init(|| {
            let model = p.model.clone();
            let path = p.model_path.clone();
            async move {
                let cache_dir = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(".senclaw")
                    .join("models");
                let eng = tokio::task::spawn_blocking(move || {
                    local_candle::CandleEngine::load(&model, path.as_deref(), cache_dir)
                })
                .await
                .context("spawn_blocking panicked during model init")??;
                Ok::<_, anyhow::Error>(std::sync::Arc::new(eng))
            }
        })
        .await?;

    let engine = std::sync::Arc::clone(engine);
    let texts = texts.to_vec();
    tokio::task::spawn_blocking(move || engine.embed(&texts))
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

// ── candle internals (compiled only when local-embed feature is on) ───────────

#[cfg(feature = "local-embed")]
pub(super) mod local_candle {
    use anyhow::{Context, Result};
    use candle_core::{DType, Device, Tensor};
    use candle_nn::VarBuilder;
    use candle_transformers::models::bert::{BertModel, Config as BertConfig};
    use hf_hub::api::sync::ApiBuilder;
    use std::path::PathBuf;
    use tokenizers::{
        PaddingDirection, PaddingParams, PaddingStrategy,
        TruncationDirection, TruncationParams, TruncationStrategy,
    };

    pub struct CandleEngine {
        model: BertModel,
        tokenizer: tokenizers::Tokenizer,
        device: Device,
    }

    // candle Tensor is Arc-backed and Send; BertModel is composed of tensors.
    unsafe impl Send for CandleEngine {}
    unsafe impl Sync for CandleEngine {}

    impl CandleEngine {
        pub fn load(model_name: &str, model_path: Option<&str>, cache_dir: PathBuf) -> Result<Self> {
            let device = best_device();

            let (config_path, tokenizer_path, weights_path) = match model_path {
                Some(p) => load_from_path(p)?,
                None => load_from_hub(model_name, cache_dir)?,
            };

            let config: BertConfig = serde_json::from_reader(
                std::fs::File::open(&config_path)
                    .with_context(|| format!("open {}", config_path.display()))?,
            )
            .context("parse bert config.json")?;

            let mut tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
                .map_err(|e| anyhow::anyhow!("load tokenizer: {e}"))?;
            tokenizer
                .with_truncation(Some(TruncationParams {
                    max_length: 512,
                    strategy: TruncationStrategy::LongestFirst,
                    stride: 0,
                    direction: TruncationDirection::Right,
                }))
                .map_err(|e| anyhow::anyhow!("set truncation: {e}"))?;
            tokenizer.with_padding(Some(PaddingParams {
                strategy: PaddingStrategy::BatchLongest,
                direction: PaddingDirection::Right,
                pad_to_multiple_of: None,
                pad_id: 0,
                pad_type_id: 0,
                pad_token: "[PAD]".to_string(),
            }));

            let vb = unsafe {
                VarBuilder::from_mmaped_safetensors(&[&weights_path], DType::F32, &device)
                    .with_context(|| format!("load weights: {}", weights_path.display()))?
            };
            let model = BertModel::load(vb, &config).context("build BertModel")?;

            tracing::info!(
                "[LocalEmbed] Ready — hidden_size={} device={device:?}",
                config.hidden_size,
            );
            Ok(Self { model, tokenizer, device })
        }

        pub fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            if texts.is_empty() {
                return Ok(vec![]);
            }
            let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
            let encodings = self
                .tokenizer
                .encode_batch(text_refs, true)
                .map_err(|e| anyhow::anyhow!("encode_batch: {e}"))?;

            let batch = encodings.len();
            let seq_len = encodings[0].get_ids().len();

            let mut input_ids_flat = Vec::with_capacity(batch * seq_len);
            let mut attn_mask_flat = Vec::with_capacity(batch * seq_len);
            let mut type_ids_flat = Vec::with_capacity(batch * seq_len);

            for enc in &encodings {
                input_ids_flat.extend(enc.get_ids().iter().map(|&x| x as i64));
                attn_mask_flat.extend(enc.get_attention_mask().iter().map(|&x| x as i64));
                type_ids_flat.extend(enc.get_type_ids().iter().map(|&x| x as i64));
            }

            let input_ids = Tensor::from_vec(input_ids_flat, (batch, seq_len), &self.device)?;
            let attn_mask = Tensor::from_vec(attn_mask_flat, (batch, seq_len), &self.device)?;
            let type_ids = Tensor::from_vec(type_ids_flat, (batch, seq_len), &self.device)?;

            // [batch, seq_len, hidden_size]
            let hidden = self
                .model
                .forward(&input_ids, &type_ids, Some(&attn_mask))
                .context("bert forward")?;

            // mean pool over real tokens → [batch, hidden_size]
            let pooled = mean_pool(&hidden, &attn_mask).context("mean pool")?;

            // L2 normalise for cosine similarity
            let norm = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
            let normalised = pooled.broadcast_div(&norm)?;

            normalised.to_vec2::<f32>().context("tensor → vec")
        }
    }

    fn best_device() -> Device {
        #[cfg(feature = "metal")]
        {
            match Device::new_metal(0) {
                Ok(d) => {
                    tracing::info!("[LocalEmbed] Apple Silicon Metal");
                    return d;
                }
                Err(e) => tracing::warn!("[LocalEmbed] Metal unavailable ({e}), using CPU"),
            }
        }
        Device::Cpu
    }

    fn mean_pool(hidden: &Tensor, attn_mask: &Tensor) -> Result<Tensor> {
        // Expand mask to [batch, seq_len, 1] then broadcast-multiply with hidden
        let mask = attn_mask.unsqueeze(2)?.to_dtype(DType::F32)?;
        let sum = hidden.broadcast_mul(&mask)?.sum(1)?;
        let count = mask.sum(1)?;
        Ok((sum / count)?)
    }

    // ── HuggingFace Hub download ──────────────────────────────────────────────

    fn load_from_hub(model_name: &str, cache_dir: PathBuf) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let repo_id = resolve_repo(model_name)?;
        tracing::info!(
            "[LocalEmbed] Fetching '{repo_id}' → {}",
            cache_dir.display()
        );
        let api = ApiBuilder::new()
            .with_cache_dir(cache_dir)
            .build()
            .context("build hf-hub api")?;
        let repo = api.model(repo_id.to_string());
        let config = repo.get("config.json").context("fetch config.json")?;
        let tokenizer = repo.get("tokenizer.json").context("fetch tokenizer.json")?;
        let weights = repo
            .get("model.safetensors")
            .or_else(|_| repo.get("pytorch_model.safetensors"))
            .context("fetch model.safetensors (not found in repo)")?;
        Ok((config, tokenizer, weights))
    }

    // ── Local directory ───────────────────────────────────────────────────────

    fn load_from_path(path: &str) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let dir = std::path::Path::new(path);
        anyhow::ensure!(dir.exists(), "SENCLAW_LOCAL_MODEL_PATH not found: '{path}'");

        let weights = if dir.join("model.safetensors").exists() {
            dir.join("model.safetensors")
        } else {
            anyhow::bail!(
                "No model.safetensors found in '{path}'.\n\
                 Convert from PyTorch: python -c \"\
                 from transformers import AutoModel; \
                 m = AutoModel.from_pretrained('{path}'); \
                 m.save_pretrained('{path}', safe_serialization=True)\""
            )
        };
        let tokenizer = dir.join("tokenizer.json");
        anyhow::ensure!(tokenizer.exists(), "Missing tokenizer.json in '{path}'");
        let config = dir.join("config.json");
        anyhow::ensure!(config.exists(), "Missing config.json in '{path}'");

        Ok((config, tokenizer, weights))
    }

    // ── model name → HuggingFace repo ID ─────────────────────────────────────

    pub fn resolve_repo(name: &str) -> Result<&'static str> {
        let lower = name.to_lowercase();
        let tail = lower.rsplit('/').next().unwrap_or(&lower);

        if tail.contains("paraphrase-multilingual-minilm-l12") {
            return Ok("sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2");
        }
        if tail.contains("all-minilm-l6") {
            return Ok("sentence-transformers/all-MiniLM-L6-v2");
        }
        if tail.contains("all-minilm-l12") {
            return Ok("sentence-transformers/all-MiniLM-L12-v2");
        }
        if tail.contains("bge-small-en") {
            return Ok("BAAI/bge-small-en-v1.5");
        }
        if tail.contains("bge-base-en") {
            return Ok("BAAI/bge-base-en-v1.5");
        }
        if tail.contains("bge-large-en") {
            return Ok("BAAI/bge-large-en-v1.5");
        }
        if tail.contains("multilingual-e5-small") || tail == "e5-small" {
            return Ok("intfloat/multilingual-e5-small");
        }
        if tail.contains("multilingual-e5-base") || tail == "e5-base" {
            return Ok("intfloat/multilingual-e5-base");
        }
        if tail.contains("multilingual-e5-large") || tail == "e5-large" {
            return Ok("intfloat/multilingual-e5-large");
        }
        anyhow::bail!(
            "Unknown local model: '{name}'.\n\
             Supported: paraphrase-multilingual-MiniLM-L12-v2, all-MiniLM-L6-v2, \
             all-MiniLM-L12-v2, bge-small/base/large-en-v1.5, \
             multilingual-e5-small/base/large.\n\
             For a custom model set SENCLAW_LOCAL_MODEL_PATH to a directory \
             with model.safetensors + tokenizer.json + config.json."
        )
    }
}
