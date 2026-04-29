//! Embedding provider implementations. Mirrors `src-old/memory/embedding-providers.ts`.
//!
//! Four providers: OpenAI (batch of 8), OpenRouter (single), Ollama (single), Local (stub —
//! requires an ML runtime like ONNX; not yet wired).

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

// ===== Local (stub) =====
//
// The TS implementation uses @xenova/transformers (Transformers.js).  The Rust
// ecosystem has no direct equivalent.  When wired, this should use `ort`
// (ONNX Runtime) or `candle` with a compatible sentence-transformer model.

pub struct LocalProvider {
    model: String,
    #[allow(dead_code)]
    model_path: Option<String>,
}

impl LocalProvider {
    pub fn new(model: Option<String>, model_path: Option<String>) -> Self {
        Self {
            model: model.unwrap_or_else(|| "Xenova/paraphrase-multilingual-MiniLM-L12-v2".into()),
            model_path,
        }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for LocalProvider {
    fn name(&self) -> &str { "local" }
    fn model(&self) -> &str { &self.model }
    fn dimensions(&self) -> u32 { 384 }

    async fn embed(&self, _texts: &[String]) -> Result<Vec<Vec<f32>>> {
        bail!(
            "Local embedding provider not yet wired in Rust port. \
             Install ONNX Runtime (`ort` crate) and a sentence-transformer model, \
             or use ollama/openai instead."
        );
    }
}
