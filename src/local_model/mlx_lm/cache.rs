use mlx_rs::{
    error::Exception,
    ops::{concatenate_axis, dequantize, quantize, reshape, zeros_dtype},
    ops::indexing::{IndexOp, TryIndexMutOp},
    transforms::eval,
    Array,
};

/// Default MLX `quantize` group size (must divide `head_dim`; Qwen3 uses 128).
pub const DEFAULT_MLX_KV_GROUP_SIZE: i32 = 64;

/// Growth chunk for pre-allocated KV buffers (matches mlx_lm / Higgs `SteppingKeyValueCache`).
const KV_CACHE_STEP: i32 = 256;

/// Unified per-layer KV cache: FP16 stepping or MLX packed-quant attention path.
#[derive(Debug, Clone)]
pub enum KvCache {
    Fp16(SteppingKeyValueCache),
    Quantized(MlxQuantizedKeyValueCache),
}

impl KvCache {
    pub fn fp16_with_max(max_seq_len: i32) -> Self {
        Self::Fp16(SteppingKeyValueCache::with_max(max_seq_len))
    }

    pub fn quantized_with_max(group_size: i32, bits: i32, max_seq_len: i32) -> Self {
        Self::Quantized(MlxQuantizedKeyValueCache::with_max(
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

/// Materialize lazy MLX arrays so prior slice/concat nodes can be freed.
fn materialize_pair(k: Array, v: Array) -> Result<(Array, Array), Exception> {
    eval(&[k.clone(), v.clone()])?;
    Ok((k, v))
}

/// Evaluate all layer caches — call after each forward to cap peak RAM (MLX lazy graphs).
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
    Quantized {
        keys: QuantizedKeys,
        values: QuantizedValues,
    },
}

/// Pre-allocated KV cache with `mlx_slice_update` writes and optional sliding window.
///
/// Shape: `[batch, n_kv_heads, seq_len, head_dim]`.
///
/// - `offset`: **absolute** RoPE position (total tokens processed); not reset on eviction.
/// - `stored_len`: tokens retained for attention (`min(offset, max_seq_len)` when capped).
///
/// Matches Higgs `SteppingKeyValueCache` growth semantics; adds SemaClaw sliding-window cap.
#[derive(Debug, Clone)]
pub struct SteppingKeyValueCache {
    keys: Option<Array>,
    values: Option<Array>,
    /// Absolute tokens seen (RoPE).
    offset: i32,
    /// Tokens currently stored along axis 2 (after eviction).
    stored_len: i32,
    max_seq_len: Option<i32>,
    step: i32,
}

/// Back-compat alias — prefer [`SteppingKeyValueCache`].
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
            offset: 0,
            stored_len: 0,
            max_seq_len: None,
            step: KV_CACHE_STEP,
        }
    }

    pub fn with_max(max_seq_len: i32) -> Self {
        Self {
            keys: None,
            values: None,
            offset: 0,
            stored_len: 0,
            max_seq_len: Some(max_seq_len.max(1)),
            step: KV_CACHE_STEP,
        }
    }

    /// Tokens in the buffer used for attention (after sliding-window eviction).
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

    /// Roll back absolute offset (e.g. rejected speculative decode). Storage is not shrunk.
    pub fn trim_by(&mut self, n: usize) {
        let trim = i32::try_from(n).unwrap_or(i32::MAX);
        let new_offset = self.offset.saturating_sub(trim).max(0);
        let drop_stored = self.stored_len.saturating_sub(new_offset);
        if drop_stored > 0 {
            if let (Some(k), Some(v)) = (&self.keys, &self.values) {
                let end = self.stored_len - drop_stored;
                if let (Ok(k2), Ok(v2)) = (slice_axis2(k, 0, end), slice_axis2(v, 0, end)) {
                    self.keys = Some(k2);
                    self.values = Some(v2);
                }
            }
            self.stored_len -= drop_stored;
        }
        self.offset = new_offset;
    }

    fn dim(shape: &[i32], i: usize, label: &'static str) -> Result<i32, Exception> {
        shape.get(i).copied().ok_or_else(|| {
            Exception::custom(format!("KV cache: missing dim {i} ({label})"))
        })
    }

    fn update_dense(&mut self, keys: &Array, values: &Array) -> Result<(Array, Array), Exception> {
        let k_shape = keys.shape();
        let v_shape = values.shape();
        let new_tokens = Self::dim(k_shape, 2, "keys T")?;

        let max_cap = self.max_seq_len;
        let target_stored = match max_cap {
            Some(m) => (self.offset + new_tokens).min(m),
            None => self.offset + new_tokens,
        };

        let drop = (self.stored_len + new_tokens - target_stored).max(0);
        if drop > 0 {
            if let (Some(k), Some(v)) = (&self.keys, &self.values) {
                let end = self.stored_len;
                self.keys = Some(slice_axis2(k, drop, end)?);
                self.values = Some(slice_axis2(v, drop, end)?);
            }
            self.stored_len -= drop;
        }

        let write_pos = self.stored_len;
        let required_slots = write_pos + new_tokens;

        let need_alloc = self.keys.is_none();
        // Bounded window: one pre-allocation of `max` slots; sliding eviction only after that.
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

        let updated_k = slice_update_axis2(k_buf, keys, write_pos, new_tokens)?;
        let updated_v = slice_update_axis2(v_buf, values, write_pos, new_tokens)?;
        self.keys = Some(updated_k);
        self.values = Some(updated_v);

        self.stored_len += new_tokens;
        self.offset += new_tokens;

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
        let (k, v) = self.update_dense(&keys, &values)?;
        let (k, v) = materialize_pair(k, v)?;
        Ok(KvFetchResult::Fp16(k, v))
    }
}

/// Back-compat alias.
pub type MlxQuantizedConcatKeyValueCache = MlxQuantizedKeyValueCache;

/// Bounded **FP16 stepping** storage + **on-the-fly quantize** for attention only.
///
/// Packed tensors are not retained between forwards (avoids double FP16 peak per layer).
#[derive(Debug, Clone)]
pub struct MlxQuantizedKeyValueCache {
    fp16: SteppingKeyValueCache,
    group_size: i32,
    bits: i32,
}

impl MlxQuantizedKeyValueCache {
    pub fn with_max(group_size: i32, bits: i32, max_seq_len: i32) -> Self {
        Self {
            fp16: SteppingKeyValueCache::with_max(max_seq_len),
            group_size,
            bits,
        }
    }

    pub fn seq_len(&self) -> i32 {
        self.fp16.seq_len()
    }

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

    #[cfg(test)]
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

impl KvCache {
    fn eval_targets(&self) -> Vec<Array> {
        match self {
            Self::Fp16(c) => c.eval_targets(),
            Self::Quantized(c) => c.fp16.eval_targets(),
        }
    }
}

impl KeyValueCache for MlxQuantizedKeyValueCache {
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
        self.fp16.offset()
    }

    fn max_size(&self) -> Option<i32> {
        self.fp16.max_size()
    }

    fn update_and_fetch(
        &mut self,
        keys: Array,
        values: Array,
    ) -> Result<KvFetchResult, Exception> {
        let d = *keys.shape().last().expect("keys rank");
        if d % self.group_size != 0 {
            return Err(Exception::custom(format!(
                "head_dim {d} is not divisible by KV group_size {}",
                self.group_size
            )));
        }

        let KvFetchResult::Fp16(k, v) = self.fp16.update_and_fetch(keys, values)? else {
            return Err(Exception::custom(
                "MlxQuantizedKeyValueCache: inner cache must stay FP16",
            ));
        };

        let (kp, ks, kb) = Self::quantize_layer(&k, self.group_size, self.bits)?;
        let (vp, vs, vb) = Self::quantize_layer(&v, self.group_size, self.bits)?;
        let (kp, ks, kb) = Self::materialize_triple(&kp, &ks, &kb)?;
        let (vp, vs, vb) = Self::materialize_triple(&vp, &vs, &vb)?;

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

/// Slice `arr` along axis 2: `arr[..., start:end, ...]`.
fn slice_axis2(arr: &Array, start: i32, end: i32) -> Result<Array, Exception> {
    Ok(arr.index((.., .., start..end, ..)))
}

/// Write `update` into `target` at `[..., start:start+n, ...]` on axis 2.
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

#[cfg(all(test, feature = "local-mlx"))]
mod tests {
    use super::*;
    use mlx_rs::{array, Array};

    fn make_kv(seq_len: i32, fill: f32) -> (Array, Array) {
        let t = Array::full(&[1, 1, seq_len, 1], array!(fill)).unwrap();
        (t.clone(), t)
    }

    #[test]
    fn unbounded_grows() {
        let mut cache = SteppingKeyValueCache::new();
        for step in 1..=5 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
            assert_eq!(cache.seq_len(), step);
            assert_eq!(cache.offset(), step);
        }
    }

    #[test]
    fn window_caps_stored_len() {
        let mut cache = SteppingKeyValueCache::with_max(3);
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
        let mut cache = SteppingKeyValueCache::with_max(2);
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
    fn stepping_prefill_chunk() {
        let mut cache = SteppingKeyValueCache::with_max(512);
        let (k, v) = make_kv(64, 1.0);
        let _ = cache.update_and_fetch(k, v).unwrap();
        assert_eq!(cache.seq_len(), 64);
        assert_eq!(cache.offset(), 64);
        let (k, v) = make_kv(1, 2.0);
        let _ = cache.update_and_fetch(k, v).unwrap();
        assert_eq!(cache.seq_len(), 65);
    }

    #[test]
    fn trim_by_rolls_back_offset() {
        let mut cache = SteppingKeyValueCache::with_max(8);
        for step in 1..=5 {
            let (k, v) = make_kv(1, step as f32);
            let _ = cache.update_and_fetch(k, v).unwrap();
        }
        cache.trim_by(2);
        assert_eq!(cache.offset(), 3);
        assert_eq!(cache.seq_len(), 3);
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
        let (q, s, bi) = MlxQuantizedKeyValueCache::quantize_layer(&x, gs, bits).unwrap();
        let back =
            MlxQuantizedKeyValueCache::dequantize_layer(&q, &s, &bi, b, h, l, d, gs, bits).unwrap();
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
        let mut cache = MlxQuantizedKeyValueCache::with_max(gs, bits, max);
        for step in 1..=5_i32 {
            let fill = step as f32 * 0.01;
            let t = Array::full(&[1, 1, 1, d], array!(fill)).unwrap();
            let _ = cache.update_and_fetch(t.clone(), t).unwrap();
            assert_eq!(cache.seq_len(), step.min(max));
            assert_eq!(cache.offset(), step);
        }
        assert!(cache.fp16.keys.is_some());
    }
}
