//! In-memory prefix cache for multi-turn KV reuse.
//!
//! Multi-turn agent chats repeatedly send prompts of the form
//! `[system + N tools + turn1 + turn2 + … + turnK]`. For a tools-heavy
//! deployment (≥ 100 MCP tools), the **system + tools** block alone is 12–15 K
//! tokens — repeating prefill for it every turn is the dominant latency cost.
//!
//! `PrefixCache` stores the post-prefill [`KvCache`] snapshot keyed by the
//! exact token-id prefix that produced it. On the next turn the engine looks
//! for the **longest** cached prefix that matches the incoming prompt and
//! restores those KV writes verbatim — prefill then only runs for the
//! *suffix* (the new user / assistant / tool turn).
//!
//! ## Why "in-memory only" (not persisted to disk)
//!
//! - KV is 2–6 GB per cached prefix (`max_kv_tokens × layers × heads × dim
//!   × 2 B`). Disk IO on Apple Silicon swaps a 60 ms prefill for a 200–500 ms
//!   read — net negative.
//! - On `idle_unload`, the whole [`Loaded`] is dropped, which drops the
//!   prefix cache too. Re-loading after a long idle means re-running prefill
//!   once; that's correct behaviour (model weights aren't in memory either).
//! - Cross-restart persistence would require versioning the cache against
//!   model weights + tokenizer + chat-template hashes — not worth the
//!   complexity for a per-machine convenience.
//!
//! ## Cache invalidation
//!
//! Entries are evicted LRU when [`MAX_ENTRIES`] is exceeded. Each generation
//! turn stores its post-prefill state under the prompt prefix (excluding the
//! final `<|im_start|>assistant\n` generation suffix, so different turns of
//! the same conversation share their common prefix).

use std::collections::VecDeque;

use super::cache::KvCache;

/// Maximum number of cached prefix entries. Each entry holds a full per-layer
/// KV snapshot (~2-6 GB for a 4B model with 32K KV window). Keep small.
pub const MAX_ENTRIES: usize = 4;

/// Minimum prefix length to bother caching. Short prefixes (a few hundred
/// tokens) prefill in well under a second — the cache lookup/clone overhead
/// would dwarf the savings.
pub const MIN_PREFIX_LEN: usize = 1024;

/// One cached `(prompt-prefix, post-prefill KV state)` pair.
pub struct PrefixCacheEntry {
    /// The exact prompt token ids that produced `caches`.
    pub tokens: Vec<u32>,
    /// Per-layer KV snapshot taken immediately after `forward_chunked` (or
    /// the single-shot prefill) finished. Same layer order as the live
    /// `Vec<Option<KvCache>>` that callers pass into the model's forward.
    pub caches: Vec<Option<KvCache>>,
    /// RoPE absolute position after the cached prefill — caller resumes
    /// decode at `rope_offset = tokens.len()` after restore.
    pub rope_offset: usize,
}

/// Small fixed-capacity LRU. We could use the `lru` crate but a 4-entry
/// `VecDeque` walked linearly is fast enough and avoids the dependency.
pub struct PrefixCache {
    entries: VecDeque<PrefixCacheEntry>,
}

impl PrefixCache {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_ENTRIES),
        }
    }

    /// Find the entry whose cached `tokens` is the **longest** prefix of
    /// `prompt`. Returns `None` when no entry shares ≥ [`MIN_PREFIX_LEN`]
    /// tokens with `prompt` (cache miss).
    ///
    /// O(`entries × min(prompt_len, cached_len)`) — fine for ≤ 4 entries.
    pub fn find_longest_match(&self, prompt: &[u32]) -> Option<&PrefixCacheEntry> {
        let mut best: Option<&PrefixCacheEntry> = None;
        let mut best_len = MIN_PREFIX_LEN.saturating_sub(1);
        for entry in &self.entries {
            if entry.tokens.len() > prompt.len() {
                continue;
            }
            if entry.tokens.len() <= best_len {
                continue;
            }
            // Compare full cached prefix against prompt.
            if entry.tokens.as_slice() == &prompt[..entry.tokens.len()] {
                best_len = entry.tokens.len();
                best = Some(entry);
            }
        }
        best
    }

    /// Insert `(tokens, caches, rope_offset)`. Touches LRU: existing entry
    /// with the exact same tokens is removed first (re-inserted to the
    /// front). Oldest entry evicted when full.
    pub fn store(
        &mut self,
        tokens: Vec<u32>,
        caches: Vec<Option<KvCache>>,
        rope_offset: usize,
    ) {
        self.entries.retain(|e| e.tokens != tokens);
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_back();
        }
        self.entries.push_front(PrefixCacheEntry {
            tokens,
            caches,
            rope_offset,
        });
    }

    /// Snapshot all cache variants in `live` via [`KvCache::try_snapshot`].
    /// Returns `None` if any layer's cache cannot be cloned (e.g. TurboQuant
    /// in active state) — in that case the caller skips caching this turn.
    pub fn snapshot_layers(live: &[Option<KvCache>]) -> Option<Vec<Option<KvCache>>> {
        live.iter()
            .map(|opt| match opt {
                Some(c) => c.try_snapshot().map(Some),
                None => Some(None),
            })
            .collect()
    }

    /// Snapshot + trim the last `trim` tokens from each layer's KV. Used to
    /// store a prefix cache entry whose key excludes the assistant
    /// generation suffix — so the next turn's prompt (which lacks that
    /// suffix at the same position) still hits the cache.
    pub fn snapshot_layers_trimmed(
        live: &[Option<KvCache>],
        trim: usize,
    ) -> Option<Vec<Option<KvCache>>> {
        let mut snap = Self::snapshot_layers(live)?;
        if trim > 0 {
            for slot in snap.iter_mut().flatten() {
                slot.trim_by(trim);
            }
        }
        Some(snap)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for PrefixCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(toks: Vec<u32>) -> PrefixCacheEntry {
        PrefixCacheEntry {
            tokens: toks,
            caches: vec![],
            rope_offset: 0,
        }
    }

    #[test]
    fn longest_prefix_wins_over_shorter_match() {
        let mut pc = PrefixCache::new();
        // Make MIN_PREFIX_LEN-token vec for the test.
        let short: Vec<u32> = (0..MIN_PREFIX_LEN as u32).collect();
        let long: Vec<u32> = (0..(MIN_PREFIX_LEN as u32 + 100)).collect();
        let prompt: Vec<u32> = (0..(MIN_PREFIX_LEN as u32 + 500)).collect();
        pc.entries.push_back(entry(short.clone()));
        pc.entries.push_back(entry(long.clone()));
        let hit = pc.find_longest_match(&prompt).expect("should hit");
        assert_eq!(hit.tokens.len(), long.len());
    }

    #[test]
    fn short_prefix_below_min_is_ignored() {
        let mut pc = PrefixCache::new();
        let tiny: Vec<u32> = (0..100).collect();
        let prompt: Vec<u32> = (0..200).collect();
        pc.entries.push_back(entry(tiny));
        assert!(pc.find_longest_match(&prompt).is_none());
    }

    #[test]
    fn divergent_prefix_is_a_miss() {
        let mut pc = PrefixCache::new();
        let cached: Vec<u32> = (0..(MIN_PREFIX_LEN as u32 + 50)).collect();
        let mut prompt = cached.clone();
        prompt[10] = 99_999; // diverge inside the prefix
        prompt.extend(0..100);
        pc.entries.push_back(entry(cached));
        assert!(pc.find_longest_match(&prompt).is_none());
    }

    #[test]
    fn lru_evicts_oldest_when_full() {
        let mut pc = PrefixCache::new();
        for i in 0..(MAX_ENTRIES + 2) {
            let toks: Vec<u32> = std::iter::repeat(i as u32).take(MIN_PREFIX_LEN + 10).collect();
            pc.store(toks, vec![], 0);
        }
        assert_eq!(pc.len(), MAX_ENTRIES);
    }
}
