//! Smoke test for Qwen3.5 OptiQ — forward + decode token sanity.
#![cfg(feature = "local-mlx")]

use std::path::PathBuf;

use mlx_rs::{
    ops::indexing::IndexOp,
    transforms::eval,
    Array,
};
use senclaw::local_model::mlx_lm::models::qwen3_5::load_qwen35_model;

fn dir() -> Option<PathBuf> {
    std::env::var_os("SENCLAW_QWEN35_DIR").map(PathBuf::from)
}

#[test]
#[ignore = "requires SENCLAW_QWEN35_DIR"]
fn prefill_decode_not_all_zero_token() {
    let model_dir = dir().expect("SENCLAW_QWEN35_DIR");
    let mut model = load_qwen35_model(&model_dir).expect("load");
    let mut cache = model.make_cache();
    // token ids: "hi" approx — use a few real ids from vocab
    let prompt: Vec<u32> = vec![248045, 8678, 198];
    let inputs = Array::from_slice(&prompt, &[1, prompt.len() as i32]);
    let logits = model
        .forward(&inputs, &mut cache, 0)
        .expect("prefill forward");
    eval(&[logits.clone()]).expect("eval logits");
    let last = logits.index((.., -1, ..));
    eval(&[last.clone()]).expect("eval last");
    let row = last.reshape(&[model.args.text_config.vocab_size]).expect("reshape");
    eval(&[row.clone()]).expect("eval row");
    let slice = row.as_slice::<f32>();
    let (best_idx, best_val) = slice
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
        .map(|(i, v)| (i, *v))
        .unwrap();
    eprintln!("argmax token id = {best_idx}, logit = {best_val}");
    assert!(best_val.is_finite(), "logit not finite");
    assert_ne!(best_idx, 0, "collapsed to token 0 — logits likely broken");
    for _ in 0..5 {
        let tok = Array::from_slice(&[best_idx as u32], &[1, 1]);
        let logits = model.forward(&tok, &mut cache, prompt.len()).expect("decode");
        eval(&[logits.clone()]).expect("eval");
        let last = logits.index((.., -1, ..));
        eval(&[last.clone()]).expect("eval last");
        let row = last.reshape(&[model.args.text_config.vocab_size]).expect("reshape");
        let slice = row.as_slice::<f32>();
        let (idx, val) = slice
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, v)| (i, *v))
            .unwrap();
        eprintln!("decode argmax = {idx}, logit = {val}");
        assert!(val.is_finite());
        assert_ne!(idx, 0, "decode step collapsed to token 0");
    }
}
