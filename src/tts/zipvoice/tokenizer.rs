//! ZipVoice token table (`tokens.txt`).
//!
//! Faithful port of the token↔id half of `k2-fsa/ZipVoice`'s tokenizer
//! (`zipvoice/tokenizer/tokenizer.py`). The file is one `"{token}\t{id}"` per
//! line; `_` is the padding token (id 0).
//!
//! **Scope:** this is the *lookup* layer only — it maps an already-produced
//! sequence of token strings to ids (and back). For the Vietnamese checkpoint
//! the tokens are pinyin-style phoneme/syllable units (`uang1`, `zh0`, `ê1`),
//! so turning raw text into those tokens requires a grapheme→phoneme (G2P)
//! front-end that is **not yet implemented** (see [`super`] roadmap). The
//! lookup layer is dependency-free and unit-tested against the real file.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

/// Padding token string (maps to id 0 in every ZipVoice token file).
pub const PAD_TOKEN: &str = "_";

/// Bidirectional token↔id table loaded from `tokens.txt`.
#[derive(Debug, Clone)]
pub struct TokenTable {
    token2id: HashMap<String, i32>,
    id2token: HashMap<i32, String>,
    pad_id: i32,
}

impl TokenTable {
    /// Parse the `"{token}\t{id}"` lines of a `tokens.txt`.
    pub fn from_text(contents: &str) -> Result<Self> {
        let mut token2id = HashMap::new();
        let mut id2token = HashMap::new();
        for (lineno, line) in contents.lines().enumerate() {
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            // Reference splits on a single tab; the token itself may be a space
            // (" \t3"), so split on the *last* tab to keep that token intact.
            let (token, id_str) = line
                .rsplit_once('\t')
                .ok_or_else(|| anyhow!("line {}: missing tab separator: {:?}", lineno + 1, line))?;
            let id: i32 = id_str
                .parse()
                .with_context(|| format!("line {}: bad id {:?}", lineno + 1, id_str))?;
            if token2id.insert(token.to_string(), id).is_some() {
                return Err(anyhow!("duplicate token {:?}", token));
            }
            id2token.insert(id, token.to_string());
        }
        let pad_id = *token2id
            .get(PAD_TOKEN)
            .ok_or_else(|| anyhow!("token file missing padding token {:?}", PAD_TOKEN))?;
        Ok(Self {
            token2id,
            id2token,
            pad_id,
        })
    }

    /// Load `<dir>/tokens.txt`.
    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let path = dir.as_ref().join("tokens.txt");
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::from_text(&s)
    }

    pub fn vocab_size(&self) -> usize {
        self.token2id.len()
    }

    pub fn pad_id(&self) -> i32 {
        self.pad_id
    }

    pub fn id_of(&self, token: &str) -> Option<i32> {
        self.token2id.get(token).copied()
    }

    pub fn token_of(&self, id: i32) -> Option<&str> {
        self.id2token.get(&id).map(|s| s.as_str())
    }

    /// Map token strings to ids, **skipping OOV** tokens — matching the
    /// reference `tokens_to_token_ids` (it logs and drops unknown tokens).
    pub fn tokens_to_ids<'a, I>(&self, tokens: I) -> Vec<i32>
    where
        I: IntoIterator<Item = &'a str>,
    {
        tokens
            .into_iter()
            .filter_map(|t| self.token2id.get(t).copied())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "_\t0\n^\t1\n$\t2\n \t3\n!\t4\na\t14\nuang1\t322\nê1\t356\n";

    #[test]
    fn parses_specials_and_lookups() {
        let t = TokenTable::from_text(SAMPLE).expect("parse");
        assert_eq!(t.pad_id(), 0);
        assert_eq!(t.vocab_size(), 8);
        assert_eq!(t.id_of("a"), Some(14));
        assert_eq!(t.id_of("uang1"), Some(322));
        assert_eq!(t.id_of("ê1"), Some(356));
        assert_eq!(t.token_of(3), Some(" ")); // space token survives the split
        assert_eq!(t.id_of("nope"), None);
    }

    #[test]
    fn tokens_to_ids_skips_oov() {
        let t = TokenTable::from_text(SAMPLE).expect("parse");
        let ids = t.tokens_to_ids(["a", "zzz", "uang1"]);
        assert_eq!(ids, vec![14, 322]); // "zzz" dropped
    }

    #[test]
    fn rejects_missing_pad() {
        assert!(TokenTable::from_text("a\t0\nb\t1\n").is_err());
    }

    #[test]
    fn rejects_duplicate() {
        assert!(TokenTable::from_text("_\t0\na\t1\na\t2\n").is_err());
    }
}
