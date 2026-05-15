use candle_core::{Result, Tensor};

/// Per-layer KV cache with an optional sliding-window memory cap.
///
/// # Sliding window
/// When `max_seq_len` is set to `N` the cache never holds more than `N` key/value
/// tokens.  Older entries are evicted from the **front** (furthest past), keeping
/// the most recent context.  This bounds KV-cache RAM at:
///
/// ```text
/// 2 × n_layers × n_kv_heads × head_dim × N × dtype_bytes
/// ```
///
/// RoPE positions are **absolute** — `offset` is never reset — so the model
/// retains correct positional encodings for recent tokens even after eviction.
///
/// # Shape convention
/// All tensors: `[batch, n_kv_heads, seq_len, head_dim]`.
pub struct KvCache {
    keys: Option<Tensor>,
    values: Option<Tensor>,
    /// Hard cap on accumulated sequence length.  `None` = unbounded.
    max_seq_len: usize,
}

impl KvCache {
    /// Unbounded cache (unlimited RAM growth).  Use `with_max` in production.
    pub fn new() -> Self {
        Self {
            keys: None,
            values: None,
            max_seq_len: usize::MAX,
        }
    }

    /// Sliding-window cache: evict oldest tokens when `> max_seq_len` tokens
    /// accumulate.  Bounds RAM regardless of generation length.
    pub fn with_max(max_seq_len: usize) -> Self {
        Self {
            keys: None,
            values: None,
            max_seq_len: max_seq_len.max(1),
        }
    }

    /// Append `k`/`v` for the current step and return the full accumulated
    /// (possibly truncated) tensors for use in attention.
    ///
    /// When the accumulated length would exceed `max_seq_len`, the oldest
    /// `(accumulated − max_seq_len)` tokens are dropped from the front.
    pub fn append(&mut self, k: Tensor, v: Tensor) -> Result<(Tensor, Tensor)> {
        let (k, v) = match (&self.keys, &self.values) {
            (Some(prev_k), Some(prev_v)) => {
                let k = Tensor::cat(&[prev_k, &k], 2)?;
                let v = Tensor::cat(&[prev_v, &v], 2)?;
                (k, v)
            }
            _ => (k, v),
        };

        // Sliding-window eviction: keep the most recent `max_seq_len` tokens.
        let (k, v) = {
            let seq_len = k.dim(2)?;
            if seq_len > self.max_seq_len {
                let start = seq_len - self.max_seq_len;
                (
                    k.narrow(2, start, self.max_seq_len)?,
                    v.narrow(2, start, self.max_seq_len)?,
                )
            } else {
                (k, v)
            }
        };

        self.keys = Some(k.clone());
        self.values = Some(v.clone());
        Ok((k, v))
    }

    /// Number of tokens currently held in the cache (after any eviction).
    pub fn seq_len(&self) -> usize {
        self.keys
            .as_ref()
            .and_then(|t| t.dim(2).ok())
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use candle_core::Device;

    fn make_kv(seq_len: usize) -> (Tensor, Tensor) {
        // Shape: [1, 2, seq_len, 4]  (batch=1, n_kv_heads=2, seq, head_dim=4)
        let data: Vec<f32> = (0..(seq_len * 2 * 4)).map(|i| i as f32).collect();
        let t = Tensor::from_vec(data, (1, 2, seq_len, 4), &Device::Cpu).unwrap();
        (t.clone(), t)
    }

    #[test]
    fn unbounded_grows() {
        let mut cache = KvCache::new();
        for step in 1..=5 {
            let (k, v) = make_kv(1);
            let (rk, _rv) = cache.append(k, v).unwrap();
            assert_eq!(rk.dim(2).unwrap(), step, "step {step}");
        }
    }

    #[test]
    fn window_caps_at_max() {
        let mut cache = KvCache::with_max(3);
        // Fill to exactly the window
        for step in 1..=3 {
            let (k, v) = make_kv(1);
            let (rk, _) = cache.append(k, v).unwrap();
            assert_eq!(rk.dim(2).unwrap(), step);
        }
        // Beyond the window — should stay at 3
        for _ in 0..5 {
            let (k, v) = make_kv(1);
            let (rk, _) = cache.append(k, v).unwrap();
            assert_eq!(rk.dim(2).unwrap(), 3, "cache must not exceed max");
        }
        assert_eq!(cache.seq_len(), 3);
    }

    #[test]
    fn window_evicts_oldest() {
        // Each step's key tensor is filled with its step number so we can
        // verify the OLDEST tokens get evicted (not the newest).
        let mut cache = KvCache::with_max(2);
        for step in 0u32..4 {
            let val = Tensor::full(step as f32, (1usize, 1usize, 1usize, 1usize), &Device::Cpu)
                .unwrap();
            cache.append(val.clone(), val).unwrap();
        }
        // After 4 steps with window=2, cache holds steps [2, 3]
        let keys = cache.keys.unwrap(); // [1, 1, 2, 1]
        let newest = keys
            .narrow(2, 1, 1)
            .unwrap()
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        assert_eq!(newest[0], 3.0, "newest token should be step 3");
        let oldest = keys
            .narrow(2, 0, 1)
            .unwrap()
            .flatten_all()
            .unwrap()
            .to_vec1::<f32>()
            .unwrap();
        assert_eq!(oldest[0], 2.0, "oldest retained token should be step 2");
    }
}
