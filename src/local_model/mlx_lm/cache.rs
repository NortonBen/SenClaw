use std::sync::atomic::{AtomicU64, Ordering};

use mlx_rs::{
    error::Exception,
    ops::{concatenate_axis, zeros_dtype},
    ops::indexing::{IndexOp, TryIndexMutOp},
    transforms::eval,
    Array, Dtype,
};
use turboquant::{
    attention::QuantizedKVCache,
    packed::TurboQuantConfig,
};

/// Default TurboQuant activation threshold (tokens before quantizing KV storage).
///
/// Once activated, every new KV pair routes through the `turboquant-rs` CPU
/// path (f16→f32 cast + heap-allocated quant batch + 4-bit pack). For typical
/// tool-calling prompts (~120 tokens / MCP tool × ~100 tools ≈ 12 K tokens),
/// activating at 2 048 forces the entire post-2 048 chunk through the slow
/// CPU path during **prefill**, easily blowing past the LLM turn timeout.
///
/// Setting this to 16 384 means TQ only kicks in for genuinely long sessions
/// (multi-turn accumulating past 16 K) where the RAM saving is meaningful.
/// Users who want earlier quantization can override `tq_activate_at` in
/// `settings.json`.
pub const DEFAULT_TQ_ACTIVATE_AT: i32 = 16_384;

/// Growth chunk for pre-allocated FP16 KV buffers (mlx_lm / Higgs stepping).
const KV_CACHE_STEP: i32 = 256;

static TQ_SEED: AtomicU64 = AtomicU64::new(0x5eed_c0de);

fn next_tq_seed() -> u64 {
    TQ_SEED.fetch_add(1, Ordering::Relaxed)
}

/// Per-layer cache: FP16 attention KV, TurboQuant attention KV, Mamba-1 SSM state,
/// or Mamba-2 SSM state.
///
/// SSM layers (Mamba-1 / Mamba-2) do not store KV pairs; they carry a fixed-size
/// recurrent state (conv_state + ssm_state). When wrapped in this enum,
/// [`KeyValueCache`] methods for SSM layers behave as a no-op (length reported as
/// tokens-seen, attention ops bypassed). Mamba blocks access the cache directly
/// via [`KvCache::as_mamba1_mut`] / [`KvCache::as_mamba2_mut`].
#[derive(Debug)]
pub enum KvCache {
    Fp16(SteppingKeyValueCache),
    TurboQuant(TurboQuantKeyValueCache),
    Mamba1(Mamba1Cache),
    Mamba2(Mamba2Cache),
    /// Qwen3.5 GatedDeltaNet: conv rolling window + SSM state.
    Qwen35Linear(Qwen35LinearCache),
}

impl KvCache {
    pub fn fp16_with_max(max_seq_len: i32) -> Self {
        Self::Fp16(SteppingKeyValueCache::with_max(max_seq_len))
    }

    pub fn turboquant_with_max(
        bits: u8,
        head_dim: i32,
        n_kv_heads: i32,
        max_seq_len: i32,
        activate_at: i32,
    ) -> Self {
        Self::TurboQuant(TurboQuantKeyValueCache::with_max(
            bits,
            head_dim,
            n_kv_heads,
            max_seq_len,
            activate_at,
        ))
    }

    /// Allocate a Mamba-2 SSM state cache for a single layer.
    pub fn mamba2(conv_dim: i32, d_conv: i32, n_heads: i32, head_dim: i32, d_state: i32) -> Self {
        Self::Mamba2(Mamba2Cache::new(conv_dim, d_conv, n_heads, head_dim, d_state))
    }

    pub fn qwen35_linear(conv_dim: i32, d_conv: i32, n_v_heads: i32, d_v: i32, d_k: i32) -> Self {
        Self::Qwen35Linear(Qwen35LinearCache::new(conv_dim, d_conv, n_v_heads, d_v, d_k))
    }

    /// Allocate a Mamba-1 SSM state cache for a single layer.
    ///
    /// Mamba-1's recurrence is per-channel (no head grouping), so we only need
    /// `d_inner` (= `intermediate_size`) and `d_state` to size the state.
    pub fn mamba1(d_inner: i32, d_conv: i32, d_state: i32) -> Self {
        Self::Mamba1(Mamba1Cache::new(d_inner, d_conv, d_state))
    }

    pub fn as_mamba2_mut(&mut self) -> Option<&mut Mamba2Cache> {
        match self {
            Self::Mamba2(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_qwen35_linear_mut(&mut self) -> Option<&mut Qwen35LinearCache> {
        match self {
            Self::Qwen35Linear(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_mamba1_mut(&mut self) -> Option<&mut Mamba1Cache> {
        match self {
            Self::Mamba1(c) => Some(c),
            _ => None,
        }
    }

    pub fn is_mamba2(&self) -> bool {
        matches!(self, Self::Mamba2(_))
    }

    pub fn is_mamba1(&self) -> bool {
        matches!(self, Self::Mamba1(_))
    }

    pub(crate) fn eval_targets(&self) -> Vec<Array> {
        match self {
            Self::Fp16(c) => c.eval_targets(),
            Self::TurboQuant(c) => c.eval_targets(),
            Self::Mamba1(c) => c.eval_targets(),
            Self::Mamba2(c) => c.eval_targets(),
            Self::Qwen35Linear(c) => c.eval_targets(),
        }
    }

    /// One-line description of cache kind for log lines (e.g. `"fp16"`,
    /// `"tq4"`, `"mamba2"`). Avoids dumping `Debug` impl (verbose, contains
    /// internal `Array` handles).
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Fp16(_) => "fp16",
            Self::TurboQuant(c) => match c.bits {
                3 => "tq3",
                4 => "tq4",
                _ => "tq",
            },
            Self::Mamba1(_) => "mamba1",
            Self::Mamba2(_) => "mamba2",
            Self::Qwen35Linear(_) => "qwen35-linear",
        }
    }

    /// Approximate **per-layer** cache memory (bytes). Sums K + V buffers
    /// (FP16 → 2 B/elt) for attention caches; SSM caches return their state
    /// tensor footprint. Used for `[mem] kv cache: …` log lines so the
    /// operator can see RAM usage growing turn-by-turn.
    pub fn approx_bytes(&self) -> usize {
        match self {
            Self::Fp16(c) => c.approx_bytes(),
            Self::TurboQuant(c) => c.approx_bytes(),
            Self::Mamba1(c) => c.approx_bytes(),
            Self::Mamba2(c) => c.approx_bytes(),
            Self::Qwen35Linear(c) => c.approx_bytes(),
        }
    }
}

/// Sum [`KvCache::approx_bytes`] across all layers + a one-line summary of
/// (cache kind, tokens-stored). Cheap (just shape inspection) so it can run
/// at every prefill/decode milestone.
pub fn summarize_caches(caches: &[Option<KvCache>]) -> (usize, i32, &'static str) {
    let mut total_bytes = 0usize;
    let mut max_stored = 0i32;
    let mut kind: &'static str = "empty";
    for c in caches.iter().flatten() {
        total_bytes += c.approx_bytes();
        max_stored = max_stored.max(c.stored_len());
        kind = c.kind_label();
    }
    (total_bytes, max_stored, kind)
}

impl KeyValueCache for KvCache {
    fn is_quantized(&self) -> bool {
        match self {
            Self::Fp16(c) => c.is_quantized(),
            Self::TurboQuant(c) => c.is_quantized(),
            Self::Mamba1(_) | Self::Mamba2(_) | Self::Qwen35Linear(_) => false,
        }
    }

    fn group_size(&self) -> Option<i32> {
        match self {
            Self::Fp16(c) => c.group_size(),
            Self::TurboQuant(c) => c.group_size(),
            Self::Mamba1(_) | Self::Mamba2(_) | Self::Qwen35Linear(_) => None,
        }
    }

    fn bits(&self) -> Option<i32> {
        match self {
            Self::Fp16(c) => c.bits(),
            Self::TurboQuant(c) => c.bits(),
            Self::Mamba1(_) | Self::Mamba2(_) | Self::Qwen35Linear(_) => None,
        }
    }

    fn stored_len(&self) -> i32 {
        match self {
            Self::Fp16(c) => c.stored_len(),
            Self::TurboQuant(c) => c.stored_len(),
            Self::Mamba1(c) => c.tokens_seen(),
            Self::Mamba2(c) => c.tokens_seen(),
            Self::Qwen35Linear(c) => c.tokens_seen(),
        }
    }

    fn max_size(&self) -> Option<i32> {
        match self {
            Self::Fp16(c) => c.max_size(),
            Self::TurboQuant(c) => c.max_size(),
            Self::Mamba1(_) | Self::Mamba2(_) | Self::Qwen35Linear(_) => None,
        }
    }

    fn update_and_fetch(&mut self, keys: Array, values: Array) -> Result<KvFetchResult, Exception> {
        match self {
            Self::Fp16(c) => c.update_and_fetch(keys, values),
            Self::TurboQuant(c) => c.update_and_fetch(keys, values),
            Self::Mamba1(_) => Err(Exception::custom(
                "Mamba1 cache does not support KV update_and_fetch; \
                 use KvCache::as_mamba1_mut from the Mamba block",
            )),
            Self::Mamba2(_) => Err(Exception::custom(
                "Mamba2 cache does not support KV update_and_fetch; \
                 use KvCache::as_mamba2_mut from the Mamba block",
            )),
            Self::Qwen35Linear(_) => Err(Exception::custom(
                "Qwen3.5 linear cache does not support KV update_and_fetch",
            )),
        }
    }

    fn turboquant_attention(
        &mut self,
        queries: &Array,
        scale: f32,
        mask: Option<&Array>,
        n_heads: i32,
        n_kv_heads: i32,
    ) -> Result<Option<Array>, Exception> {
        match self {
            Self::Fp16(c) => {
                c.turboquant_attention(queries, scale, mask, n_heads, n_kv_heads)
            }
            Self::TurboQuant(c) => {
                c.turboquant_attention(queries, scale, mask, n_heads, n_kv_heads)
            }
            Self::Mamba1(_) | Self::Mamba2(_) | Self::Qwen35Linear(_) => Ok(None),
        }
    }
}

fn materialize_pair(k: Array, v: Array) -> Result<(Array, Array), Exception> {
    eval(&[k.clone(), v.clone()])?;
    Ok((k, v))
}

pub fn eval_all_caches(caches: &mut [Option<KvCache>]) -> Result<(), Exception> {
    let mut batch = Vec::new();
    for cache in caches.iter_mut().flatten() {
        batch.extend(cache.eval_targets());
    }
    if !batch.is_empty() {
        eval(&batch)?;
    }
    Ok(())
}

/// Normalize settings `kv_cache_bits` (2 → TQ3).
pub fn normalize_turboquant_bits(bits: u8) -> u8 {
    match bits {
        4 => 4,
        2 | 3 => 3,
        _ => 3,
    }
}

pub trait KeyValueCache {
    fn is_quantized(&self) -> bool {
        false
    }

    fn group_size(&self) -> Option<i32> {
        None
    }

    fn bits(&self) -> Option<i32> {
        None
    }

    /// Tokens currently in cache (for attention mask width), not RoPE position.
    fn stored_len(&self) -> i32;

    fn max_size(&self) -> Option<i32>;

    fn update_and_fetch(&mut self, keys: Array, values: Array)
        -> Result<KvFetchResult, Exception>;

    /// When TurboQuant storage is active, run approximate GQA attention on CPU.
    fn turboquant_attention(
        &mut self,
        _queries: &Array,
        _scale: f32,
        _mask: Option<&Array>,
        _n_heads: i32,
        _n_kv_heads: i32,
    ) -> Result<Option<Array>, Exception> {
        Ok(None)
    }
}

impl<T> KeyValueCache for &'_ mut T
where
    T: KeyValueCache,
{
    fn is_quantized(&self) -> bool {
        T::is_quantized(self)
    }

    fn group_size(&self) -> Option<i32> {
        T::group_size(self)
    }

    fn bits(&self) -> Option<i32> {
        T::bits(self)
    }

    fn stored_len(&self) -> i32 {
        T::stored_len(self)
    }

    fn max_size(&self) -> Option<i32> {
        T::max_size(self)
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        T::update_and_fetch(self, keys, values)
    }

    fn turboquant_attention(
        &mut self,
        queries: &Array,
        scale: f32,
        mask: Option<&Array>,
        n_heads: i32,
        n_kv_heads: i32,
    ) -> Result<Option<Array>, Exception> {
        T::turboquant_attention(self, queries, scale, mask, n_heads, n_kv_heads)
    }
}

#[derive(Debug, Clone)]
pub struct QuantizedKeys {
    pub keys: Array,
    pub scales: Array,
    pub biases: Array,
}

#[derive(Debug, Clone)]
pub struct QuantizedValues {
    pub values: Array,
    pub scales: Array,
    pub biases: Array,
}

#[derive(Debug)]
pub enum KvFetchResult {
    Fp16(Array, Array),
    /// Attention uses [`super::utils::turboquant_attn::turboquant_gqa_attention`].
    TurboQuant,
}

/// FP16 KV: `slice_update` writes + grow-by-256 (unbounded) or single alloc of `max` (bounded).
///
/// RoPE positions come from the **caller** (`ModelInput::rope_offset`), not this struct.
#[derive(Debug, Clone)]
pub struct SteppingKeyValueCache {
    keys: Option<Array>,
    values: Option<Array>,
    stored_len: i32,
    max_seq_len: Option<i32>,
    step: i32,
}

pub type ConcatKeyValueCache = SteppingKeyValueCache;

impl Default for SteppingKeyValueCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SteppingKeyValueCache {
    pub fn new() -> Self {
        Self {
            keys: None,
            values: None,
            stored_len: 0,
            max_seq_len: None,
            step: KV_CACHE_STEP,
        }
    }

    pub fn with_max(max_seq_len: i32) -> Self {
        Self {
            keys: None,
            values: None,
            stored_len: 0,
            max_seq_len: Some(max_seq_len.max(1)),
            step: KV_CACHE_STEP,
        }
    }

    pub fn seq_len(&self) -> i32 {
        self.stored_len
    }

    pub(crate) fn eval_targets(&self) -> Vec<Array> {
        let mut out = Vec::with_capacity(2);
        if let Some(k) = &self.keys {
            out.push(k.clone());
        }
        if let Some(v) = &self.values {
            out.push(v.clone());
        }
        out
    }

    /// Bytes used by the K + V buffers for this layer. Assumes the FP16 KV
    /// stepping layout (`numel × 2 bytes`) — actual element size could be
    /// BF16/FP16 which are both 2 B, so the figure is correct in practice.
    pub(crate) fn approx_bytes(&self) -> usize {
        fn array_bytes(a: &Array) -> usize {
            let n: usize = a.shape().iter().map(|&d| d.max(0) as usize).product();
            n * 2 // f16 / bf16 = 2 B per element
        }
        let mut total = 0usize;
        if let Some(k) = &self.keys {
            total += array_bytes(k);
        }
        if let Some(v) = &self.values {
            total += array_bytes(v);
        }
        total
    }

    pub fn trim_by(&mut self, n: usize) {
        let trim = i32::try_from(n).unwrap_or(i32::MAX);
        if trim <= 0 {
            return;
        }
        let new_len = self.stored_len.saturating_sub(trim);
        if new_len < self.stored_len {
            if let (Some(k), Some(v)) = (&self.keys, &self.values) {
                if let (Ok(k2), Ok(v2)) = (slice_axis2(k, 0, new_len), slice_axis2(v, 0, new_len))
                {
                    self.keys = Some(k2);
                    self.values = Some(v2);
                }
            }
            self.stored_len = new_len;
        }
    }

    fn dim(shape: &[i32], i: usize, label: &'static str) -> Result<i32, Exception> {
        shape
            .get(i)
            .copied()
            .ok_or_else(|| Exception::custom(format!("KV cache: missing dim {i} ({label})")))
    }

    fn update_dense(&mut self, keys: &Array, values: &Array) -> Result<(Array, Array), Exception> {
        let k_shape = keys.shape();
        let v_shape = values.shape();
        let new_tokens = Self::dim(k_shape, 2, "keys T")?;

        let max_cap = self.max_seq_len;
        let target_stored = match max_cap {
            Some(m) => (self.stored_len + new_tokens).min(m),
            None => self.stored_len + new_tokens,
        };

        let drop = (self.stored_len + new_tokens - target_stored).max(0);
        if drop > 0 {
            if let (Some(k), Some(v)) = (&self.keys, &self.values) {
                self.keys = Some(slice_axis2(k, drop, self.stored_len)?);
                self.values = Some(slice_axis2(v, drop, self.stored_len)?);
            }
            self.stored_len -= drop;
        }

        let write_pos = self.stored_len;
        let required_slots = write_pos + new_tokens;

        let need_alloc = self.keys.is_none();
        let need_grow = match max_cap {
            Some(_) => need_alloc,
            None => match self.keys.as_ref() {
                None => true,
                Some(k) => Self::dim(k.shape(), 2, "cached keys T")? < required_slots,
            },
        };

        if need_alloc || need_grow {
            let b = Self::dim(k_shape, 0, "keys B")?;
            let n_kv_heads = Self::dim(k_shape, 1, "keys H")?;
            let k_head_dim = Self::dim(k_shape, 3, "keys D")?;
            let v_head_dim = Self::dim(v_shape, 3, "values D")?;

            let new_slots = match max_cap {
                Some(m) => m,
                None => {
                    let n_steps = (self.step + new_tokens - 1) / self.step;
                    let grow = n_steps * self.step;
                    match self.keys.as_ref() {
                        Some(k) => {
                            let cap = Self::dim(k.shape(), 2, "cached keys T")?;
                            (cap + grow).max(required_slots)
                        }
                        None => grow.max(required_slots),
                    }
                }
            };

            let new_k = zeros_dtype(&[b, n_kv_heads, new_slots, k_head_dim], keys.dtype())?;
            let new_v = zeros_dtype(&[b, n_kv_heads, new_slots, v_head_dim], values.dtype())?;

            let (grown_k, grown_v) = match (self.keys.take(), self.values.take()) {
                (Some(old_k), Some(old_v)) => {
                    let trimmed_k = slice_axis2(&old_k, 0, self.stored_len)?;
                    let trimmed_v = slice_axis2(&old_v, 0, self.stored_len)?;
                    (
                        concatenate_axis(&[trimmed_k, new_k], 2)?,
                        concatenate_axis(&[trimmed_v, new_v], 2)?,
                    )
                }
                _ => (new_k, new_v),
            };
            self.keys = Some(grown_k);
            self.values = Some(grown_v);
        }

        let k_buf = self
            .keys
            .as_ref()
            .ok_or_else(|| Exception::custom("Keys cannot be None after grow"))?;
        let v_buf = self
            .values
            .as_ref()
            .ok_or_else(|| Exception::custom("Values cannot be None after grow"))?;

        self.keys = Some(slice_update_axis2(k_buf, keys, write_pos, new_tokens)?);
        self.values = Some(slice_update_axis2(v_buf, values, write_pos, new_tokens)?);

        self.stored_len += new_tokens;

        let result_k = slice_axis2(
            self.keys
                .as_ref()
                .ok_or_else(|| Exception::custom("Keys cannot be None after update"))?,
            0,
            self.stored_len,
        )?;
        let result_v = slice_axis2(
            self.values
                .as_ref()
                .ok_or_else(|| Exception::custom("Values cannot be None after update"))?,
            0,
            self.stored_len,
        )?;

        Ok((result_k, result_v))
    }
}

impl KeyValueCache for SteppingKeyValueCache {
    fn stored_len(&self) -> i32 {
        self.stored_len
    }

    fn max_size(&self) -> Option<i32> {
        self.max_seq_len
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        let (k, v) = self.update_dense(&keys, &values)?;
        let (k, v) = materialize_pair(k, v)?;
        Ok(KvFetchResult::Fp16(k, v))
    }
}

/// TurboQuant KV per layer (one `QuantizedKVCache` with `num_layers = 1`).
///
/// Before `activate_at` tokens (sum of `stored_len` updates), uses FP16 [`SteppingKeyValueCache`].
/// After activation, pushes packed TQ blocks and serves attention via turboquant-rs.
pub struct TurboQuantKeyValueCache {
    staging: SteppingKeyValueCache,
    tq: QuantizedKVCache,
    active: bool,
    activate_at: i32,
    bits: u8,
    head_dim: i32,
    n_kv_heads: i32,
}

impl std::fmt::Debug for TurboQuantKeyValueCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurboQuantKeyValueCache")
            .field("active", &self.active)
            .field("activate_at", &self.activate_at)
            .field("bits", &self.bits)
            .field("stored_len", &self.stored_len())
            .finish()
    }
}

impl TurboQuantKeyValueCache {
    pub fn with_max(
        bits: u8,
        head_dim: i32,
        n_kv_heads: i32,
        max_seq_len: i32,
        activate_at: i32,
    ) -> Self {
        let bits = normalize_turboquant_bits(bits);
        let config = TurboQuantConfig::new(bits, head_dim as usize)
            .expect("TurboQuantConfig::new validated at runtime");
        Self {
            staging: SteppingKeyValueCache::with_max(max_seq_len),
            tq: QuantizedKVCache::new(config, 1, next_tq_seed()),
            active: false,
            activate_at: activate_at.max(0),
            bits,
            head_dim,
            n_kv_heads,
        }
    }

    pub fn tq(&self) -> &QuantizedKVCache {
        &self.tq
    }

    pub fn head_dim(&self) -> i32 {
        self.head_dim
    }

    pub fn is_turbo_active(&self) -> bool {
        self.active
    }

    fn tokens_in_tq(&self) -> i32 {
        if !self.active {
            return 0;
        }
        let entries = self.tq.entry_count(0);
        (entries / self.n_kv_heads as usize) as i32
    }

    fn maybe_activate(&mut self) -> Result<(), Exception> {
        if self.active {
            return Ok(());
        }
        if self.staging.stored_len < self.activate_at {
            return Ok(());
        }
        if let (Some(k), Some(v)) = (
            self.staging.keys.as_ref(),
            self.staging.values.as_ref(),
        ) {
            let k = slice_axis2(k, 0, self.staging.stored_len)?;
            let v = slice_axis2(v, 0, self.staging.stored_len)?;
            push_kv_arrays(&mut self.tq, 0, &k, &v, self.n_kv_heads)?;
        }
        self.staging.keys = None;
        self.staging.values = None;
        self.staging.stored_len = 0;
        self.active = true;
        Ok(())
    }

    fn trim_tq_if_needed(&mut self) -> Result<(), Exception> {
        let Some(max) = self.staging.max_seq_len else {
            return Ok(());
        };
        let max = max as usize;
        let n_h = self.n_kv_heads as usize;
        let n = self.tq.entry_count(0);
        let tokens = n / n_h;
        if tokens <= max {
            return Ok(());
        }
        let drop_tokens = tokens - max;
        let drop_entries = drop_tokens * n_h;
        let keys = self
            .tq
            .dequantize_keys_range(0, drop_entries, n)
            .map_err(|e| Exception::custom(format!("tq trim keys: {e}")))?;
        let vals = self
            .tq
            .dequantize_values_range(0, drop_entries, n)
            .map_err(|e| Exception::custom(format!("tq trim values: {e}")))?;
        let config = TurboQuantConfig::new(self.bits, self.head_dim as usize)
            .map_err(|e| Exception::custom(format!("tq trim TurboQuantConfig::new: {e}")))?;
        let seed = self.tq.qjl_seed();
        let mut fresh = QuantizedKVCache::new(config, 1, seed);
        let key_refs: Vec<&[f32]> = keys.iter().map(|v| v.as_slice()).collect();
        let val_refs: Vec<&[f32]> = vals.iter().map(|v| v.as_slice()).collect();
        fresh
            .push_batch(0, &key_refs, &val_refs)
            .map_err(|e| Exception::custom(format!("tq trim re-push: {e}")))?;
        self.tq = fresh;
        Ok(())
    }

    pub(crate) fn eval_targets(&self) -> Vec<Array> {
        if self.active {
            Vec::new()
        } else {
            self.staging.eval_targets()
        }
    }

    /// Per-layer bytes. While staging (pre-activation) the storage matches
    /// FP16 KV; once active the storage is TQ packed at `bits` bits per
    /// element (plus small header / scale overhead, ignored here — rough
    /// estimate only). Used for `[mem] kv cache:` log lines.
    pub(crate) fn approx_bytes(&self) -> usize {
        if !self.active {
            return self.staging.approx_bytes();
        }
        let entries = self.tq.entry_count(0);
        // bits per element, K + V = 2× the per-element cost
        let bytes_per_elem = self.head_dim as usize * self.bits as usize / 8;
        // entries are (tokens × heads); each entry holds head_dim packed elems
        // for one of K or V (entry_count already counts K and V separately).
        entries * bytes_per_elem
    }
}

impl KeyValueCache for TurboQuantKeyValueCache {
    fn is_quantized(&self) -> bool {
        self.active
    }

    fn bits(&self) -> Option<i32> {
        self.active.then_some(self.bits as i32)
    }

    fn stored_len(&self) -> i32 {
        if self.active {
            self.tokens_in_tq()
        } else {
            self.staging.stored_len()
        }
    }

    fn max_size(&self) -> Option<i32> {
        self.staging.max_seq_len
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        if !self.active {
            let out = self.staging.update_and_fetch(keys.clone(), values.clone())?;
            let KvFetchResult::Fp16(k, v) = out else {
                return Err(Exception::custom("staging must return FP16"));
            };
            self.maybe_activate()?;
            if self.active {
                push_kv_arrays(&mut self.tq, 0, &k, &v, self.n_kv_heads)?;
                self.trim_tq_if_needed()?;
                return Ok(KvFetchResult::TurboQuant);
            }
            return Ok(KvFetchResult::Fp16(k, v));
        }
        push_kv_arrays(&mut self.tq, 0, &keys, &values, self.n_kv_heads)?;
        self.trim_tq_if_needed()?;
        Ok(KvFetchResult::TurboQuant)
    }

    fn turboquant_attention(
        &mut self,
        queries: &Array,
        scale: f32,
        mask: Option<&Array>,
        n_heads: i32,
        n_kv_heads: i32,
    ) -> Result<Option<Array>, Exception> {
        if !self.active {
            return Ok(None);
        }
        super::utils::turboquant_attn::turboquant_gqa_attention(
            queries,
            self,
            scale,
            mask,
            n_heads,
            n_kv_heads,
        )
        .map(Some)
    }
}

fn push_kv_arrays(
    tq: &mut QuantizedKVCache,
    layer: usize,
    keys: &Array,
    values: &Array,
    n_kv_heads: i32,
) -> Result<(), Exception> {
    eval(&[keys.clone(), values.clone()])?;
    let k = keys.as_dtype(Dtype::Float32)?;
    let v = values.as_dtype(Dtype::Float32)?;
    let sh = k.shape();
    if sh.len() != 4 {
        return Err(Exception::custom("push_kv_arrays: keys must be 4D [B,H,T,D]"));
    }
    let t = sh[2] as usize;
    let h = n_kv_heads as usize;
    let d = sh[3] as usize;
    let k_flat = k.as_slice::<f32>();
    let v_flat = v.as_slice::<f32>();
    let mut key_bufs = Vec::with_capacity(t * h);
    let mut val_bufs = Vec::with_capacity(t * h);
    for ti in 0..t {
        for hi in 0..h {
            let start = (hi * t + ti) * d;
            key_bufs.push(k_flat[start..start + d].to_vec());
            val_bufs.push(v_flat[start..start + d].to_vec());
        }
    }
    let key_refs: Vec<&[f32]> = key_bufs.iter().map(|s| s.as_slice()).collect();
    let val_refs: Vec<&[f32]> = val_bufs.iter().map(|s| s.as_slice()).collect();
    tq.push_batch(layer, &key_refs, &val_refs)
        .map_err(|e| Exception::custom(format!("turboquant push_batch: {e}")))
}

fn slice_axis2(arr: &Array, start: i32, end: i32) -> Result<Array, Exception> {
    Ok(arr.index((.., .., start..end, ..)))
}

fn slice_update_axis2(
    target: &Array,
    update: &Array,
    start: i32,
    n: i32,
) -> Result<Array, Exception> {
    let mut out = target.clone();
    out.try_index_mut((.., .., start..start + n, ..), update.clone())?;
    Ok(out)
}

/// Mamba-2 per-layer recurrent state.
///
/// Two pieces of state are carried across timesteps (mirroring `mlx-lm`'s
/// `mamba2.py` reference):
///
/// - **conv_state**: rolling window into the depthwise short conv, shape
///   `[B, d_conv - 1, conv_dim]` where `conv_dim = d_inner + 2 * n_groups * d_state`.
///   Stored channels-last to match `mlx_rs::nn::Conv1d` NLC layout, so the block
///   can `concatenate_axis(&[conv_state, xBC_token], 1)` directly.
/// - **ssm_state**: per-head SSM hidden state, shape `[B, n_heads, head_dim, d_state]`.
///   On the very first prefill call the cache is zero-initialised lazily so the
///   block can support arbitrary batch sizes without re-allocating up front.
///
/// `tokens_seen` tracks total absolute position so callers can advance positional
/// state if needed (mirrors `RopeInput::offset` semantics for SSM layers).
#[derive(Debug)]
pub struct Mamba2Cache {
    pub conv_dim: i32,
    pub d_conv: i32,
    pub n_heads: i32,
    pub head_dim: i32,
    pub d_state: i32,
    conv_state: Option<Array>,
    ssm_state: Option<Array>,
    tokens_seen: i32,
}

impl Mamba2Cache {
    pub fn new(conv_dim: i32, d_conv: i32, n_heads: i32, head_dim: i32, d_state: i32) -> Self {
        Self {
            conv_dim,
            d_conv,
            n_heads,
            head_dim,
            d_state,
            conv_state: None,
            ssm_state: None,
            tokens_seen: 0,
        }
    }

    pub fn tokens_seen(&self) -> i32 {
        self.tokens_seen
    }

    pub fn advance(&mut self, n: i32) {
        self.tokens_seen = self.tokens_seen.saturating_add(n);
    }

    /// Channels-last conv state of length `d_conv - 1`, lazily zero-initialised.
    pub fn conv_state_or_init(&mut self, batch: i32, dtype: Dtype) -> Result<&Array, Exception> {
        if self.conv_state.is_none() {
            let pad = (self.d_conv - 1).max(0);
            self.conv_state = Some(zeros_dtype(&[batch, pad, self.conv_dim], dtype)?);
        }
        Ok(self
            .conv_state
            .as_ref()
            .expect("conv_state initialised above"))
    }

    pub fn set_conv_state(&mut self, state: Array) {
        self.conv_state = Some(state);
    }

    /// Per-head SSM hidden state `[B, n_heads, head_dim, d_state]`, lazily init.
    pub fn ssm_state_or_init(&mut self, batch: i32, dtype: Dtype) -> Result<&Array, Exception> {
        if self.ssm_state.is_none() {
            self.ssm_state = Some(zeros_dtype(
                &[batch, self.n_heads, self.head_dim, self.d_state],
                dtype,
            )?);
        }
        Ok(self
            .ssm_state
            .as_ref()
            .expect("ssm_state initialised above"))
    }

    pub fn set_ssm_state(&mut self, state: Array) {
        self.ssm_state = Some(state);
    }

    pub(crate) fn eval_targets(&self) -> Vec<Array> {
        let mut out = Vec::with_capacity(2);
        if let Some(c) = &self.conv_state {
            out.push(c.clone());
        }
        if let Some(s) = &self.ssm_state {
            out.push(s.clone());
        }
        out
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        fn array_bytes(a: &Array) -> usize {
            let n: usize = a.shape().iter().map(|&d| d.max(0) as usize).product();
            n * 2
        }
        let mut total = 0;
        if let Some(c) = &self.conv_state { total += array_bytes(c); }
        if let Some(s) = &self.ssm_state { total += array_bytes(s); }
        total
    }
}

/// Mamba-1 per-layer recurrent state.
///
/// Two pieces of state are carried across timesteps, matching the reference
/// `mlx-lm` `mamba.py` (`ArraysCache(size=2)` slots):
///
/// - **conv_state**: rolling window into the depthwise short conv,
///   shape `[B, d_conv - 1, d_inner]`. Stored channels-last to match the
///   `mlx_rs::nn::Conv1d` NLC layout so the block can
///   `concatenate_axis(&[conv_state, x_inner_token], 1)` directly.
/// - **ssm_state**: per-channel SSM hidden state, shape `[B, d_inner, d_state]`.
///   Unlike Mamba-2, Mamba-1 has no head/group structure — every channel of
///   `d_inner` carries its own `d_state`-wide hidden state.
///
/// Both slots are lazily zero-initialised on the first prefill call so the
/// block can support arbitrary batch sizes without re-allocating up front.
/// `tokens_seen` tracks total absolute position (mirrors [`Mamba2Cache`]).
#[derive(Debug)]
pub struct Mamba1Cache {
    pub d_inner: i32,
    pub d_conv: i32,
    pub d_state: i32,
    conv_state: Option<Array>,
    ssm_state: Option<Array>,
    tokens_seen: i32,
}

impl Mamba1Cache {
    pub fn new(d_inner: i32, d_conv: i32, d_state: i32) -> Self {
        Self {
            d_inner,
            d_conv,
            d_state,
            conv_state: None,
            ssm_state: None,
            tokens_seen: 0,
        }
    }

    pub fn tokens_seen(&self) -> i32 {
        self.tokens_seen
    }

    pub fn advance(&mut self, n: i32) {
        self.tokens_seen = self.tokens_seen.saturating_add(n);
    }

    /// Channels-last conv state of length `d_conv - 1`, lazily zero-initialised.
    pub fn conv_state_or_init(&mut self, batch: i32, dtype: Dtype) -> Result<&Array, Exception> {
        if self.conv_state.is_none() {
            let pad = (self.d_conv - 1).max(0);
            self.conv_state = Some(zeros_dtype(&[batch, pad, self.d_inner], dtype)?);
        }
        Ok(self
            .conv_state
            .as_ref()
            .expect("conv_state initialised above"))
    }

    pub fn set_conv_state(&mut self, state: Array) {
        self.conv_state = Some(state);
    }

    /// Has the SSM hidden state been populated yet (i.e. seen any tokens)?
    pub fn has_ssm_state(&self) -> bool {
        self.ssm_state.is_some()
    }

    /// Per-channel SSM hidden state `[B, d_inner, d_state]`. Returns `None` until
    /// the first token has been processed — Mamba-1's recurrence skips the
    /// `state * exp(dt * A)` decay term on the very first step (matching
    /// `state is not None` in the Python reference).
    pub fn ssm_state(&self) -> Option<&Array> {
        self.ssm_state.as_ref()
    }

    pub fn set_ssm_state(&mut self, state: Array) {
        self.ssm_state = Some(state);
    }

    pub(crate) fn eval_targets(&self) -> Vec<Array> {
        let mut out = Vec::with_capacity(2);
        if let Some(c) = &self.conv_state {
            out.push(c.clone());
        }
        if let Some(s) = &self.ssm_state {
            out.push(s.clone());
        }
        out
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        fn array_bytes(a: &Array) -> usize {
            let n: usize = a.shape().iter().map(|&d| d.max(0) as usize).product();
            n * 2
        }
        let mut total = 0;
        if let Some(c) = &self.conv_state { total += array_bytes(c); }
        if let Some(s) = &self.ssm_state { total += array_bytes(s); }
        total
    }
}

/// Qwen3.5 GatedDeltaNet recurrent state (conv window + linear SSM).
#[derive(Debug)]
pub struct Qwen35LinearCache {
    pub conv_dim: i32,
    pub d_conv: i32,
    pub n_v_heads: i32,
    pub d_v: i32,
    pub d_k: i32,
    conv_state: Option<Array>,
    ssm_state: Option<Array>,
    tokens_seen: i32,
}

impl Qwen35LinearCache {
    pub fn new(conv_dim: i32, d_conv: i32, n_v_heads: i32, d_v: i32, d_k: i32) -> Self {
        Self {
            conv_dim,
            d_conv,
            n_v_heads,
            d_v,
            d_k,
            conv_state: None,
            ssm_state: None,
            tokens_seen: 0,
        }
    }

    pub fn tokens_seen(&self) -> i32 {
        self.tokens_seen
    }

    pub fn advance(&mut self, n: i32) {
        self.tokens_seen = self.tokens_seen.saturating_add(n);
    }

    pub fn conv_state_or_init(&mut self, batch: i32, dtype: Dtype) -> Result<&Array, Exception> {
        if self.conv_state.is_none() {
            let pad = (self.d_conv - 1).max(0);
            self.conv_state = Some(zeros_dtype(&[batch, pad, self.conv_dim], dtype)?);
        }
        Ok(self.conv_state.as_ref().expect("conv_state"))
    }

    pub fn set_conv_state(&mut self, state: Array) {
        self.conv_state = Some(state);
    }

    pub fn ssm_state_or_init(&mut self, batch: i32) -> Result<&Array, Exception> {
        if self.ssm_state.is_none() {
            self.ssm_state = Some(zeros_dtype(
                &[batch, self.n_v_heads, self.d_v, self.d_k],
                Dtype::Float32,
            )?);
        }
        Ok(self.ssm_state.as_ref().expect("ssm_state"))
    }

    pub fn set_ssm_state(&mut self, state: Array) {
        self.ssm_state = Some(state);
    }

    pub(crate) fn eval_targets(&self) -> Vec<Array> {
        let mut out = Vec::with_capacity(2);
        if let Some(c) = &self.conv_state {
            out.push(c.clone());
        }
        if let Some(s) = &self.ssm_state {
            out.push(s.clone());
        }
        out
    }

    pub(crate) fn approx_bytes(&self) -> usize {
        fn array_bytes(a: &Array) -> usize {
            let n: usize = a.shape().iter().map(|&d| d.max(0) as usize).product();
            // ssm_state for Qwen3.5 is FP32 (4 B); conv is FP16. Approximate as 2 B avg.
            n * 2
        }
        let mut total = 0;
        if let Some(c) = &self.conv_state { total += array_bytes(c); }
        if let Some(s) = &self.ssm_state { total += array_bytes(s); }
        total
    }
}

#[cfg(all(test, feature = "local-mlx"))]
mod tests {
    use super::*;
    use mlx_rs::{array, Array};

    fn make_kv(seq_len: i32, fill: f32) -> (Array, Array) {
        let t = Array::full::<f32>(&[1, 1, seq_len, 1], array!(fill)).unwrap();
        (t.clone(), t)
    }

    #[test]
    fn stepping_window_evicts() {
        let mut cache = SteppingKeyValueCache::with_max(2);
        for step in 0..4 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
        }
        assert_eq!(cache.stored_len(), 2);
        let keys = cache.keys.as_ref().unwrap();
        assert_eq!(keys.index((.., .., 0, ..)).item::<f32>(), 2.0);
        assert_eq!(keys.index((.., .., 1, ..)).item::<f32>(), 3.0);
    }
}
