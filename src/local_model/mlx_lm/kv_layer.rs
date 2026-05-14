//! Per-layer KV cache adapter for Qwen3 native inference.
//!
//! [`Qwen3LayerKv`] wraps [`higgs_cache::SteppingKeyValueCache`] and bridges
//! it to the [`mlx_lm::cache::KeyValueCache`] trait used by the Qwen3 attention module.
//!
//! Dense path (no TurboQuant configured):
//!   - pre-allocated 256-slot stepping buffer, `mlx_slice_update` writes (no per-token concat)
//!   - returns `KvFetchResult::Fp16` → native MLX SDPA on GPU
//!
//! Higgs TurboQuant path (`kv_cache_bits = 3 | 4` in settings.json):
//!   - prefill uses dense fp16 + GPU SDPA (first multi-token call)
//!   - first decode token bulk-quantizes prefill KV on GPU, then stores packed codes
//!   - decode returns `KvFetchResult::TurboQuant` → `turboquant_attention` runs
//!     GPU Metal kernels (`decode_scores` + softmax + `decode_values`)
//!   - activation threshold via `HIGGS_TURBOQUANT_MIN_TOKENS` env var (default 2048)

use mlx_rs::{Array, error::Exception};

use crate::local_model::higgs_cache::{
    KeyValueCache as HiggsKvCache, KvCacheView, SteppingKeyValueCache, TurboQuantKvView,
};
use crate::local_model::higgs_turboquant::{KvCacheConfig, KvCacheMode};
use super::cache::{KeyValueCache, KvFetchResult};

/// Per-layer KV cache for Qwen3.  Bridges [`SteppingKeyValueCache`] to the
/// [`mlx_lm::cache::KeyValueCache`] interface expected by `qwen3::Attention`.
pub struct Qwen3LayerKv {
    inner: SteppingKeyValueCache,
    /// Stored TQ view from the last `update_and_fetch` call; used by `turboquant_attention`.
    last_tq_view: Option<TurboQuantKvView>,
}

impl Qwen3LayerKv {
    /// Dense FP16 stepping cache (no TurboQuant).
    pub fn dense() -> Self {
        Self {
            inner: SteppingKeyValueCache::new(),
            last_tq_view: None,
        }
    }

    /// Higgs GPU TurboQuant cache.
    ///
    /// `bits` must be 3 (key=2bit, value=3bit) or 4 (key=3bit, value=4bit).
    pub fn turbo(bits: u8, n_kv_heads: i32, head_dim: i32) -> Result<Self, Exception> {
        let config = KvCacheConfig {
            mode: KvCacheMode::Turboquant,
            bits,
            norm_correction: true,
            seed: 0,
            ..KvCacheConfig::default()
        };
        Ok(Self {
            inner: SteppingKeyValueCache::new_turbo(config, n_kv_heads, head_dim)?,
            last_tq_view: None,
        })
    }
}

impl KeyValueCache for Qwen3LayerKv {
    fn is_quantized(&self) -> bool {
        self.inner.is_quantized()
    }

    fn bits(&self) -> Option<i32> {
        self.inner.bits()
    }

    fn offset(&self) -> i32 {
        self.inner.offset()
    }

    fn max_size(&self) -> Option<i32> {
        self.inner.max_size()
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        match self.inner.update_and_view(keys, values)? {
            KvCacheView::Dense { keys: k, values: v } => {
                self.last_tq_view = None;
                Ok(KvFetchResult::Fp16(k, v))
            }
            KvCacheView::TurboQuant(tq) => {
                self.last_tq_view = Some(tq);
                Ok(KvFetchResult::TurboQuant)
            }
        }
    }

    /// GPU-accelerated TurboQuant decode attention (Higgs Metal kernels).
    ///
    /// Only valid when the cache has switched to TQ storage (i.e. after
    /// `update_and_fetch` returned `KvFetchResult::TurboQuant`).
    /// Only supports `q_len = 1` (single decode token per step).
    fn turboquant_attention(
        &mut self,
        queries: Array,
        scale: f32,
        _mask: Option<&Array>,
        _batch: i32,
        q_len: i32,
        _kv_past_len: i32,
        n_heads: i32,
        _n_kv_heads: i32,
        _head_dim: i32,
    ) -> Result<Array, Exception> {
        let view = self.last_tq_view.as_ref().ok_or_else(|| {
            Exception::custom(
                "Higgs TQ: turboquant_attention called but no TQ view is stored; \
                 update_and_fetch must be called first and must return TurboQuant",
            )
        })?;

        if q_len != 1 {
            return Err(Exception::custom(format!(
                "Higgs TQ: turboquant_attention supports q_len=1 only (got {q_len}); \
                 prefill uses the dense SDPA path"
            )));
        }

        // queries shape: [1, n_heads, 1, head_dim] (RoPE already applied by Attention)
        // GPU Metal kernel computes dot-products against packed key codes.
        let scores = view.decode_scores(&queries, n_heads)?;
        // scores shape: [n_heads, seq_len]

        let scaled = scores.multiply(Array::from_f32(scale))?;
        // No causal mask needed: all tokens in the TQ view precede (or are) the current
        // query position — the cache never stores future positions.
        let weights = mlx_rs::ops::softmax_axis(&scaled, -1, None)?;

        // GPU Metal kernel: weighted sum of dequantized value codes → [1, n_heads, 1, head_dim]
        view.decode_values(&weights, n_heads)
    }
}
