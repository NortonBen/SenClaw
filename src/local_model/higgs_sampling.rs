use std::collections::HashMap;

use mlx_rs::{Array, argmax_axis, array, categorical, error::Exception, ops};

/// Parameters controlling token sampling behavior.
#[derive(Debug, Clone)]
pub struct SamplingParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: Option<u32>,
    pub min_p: Option<f32>,
    pub repetition_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub presence_penalty: Option<f32>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 1.0,
            top_p: 1.0,
            top_k: None,
            min_p: None,
            repetition_penalty: None,
            frequency_penalty: None,
            presence_penalty: None,
        }
    }
}

impl SamplingParams {
    /// Whether any penalty parameters are active.
    #[allow(clippy::float_cmp)]
    pub fn has_penalties(&self) -> bool {
        self.repetition_penalty.is_some_and(|p| p != 1.0)
            || self.frequency_penalty.is_some_and(|p| p != 0.0)
            || self.presence_penalty.is_some_and(|p| p != 0.0)
    }

    /// Whether any `top_k`/`min_p`/`top_p` filtering is needed beyond basic categorical.
    fn needs_filtering(&self) -> bool {
        self.top_k.is_some() || self.min_p.is_some() || self.top_p < 1.0
    }
}

/// Apply repetition/frequency/presence penalties to logits based on token history.
///
/// Creates penalty adjustment arrays on CPU and applies them as MLX ops.
/// Safe to call even when no penalties are active (returns logits unchanged).
#[allow(clippy::float_cmp)]
pub fn apply_penalties(
    logits: &Array,
    generated_tokens: &[u32],
    params: &SamplingParams,
) -> Result<Array, Exception> {
    if !params.has_penalties() || generated_tokens.is_empty() {
        return Ok(logits.clone());
    }

    let vocab_size = usize::try_from(
        *logits
            .shape()
            .last()
            .ok_or_else(|| Exception::custom("logits must have at least 1 dimension"))?,
    )
    .map_err(|_| Exception::custom("negative vocab size"))?;

    let vocab_size_i32 =
        i32::try_from(vocab_size).map_err(|_| Exception::custom("vocab size overflow for i32"))?;

    // Count token occurrences
    let mut counts: HashMap<u32, u32> = HashMap::new();
    for &tid in generated_tokens {
        if usize::try_from(tid).is_ok_and(|t| t < vocab_size) {
            *counts.entry(tid).or_insert(0) += 1;
        }
    }

    let shape: Vec<i32> = logits.shape().to_vec();
    let mut result = logits.clone();

    // Repetition penalty: for seen tokens, divide positive logits by the penalty
    // and multiply negative logits by the penalty. This moves positive logits
    // down and negative logits further negative, making seen tokens less likely.
    if let Some(rep_penalty) = params.repetition_penalty {
        if rep_penalty != 1.0 {
            let inv = 1.0 / rep_penalty;
            let mut pos_factors = vec![1.0f32; vocab_size];
            let mut neg_factors = vec![1.0f32; vocab_size];
            for &tid in counts.keys() {
                let idx = usize::try_from(tid).unwrap_or(usize::MAX);
                if let Some(slot) = pos_factors.get_mut(idx) {
                    *slot = inv;
                }
                if let Some(slot) = neg_factors.get_mut(idx) {
                    *slot = rep_penalty;
                }
            }
            let pos_arr = Array::from_slice(&pos_factors, &[vocab_size_i32]).reshape(&shape)?;
            let neg_arr = Array::from_slice(&neg_factors, &[vocab_size_i32]).reshape(&shape)?;
            let is_positive = result.gt(Array::from_f32(0.0))?;
            let factor = ops::r#where(&is_positive, &pos_arr, &neg_arr)?;
            result = result.multiply(factor)?;
        }
    }

    // Frequency penalty: logits[t] -= freq_penalty * count[t]
    if let Some(freq_penalty) = params.frequency_penalty {
        if freq_penalty != 0.0 {
            let mut freq = vec![0.0f32; vocab_size];
            for (&tid, &count) in &counts {
                if let Some(slot) = freq.get_mut(usize::try_from(tid).unwrap_or(usize::MAX)) {
                    *slot = freq_penalty * f32::from(u16::try_from(count).unwrap_or(u16::MAX));
                }
            }
            let freq_array = Array::from_slice(&freq, &[vocab_size_i32]).reshape(&shape)?;
            result = result.subtract(freq_array)?;
        }
    }

    // Presence penalty: logits[t] -= pres_penalty for all seen tokens
    if let Some(pres_penalty) = params.presence_penalty {
        if pres_penalty != 0.0 {
            let mut pres = vec![0.0f32; vocab_size];
            for &tid in counts.keys() {
                if let Some(slot) = pres.get_mut(usize::try_from(tid).unwrap_or(usize::MAX)) {
                    *slot = pres_penalty;
                }
            }
            let pres_array = Array::from_slice(&pres, &[vocab_size_i32]).reshape(&shape)?;
            result = result.subtract(pres_array)?;
        }
    }

    Ok(result)
}

/// Sample a token from logits using the given sampling parameters.
///
/// Shared across all model architectures. Penalties should be applied to
/// `logits` via [`apply_penalties`] before calling this function.
pub fn sample(logits: &Array, params: &SamplingParams) -> Result<Array, Exception> {
    if params.temperature == 0.0 {
        return argmax_axis!(logits, -1);
    }

    let scaled = logits.multiply(array!(1.0 / params.temperature))?;

    if params.needs_filtering() {
        sample_filtered(&scaled, params)
    } else {
        categorical!(scaled)
    }
}

/// Combined top-k + min-p + top-p filtering followed by categorical sampling.
///
/// When `top_k` is set and small (<= [`TOPK_PARTIAL_SORT_THRESHOLD`]) the
/// partial-sort path is used: it picks the top-k vocab positions via
/// `argpartition` and sorts only that small slice, instead of sorting the full
/// vocab. Otherwise the full-sort path runs (necessary when top-p needs the
/// full distribution's cumulative sum, or when k is large enough that partition
/// + sort isn't a win).
fn sample_filtered(logits: &Array, params: &SamplingParams) -> Result<Array, Exception> {
    use mlx_rs::ops::softmax_axis;

    let probs = softmax_axis(logits, -1, None)?;
    let n_vocab_i32 = *probs
        .shape()
        .last()
        .ok_or_else(|| Exception::custom("logits must have at least 1 dimension"))?;
    let n_vocab =
        usize::try_from(n_vocab_i32).map_err(|_| Exception::custom("negative vocab size"))?;

    let effective_k = params.top_k.map_or(n_vocab, |k| {
        usize::try_from(k).unwrap_or(1).clamp(1, n_vocab)
    });

    if effective_k <= TOPK_PARTIAL_SORT_THRESHOLD && effective_k < n_vocab {
        sample_filtered_topk(&probs, effective_k, params)
    } else {
        sample_filtered_full(&probs, n_vocab_i32, n_vocab, effective_k, params)
    }
}

/// Threshold at which `sample_filtered` switches from full argsort to partial sort.
///
/// Values below this win on the partial path; values at or above this match (or
/// beat) the partition path in the full-sort path.
pub const TOPK_PARTIAL_SORT_THRESHOLD: usize = 1024;

/// Partial-sort path: argpartition for top-k, then small in-slice sort.
///
/// `k` must satisfy `0 < k < n_vocab`.
#[allow(clippy::shadow_reuse)]
fn sample_filtered_topk(
    probs: &Array,
    k: usize,
    params: &SamplingParams,
) -> Result<Array, Exception> {
    use mlx_rs::ops::{
        argpartition_axis, argsort_axis, concatenate_axis, indexing::IndexOp, maximum,
    };

    let k_i32 = i32::try_from(k).map_err(|_| Exception::custom("k overflow for i32"))?;

    // Argpartition over negated probs: first `k` entries are indices of the
    // top-k probabilities (in some order). The partition index is `k - 1` so
    // positions `0..=k-1` end up holding the smallest negated probs (= top-k).
    let neg_probs = probs.negative()?;
    let part_indices = argpartition_axis(&neg_probs, k_i32 - 1, -1)?;
    let topk_indices_unsorted = part_indices.index((.., 0..k_i32));
    let topk_probs_unsorted = probs.take_along_axis(&topk_indices_unsorted, -1)?;

    // Sort the small [..., k] slice in descending order.
    let neg_topk = topk_probs_unsorted.negative()?;
    let order = argsort_axis(&neg_topk, -1)?;
    let topk_indices = topk_indices_unsorted.take_along_axis(&order, -1)?;
    let topk_probs = topk_probs_unsorted.take_along_axis(&order, -1)?;

    // Apply min-p filtering: remove tokens below the minimum probability threshold.
    let (topk_probs_filtered, topk_indices_filtered) = if let Some(min_p) = params.min_p {
        let max_prob = ops::max_axis(&topk_probs, -1, true)?;
        let threshold = max_prob.multiply(Array::from_f32(min_p))?;
        let mask = topk_probs.ge(&threshold)?;
        let masked_probs = topk_probs.multiply(&mask)?;
        let masked_indices = topk_indices.multiply(&mask)?;
        // Filter out zeros
        let nonzero_mask = masked_probs.gt(Array::from_f32(0.0))?;
        let filtered_probs = masked_probs.take_along_axis(&nonzero_mask, -1)?;
        let filtered_indices = masked_indices.take_along_axis(&nonzero_mask, -1)?;
        (filtered_probs, filtered_indices)
    } else {
        (topk_probs, topk_indices)
    };

    // Apply top-p (nucleus) filtering.
    let (filtered_probs, filtered_indices) = if params.top_p < 1.0 {
        let cumsum = ops::cumsum(&topk_probs_filtered, -1, None, false)?;
        let total = cumsum.index((.., -1..));
        let threshold = total.multiply(Array::from_f32(params.top_p))?;
        let mask = cumsum.le(&threshold)?;
        let masked_probs = topk_probs_filtered.multiply(&mask)?;
        let masked_indices = topk_indices_filtered.multiply(&mask)?;
        let nonzero_mask = masked_probs.gt(Array::from_f32(0.0))?;
        let final_probs = masked_probs.take_along_axis(&nonzero_mask, -1)?;
        let final_indices = masked_indices.take_along_axis(&nonzero_mask, -1)?;
        (final_probs, final_indices)
    } else {
        (topk_probs_filtered, topk_indices_filtered)
    };

    // Normalize and sample
    let sum = ops::sum(&filtered_probs, None)?;
    let normalized = filtered_probs.divide(&sum)?;
    let sampled_idx = categorical!(normalized)?;
    filtered_indices.take_along_axis(&sampled_idx.reshape(&[1, 1])?, -1)
}

/// Full-sort path: argsort the entire vocab, then apply top-k/min-p/top-p filtering.
fn sample_filtered_full(
    probs: &Array,
    n_vocab_i32: i32,
    n_vocab: usize,
    effective_k: usize,
    params: &SamplingParams,
) -> Result<Array, Exception> {
    use mlx_rs::ops::{argsort_axis, indexing::IndexOp};

    // Sort in descending order
    let neg_probs = probs.negative()?;
    let order = argsort_axis(&neg_probs, -1)?;
    let sorted_probs = probs.take_along_axis(&order, -1)?;
    let sorted_indices = order;

    // Apply top-k
    let k_i32 = i32::try_from(effective_k).map_err(|_| Exception::custom("k overflow for i32"))?;
    let topk_probs = sorted_probs.index((.., 0..k_i32));
    let topk_indices = sorted_indices.index((.., 0..k_i32));

    // Apply min-p
    let (filtered_probs, filtered_indices) = if let Some(min_p) = params.min_p {
        let max_prob = ops::max_axis(&topk_probs, -1, true)?;
        let threshold = max_prob.multiply(Array::from_f32(min_p))?;
        let mask = topk_probs.ge(&threshold)?;
        let masked_probs = topk_probs.multiply(&mask)?;
        let masked_indices = topk_indices.multiply(&mask)?;
        let nonzero_mask = masked_probs.gt(Array::from_f32(0.0))?;
        let final_probs = masked_probs.take_along_axis(&nonzero_mask, -1)?;
        let final_indices = masked_indices.take_along_axis(&nonzero_mask, -1)?;
        (final_probs, final_indices)
    } else {
        (topk_probs, topk_indices)
    };

    // Apply top-p
    let (filtered_probs, filtered_indices) = if params.top_p < 1.0 {
        let cumsum = ops::cumsum(&filtered_probs, -1, None, false)?;
        let total = cumsum.index((.., -1..));
        let threshold = total.multiply(Array::from_f32(params.top_p))?;
        let mask = cumsum.le(&threshold)?;
        let masked_probs = filtered_probs.multiply(&mask)?;
        let masked_indices = filtered_indices.multiply(&mask)?;
        let nonzero_mask = masked_probs.gt(Array::from_f32(0.0))?;
        let final_probs = masked_probs.take_along_axis(&nonzero_mask, -1)?;
        let final_indices = masked_indices.take_along_axis(&nonzero_mask, -1)?;
        (final_probs, final_indices)
    } else {
        (filtered_probs, filtered_indices)
    };

    // Normalize and sample
    let sum = ops::sum(&filtered_probs, None)?;
    let normalized = filtered_probs.divide(&sum)?;
    let sampled_idx = categorical!(normalized)?;
    filtered_indices.take_along_axis(&sampled_idx.reshape(&[1, 1])?, -1)
}
