use mlx_rs::{
    error::Exception,
    ops::{concatenate_axis, quantize, reshape},
    ops::indexing::IndexOp,
    transforms::eval,
    Array,
};

/// Materialize lazy MLX arrays so prior concat/slice nodes can be freed.
fn materialize_pair(k: Array, v: Array) -> Result<(Array, Array), Exception> {
    eval(&[k.clone(), v.clone()])?;
    Ok((k, v))
}

/// Evaluate all layer caches — call after each forward to cap peak RAM (MLX lazy graphs).
pub fn eval_all_caches(caches: &mut [Option<ConcatKeyValueCache>]) -> Result<(), Exception> {
    let mut batch = Vec::new();
    for cache in caches.iter_mut().flatten() {
        if let Some(k) = cache.keys.as_ref() {
            batch.push(k.clone());
        }
        if let Some(v) = cache.values.as_ref() {
            batch.push(v.clone());
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

/// KV cache using MLX `quantize` + [`quantized_scaled_dot_product_attention`] (packed int weights).
///
/// **Experimental / currently unused in `mlx_native`**: concatenating quantized tensors along the
/// sequence axis produced broken attention (model emitted newline-token loops). Prefer
/// [`ConcatKeyValueCache`] until this layout is validated against upstream MLX LM.
#[derive(Debug, Clone)]
pub struct MlxQuantizedConcatKeyValueCache {
    group_size: i32,
    bits: i32,
    keys_packed: Option<Array>,
    keys_scales: Option<Array>,
    keys_biases: Option<Array>,
    values_packed: Option<Array>,
    values_scales: Option<Array>,
    values_biases: Option<Array>,
    offset: i32,
}

impl MlxQuantizedConcatKeyValueCache {
    pub fn new(group_size: i32, bits: i32) -> Self {
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
        }
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
    ) -> Result<(Array, Array, Array, i32), Exception> {
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
        Ok((q4, s4, b4, l))
    }

    fn concat_triple(
        prev: Option<(Array, Array, Array)>,
        new: (Array, Array, Array),
        axis: i32,
    ) -> Result<(Array, Array, Array), Exception> {
        match prev {
            None => Ok(new),
            Some((a, b, c)) => Ok((
                concatenate_axis(&[a, new.0], axis)?,
                concatenate_axis(&[b, new.1], axis)?,
                concatenate_axis(&[c, new.2], axis)?,
            )),
        }
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
        None
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

        let (kp, ks, kb, l_step) = Self::quantize_layer(&keys, self.group_size, self.bits)?;
        let (vp, vs, vb, l_v) = Self::quantize_layer(&values, self.group_size, self.bits)?;
        debug_assert_eq!(l_step, l_v);

        let prev_keys = match (
            self.keys_packed.take(),
            self.keys_scales.take(),
            self.keys_biases.take(),
        ) {
            (Some(a), Some(b), Some(c)) => Some((a, b, c)),
            (None, None, None) => None,
            _ => {
                return Err(Exception::custom(
                    "MlxQuantizedConcatKeyValueCache: partial keys triple",
                ));
            }
        };
        let prev_vals = match (
            self.values_packed.take(),
            self.values_scales.take(),
            self.values_biases.take(),
        ) {
            (Some(a), Some(b), Some(c)) => Some((a, b, c)),
            (None, None, None) => None,
            _ => {
                return Err(Exception::custom(
                    "MlxQuantizedConcatKeyValueCache: partial values triple",
                ));
            }
        };

        let (kp, ks, kb) = Self::concat_triple(prev_keys, (kp, ks, kb), 2)?;
        let (vp, vs, vb) = Self::concat_triple(prev_vals, (vp, vs, vb), 2)?;

        self.offset = kp.shape()[2];
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
}
