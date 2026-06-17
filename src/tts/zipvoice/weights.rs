//! ZipVoice weight inventory + loader.
//!
//! The checkpoint ships sharded `safetensors` described by
//! `model.safetensors.index.json` (`{"weight_map": {tensor: shard, ...}}`).
//! [`WeightIndex`] parses that map and lets us *audit* the inventory — confirm
//! every expected module is present and grouped correctly — without the MLX
//! stack (pure-Rust, unit-testable).
//!
//! Actually materializing tensors into `mlx_rs::Array`s requires the MLX
//! feature and lives in [`load_arrays`] behind `local-mlx-tts`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RawIndex {
    #[serde(default)]
    weight_map: BTreeMap<String, String>,
}

/// Parsed `model.safetensors.index.json` — the tensor→shard map.
#[derive(Debug, Clone)]
pub struct WeightIndex {
    /// tensor name → shard filename.
    pub weight_map: BTreeMap<String, String>,
}

impl WeightIndex {
    pub fn from_json(s: &str) -> Result<Self> {
        // Some index files are a bare `{tensor: shard}` map without the
        // `weight_map` wrapper — accept both.
        let raw: RawIndex = serde_json::from_str(s).context("parsing index json")?;
        let weight_map = if raw.weight_map.is_empty() {
            serde_json::from_str::<BTreeMap<String, String>>(s)
                .context("parsing bare weight map")?
                .into_iter()
                // drop a possible "metadata" object key if present
                .filter(|(k, _)| k != "metadata")
                .collect()
        } else {
            raw.weight_map
        };
        if weight_map.is_empty() {
            return Err(anyhow!("empty weight map"));
        }
        Ok(Self { weight_map })
    }

    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let path = dir.as_ref().join("model.safetensors.index.json");
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&s)
    }

    pub fn len(&self) -> usize {
        self.weight_map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.weight_map.is_empty()
    }

    /// All tensor names sharing a top-level prefix (the part before the first `.`).
    pub fn count_with_prefix(&self, prefix: &str) -> usize {
        self.weight_map
            .keys()
            .filter(|k| k.starts_with(prefix))
            .count()
    }

    /// Count tensors grouped by their top-level module (`fm_decoder`, …).
    pub fn top_level_groups(&self) -> BTreeMap<String, usize> {
        let mut groups = BTreeMap::new();
        for k in self.weight_map.keys() {
            let top = k.split('.').next().unwrap_or(k).to_string();
            *groups.entry(top).or_insert(0) += 1;
        }
        groups
    }

    /// Distinct shard filenames referenced by the map.
    pub fn shards(&self) -> Vec<String> {
        let mut s: Vec<String> = self.weight_map.values().cloned().collect();
        s.sort();
        s.dedup();
        s
    }
}

/// Load every tensor named in the index into `mlx_rs::Array`s.
///
/// Reads each shard once via `Array::load_safetensors` and returns the merged
/// map. Errors if a tensor named in the index is absent from its shard.
#[cfg(feature = "local-mlx-tts")]
pub fn load_arrays(
    dir: impl AsRef<Path>,
    index: &WeightIndex,
) -> Result<std::collections::HashMap<String, mlx_rs::Array>> {
    use std::collections::{HashMap, HashSet};

    let dir = dir.as_ref();
    let mut by_shard: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    for (tensor, shard) in &index.weight_map {
        by_shard.entry(shard.clone()).or_default().insert(tensor.clone());
    }

    let mut out: HashMap<String, mlx_rs::Array> = HashMap::new();
    for (shard, wanted) in by_shard {
        let path = dir.join(&shard);
        let loaded = mlx_rs::Array::load_safetensors(&path)
            .map_err(|e| anyhow!("loading shard {}: {e}", path.display()))?;
        for name in wanted {
            let arr = loaded
                .get(&name)
                .ok_or_else(|| anyhow!("tensor {name} missing from shard {shard}"))?;
            out.insert(name, arr.clone());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_weight_map_wrapper() {
        let s = r#"{"metadata":{"total_size":10},
                    "weight_map":{
                        "text_encoder.embed.weight":"model.safetensors",
                        "fm_decoder.in_proj.weight":"model.safetensors",
                        "fm_decoder.in_proj.bias":"model.safetensors"
                    }}"#;
        let idx = WeightIndex::from_json(s).unwrap();
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.count_with_prefix("fm_decoder"), 2);
        assert_eq!(idx.count_with_prefix("text_encoder"), 1);
        let groups = idx.top_level_groups();
        assert_eq!(groups.get("fm_decoder"), Some(&2));
        assert_eq!(groups.get("text_encoder"), Some(&1));
        assert_eq!(idx.shards(), vec!["model.safetensors".to_string()]);
    }

    #[test]
    fn parses_bare_map() {
        let s = r#"{"a.b":"s1","c.d":"s2"}"#;
        let idx = WeightIndex::from_json(s).unwrap();
        assert_eq!(idx.len(), 2);
        assert_eq!(idx.shards().len(), 2);
    }

    #[test]
    fn rejects_empty() {
        assert!(WeightIndex::from_json(r#"{"weight_map":{}}"#).is_err());
    }

    /// Audit the real `zipvoice-vietnamese` checkpoint if present.
    /// Run with: `cargo test --features local-mlx-tts -- --ignored real_zipvoice`
    #[test]
    #[ignore = "requires the downloaded zipvoice-vietnamese checkpoint"]
    fn real_zipvoice_index_groups() {
        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join(".senclaw/tts-models/mlx-community__zipvoice-vietnamese");
        let idx = WeightIndex::from_dir(&dir).expect("load index");
        let groups = idx.top_level_groups();
        assert_eq!(idx.len(), 887, "total tensor count");
        assert_eq!(groups.get("fm_decoder"), Some(&710));
        assert_eq!(groups.get("text_encoder"), Some(&177));
    }

    /// Materialize every tensor and confirm none are missing from the shard.
    #[cfg(feature = "local-mlx-tts")]
    #[test]
    #[ignore = "requires the downloaded zipvoice-vietnamese checkpoint"]
    fn real_zipvoice_loads_all_arrays() {
        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join(".senclaw/tts-models/mlx-community__zipvoice-vietnamese");
        let idx = WeightIndex::from_dir(&dir).expect("load index");
        let arrays = super::load_arrays(&dir, &idx).expect("load arrays");
        assert_eq!(arrays.len(), idx.len(), "every indexed tensor loaded");
        // Spot-check a couple of known tensors are non-empty.
        let emb = arrays
            .get("text_encoder.embed.weight")
            .expect("embed weight present");
        assert!(emb.size() > 0);
    }
}
