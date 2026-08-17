//! Everything the local-model Space Apps share except the engine itself.
//!
//! `mlx-lm` and `candle` are the same app twice over — download a checkpoint,
//! list what is installed, keep sampling settings, serve OpenAI — differing only
//! in what actually runs the weights. That shared half lives here so the two
//! cannot drift apart on the parts a user notices: where models are stored, what
//! a setting means, and what the management screen can do.
//!
//! - [`store`] — the shared model root (`~/.senclaw/local-models`), id
//!   normalisation, what is installed.
//! - [`download`] — HuggingFace fetch with progress, resume and cancel.
//! - [`settings`] — sampling and memory knobs, and what `None` defers to.
//! - [`api`] — the management REST surface both apps mount.

pub mod api;
pub mod download;
pub mod settings;
pub mod store;

pub use api::EngineHost;
pub use settings::Settings;
