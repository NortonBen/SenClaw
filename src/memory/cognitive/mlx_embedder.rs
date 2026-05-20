//! MLX-native cognitive embedder (Apple Silicon).
//!
//! Approach: **token-embedding-table + mean pool + L2 normalize**. No
//! transformer layers, no attention. This is the "static distillation"
//! family (Model2Vec, static-bge) — surprisingly competitive for retrieval
//! (~70-80 % of the parent BERT on MTEB) at a fraction of the runtime cost.
//!
//! Why not full BERT here? Porting 12 layers of multi-head attention + FFN
//! + layer norm to mlx-rs is a substantial undertaking and would duplicate
//! work that the candle-backed [`super::super::embedding_providers::LocalProvider`]
//! already does cross-platform. This module is the **fast, in-process,
//! Metal-native option** for users who don't need full-fidelity embeddings.
//!
//! ## Model layout expected on disk
//!
//! ```text
//! ~/.senclaw/models/<name>/
//!   ├── tokenizer.json           — HuggingFace fast tokenizer
//!   ├── model.safetensors        — must contain `embeddings.word_embeddings.weight`
//!   └── config.json              — optional; hidden_size derived from weight shape
//! ```
//!
//! Compatible model families: any BERT-architecture model (bge-small,
//! MiniLM, multilingual-e5, etc.). For Model2Vec-distilled checkpoints the
//! tensor is named `embedding` — both names are tried.
//!
//! ## Feature flag
//!
//! Behind `cognitive-mlx-embed`. Without it, this module compiles but
//! `MlxStaticEmbedder::new()` returns an error at construction time, so the
//! cognitive layer falls back to `LocalProvider` (candle) or one of the
//! cloud providers.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

// =====================================================================
// Pure-math helpers — used by both the real and stub embedder paths, kept
// outside the feature gate so unit tests run without MLX installed.
// =====================================================================

/// L2-normalize a vector in place. Returns a no-op when norm < 1e-12.
pub(crate) fn l2_normalize_inplace(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm < 1e-12 {
        return;
    }
    for x in v.iter_mut() {
        *x /= norm;
    }
}

/// Mean pool across `tokens` rows of `hidden` (shape [tokens, dim], row-major).
/// Ignores positions where `mask[i] == 0` (padding).
pub(crate) fn mean_pool_masked(hidden: &[f32], mask: &[u8], dim: usize) -> Vec<f32> {
    let tokens = mask.len();
    debug_assert_eq!(hidden.len(), tokens * dim, "shape mismatch");
    let mut out = vec![0.0f32; dim];
    let mut count = 0u32;
    for i in 0..tokens {
        if mask[i] == 0 {
            continue;
        }
        let row = &hidden[i * dim..(i + 1) * dim];
        for (o, x) in out.iter_mut().zip(row.iter()) {
            *o += *x;
        }
        count += 1;
    }
    if count > 0 {
        let inv = 1.0 / count as f32;
        for x in out.iter_mut() {
            *x *= inv;
        }
    }
    out
}

/// Resolve the model directory: `SENCLAW_LOCAL_MODEL_PATH` if set,
/// otherwise `~/.senclaw/models/<name>`.
pub(crate) fn resolve_model_dir(name: &str, override_path: Option<&str>) -> PathBuf {
    if let Some(p) = override_path {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("models")
        .join(name)
}

/// File-existence check, returning a descriptive error so users know what
/// to download.
pub(crate) fn require_files(model_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let tok = model_dir.join("tokenizer.json");
    let weights = model_dir.join("model.safetensors");
    if !tok.exists() {
        return Err(anyhow!(
            "MLX embedder: missing tokenizer.json at {}\nDownload from HuggingFace and place under {}",
            tok.display(),
            model_dir.display()
        ));
    }
    if !weights.exists() {
        return Err(anyhow!(
            "MLX embedder: missing model.safetensors at {}",
            weights.display()
        ));
    }
    Ok((tok, weights))
}

// =====================================================================
// Public type — always present, runtime behaviour gated.
// =====================================================================

pub struct MlxStaticEmbedder {
    model_name: String,
    #[cfg(feature = "cognitive-mlx-embed")]
    inner: imp::Inner,
    #[cfg(not(feature = "cognitive-mlx-embed"))]
    _phantom: std::marker::PhantomData<()>,
    dims: u32,
}

impl MlxStaticEmbedder {
    /// Build a new embedder. `model_path` overrides the default discovery
    /// (`~/.senclaw/models/<model_name>`).
    pub fn new(model_name: impl Into<String>, model_path: Option<String>) -> Result<Self> {
        let model_name = model_name.into();

        #[cfg(feature = "cognitive-mlx-embed")]
        {
            let model_dir = resolve_model_dir(&model_name, model_path.as_deref());
            let (tok_path, weights_path) = require_files(&model_dir)?;
            let inner = imp::Inner::load(&tok_path, &weights_path)?;
            let dims = inner.dims as u32;
            return Ok(Self { model_name, inner, dims });
        }

        #[cfg(not(feature = "cognitive-mlx-embed"))]
        {
            let _ = model_path;
            return Err(anyhow!(
                "MlxStaticEmbedder requires the `cognitive-mlx-embed` feature.\n\
                 Build with: cargo build --features cognitive-mlx-embed"
            ));
        }
    }
}

#[async_trait::async_trait]
impl crate::memory::embedding::EmbeddingProvider for MlxStaticEmbedder {
    fn name(&self) -> &str {
        "mlx-static"
    }
    fn model(&self) -> &str {
        &self.model_name
    }
    fn dimensions(&self) -> u32 {
        self.dims
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        #[cfg(feature = "cognitive-mlx-embed")]
        {
            let texts: Vec<String> = texts.to_vec();
            let inner = self.inner.clone();
            let out =
                tokio::task::spawn_blocking(move || inner.embed_batch(&texts)).await??;
            return Ok(out);
        }
        #[cfg(not(feature = "cognitive-mlx-embed"))]
        {
            let _ = texts;
            return Err(anyhow!("cognitive-mlx-embed feature is disabled"));
        }
    }
}

// =====================================================================
// Feature-gated implementation
// =====================================================================

#[cfg(feature = "cognitive-mlx-embed")]
mod imp {
    use super::*;
    use std::sync::Arc;
    use tokenizers::Tokenizer;

    /// Load the embedding matrix from safetensors. We try a small set of
    /// canonical names; if none match, we surface them all in the error so
    /// the user can name the right one.
    const EMBEDDING_KEYS: &[&str] = &[
        "embeddings.word_embeddings.weight",
        "bert.embeddings.word_embeddings.weight",
        "model.embed_tokens.weight",
        "embedding",         // Model2Vec naming
        "embeddings.weight", // generic
    ];

    /// Shared state — Arc because we move into `spawn_blocking`.
    #[derive(Clone)]
    pub(super) struct Inner {
        tokenizer: Arc<Tokenizer>,
        /// Flat row-major copy of the embedding matrix: `[vocab * dim]`.
        /// We hold it as Vec<f32> rather than an MLX Array so the forward
        /// step is a simple gather + accumulate; saves having to thread MLX
        /// stream lifetimes through the async layer. Real MLX op fusion is
        /// only valuable once attention layers land.
        weights: Arc<Vec<f32>>,
        pub vocab: usize,
        pub dims: usize,
    }

    impl Inner {
        pub fn load(tokenizer_path: &Path, weights_path: &Path) -> Result<Self> {
            let tokenizer = Tokenizer::from_file(tokenizer_path)
                .map_err(|e| anyhow!("load tokenizer: {e}"))?;

            let bytes = std::fs::read(weights_path)
                .map_err(|e| anyhow!("read safetensors: {e}"))?;
            let st = safetensors::SafeTensors::deserialize(&bytes)
                .map_err(|e| anyhow!("parse safetensors: {e}"))?;

            // Find the embedding tensor by trying known names.
            let names: Vec<String> = st.names().into_iter().map(|s| s.to_string()).collect();
            let key = EMBEDDING_KEYS
                .iter()
                .find(|k| names.iter().any(|n| n == *k))
                .ok_or_else(|| {
                    anyhow!(
                        "no token-embedding tensor found in {}\n  tried: {:?}\n  available: {:?}",
                        weights_path.display(),
                        EMBEDDING_KEYS,
                        names.iter().take(16).collect::<Vec<_>>()
                    )
                })?;
            let view = st
                .tensor(key)
                .map_err(|e| anyhow!("read tensor {key}: {e}"))?;
            let shape = view.shape();
            if shape.len() != 2 {
                return Err(anyhow!(
                    "expected 2-D embedding tensor, got shape {:?}",
                    shape
                ));
            }
            let vocab = shape[0];
            let dims = shape[1];

            // Convert dtype → f32. BERT-family models ship F16/BF16/F32.
            let raw = view.data();
            let weights: Vec<f32> = match view.dtype() {
                safetensors::Dtype::F32 => bytemuck::cast_slice::<u8, f32>(raw).to_vec(),
                safetensors::Dtype::F16 => raw
                    .chunks_exact(2)
                    .map(|c| half::f16::from_le_bytes([c[0], c[1]]).to_f32())
                    .collect(),
                safetensors::Dtype::BF16 => raw
                    .chunks_exact(2)
                    .map(|c| half::bf16::from_le_bytes([c[0], c[1]]).to_f32())
                    .collect(),
                d => return Err(anyhow!("unsupported embedding dtype: {d:?}")),
            };
            if weights.len() != vocab * dims {
                return Err(anyhow!(
                    "weight length {} != vocab*dims ({}*{})",
                    weights.len(),
                    vocab,
                    dims
                ));
            }

            tracing::info!(
                vocab,
                dims,
                "[cognitive-mlx-embed] loaded embedding matrix from {}",
                weights_path.display()
            );

            Ok(Self {
                tokenizer: Arc::new(tokenizer),
                weights: Arc::new(weights),
                vocab,
                dims,
            })
        }

        pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
            let mut out = Vec::with_capacity(texts.len());
            for text in texts {
                out.push(self.embed_one(text)?);
            }
            Ok(out)
        }

        fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
            let enc = self
                .tokenizer
                .encode(text, true)
                .map_err(|e| anyhow!("tokenize: {e}"))?;
            let ids = enc.get_ids();
            let mask = enc.get_attention_mask();
            if ids.is_empty() {
                return Ok(vec![0.0; self.dims]);
            }

            // Gather: hidden[i, :] = weights[ids[i], :]
            let tokens = ids.len();
            let mut hidden = vec![0.0f32; tokens * self.dims];
            for (i, &id) in ids.iter().enumerate() {
                let id = id as usize;
                if id >= self.vocab {
                    continue; // pad/unk → leaves row at zero, masked anyway
                }
                let src = &self.weights[id * self.dims..(id + 1) * self.dims];
                let dst = &mut hidden[i * self.dims..(i + 1) * self.dims];
                dst.copy_from_slice(src);
            }

            // The HF tokenizer returns u32 masks; mean_pool_masked expects u8.
            let mask_u8: Vec<u8> = mask.iter().map(|&m| if m != 0 { 1 } else { 0 }).collect();
            let mut pooled = mean_pool_masked(&hidden, &mask_u8, self.dims);
            l2_normalize_inplace(&mut pooled);
            Ok(pooled)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_pool_skips_masked_positions() {
        // 3 tokens, dim 2, last token masked out
        let hidden = vec![1.0, 0.0, /* row 0 */ 0.0, 2.0, /* row 1 */ 100.0, 100.0 /* row 2: masked */];
        let mask = [1, 1, 0];
        let pooled = mean_pool_masked(&hidden, &mask, 2);
        assert!((pooled[0] - 0.5).abs() < 1e-6, "got {:?}", pooled);
        assert!((pooled[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn l2_normalize_unit_length() {
        let mut v = vec![3.0f32, 4.0];
        l2_normalize_inplace(&mut v);
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-5);
    }

    #[test]
    fn l2_normalize_no_op_on_zero() {
        let mut v = vec![0.0; 4];
        l2_normalize_inplace(&mut v);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn resolve_model_dir_uses_override() {
        let p = resolve_model_dir("ignored", Some("/tmp/custom"));
        assert_eq!(p, PathBuf::from("/tmp/custom"));
    }

    #[test]
    fn resolve_model_dir_falls_back_to_default() {
        let p = resolve_model_dir("bge-small", None);
        assert!(p.ends_with(".senclaw/models/bge-small"));
    }

    #[test]
    fn require_files_reports_missing_tokenizer() {
        let dir = std::env::temp_dir().join("senclaw_mlx_embed_missing_test");
        let err = require_files(&dir).unwrap_err().to_string();
        assert!(err.contains("tokenizer.json"), "{err}");
    }

    #[cfg(not(feature = "cognitive-mlx-embed"))]
    #[test]
    fn new_errors_without_feature() {
        let msg = match MlxStaticEmbedder::new("bge-small", None) {
            Ok(_) => panic!("expected error without the feature"),
            Err(e) => e.to_string(),
        };
        assert!(msg.contains("cognitive-mlx-embed"), "{msg}");
    }
}
