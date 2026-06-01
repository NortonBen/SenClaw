use candle_core::{DType, Device, Result, Tensor};

/// Precomputed RoPE sin/cos tables (Qwen3 convention: cat([freqs, freqs])).
pub struct RotaryEmbedding {
    /// [max_seq_len, head_dim]
    cos: Tensor,
    sin: Tensor,
}

impl RotaryEmbedding {
    pub fn new(
        head_dim: usize,
        max_seq_len: usize,
        theta: f64,
        dtype: DType,
        device: &Device,
    ) -> Result<Self> {
        let inv_freq: Vec<f32> = (0..head_dim / 2)
            .map(|i| 1.0f32 / (theta as f32).powf(2.0 * i as f32 / head_dim as f32))
            .collect();
        let inv_freq = Tensor::new(inv_freq.as_slice(), device)?;
        let t = Tensor::arange(0u32, max_seq_len as u32, device)?.to_dtype(DType::F32)?;
        // freqs: [max_seq_len, head_dim/2]
        let freqs = t.unsqueeze(1)?.broadcast_mul(&inv_freq.unsqueeze(0)?)?;
        // emb = cat([freqs, freqs], dim=-1): [max_seq_len, head_dim]
        let emb = Tensor::cat(&[&freqs, &freqs], 1)?;
        Ok(Self {
            cos: emb.cos()?.to_dtype(dtype)?,
            sin: emb.sin()?.to_dtype(dtype)?,
        })
    }

    /// Apply RoPE to `q` and `k` (both `[bsz, n_heads, seq_len, head_dim]`)
    /// starting at cache `offset`.
    pub fn apply(&self, q: &Tensor, k: &Tensor, offset: usize) -> Result<(Tensor, Tensor)> {
        let seq_len = q.dim(2)?;
        // cos/sin slice: [seq_len, head_dim] → broadcast to [1, 1, seq_len, head_dim]
        let cos = self
            .cos
            .narrow(0, offset, seq_len)?
            .unsqueeze(0)?
            .unsqueeze(0)?;
        let sin = self
            .sin
            .narrow(0, offset, seq_len)?
            .unsqueeze(0)?
            .unsqueeze(0)?;
        Ok((rotate(q, &cos, &sin)?, rotate(k, &cos, &sin)?))
    }
}

/// `q_embed = q * cos + rotate_half(q) * sin`
/// `rotate_half(x)` = cat([-x2, x1]) where x1 = x[..., :half], x2 = x[..., half:]
fn rotate(x: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor> {
    let half = x.dim(candle_core::D::Minus1)? / 2;
    let x1 = x.narrow(candle_core::D::Minus1, 0, half)?;
    let x2 = x.narrow(candle_core::D::Minus1, half, half)?;
    let rotated = Tensor::cat(&[&x2.neg()?, &x1], candle_core::D::Minus1)?;
    x.broadcast_mul(cos)?
        .broadcast_add(&rotated.broadcast_mul(sin)?)
}
