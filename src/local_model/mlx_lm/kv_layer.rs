//! Per-layer KV cache type for Qwen3 native inference.
//!
//! Without `local-mlx-turboquant`, this is a type alias for [`super::cache::ConcatKeyValueCache`].
//! With `local-mlx-turboquant`, [`Qwen3LayerKv`] can be either FP16 concat or turboquant-rs storage.

#[cfg(not(feature = "local-mlx-turboquant"))]
pub use super::cache::ConcatKeyValueCache as Qwen3LayerKv;

#[cfg(feature = "local-mlx-turboquant")]
pub enum Qwen3LayerKv {
    Concat(super::cache::ConcatKeyValueCache),
    TurboQuant(super::turboquant_kv::TurboQuantKeyValueCache),
}

#[cfg(feature = "local-mlx-turboquant")]
impl super::cache::KeyValueCache for Qwen3LayerKv {
    fn is_quantized(&self) -> bool {
        match self {
            Qwen3LayerKv::Concat(c) => c.is_quantized(),
            Qwen3LayerKv::TurboQuant(t) => t.is_quantized(),
        }
    }

    fn group_size(&self) -> Option<i32> {
        match self {
            Qwen3LayerKv::Concat(c) => c.group_size(),
            Qwen3LayerKv::TurboQuant(t) => t.group_size(),
        }
    }

    fn bits(&self) -> Option<i32> {
        match self {
            Qwen3LayerKv::Concat(c) => c.bits(),
            Qwen3LayerKv::TurboQuant(t) => t.bits(),
        }
    }

    fn offset(&self) -> i32 {
        match self {
            Qwen3LayerKv::Concat(c) => c.offset(),
            Qwen3LayerKv::TurboQuant(t) => t.offset(),
        }
    }

    fn max_size(&self) -> Option<i32> {
        match self {
            Qwen3LayerKv::Concat(c) => c.max_size(),
            Qwen3LayerKv::TurboQuant(t) => t.max_size(),
        }
    }

    fn update_and_fetch(
        &mut self,
        keys: mlx_rs::Array,
        values: mlx_rs::Array,
    ) -> Result<super::cache::KvFetchResult, mlx_rs::error::Exception> {
        match self {
            Qwen3LayerKv::Concat(c) => c.update_and_fetch(keys, values),
            Qwen3LayerKv::TurboQuant(t) => t.update_and_fetch(keys, values),
        }
    }

    fn turboquant_attention(
        &mut self,
        queries: mlx_rs::Array,
        scale: f32,
        mask: Option<&mlx_rs::Array>,
        batch: i32,
        q_len: i32,
        kv_past_len: i32,
        n_heads: i32,
        n_kv_heads: i32,
        head_dim: i32,
    ) -> Result<mlx_rs::Array, mlx_rs::error::Exception> {
        match self {
            Qwen3LayerKv::Concat(c) => c.turboquant_attention(
                queries,
                scale,
                mask,
                batch,
                q_len,
                kv_past_len,
                n_heads,
                n_kv_heads,
                head_dim,
            ),
            Qwen3LayerKv::TurboQuant(t) => t.turboquant_attention(
                queries,
                scale,
                mask,
                batch,
                q_len,
                kv_past_len,
                n_heads,
                n_kv_heads,
                head_dim,
            ),
        }
    }
}
