//! MLX model graphs: in-tree decoders ([`qwen3`], [`llama`], …) plus vendored Higgs-style
//! checkpoints (Phi-3, Gemma 2, Starcoder2, Qwen3-Next, Qwen3-MoE) and shared loaders.

pub mod higgs_attn_utils;
pub mod higgs_kv;
pub mod higgs_turboquant_mlx;
pub mod higgs_weights;

pub mod gemma2;
pub mod gemma4;
pub mod phi3;
pub mod qwen3_moe;
pub mod qwen3_next;
// pub mod qwen3_5;
pub mod starcoder2;

pub mod llama;
pub mod mistral;
pub mod qwen2;
pub mod qwen3;
pub mod qwen3_5;
