//! Numerical tests for [`SequentialScan`] (Mamba-2 SSD recurrence).
//!
//! Each test builds a tiny, fully-determined configuration and cross-checks the
//! mlx-rs scan against a hand-rolled f32 reference loop encoding the canonical
//! recurrence:
//!
//! ```text
//! state[t] = state[t-1] * exp(dt[t] * A)
//!          + dt[t] * x[t] (outer) B[group(h), t]
//! y[t]     = state[t] @ C[group(h), t]^T + D * x[t]
//! ```
//!
//! Shapes match `SsmScanBackend::scan`:
//! - `x`:        `[B, L, H, P]`
//! - `dt`:       `[B, L, H]`
//! - `A`, `D`:   `[H]`
//! - `B`, `C`:   `[B, L, G, N]`
//! - `state`:    `[B, H, P, N]`
//!
//! Runs under `--features local-mlx`; skipped otherwise.

#![cfg(feature = "local-mlx")]

use mlx_rs::{transforms::eval, Array};
use senclaw::local_model::mlx_lm::models::mamba2::{SequentialScan, SsmScanBackend};

/// Hand-rolled f32 reference scan. Mirrors the docstring formula 1:1.
#[allow(clippy::too_many_arguments)]
fn reference_scan(
    x: &[f32],
    dt: &[f32],
    a: &[f32],
    b: &[f32],
    c: &[f32],
    d: &[f32],
    state_in: &[f32],
    b_size: usize,
    seq_len: usize,
    n_heads: usize,
    head_dim: usize,
    n_groups: usize,
    d_state: usize,
) -> (Vec<f32>, Vec<f32>) {
    let heads_per_group = n_heads / n_groups;
    let mut state = state_in.to_vec();
    let mut y = vec![0.0_f32; b_size * seq_len * n_heads * head_dim];

    for bi in 0..b_size {
        for t in 0..seq_len {
            for h in 0..n_heads {
                let g = h / heads_per_group;
                let dt_v = dt[(bi * seq_len + t) * n_heads + h];
                let a_v = a[h];
                let d_a = (dt_v * a_v).exp();

                let s_base = ((bi * n_heads + h) * head_dim) * d_state;
                let x_base = ((bi * seq_len + t) * n_heads + h) * head_dim;
                let b_base = ((bi * seq_len + t) * n_groups + g) * d_state;
                let c_base = b_base;

                for p in 0..head_dim {
                    let x_v = x[x_base + p];
                    for n in 0..d_state {
                        let dbx = dt_v * x_v * b[b_base + n];
                        let idx = s_base + p * d_state + n;
                        state[idx] = state[idx] * d_a + dbx;
                    }
                }
                for p in 0..head_dim {
                    let mut acc = 0.0_f32;
                    for n in 0..d_state {
                        acc += state[s_base + p * d_state + n] * c[c_base + n];
                    }
                    y[x_base + p] = acc + d[h] * x[x_base + p];
                }
            }
        }
    }
    (y, state)
}

#[allow(clippy::too_many_arguments)]
fn mlx_scan_collect(
    x: &[f32],
    dt: &[f32],
    a: &[f32],
    b: &[f32],
    c: &[f32],
    d: &[f32],
    state_in: &[f32],
    b_size: i32,
    seq_len: i32,
    n_heads: i32,
    head_dim: i32,
    n_groups: i32,
    d_state: i32,
) -> (Vec<f32>, Vec<f32>) {
    let x_a = Array::from_slice(x, &[b_size, seq_len, n_heads, head_dim]);
    let dt_a = Array::from_slice(dt, &[b_size, seq_len, n_heads]);
    let a_a = Array::from_slice(a, &[n_heads]);
    let b_a = Array::from_slice(b, &[b_size, seq_len, n_groups, d_state]);
    let c_a = Array::from_slice(c, &[b_size, seq_len, n_groups, d_state]);
    let d_a = Array::from_slice(d, &[n_heads]);
    let s_a = Array::from_slice(state_in, &[b_size, n_heads, head_dim, d_state]);

    let scan = SequentialScan;
    let (y, s_out) = scan
        .scan(&x_a, &dt_a, &a_a, &b_a, &c_a, &d_a, &s_a)
        .expect("scan");
    eval(&[y.clone(), s_out.clone()]).expect("eval");
    (
        y.as_slice::<f32>().to_vec(),
        s_out.as_slice::<f32>().to_vec(),
    )
}

fn approx_eq(actual: &[f32], expected: &[f32], tol: f32) {
    assert_eq!(actual.len(), expected.len(), "length mismatch");
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        let diff = (a - e).abs();
        let allow = tol * (1.0 + e.abs());
        assert!(diff <= allow, "elem {i}: |{a} - {e}| = {diff} > {allow}");
    }
}

/// Skip-connection only: A=-∞ (state decays to 0), B=0, C=0, D=1, state_in=0.
/// Expected: y == x.
#[test]
fn scan_skip_connection_only() {
    let (b, l, h, p, g, n) = (1, 4, 2, 3, 1, 2);
    let x: Vec<f32> = (0..(b * l * h * p)).map(|i| i as f32 * 0.1).collect();
    let dt = vec![1.0; b * l * h];
    // Hugely negative A so exp(dt*A) ≈ 0 in f32 — kills any state contribution.
    let a = vec![-1.0e6; h];
    let b_vec = vec![0.0; b * l * g * n];
    let c_vec = vec![0.0; b * l * g * n];
    let d = vec![1.0; h];
    let state_in = vec![0.0; b * h * p * n];

    let (y, _) = mlx_scan_collect(
        &x, &dt, &a, &b_vec, &c_vec, &d, &state_in, b as i32, l as i32, h as i32, p as i32,
        g as i32, n as i32,
    );
    approx_eq(&y, &x, 1e-5);
}

/// Pure state decay: x=0, B=0, D=0, state_in=1, C=1, A=ln(0.5)/dt.
/// Expected: state and y decay by 0.5 each step.
#[test]
fn scan_pure_state_decay() {
    let (b, l, h, p, g, n) = (1, 3, 1, 2, 1, 2);
    let dt = vec![1.0; b * l * h];
    let a = vec![0.5_f32.ln(); h];
    let x = vec![0.0; b * l * h * p];
    let b_vec = vec![0.0; b * l * g * n];
    let c_vec = vec![1.0; b * l * g * n];
    let d = vec![0.0; h];
    let state_in = vec![1.0; b * h * p * n];

    let (y, s_out) = mlx_scan_collect(
        &x, &dt, &a, &b_vec, &c_vec, &d, &state_in, b as i32, l as i32, h as i32, p as i32,
        g as i32, n as i32,
    );

    // After L=3 steps each state element should be (1/2)^3 = 0.125.
    for v in &s_out {
        assert!((v - 0.125).abs() < 1e-5, "state decay: {v}");
    }
    // y[last,p] = sum_n state[last,p,n] * 1; n=2 lanes → 2 * 0.125 = 0.25.
    let last_y_start = (b * (l - 1) * h * p) as usize;
    for v in &y[last_y_start..last_y_start + (h * p) as usize] {
        assert!((v - 0.25).abs() < 1e-5, "last step y: {v}");
    }
}

/// Cross-check the full recurrence against the reference loop on a non-trivial
/// case with grouped B/C (heads_per_group = 2).
#[test]
fn scan_matches_reference_grouped() {
    let (b, l, h, p, g, n) = (1, 5, 4, 3, 2, 2);
    let x: Vec<f32> = (0..(b * l * h * p))
        .map(|i| (i as f32 * 0.013) - 0.2)
        .collect();
    let dt: Vec<f32> = (0..(b * l * h)).map(|i| 0.1 + i as f32 * 0.07).collect();
    let a: Vec<f32> = (0..h).map(|i| -0.3 - 0.1 * i as f32).collect();
    let b_vec: Vec<f32> = (0..(b * l * g * n))
        .map(|i| 0.05 + (i as f32 * 0.011))
        .collect();
    let c_vec: Vec<f32> = (0..(b * l * g * n))
        .map(|i| -0.04 + (i as f32 * 0.009))
        .collect();
    let d: Vec<f32> = (0..h).map(|i| 0.2 + 0.1 * i as f32).collect();
    let state_in: Vec<f32> = (0..(b * h * p * n))
        .map(|i| 0.02 + i as f32 * 0.003)
        .collect();

    let (y_ref, s_ref) =
        reference_scan(&x, &dt, &a, &b_vec, &c_vec, &d, &state_in, b, l, h, p, g, n);
    let (y_mlx, s_mlx) = mlx_scan_collect(
        &x, &dt, &a, &b_vec, &c_vec, &d, &state_in, b as i32, l as i32, h as i32, p as i32,
        g as i32, n as i32,
    );
    approx_eq(&y_mlx, &y_ref, 5e-5);
    approx_eq(&s_mlx, &s_ref, 5e-5);
}

/// Sanity: seq_len=1, batch=2, ungrouped (heads_per_group=2). Verifies single-
/// step decode path and that batch dim is honoured.
#[test]
fn scan_single_step_shape() {
    let (b, l, h, p, g, n) = (2, 1, 2, 2, 1, 2);
    let x: Vec<f32> = (0..(b * l * h * p)).map(|i| i as f32 * 0.01).collect();
    let dt = vec![0.5; b * l * h];
    let a = vec![-0.2; h];
    let b_vec: Vec<f32> = (0..(b * l * g * n))
        .map(|i| 0.1 + 0.01 * i as f32)
        .collect();
    let c_vec: Vec<f32> = (0..(b * l * g * n))
        .map(|i| 0.2 + 0.01 * i as f32)
        .collect();
    let d = vec![0.0; h];
    let state_in = vec![0.0; b * h * p * n];

    let (y_ref, s_ref) =
        reference_scan(&x, &dt, &a, &b_vec, &c_vec, &d, &state_in, b, l, h, p, g, n);
    let (y_mlx, s_mlx) = mlx_scan_collect(
        &x, &dt, &a, &b_vec, &c_vec, &d, &state_in, b as i32, l as i32, h as i32, p as i32,
        g as i32, n as i32,
    );
    assert_eq!(y_mlx.len(), b * l * h * p);
    assert_eq!(s_mlx.len(), b * h * p * n);
    approx_eq(&y_mlx, &y_ref, 5e-5);
    approx_eq(&s_mlx, &s_ref, 5e-5);
}
