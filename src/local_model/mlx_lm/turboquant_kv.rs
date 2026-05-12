//! TurboQuant KV cache integration (`turboquant-rs`) for Qwen3 — requires **`local-mlx-turboquant`**.
//!
//! One shared [`QuantizedKVCache`] holds all layers; virtual layer id =
//! `physical_layer * n_kv_heads + kv_head_index`.

use std::sync::{Arc, Mutex};

use mlx_rs::{error::Exception, transforms::eval, Array};
use turboquant::QuantizedKVCache;

use super::cache::{KeyValueCache, KvFetchResult};

fn softmax_1d(xs: &mut [f32]) {
    if xs.is_empty() {
        return;
    }
    let m = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut s = 0f32;
    for x in xs.iter_mut() {
        *x = (*x - m).exp();
        s += *x;
    }
    let inv = (s.max(1e-10)).recip();
    for x in xs.iter_mut() {
        *x *= inv;
    }
}

/// KV cache slot for one transformer layer: shares one [`QuantizedKVCache`] across all layers via [`Arc`].
pub struct TurboQuantKeyValueCache {
    inner: Arc<Mutex<QuantizedKVCache>>,
    physical_layer: usize,
    n_kv_heads: i32,
    /// Sequence length stored after the last `update_and_fetch` (matches FP16 [`super::ConcatKeyValueCache`] `offset`).
    seq_len: i32,
    /// `seq_len` snapshot taken at the **start** of the last `update_and_fetch` (for causal masking).
    past_snapshot: i32,
}

impl TurboQuantKeyValueCache {
    pub fn new(inner: Arc<Mutex<QuantizedKVCache>>, physical_layer: usize, n_kv_heads: i32) -> Self {
        Self {
            inner,
            physical_layer,
            n_kv_heads,
            seq_len: 0,
            past_snapshot: 0,
        }
    }

    fn virtual_layer(&self, kv_head: i32) -> usize {
        self.physical_layer * (self.n_kv_heads as usize) + (kv_head as usize)
    }

    fn mlx_vec_f32(a: &Array) -> Result<Vec<f32>, Exception> {
        let a = a.as_type::<f32>()?;
        eval(std::iter::once(&a))?;
        Ok(a.as_slice::<f32>().to_vec())
    }

    /// Row views into `flat` for one KV head: shape `[n_kv, seq_l, head_dim]` flattened in MLX order.
    fn batch_row_refs<'a>(
        flat: &'a [f32],
        seq_l: i32,
        head_dim: i32,
        kv_h: i32,
    ) -> Vec<&'a [f32]> {
        let hd = head_dim as usize;
        let sl = seq_l as usize;
        let kv = kv_h as usize;
        (0..sl)
            .map(|p| {
                let base = (((kv * sl) + p) * hd);
                &flat[base..base + hd]
            })
            .collect()
    }
}

impl KeyValueCache for TurboQuantKeyValueCache {
    fn offset(&self) -> i32 {
        self.seq_len
    }

    fn max_size(&self) -> Option<i32> {
        None
    }

    fn update_and_fetch(&mut self, keys: Array, values: Array) -> Result<KvFetchResult, Exception> {
        self.past_snapshot = self.seq_len;

        let shape = keys.shape();
        if shape.len() != 4 {
            return Err(Exception::custom(format!(
                "turboquant KV: expected keys rank 4, got {:?}",
                shape
            )));
        }
        let _b = shape[0];
        let n_kv = shape[1];
        let seq_l = shape[2];
        let head_dim = shape[3];

        let vs = values.shape();
        if vs != shape {
            return Err(Exception::custom("turboquant KV: keys/values shape mismatch"));
        }

        let keys_f = Self::mlx_vec_f32(&keys)?;
        let vals_f = Self::mlx_vec_f32(&values)?;

        let mut guard = self
            .inner
            .lock()
            .map_err(|_| Exception::custom("turboquant KV cache mutex poisoned"))?;

        for kv_h in 0..n_kv {
            let kr = Self::batch_row_refs(&keys_f, seq_l, head_dim, kv_h);
            let vr = Self::batch_row_refs(&vals_f, seq_l, head_dim, kv_h);
            let vl = self.virtual_layer(kv_h);
            guard
                .push_batch(vl, &kr, &vr)
                .map_err(|e| Exception::custom(format!("turboquant push_batch: {e:?}")))?;
        }

        drop(guard);

        self.seq_len += seq_l;

        Ok(KvFetchResult::TurboQuant)
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
        if mask.is_some() {
            tracing::debug!(
                "[turboquant-kv] bool/additive mask passed — causal masking uses token indices only; verify quality if using sliding-window masks"
            );
        }

        if batch != 1 {
            return Err(Exception::custom(format!(
                "turboquant KV: only batch size 1 supported (got {batch})"
            )));
        }

        let n_rep = (n_heads / n_kv_heads) as usize;
        if (n_rep as i32) * n_kv_heads != n_heads {
            return Err(Exception::custom(format!(
                "turboquant KV: n_heads={n_heads} not divisible by n_kv_heads={n_kv_heads}"
            )));
        }

        let q = queries.as_type::<f32>()?;
        eval(std::iter::once(&q))?;
        let qsl = q.as_slice::<f32>();

        let mut out = vec![0f32; (batch * n_heads * q_len * head_dim) as usize];

        let hd = head_dim as usize;
        let lq = q_len as usize;
        let nh = n_heads as usize;

        for lqi in 0..lq {
            let global_q = kv_past_len + (lqi as i32);
            for qh in 0..nh {
                let kvh = (qh / n_rep) as i32;
                let vl = self.virtual_layer(kvh);

                let base = (((qh * lq) + lqi) * hd) as usize;
                let q_slice = &qsl[base..base + hd];

                let head_out = {
                    let guard = self
                        .inner
                        .lock()
                        .map_err(|_| Exception::custom("turboquant KV mutex poisoned"))?;
                    let mut scores = guard
                        .attention_scores(vl, q_slice)
                        .map_err(|e| Exception::custom(format!("turboquant attention_scores: {e:?}")))?;

                    for (j, s) in scores.iter_mut().enumerate() {
                        if (j as i32) > global_q {
                            *s = f32::NEG_INFINITY;
                        }
                        *s *= scale;
                    }

                    softmax_1d(&mut scores);

                    guard
                        .weighted_values(vl, &scores)
                        .map_err(|e| Exception::custom(format!("turboquant weighted_values: {e:?}")))?
                };

                let obase = (((qh * lq) + lqi) * hd) as usize;
                out[obase..obase + hd].copy_from_slice(&head_out[..hd]);
            }
        }

        Ok(Array::from_slice(out.as_slice(), &[batch, n_heads, q_len, head_dim]))
    }
}
