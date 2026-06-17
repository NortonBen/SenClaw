//! Pure-Rust MLX port of **ZipVoice** flow-matching TTS (in progress).
//!
//! The `mlx-community/zipvoice-*` checkpoint is a flow-matching TTS:
//! a Zipformer2 **text encoder** conditions a U-Net Zipformer **flow-matching
//! decoder** (`fm_decoder`) that predicts a 100-channel mel; a *separate* Vocos
//! vocoder (not in this checkpoint) turns the mel into a 24 kHz waveform. It is
//! **zero-shot voice cloning** — synthesis needs a `(prompt_wav, prompt_text)`
//! reference, not just text.
//!
//! Implements [`crate::tts::TtsBackend`] under id `"zipvoice"`; the dispatch in
//! [`crate::tts::select_backend`] routes any non-`macos-speech` model id to
//! this backend (HuggingFace MLX models live here for now).
//!
//! ## Port status (foundation landed; audio path not yet wired)
//! - [x] [`config`]   — `config.json` parsing (tested)
//! - [x] [`tokenizer`] — `tokens.txt` token↔id table (tested)
//! - [x] [`weights`]  — safetensors index audit + array loader (tested)
//! - [x] [`scaling`]  — Zipformer2 primitives: SwooshL/R, BiasNorm (mlx-rs, tested)
//! - [x] [`blocks`]   — Linear, FeedForward, ConvolutionModule (mlx-rs, real-weights tested)
//! - [ ] Vietnamese grapheme→phoneme front-end (tokens are pinyin-style units)
//! - [ ] Zipformer2 attention blocks: NonlinAttention, RelPosMHA, encoder layer
//! - [ ] U-Net `fm_decoder` (5 stages, down/upsample + skips) + time embedding
//! - [ ] Flow-matching ODE solver (Euler/midpoint + classifier-free guidance)
//! - [ ] Vocos vocoder port + `charactr/vocos-mel-24khz` weights
//! - [ ] Reference-audio prompt handling + API surface for voice cloning
//!
//! Until the synthesis path is complete, [`ZipVoiceBackend::synthesize`]
//! returns [`TtsError::NotImplemented`] so the HTTP layer surfaces 501.

use super::{SynthesisRequest, TtsBackend, TtsError};

pub mod config;
pub mod tokenizer;
pub mod weights;

#[cfg(feature = "local-mlx-tts")]
pub mod scaling;

#[cfg(feature = "local-mlx-tts")]
pub mod blocks;

/// Marker / status message returned from the stub. Kept as a constant so the
/// contract test in `gateway::ui_server::tts::synth_tests` can pin it.
pub const STUB_MESSAGE: &str = "Pure-Rust MLX ZipVoice is under construction: \
    config, tokenizer, weight loader, and Zipformer2 primitives are implemented \
    and tested, but the encoder/flow-decoder/vocoder synthesis path is not yet wired.";

/// `TtsBackend` impl for the in-progress native ZipVoice runtime.
pub struct ZipVoiceBackend;

impl TtsBackend for ZipVoiceBackend {
    fn id(&self) -> &str {
        "zipvoice"
    }

    fn label(&self) -> &str {
        "ZipVoice (pure-Rust MLX, WIP)"
    }

    fn synthesize(&self, _req: &SynthesisRequest<'_>) -> Result<Vec<u8>, TtsError> {
        Err(TtsError::NotImplemented(STUB_MESSAGE.into()))
    }
}
