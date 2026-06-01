//! Gated-delta (linear attention) recurrence for Qwen3.5 linear-attn layers.
//!
//! Port of `mlx_lm.models.gated_delta` — ops-based sequential scan (no Metal kernel).

use mlx_rs::{
    array,
    error::Exception,
    nn,
    ops::{
        expand_dims,
        indexing::{IndexOp, NewAxis},
        repeat_axis, sigmoid, stack_axis, sum_axis, zeros_dtype,
    },
    transforms::compile::compile,
    Array, Dtype,
};

fn compute_g(a_log: &Array, a: &Array, dt_bias: &Array) -> Result<Array, Exception> {
    let inner = a.add(dt_bias)?;
    let soft = nn::softplus(&inner)?;
    let neg_a = a_log.exp()?.multiply(&array!(-1.0_f32))?;
    Ok(soft.multiply(&neg_a)?.exp()?)
}

fn gated_delta_step(
    q: &Array,
    k: &Array,
    v: &Array,
    g: &Array,
    beta: &Array,
    state: &Array,
    mask: Option<&Array>,
) -> Result<(Array, Array), Exception> {
    let old_state = state;
    let decay = if g.shape().len() == 2 {
        g.index((.., .., NewAxis, NewAxis))
    } else {
        g.index((.., .., .., NewAxis))
    };
    let mut state = state.multiply(&decay)?;
    let kv_mem = sum_axis(&state.multiply(&k.index((.., .., NewAxis, ..)))?, -1, false)?;
    let delta = v
        .subtract(&kv_mem)?
        .multiply(&beta.index((.., .., NewAxis)))?;
    state = state.add(&k.index((.., .., NewAxis, ..)).multiply(&delta.index((
        ..,
        ..,
        ..,
        NewAxis,
    )))?)?;
    let y = sum_axis(&state.multiply(&q.index((.., .., NewAxis, ..)))?, -1, false)?;
    let state = if let Some(mask) = mask {
        let mask = expand_dims(mask, 1)?.expand_dims(2)?.expand_dims(3)?;
        mlx_rs::ops::r#where(&mask, &state, old_state)?
    } else {
        state
    };
    Ok((y, state))
}

/// Run gated-delta over a sequence (prefill / decode).
///
/// Shapes: `q,k: [B,T,Hk,Dk]`, `v: [B,T,Hv,Dv]`, `g,beta: [B,T,Hv]` or `g: [B,T,Hv,Dk]`.
pub fn gated_delta_update(
    q: &Array,
    k: &Array,
    v: &Array,
    a: &Array,
    b: &Array,
    a_log: &Array,
    dt_bias: &Array,
    state: Option<&Array>,
    mask: Option<&Array>,
) -> Result<(Array, Array), Exception> {
    let shape = q.shape();
    let b_size = shape[0];
    let seq_len = shape[1];
    let n_kv = shape[2];
    let d_k = shape[3];
    let h_v = v.shape()[2];
    let d_v = v.shape()[3];

    let beta = sigmoid(b)?;
    let g = compute_g(a_log, a, dt_bias)?;

    let repeat = h_v / n_kv;
    let (q_use, k_use) = if repeat > 1 {
        (
            repeat_axis::<f32>(q.clone(), repeat as i32, 2)?,
            repeat_axis::<f32>(k.clone(), repeat as i32, 2)?,
        )
    } else {
        (q.clone(), k.clone())
    };

    let state = match state {
        Some(s) => s.clone(),
        None => zeros_dtype(&[b_size, h_v, d_v, d_k], Dtype::Float32)?,
    };

    let mut ys = Vec::with_capacity(seq_len as usize);
    let mut state = state;

    // The recurrence is sequential, so prefill runs `seq_len` steps; each step is
    // ~7 tiny ops. Dispatching them one-by-one is CPU-bound (the GPU starves —
    // 100% CPU / ~60% GPU) and dominates Qwen3.5 prefill. `compile` fuses the
    // step into a single kernel (≈7× fewer host dispatches), mirroring mlx-lm's
    // `@mx.compile` on `_gated_delta_step_ops`. mlx caches the compiled graph by
    // the closure's type, so it traces once and is reused across steps/layers.
    // Only the no-mask path (prefill / decode for Qwen3.5) is compiled; the rare
    // masked path falls back to the plain step.
    if mask.is_none() {
        let mut step = compile(
            |inp: &[Array]| -> Result<Vec<Array>, Exception> {
                let (q, k, v, g, beta, st) = (&inp[0], &inp[1], &inp[2], &inp[3], &inp[4], &inp[5]);
                let decay = if g.shape().len() == 2 {
                    g.index((.., .., NewAxis, NewAxis))
                } else {
                    g.index((.., .., .., NewAxis))
                };
                let mut s = st.multiply(&decay)?;
                let kv_mem = sum_axis(&s.multiply(&k.index((.., .., NewAxis, ..)))?, -1, false)?;
                let delta = v
                    .subtract(&kv_mem)?
                    .multiply(&beta.index((.., .., NewAxis)))?;
                s = s.add(&k.index((.., .., NewAxis, ..)).multiply(&delta.index((
                    ..,
                    ..,
                    ..,
                    NewAxis,
                )))?)?;
                let y = sum_axis(&s.multiply(&q.index((.., .., NewAxis, ..)))?, -1, false)?;
                Ok(vec![y, s])
            },
            None,
        );
        for t in 0..seq_len {
            let q_t = q_use.index((.., t, .., ..));
            let k_t = k_use.index((.., t, .., ..));
            let v_t = v.index((.., t, .., ..));
            let g_t = g.index((.., t, ..));
            let beta_t = beta.index((.., t, ..));
            let out = step(&[q_t, k_t, v_t, g_t, beta_t, state.clone()])?;
            ys.push(out[0].clone());
            state = out[1].clone();
        }
    } else {
        for t in 0..seq_len {
            let q_t = q_use.index((.., t, .., ..));
            let k_t = k_use.index((.., t, .., ..));
            let v_t = v.index((.., t, .., ..));
            let g_t = g.index((.., t, ..));
            let beta_t = beta.index((.., t, ..));
            let mask_t = mask.map(|m| m.index((.., t)));
            let (y_t, s) =
                gated_delta_step(&q_t, &k_t, &v_t, &g_t, &beta_t, &state, mask_t.as_ref())?;
            state = s;
            ys.push(y_t);
        }
    }
    let y = stack_axis(&ys, 1)?;
    Ok((y, state))
}
