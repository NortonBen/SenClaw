//! Built-in OCR model catalog.
//!
//! Each entry points at three URLs (det / rec / keys). PaddleOCR upstream
//! ships PaddlePaddle weights (`.pdiparams`), not the MNN format this engine
//! needs — so the URLs below target known community-hosted MNN mirrors. If a
//! mirror goes offline the user can:
//!
//!   1. Paste their own URLs through the **Custom Model** form in the Web UI.
//!   2. Drop their own `det.mnn` + `rec.mnn` + `keys.txt` straight into
//!      `~/.senclaw/ocr-models/{safe-id}/`.
//!   3. Convert from the official PaddleOCR `.pdiparams` themselves and host
//!      anywhere.
//!
//! The catalog is small on purpose; if you only ever OCR Vietnamese/English,
//! a single download covers it (`PP-OCRv5_mobile_latin`).

use std::path::{Path, PathBuf};

/// Filenames inside `{ocr_models_dir}/{safe-id}/`.
pub const DET_FILE: &str = "det.mnn";
pub const REC_FILE: &str = "rec.mnn";
pub const KEYS_FILE: &str = "keys.txt";

/// Default model id surfaced in Settings UI. Tiếng Việt + 40 ngôn ngữ Latin.
pub const DEFAULT_MODEL_ID: &str = "PP-OCRv5_mobile_latin";

/// One catalog entry.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub id: &'static str,
    pub label: &'static str,
    /// One-line description shown in the UI subtitle.
    pub description: &'static str,
    /// Direct URL to the detection model (`.mnn`).
    pub det_url: &'static str,
    /// Direct URL to the recognition model (`.mnn`).
    pub rec_url: &'static str,
    /// Direct URL to the charset (`ppocr_keys_*.txt`).
    pub keys_url: &'static str,
    /// Approximate **total** download size (det + rec + keys).
    pub approx_size_mb: f32,
    /// IETF tag suggested when this model is selected: `"vi"`, `"en"`, `"zh"`,
    /// `"multi"`, `"ja"`, `"ko"`.
    pub default_language: &'static str,
    /// PaddleOCR series — `4` or `5`.
    pub version: u8,
    /// True for the single recommended entry surfaced first in the UI.
    pub is_default: bool,
}

// Base URL for community-hosted MNN mirror. Single point of edit when a new
// mirror comes online.
const HF_BASE: &str = "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main";

/// Bundled catalog. The first entry (`is_default = true`) is the recommended
/// model for tiếng Việt + English; downloading it covers 95% of users.
pub static CATALOG: &[CatalogEntry] = &[
    // ────────── Default (Vietnamese + Latin) ──────────
    CatalogEntry {
        id: DEFAULT_MODEL_ID,
        label: "PP-OCRv5 mobile · Latin (mặc định)",
        description:
            "Tiếng Việt, English, French, German, Spanish, Portuguese, Indonesian, Malay và 40+ ngôn ngữ Latin khác.",
        det_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/PP-OCRv5_mobile_det.mnn"
        ),
        rec_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/latin_PP-OCRv5_mobile_rec.mnn"
        ),
        keys_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ppocr_keys_latin.txt"
        ),
        approx_size_mb: 17.0,
        default_language: "vi",
        version: 5,
        is_default: true,
    },
    // ────────── Multi-language (CN + EN + JP) ──────────
    CatalogEntry {
        id: "PP-OCRv5_mobile_ch",
        label: "PP-OCRv5 mobile · Chinese + English + Japanese",
        description: "Default multi-language rec — 简体中文, 繁體中文, English, 日本語, Pinyin.",
        det_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/PP-OCRv5_mobile_det.mnn"
        ),
        rec_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/PP-OCRv5_mobile_rec.mnn"
        ),
        keys_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ppocr_keys_v5.txt"
        ),
        approx_size_mb: 17.0,
        default_language: "multi",
        version: 5,
        is_default: false,
    },
    // ────────── FP16 variant (smaller, faster) ──────────
    CatalogEntry {
        id: "PP-OCRv5_mobile_ch_fp16",
        label: "PP-OCRv5 mobile FP16 · Chinese + English (nhẹ hơn ~50%)",
        description:
            "Cùng đa ngôn ngữ như bản trên, dung lượng & RAM giảm một nửa. Độ chính xác thấp hơn chút.",
        det_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/PP-OCRv5_mobile_det_fp16.mnn"
        ),
        rec_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/PP-OCRv5_mobile_rec_fp16.mnn"
        ),
        keys_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ppocr_keys_v5.txt"
        ),
        approx_size_mb: 9.0,
        default_language: "multi",
        version: 5,
        is_default: false,
    },
    // ────────── Korean ──────────
    CatalogEntry {
        id: "PP-OCRv5_mobile_korean",
        label: "PP-OCRv5 mobile · Korean (한국어)",
        description: "Chuyên biệt cho tiếng Hàn.",
        det_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/PP-OCRv5_mobile_det.mnn"
        ),
        rec_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/korean_PP-OCRv5_mobile_rec_infer.mnn"
        ),
        keys_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ppocr_keys_korean.txt"
        ),
        approx_size_mb: 17.0,
        default_language: "ko",
        version: 5,
        is_default: false,
    },
    // ────────── PP-OCRv4 legacy (Chinese + English) ──────────
    CatalogEntry {
        id: "PP-OCRv4_mobile_ch",
        label: "PP-OCRv4 mobile · Chinese + English (legacy)",
        description: "PaddleOCR v4 — phiên bản cũ, nhỏ hơn, tương thích phần cứng rộng hơn.",
        det_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ch_PP-OCRv4_det_infer.mnn"
        ),
        rec_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ch_PP-OCRv4_rec_infer.mnn"
        ),
        keys_url: concat!(
            "https://huggingface.co/zibo-chen/rust-paddle-ocr-models/resolve/main",
            "/ppocr_keys_v4.txt"
        ),
        approx_size_mb: 14.0,
        default_language: "multi",
        version: 4,
        is_default: false,
    },
];

// Silence unused-import warning when the HF_BASE constant becomes unused
// after switching catalog entries away from the helper.
#[allow(dead_code)]
const _HF_BASE_REF: &str = HF_BASE;

/// Sanitize an id for use as a directory name (matches the project-wide
/// convention used by `whisper.rs` and `tts.rs`).
pub fn safe_dirname(id: &str) -> String {
    id.replace('/', "__")
}

/// Return the absolute path to the model directory for `id`.
pub fn installed_model_dir(ocr_models_dir: &Path, id: &str) -> PathBuf {
    ocr_models_dir.join(safe_dirname(id))
}

/// `(det_path, rec_path, keys_path)` inside an installed model directory.
pub fn installed_model_files(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (dir.join(DET_FILE), dir.join(REC_FILE), dir.join(KEYS_FILE))
}

/// True iff all three expected files exist and are non-empty.
pub fn is_installed(dir: &Path) -> bool {
    let (det, rec, keys) = installed_model_files(dir);
    [det, rec, keys].iter().all(|p| {
        std::fs::metadata(p)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    })
}

pub fn lookup(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// The single recommended default — the one auto-selected after first install
/// when no explicit setting exists yet.
pub fn default_entry() -> &'static CatalogEntry {
    CATALOG
        .iter()
        .find(|e| e.is_default)
        .unwrap_or(&CATALOG[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_default_entry() {
        let defaults: Vec<_> = CATALOG.iter().filter(|e| e.is_default).collect();
        assert_eq!(
            defaults.len(),
            1,
            "expected exactly one is_default=true entry in CATALOG"
        );
        assert_eq!(defaults[0].id, DEFAULT_MODEL_ID);
    }

    #[test]
    fn default_entry_lookup() {
        assert_eq!(default_entry().id, DEFAULT_MODEL_ID);
    }

    #[test]
    fn all_urls_have_expected_extensions() {
        for e in CATALOG {
            assert!(e.det_url.ends_with(".mnn"), "det_url for {}", e.id);
            assert!(e.rec_url.ends_with(".mnn"), "rec_url for {}", e.id);
            assert!(e.keys_url.ends_with(".txt"), "keys_url for {}", e.id);
        }
    }

    #[test]
    fn lookup_finds_every_entry() {
        for e in CATALOG {
            assert!(lookup(e.id).is_some(), "lookup({}) returned None", e.id);
        }
    }
}
