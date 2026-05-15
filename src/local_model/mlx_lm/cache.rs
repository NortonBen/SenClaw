use mlx_rs::{
    error::Exception,
    ops::{concatenate_axis, dequantize, quantize, reshape},
    ops::indexing::IndexOp,
    transforms::eval,
    Array,
};

/// Default MLX `quantize` group size (must divide `head_dim`; Qwen3 uses 128).
pub const DEFAULT_MLX_KV_GROUP_SIZE: i32 = 64;

/// Unified per-layer KV cache: FP16 concat or MLX packed-quant (Metal).
#[derive(Debug, Clone)]
pub enum KvCache {
    Fp16(ConcatKeyValueCache),
    Quantized(MlxQuantizedConcatKeyValueCache),
}

impl KvCache {
    pub fn fp16_with_max(max_seq_len: i32) -> Self {
        Self::Fp16(ConcatKeyValueCache::with_max(max_seq_len))
    }

    pub fn quantized_with_max(group_size: i32, bits: i32, max_seq_len: i32) -> Self {
        Self::Quantized(MlxQuantizedConcatKeyValueCache::with_max(
            group_size, bits, max_seq_len,
        ))
    }
}

impl KeyValueCache for KvCache {
    fn is_quantized(&self) -> bool {
        match self {
            Self::Fp16(c) => c.is_quantized(),
            Self::Quantized(c) => c.is_quantized(),
        }
    }

    fn group_size(&self) -> Option<i32> {
        match self {
            Self::Fp16(c) => c.group_size(),
            Self::Quantized(c) => c.group_size(),
        }
    }

    fn bits(&self) -> Option<i32> {
        match self {
            Self::Fp16(c) => c.bits(),
            Self::Quantized(c) => c.bits(),
        }
    }

    fn offset(&self) -> i32 {
        match self {
            Self::Fp16(c) => c.offset(),
            Self::Quantized(c) => c.offset(),
        }
    }

    fn max_size(&self) -> Option<i32> {
        match self {
            Self::Fp16(c) => c.max_size(),
            Self::Quantized(c) => c.max_size(),
        }
    }

    fn update_and_fetch(&mut self, keys: Array, values: Array) -> Result<KvFetchResult, Exception> {
        match self {
            Self::Fp16(c) => c.update_and_fetch(keys, values),
            Self::Quantized(c) => c.update_and_fetch(keys, values),
        }
    }
}

/// Materialize lazy MLX arrays so prior concat/slice nodes can be freed.
fn materialize_pair(k: Array, v: Array) -> Result<(Array, Array), Exception> {
    eval(&[k.clone(), v.clone()])?;
    Ok((k, v))
}

/// Evaluate all layer caches — call after each forward to cap peak RAM (MLX lazy graphs).
pub fn eval_all_caches(caches: &mut [Option<KvCache>]) -> Result<(), Exception> {
    let mut batch = Vec::new();
    for cache in caches.iter_mut().flatten() {
        match cache {
            KvCache::Fp16(c) => {
                if let Some(k) = c.keys.as_ref() {
                    batch.push(k.clone());
                }
                if let Some(v) = c.values.as_ref() {
                    batch.push(v.clone());
                }
            }
            KvCache::Quantized(c) => {
                if let Some(k) = c.keys_packed.as_ref() {
                    batch.push(k.clone());
                }
                if let Some(s) = c.keys_scales.as_ref() {
                    batch.push(s.clone());
                }
                if let Some(b) = c.keys_biases.as_ref() {
                    batch.push(b.clone());
                }
                if let Some(v) = c.values_packed.as_ref() {
                    batch.push(v.clone());
                }
                if let Some(s) = c.values_scales.as_ref() {
                    batch.push(s.clone());
                }
                if let Some(b) = c.values_biases.as_ref() {
                    batch.push(b.clone());
                }
            }
        }
    }
    if !batch.is_empty() {
        eval(&batch)?;
    }
    Ok(())
}

// TODO: somehow move quantized methods to a separate trait?
pub trait KeyValueCache {
    fn is_quantized(&self) -> bool {
        false
    }

    /// Returns the group size used for quantization. `None` if not quantized.
    fn group_size(&self) -> Option<i32> {
        None
    }

    /// Returns the number of bits used for quantization. `None` if not quantized.
    fn bits(&self) -> Option<i32> {
        None
    }

    fn offset(&self) -> i32;

    fn max_size(&self) -> Option<i32>;

    fn update_and_fetch(&mut self, keys: Array, values: Array)
        -> Result<KvFetchResult, Exception>;
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

    fn offset(&self) -> i32 {
        T::offset(self)
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
}

/// Packed KV tensors for MLX `quantized_scaled_dot_product_attention` (see `mlx_lm::utils`).
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

/// Result of appending one step to the KV cache for attention.
#[derive(Debug)]
pub enum KvFetchResult {
    Fp16(Array, Array),
    Quantized {
        keys: QuantizedKeys,
        values: QuantizedValues,
    },
}

/// Per-layer FP16 KV cache with optional sliding-window cap.
///
/// Shape: `[batch, n_kv_heads, seq_len, head_dim]`.  `offset` is the **absolute**
/// RoPE position (total tokens processed); it is not reset when old entries are
/// evicted.  `max_seq_len`, when set, bounds stored `seq_len` by dropping the
/// oldest tokens from the front.
#[derive(Debug, Clone)]
pub struct ConcatKeyValueCache {
    keys: Option<Array>,
    values: Option<Array>,
    /// Cumulative tokens seen (RoPE); not truncated by the sliding window.
    offset: i32,
    max_seq_len: Option<i32>,
}

impl Default for ConcatKeyValueCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ConcatKeyValueCache {
    /// Unbounded cache (KV RAM grows with context length).
    pub fn new() -> Self {
        Self {
            keys: None,
            values: None,
            offset: 0,
            max_seq_len: None,
        }
    }

    /// Sliding-window cache: at most `max_seq_len` key/value positions retained.
    pub fn with_max(max_seq_len: i32) -> Self {
        Self {
            keys: None,
            values: None,
            offset: 0,
            max_seq_len: Some(max_seq_len.max(1)),
        }
    }

    /// Tokens currently stored (after eviction), not the absolute RoPE offset.
    pub fn seq_len(&self) -> i32 {
        self.keys
            .as_ref()
            .map(|k| {
                let sh = k.shape();
                sh[sh.len() - 2]
            })
            .unwrap_or(0)
    }

    /// Drop oldest positions so `seq_len <= max_seq_len` on axis -2.
    fn trim_to_max(k: Array, v: Array, max_seq_len: i32) -> Result<(Array, Array), Exception> {
        let sh = k.shape();
        let seq_len = sh[sh.len() - 2];
        if seq_len <= max_seq_len {
            return Ok((k, v));
        }
        let start = seq_len - max_seq_len;
        Ok((
            k.index((.., .., start.., ..)),
            v.index((.., .., start.., ..)),
        ))
    }
}

impl KeyValueCache for ConcatKeyValueCache {
    fn offset(&self) -> i32 {
        self.offset
    }

    fn max_size(&self) -> Option<i32> {
        self.max_seq_len
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        let step_len = keys.shape()[keys.shape().len() - 2];

        let (mut k, mut v) = match (self.keys.take(), self.values.take()) {
            (Some(prev_k), Some(prev_v)) => (
                concatenate_axis(&[prev_k, keys], -2)?,
                concatenate_axis(&[prev_v, values], -2)?,
            ),
            _ => (keys, values),
        };

        if let Some(max) = self.max_seq_len {
            (k, v) = Self::trim_to_max(k, v, max)?;
        }

        self.offset += step_len;
        let (k, v) = materialize_pair(k, v)?;
        self.keys = Some(k.clone());
        self.values = Some(v.clone());

        Ok(KvFetchResult::Fp16(k, v))
    }
}

/// KV cache using MLX `quantize` + [`quantized_scaled_dot_product_attention`] on Metal.
///
/// Stores only packed K/V. Each step: dequantize prior cache → concat FP16 with new
/// tokens → optional sliding trim → re-quantize the full sequence. Per-step quantize-then-concat
/// of packed tensors was incorrect (broken attention / newline loops).
#[derive(Debug, Clone)]
pub struct MlxQuantizedConcatKeyValueCache {
    group_size: i32,
    bits: i32,
    pub(crate) keys_packed: Option<Array>,
    pub(crate) keys_scales: Option<Array>,
    pub(crate) keys_biases: Option<Array>,
    pub(crate) values_packed: Option<Array>,
    pub(crate) values_scales: Option<Array>,
    pub(crate) values_biases: Option<Array>,
    /// Absolute RoPE offset (tokens processed); not reset on sliding-window eviction.
    offset: i32,
    max_seq_len: Option<i32>,
}

impl MlxQuantizedConcatKeyValueCache {
    pub fn with_max(group_size: i32, bits: i32, max_seq_len: i32) -> Self {
        Self {
            group_size,
            bits,
            keys_packed: None,
            keys_scales: None,
            keys_biases: None,
            values_packed: None,
            values_scales: None,
            values_biases: None,
            offset: 0,
            max_seq_len: Some(max_seq_len.max(1)),
        }
    }

    pub fn seq_len(&self) -> i32 {
        self.keys_packed
            .as_ref()
            .map(|k| {
                let sh = k.shape();
                sh[sh.len() - 2]
            })
            .unwrap_or(0)
    }

    /// Pad row dimension to a multiple of 32 (MLX `quantize` constraint on 2D inputs).
    fn pad_rows(x: &Array, multiple: i32, last_dim: i32) -> Result<(Array, i32), Exception> {
        let sh = x.shape();
        let rows = sh[0];
        let pad = (multiple - (rows % multiple)) % multiple;
        if pad == 0 {
            return Ok((x.clone(), rows));
        }
        let z = Array::zeros::<f32>(&[pad, last_dim])?;
        Ok((concatenate_axis(&[x.clone(), z], 0)?, rows))
    }

    fn quantize_layer(
        x: &Array,
        group_size: i32,
        bits: i32,
    ) -> Result<(Array, Array, Array), Exception> {
        let sh = x.shape();
        let b = sh[0];
        let h = sh[1];
        let l = sh[2];
        let d = sh[3];
        let flat = b * h * l;
        let x2 = reshape(x, &[flat, d])?;
        let (x_pad, orig_rows) = Self::pad_rows(&x2, 32, d)?;
        let (q, s, bia) = quantize(&x_pad, group_size, bits)?;
        let q = q.index((..orig_rows, ..));
        let s = s.index((..orig_rows, ..));
        let bia = bia.index((..orig_rows, ..));
        let pc = q.shape()[1];
        let q4 = reshape(&q, &[b, h, l, pc])?;
        let sg = s.shape()[1];
        let s4 = reshape(&s, &[b, h, l, sg])?;
        let b4 = reshape(&bia, &[b, h, l, sg])?;
        Ok((q4, s4, b4))
    }

    fn dequantize_layer(
        packed: &Array,
        scales: &Array,
        biases: &Array,
        b: i32,
        h: i32,
        l: i32,
        d: i32,
        group_size: i32,
        bits: i32,
    ) -> Result<Array, Exception> {
        let flat = b * h * l;
        let pc = packed.shape()[packed.shape().len() - 1];
        let sg = scales.shape()[scales.shape().len() - 1];
        let flat_packed = reshape(packed, &[flat, pc])?;
        let flat_scales = reshape(scales, &[flat, sg])?;
        let flat_biases = reshape(biases, &[flat, sg])?;
        let dq = dequantize(&flat_packed, &flat_scales, &flat_biases, group_size, bits)?;
        reshape(&dq, &[b, h, l, d])
    }

    fn materialize_triple(
        packed: &Array,
        scales: &Array,
        biases: &Array,
    ) -> Result<(Array, Array, Array), Exception> {
        eval(&[packed.clone(), scales.clone(), biases.clone()])?;
        Ok((packed.clone(), scales.clone(), biases.clone()))
    }
}

impl KeyValueCache for MlxQuantizedConcatKeyValueCache {
    fn is_quantized(&self) -> bool {
        true
    }

    fn group_size(&self) -> Option<i32> {
        Some(self.group_size)
    }

    fn bits(&self) -> Option<i32> {
        Some(self.bits)
    }

    fn offset(&self) -> i32 {
        self.offset
    }

    fn max_size(&self) -> Option<i32> {
        self.max_seq_len
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        let sh = keys.shape();
        let d = *sh.last().expect("keys rank");
        if d % self.group_size != 0 {
            return Err(Exception::custom(format!(
                "head_dim {d} is not divisible by KV group_size {}",
                self.group_size
            )));
        }
        let step_len = sh[sh.len() - 2];
        let b = sh[0];
        let h = sh[1];

        let (mut k_fp16, mut v_fp16) = match (
            self.keys_packed.take(),
            self.keys_scales.take(),
            self.keys_biases.take(),
            self.values_packed.take(),
            self.values_scales.take(),
            self.values_biases.take(),
        ) {
            (
                Some(kp),
                Some(ks),
                Some(kb),
                Some(vp),
                Some(vs),
                Some(vb),
            ) => {
                let l_prev = kp.shape()[2];
                let k_prev = Self::dequantize_layer(
                    &kp, &ks, &kb, b, h, l_prev, d, self.group_size, self.bits,
                )?;
                let v_prev = Self::dequantize_layer(
                    &vp, &vs, &vb, b, h, l_prev, d, self.group_size, self.bits,
                )?;
                (
                    concatenate_axis(&[k_prev, keys], -2)?,
                    concatenate_axis(&[v_prev, values], -2)?,
                )
            }
            (None, None, None, None, None, None) => (keys, values),
            _ => {
                return Err(Exception::custom(
                    "MlxQuantizedConcatKeyValueCache: partial packed triple",
                ));
            }
        };

        if let Some(max) = self.max_seq_len {
            (k_fp16, v_fp16) = ConcatKeyValueCache::trim_to_max(k_fp16, v_fp16, max)?;
        }

        self.offset += step_len;

        let (kp, ks, kb) = Self::quantize_layer(&k_fp16, self.group_size, self.bits)?;
        let (vp, vs, vb) = Self::quantize_layer(&v_fp16, self.group_size, self.bits)?;
        let (kp, ks, kb) = Self::materialize_triple(&kp, &ks, &kb)?;
        let (vp, vs, vb) = Self::materialize_triple(&vp, &vs, &vb)?;

        self.keys_packed = Some(kp.clone());
        self.keys_scales = Some(ks.clone());
        self.keys_biases = Some(kb.clone());
        self.values_packed = Some(vp.clone());
        self.values_scales = Some(vs.clone());
        self.values_biases = Some(vb.clone());

        Ok(KvFetchResult::Quantized {
            keys: QuantizedKeys {
                keys: kp,
                scales: ks,
                biases: kb,
            },
            values: QuantizedValues {
                values: vp,
                scales: vs,
                biases: vb,
            },
        })
    }
}

/// TODO: A generic KV Cache
pub struct DefaultKeyValueCache {}

#[cfg(all(test, feature = "local-mlx"))]
mod tests {
    use super::*;
    use mlx_rs::Array;

    /// Shape `[1, 1, seq, 1]` — minimal 4D KV layout.
    fn make_kv(seq_len: i32, fill: f32) -> (Array, Array) {
        let t = Array::full(fill, &[1, 1, seq_len, 1]).unwrap();
        (t.clone(), t)
    }

    #[test]
    fn unbounded_grows() {
        let mut cache = ConcatKeyValueCache::new();
        for step in 1..=5 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
            assert_eq!(cache.seq_len(), step);
            assert_eq!(cache.offset(), step);
        }
    }

    #[test]
    fn window_caps_stored_len() {
        let mut cache = ConcatKeyValueCache::with_max(3);
        for step in 1..=3 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
            assert_eq!(cache.seq_len(), step);
        }
        for step in 4..=8 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
            assert_eq!(cache.seq_len(), 3, "stored KV must not exceed window");
            assert_eq!(cache.offset(), step);
        }
    }

    #[test]
    fn window_evicts_oldest() {
        let mut cache = ConcatKeyValueCache::with_max(2);
        for step in 0..4 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
        }
        let keys = cache.keys.as_ref().unwrap();
        let oldest = keys.index((.., .., 0, ..)).item::<f32>();
        let newest = keys.index((.., .., 1, ..)).item::<f32>();
        assert_eq!(oldest, 2.0, "oldest retained slot should be step 2");
        assert_eq!(newest, 3.0, "newest slot should be step 3");
        assert_eq!(cache.offset(), 4);
    }

    #[test]
    fn quantized_roundtrip_low_error() {
        let b = 1;
        let h = 2;
        let l = 4;
        let d = 128;
        let x = Array::arange::<_, f32>(0.0, (b * h * l * d) as f32, None).unwrap();
        let x = x.reshape(&[b, h, l, d]).unwrap();
        let gs = DEFAULT_MLX_KV_GROUP_SIZE;
        let bits = 4;
        let (q, s, bi) = MlxQuantizedConcatKeyValueCache::quantize_layer(&x, gs, bits).unwrap();
        let back =
            MlxQuantizedConcatKeyValueCache::dequantize_layer(&q, &s, &bi, b, h, l, d, gs, bits)
                .unwrap();
        eval(&[back.clone()]).unwrap();
        let max_diff = ((&x - &back).abs().unwrap().max(None).unwrap()).item::<f32>();
        assert!(
            max_diff <= 0.05,
            "quantize/dequantize max abs error {max_diff} too large"
        );
    }

    #[test]
    fn quantized_incremental_steps() {
        let gs = DEFAULT_MLX_KV_GROUP_SIZE;
        let bits = 4;
        let max = 8;
        let d = 128;
        let mut cache = MlxQuantizedConcatKeyValueCache::with_max(gs, bits, max);
        for step in 1..=5_i32 {
            let fill = step as f32 * 0.01;
            let t = Array::full(fill, &[1, 1, 1, d]).unwrap();
            let _ = cache.update_and_fetch(t.clone(), t).unwrap();
            assert_eq!(cache.seq_len(), step.min(max));
            assert_eq!(cache.offset(), step);
        }
        assert!(cache.keys_packed.is_some());
    }
}
