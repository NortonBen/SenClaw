use mlx_rs::{
    error::Exception,
    ops::{concatenate_axis, quantize, reshape},
    ops::indexing::IndexOp,
    Array,
};

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

    /// Approximate attention via **turboquant-rs** CPU path (`local-mlx-turboquant` only).
    /// Default: not supported — [`ConcatKeyValueCache`] uses MLX SDPA instead.
    fn turboquant_attention(
        &mut self,
        _queries: Array,
        _scale: f32,
        _mask: Option<&Array>,
        _batch: i32,
        _q_len: i32,
        _kv_past_len: i32,
        _n_heads: i32,
        _n_kv_heads: i32,
        _head_dim: i32,
    ) -> Result<Array, Exception> {
        Err(Exception::custom(
            "turboquant_attention: cache is not a TurboQuant KV backend",
        ))
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

    fn turboquant_attention(
        &mut self,
        queries: Array,
        scale: f32,
        mask: Option<&Array>,
        batch: i32,
        q_len: i32,
        kv_past_len: i32,
        n_heads: i32,
        n_kv_heads: i32,
        head_dim: i32,
    ) -> Result<Array, Exception> {
        T::turboquant_attention(
            self,
            queries,
            scale,
            mask,
            batch,
            q_len,
            kv_past_len,
            n_heads,
            n_kv_heads,
            head_dim,
        )
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
    /// KV stored via turboquant-rs; run [`KeyValueCache::turboquant_attention`] instead of MLX SDPA.
    TurboQuant,
}

#[derive(Debug, Clone, Default)]
pub struct ConcatKeyValueCache {
    keys: Option<Array>,
    values: Option<Array>,
    offset: i32,
}

impl ConcatKeyValueCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl KeyValueCache for ConcatKeyValueCache {
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
        match (self.keys.take(), self.values.take()) {
            (Some(k), Some(v)) => {
                self.keys = Some(concatenate_axis(&[k, keys], -2)?);
                self.values = Some(concatenate_axis(&[v, values], -2)?);
            }
            _ => {
                self.keys = Some(keys);
                self.values = Some(values);
            }
        }
        let shape = self.keys.as_ref().expect("Keys cannot be None").shape();
        self.offset = shape[shape.len() - 2];

        Ok(KvFetchResult::Fp16(
            self.keys.clone().expect("Keys cannot be None"),
            self.values.clone().expect("Values cannot be None"),
        ))
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
