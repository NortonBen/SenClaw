//! Character tokenizer for HF `VitsTokenizer` (`facebook/mms-tts-*`).
//!
//! Mirrors `transformers/models/vits/tokenization_vits.py` for the MMS case:
//! `normalize=true`, `add_blank=true`, `phonemize=false`, `is_uroman=false`
//! (Vietnamese is Latin-script — no romanizer needed).
//!
//! Pipeline: NFC-compose → lowercase chars not in vocab → drop chars outside
//! the vocab (punctuation) → trim → intersperse the blank/pad id between every
//! character: `[pad, c1, pad, c2, …, pad]`.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use unicode_normalization::UnicodeNormalization;

/// The blank/pad token id VITS intersperses between characters. For every MMS
/// checkpoint the pad token is the vocab entry with id 0.
pub const BLANK_ID: u32 = 0;

#[derive(Debug, Clone)]
pub struct VitsTokenizer {
    vocab: HashMap<char, u32>,
}

impl VitsTokenizer {
    /// Build from a `vocab.json` string (single-char token → id).
    pub fn from_vocab_json(s: &str) -> Result<Self> {
        let raw: HashMap<String, u32> = serde_json::from_str(s).context("parsing vocab.json")?;
        let mut vocab = HashMap::with_capacity(raw.len());
        for (tok, id) in raw {
            let mut chars = tok.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                // MMS vocabs are strictly single-character; skip anything else
                // (e.g. a hypothetical multi-char special token) rather than fail.
                continue;
            };
            vocab.insert(c, id);
        }
        if vocab.is_empty() {
            return Err(anyhow!("vocab.json contained no usable tokens"));
        }
        Ok(Self { vocab })
    }

    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let path = dir.as_ref().join("vocab.json");
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::from_vocab_json(&s)
    }

    /// Normalized text after casing + vocab filtering (pre-interspersing).
    /// Exposed for diagnostics ("what will actually be spoken").
    pub fn normalize(&self, text: &str) -> String {
        let mut filtered = String::with_capacity(text.len());
        for c in text.nfc() {
            if self.vocab.contains_key(&c) {
                // Keep vocab characters verbatim (HF respects exact matches first).
                filtered.push(c);
            } else {
                // Lowercase, then keep only what the vocab knows (drops punctuation).
                for lc in c.to_lowercase() {
                    if self.vocab.contains_key(&lc) {
                        filtered.push(lc);
                    }
                }
            }
        }
        filtered.trim().to_string()
    }

    /// Encode to model input ids: normalized chars interspersed with [`BLANK_ID`].
    /// Returns an error when nothing in the text survives normalization —
    /// synthesizing silence for unspeakable input would be a confusing no-op.
    pub fn encode(&self, text: &str) -> Result<Vec<u32>> {
        let norm = self.normalize(text);
        if norm.is_empty() {
            return Err(anyhow!(
                "no speakable characters left after tokenizer normalization"
            ));
        }
        let mut ids = Vec::with_capacity(norm.chars().count() * 2 + 1);
        ids.push(BLANK_ID);
        for c in norm.chars() {
            ids.push(self.vocab[&c]);
            ids.push(BLANK_ID);
        }
        Ok(ids)
    }

    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toy() -> VitsTokenizer {
        // Subset of the real facebook/mms-tts-vie vocab (real ids).
        VitsTokenizer::from_vocab_json(
            r#"{"ụ":0,"x":1,"i":30,"n":90," ":84,"c":13,"h":85,"à":35,"o":31,"đ":55,"ẹ":72,"p":78,"t":80,"r":92,"ờ":36}"#,
        )
        .unwrap()
    }

    #[test]
    fn encodes_with_interspersed_blanks() {
        let t = toy();
        let ids = t.encode("xin").unwrap();
        assert_eq!(ids, vec![0, 1, 0, 30, 0, 90, 0]);
    }

    #[test]
    fn lowercases_and_strips_punctuation() {
        let t = toy();
        // Uppercase folds to the vocab char; comma/exclamation are dropped.
        assert_eq!(t.normalize("Xin chào!"), "xin chào");
        let ids = t.encode("Xin, chào!").unwrap();
        // ", " collapses to the surviving space between words.
        assert_eq!(ids, vec![0, 1, 0, 30, 0, 90, 0, 84, 0, 13, 0, 85, 0, 35, 0, 31, 0]);
    }

    #[test]
    fn nfc_composes_decomposed_vietnamese() {
        let t = toy();
        // "trời" typed in NFD (o + combining horn + combining grave).
        let nfd = "tr\u{006F}\u{031B}\u{0300}i";
        assert_eq!(t.normalize(nfd), "trời");
    }

    #[test]
    fn rejects_unspeakable_input() {
        let t = toy();
        assert!(t.encode("!!! ???").is_err());
        assert!(t.encode("").is_err());
    }
}
