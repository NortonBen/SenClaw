//! Registry of curated local models.
//!
//! Sizes are approximate disk footprints of the MLX 4-bit conversions.

#[derive(Debug, Clone, Copy)]
pub struct KnownModel {
    pub id: &'static str,
    pub label: &'static str,
    pub approx_size_gb: f32,
    pub context_length: u32,
    /// Whether the in-tree Qwen3 MLX loader supports this checkpoint today.
    /// Used by the UI to mark "pending upstream" entries.
    pub native_supported: bool,
}

pub const KNOWN_MODELS: &[KnownModel] = &[
    // ── Qwen3 bf16 — native Rust loader works on unquantized weights ─────
    KnownModel {
        id: "mlx-community/Qwen3-0.6B-bf16",
        label: "Qwen3 0.6B (bf16)",
        approx_size_gb: 1.2,
        context_length: 32_768,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-1.7B-bf16",
        label: "Qwen3 1.7B (bf16)",
        approx_size_gb: 3.4,
        context_length: 32_768,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-4B-bf16",
        label: "Qwen3 4B (bf16) — recommended",
        approx_size_gb: 8.0,
        context_length: 32_768,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-8B-bf16",
        label: "Qwen3 8B (bf16)",
        approx_size_gb: 16.0,
        context_length: 32_768,
        native_supported: true,
    },

    // ── Qwen3 4-bit — loaded via `nn::quantize` + custom safetensor remap
    //    (mlx-rs stores quantized weights at `*.inner.weight`).
    KnownModel {
        id: "mlx-community/Qwen3-4B-4bit",
        label: "Qwen3 4B 4-bit",
        approx_size_gb: 2.3,
        context_length: 32_768,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-8B-4bit",
        label: "Qwen3 8B 4-bit",
        approx_size_gb: 4.6,
        context_length: 32_768,
        native_supported: true,
    },
];

pub fn find(id: &str) -> Option<&'static KnownModel> {
    KNOWN_MODELS.iter().find(|m| m.id == id)
}
