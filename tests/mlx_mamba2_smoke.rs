//! End-to-end smoke test for the Mamba-2 loader + forward pass with **real**
//! `mlx-community/mamba2-*` weights.
//!
//! This test is `#[ignore]`d by default because it requires:
//! 1. Apple Silicon with `--features local-mlx` enabled.
//! 2. A downloaded checkpoint on disk (we do NOT fetch from HuggingFace here —
//!    this test is a smoke check, not a download harness).
//!
//! ## Running
//!
//! ```sh
//! # Point at any local mlx-community/mamba2-* directory.
//! export SENCLAW_MAMBA2_DIR=$HOME/models/mamba2-130m
//! cargo test --features local-mlx --test mlx_mamba2_smoke \
//!     -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! The test verifies, in order:
//! 1. `config.json` parses and the model instantiates with sane dims.
//! 2. Every safetensors key matches a parameter slot (zero unfilled).
//! 3. A prefill over a tiny prompt (`[1, 8]` token tensor) produces logits of
//!    the expected shape (`[1, 8, vocab_size]`) with finite values.
//! 4. A single decode step (`[1, 1]`) re-uses the SSM cache and again returns
//!    a finite logit row.
//!
//! Numerical *correctness* against the Python reference is out of scope here —
//! that requires comparing against a saved Python forward; we treat that as a
//! follow-up once the loader path is exercised on a real model.

#![cfg(feature = "local-mlx")]

use std::path::PathBuf;

use mlx_rs::{
    ops::indexing::{IndexOp, NewAxis},
    transforms::eval,
    Array,
};
use senclaw::local_model::mlx_lm::cache::KvCache;
use senclaw::local_model::mlx_lm::models::mamba2::{
    get_mamba2_model_args, load_mamba2_model, SequentialScan,
};

fn weights_dir() -> Option<PathBuf> {
    std::env::var_os("SENCLAW_MAMBA2_DIR").map(PathBuf::from)
}

fn check_finite(name: &str, arr: &Array) {
    eval(&[arr.clone()]).expect("eval");
    let slice = arr.as_slice::<f32>();
    let bad = slice
        .iter()
        .enumerate()
        .find(|(_, v)| !v.is_finite())
        .map(|(i, v)| (i, *v));
    assert!(
        bad.is_none(),
        "{name} has non-finite value at index {:?}",
        bad
    );
}

#[test]
#[ignore = "requires SENCLAW_MAMBA2_DIR=<path/to/mlx-community/mamba2-*> on disk"]
fn config_parses_and_dims_are_consistent() {
    let dir = weights_dir().expect("SENCLAW_MAMBA2_DIR not set");
    let args = get_mamba2_model_args(&dir).expect("config.json parse");
    assert!(args.hidden_size > 0);
    assert!(args.num_hidden_layers > 0);
    assert_eq!(
        args.intermediate_size % args.num_heads,
        0,
        "intermediate_size must be divisible by num_heads"
    );
    assert_eq!(
        args.num_heads % args.n_groups.max(1),
        0,
        "num_heads must be divisible by n_groups (grouped B/C share heads)"
    );
    assert_eq!(
        args.intermediate_size / args.num_heads,
        args.head_dim,
        "head_dim should equal intermediate_size / num_heads"
    );
    eprintln!(
        "[smoke] model_type={} hidden={} layers={} d_inner={} n_heads={} head_dim={} n_groups={} d_state={} d_conv={} vocab={}",
        args.model_type,
        args.hidden_size,
        args.num_hidden_layers,
        args.intermediate_size,
        args.num_heads,
        args.head_dim,
        args.n_groups,
        args.state_size,
        args.conv_kernel,
        args.vocab_size,
    );
}

#[test]
#[ignore = "requires SENCLAW_MAMBA2_DIR=<path/to/mlx-community/mamba2-*> on disk"]
fn loads_weights_and_runs_prefill_decode() {
    let dir = weights_dir().expect("SENCLAW_MAMBA2_DIR not set");

    let mut model = load_mamba2_model(&dir).expect("load_mamba2_model");
    let vocab = model.args.vocab_size;
    let mut cache: Vec<Option<KvCache>> = model.make_cache();
    let scan = SequentialScan;

    // --- prefill -------------------------------------------------------------
    // Use a deterministic short prompt; values < vocab to be safe.
    let prompt_ids: Vec<u32> = (0..8u32).map(|i| (i * 31) % (vocab as u32)).collect();
    let prompt = Array::from_slice(
        &prompt_ids.iter().map(|&x| x as i32).collect::<Vec<i32>>(),
        &[1, 8],
    );
    let logits = model
        .forward(&prompt, &mut cache, &scan)
        .expect("prefill forward");
    let shape = logits.shape();
    assert_eq!(shape, &[1_i32, 8, vocab], "prefill logits shape");
    check_finite("prefill logits", &logits);

    // --- single decode step --------------------------------------------------
    let last = logits.index((.., -1, ..));
    eval(&[last.clone()]).expect("eval last");
    // Pick argmax token (cheap manual argmax to avoid extra macro deps).
    let last_flat = last.as_slice::<f32>();
    let next_tok = (0..vocab as usize)
        .max_by(|&a, &b| {
            last_flat[a]
                .partial_cmp(&last_flat[b])
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0) as i32;
    let next = Array::from_slice(&[next_tok], &[1, 1]);
    let logits2 = model
        .forward(&next, &mut cache, &scan)
        .expect("decode forward");
    assert_eq!(logits2.shape(), &[1_i32, 1, vocab], "decode logits shape");
    check_finite("decode logits", &logits2);

    eprintln!(
        "[smoke] prefill+decode OK — tokens_seen per layer = {}",
        cache
            .iter()
            .filter_map(|c| c.as_ref())
            .filter_map(|c| match c {
                KvCache::Mamba2(_) => Some(()),
                _ => None,
            })
            .count()
    );
    // Touch NewAxis import so it isn't pruned by feature pruning later.
    let _ = NewAxis;
}
