//! Embedding abstraction layer. Mirrors `src-old/memory/embedding.ts`.
//!
//! Supports: none, openai, openrouter, ollama, local (local is a stub).

use std::sync::Arc;

use anyhow::Result;
use sha2::{Digest, Sha256};

use crate::config::{Config, EmbeddingProvider as EmbeddingProviderKind};
use crate::db::Db;

// ===== Trait =====

#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &str;
    fn model(&self) -> &str;
    fn dimensions(&self) -> u32;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

// ===== Cache wrapper =====

pub struct CachedEmbeddingProvider {
    inner: Box<dyn EmbeddingProvider>,
    db: Arc<Db>,
}

impl CachedEmbeddingProvider {
    pub fn new(inner: Box<dyn EmbeddingProvider>, db: Arc<Db>) -> Self {
        Self { inner, db }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for CachedEmbeddingProvider {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn model(&self) -> &str {
        self.inner.model()
    }
    fn dimensions(&self) -> u32 {
        self.inner.dimensions()
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut uncached: Vec<(usize, &str)> = Vec::new();

        for (i, text) in texts.iter().enumerate() {
            let hash = text_hash(text);
            if let Ok(Some(buf)) = self
                .db
                .get_cached_embedding(self.name(), self.model(), &hash)
            {
                if buf.len() % 4 == 0 {
                    let floats: Vec<f32> = buf
                        .chunks_exact(4)
                        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                        .collect();
                    results[i] = Some(floats);
                    continue;
                }
            }
            uncached.push((i, text));
        }

        if !uncached.is_empty() {
            let uncached_texts: Vec<String> = uncached.iter().map(|(_, t)| t.to_string()).collect();
            let embeddings = self.inner.embed(&uncached_texts).await?;

            for (j, (idx, text)) in uncached.iter().enumerate() {
                let vec = &embeddings[j];
                results[*idx] = Some(vec.clone());

                let hash = text_hash(text);
                let buf: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
                if let Err(e) =
                    self.db
                        .insert_cached_embedding(self.name(), self.model(), &hash, &buf)
                {
                    tracing::warn!(error = %e, "[Embedding] cache insert failed");
                }
            }
        }

        Ok(results.into_iter().map(|o| o.unwrap_or_default()).collect())
    }
}

fn text_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex::encode(h.finalize())
}

// ===== Factory =====

/// Create an embedding provider from config. Returns `None` to disable embeddings (FTS-only).
pub fn create_embedding_provider(
    config: &Config,
    db: Arc<Db>,
) -> Option<Box<dyn EmbeddingProvider>> {
    let provider = config.memory.embedding_provider;

    if provider == EmbeddingProviderKind::None {
        return None;
    }

    if provider == EmbeddingProviderKind::Openai {
        let api_key = if config.memory.openai_api_key.is_empty() {
            tracing::warn!("[Embedding] Falling back to FTS-only: openai selected but no API key");
            return None;
        } else {
            config.memory.openai_api_key.clone()
        };
        let base_url = if config.memory.openai_base_url.is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            config.memory.openai_base_url.clone()
        };
        let inner = super::embedding_providers::OpenAiProvider::new(api_key, base_url);
        return Some(Box::new(CachedEmbeddingProvider::new(Box::new(inner), db)));
    }

    if provider == EmbeddingProviderKind::Openrouter {
        let api_key = if config.memory.openrouter_api_key.is_empty() {
            tracing::warn!(
                "[Embedding] Falling back to FTS-only: openrouter selected but no API key"
            );
            return None;
        } else {
            config.memory.openrouter_api_key.clone()
        };
        let base_url = if config.memory.openrouter_base_url.is_empty() {
            "https://openrouter.ai/api/v1".to_string()
        } else {
            config.memory.openrouter_base_url.clone()
        };
        let model = if config.memory.openrouter_model.is_empty() {
            "nvidia/llama-nemotron-embed-vl-1b-v2".to_string()
        } else {
            config.memory.openrouter_model.clone()
        };
        let inner = super::embedding_providers::OpenRouterProvider::new(api_key, base_url, model);
        return Some(Box::new(CachedEmbeddingProvider::new(Box::new(inner), db)));
    }

    if provider == EmbeddingProviderKind::Ollama {
        let base_url = if config.memory.ollama_base_url.is_empty() {
            "http://localhost:11434".to_string()
        } else {
            config.memory.ollama_base_url.clone()
        };
        let model = if config.memory.ollama_model.is_empty() {
            "nomic-embed-text".to_string()
        } else {
            config.memory.ollama_model.clone()
        };
        let inner = super::embedding_providers::OllamaProvider::new(base_url, model);
        return Some(Box::new(CachedEmbeddingProvider::new(Box::new(inner), db)));
    }

    if provider == EmbeddingProviderKind::Local {
        let model = if config.memory.local_model.is_empty() {
            None
        } else {
            Some(config.memory.local_model.clone())
        };
        let model_path = if config.memory.local_model_path.is_empty() {
            None
        } else {
            Some(config.memory.local_model_path.clone())
        };

        // Prefer the MLX-native cognitive embedder when its feature is on,
        // model files are present, and the user opted in via env var.
        // Falls back to candle `LocalProvider` otherwise.
        #[cfg(feature = "cognitive-mlx-embed")]
        {
            let want_mlx =
                std::env::var("SENCLAW_LOCAL_EMBED_BACKEND").ok().as_deref() == Some("mlx");
            if want_mlx {
                let name = model.clone().unwrap_or_else(|| "bge-small-en-v1.5".into());
                match super::cognitive::MlxStaticEmbedder::new(name, model_path.clone()) {
                    Ok(emb) => {
                        tracing::info!(
                            "[Embedding] Using MLX-native cognitive embedder (mlx-static)"
                        );
                        return Some(Box::new(CachedEmbeddingProvider::new(Box::new(emb), db)));
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "[Embedding] MLX embedder unavailable; falling back to candle LocalProvider"
                        );
                    }
                }
            }
        }

        let inner = super::embedding_providers::LocalProvider::new(model, model_path);
        return Some(Box::new(CachedEmbeddingProvider::new(Box::new(inner), db)));
    }

    None
}
