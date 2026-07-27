//! Built-in materials — port of `internal/material/seed.go`. The embedded
//! `material_builtin.json` is the Go `builtin.json` copied verbatim.

use crate::db::Db;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

const BUILTIN_JSON: &str = include_str!("material_builtin.json");

/// One row of the `material` table (mirrors the Go Entry struct).
#[derive(Deserialize, Clone, Default)]
pub struct Entry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub style_instruction: String,
    #[serde(default)]
    pub negative_prompt: String,
    #[serde(default)]
    pub scene_prefix: String,
    #[serde(default)]
    pub lighting: String,
}

fn with_defaults(key: String, mut e: Entry) -> Entry {
    if e.id.is_empty() {
        e.id = key;
    }
    if e.lighting.is_empty() {
        e.lighting = "Studio lighting, highly detailed".to_string();
    }
    e
}

/// All built-in materials from the embedded JSON (map id → entry).
pub fn builtin() -> Vec<Entry> {
    let raw: HashMap<String, Entry> = serde_json::from_str(BUILTIN_JSON).unwrap_or_default();
    let mut v: Vec<Entry> = raw.into_iter().map(|(k, e)| with_defaults(k, e)).collect();
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

/// Insert-or-ignore every built-in (safe to call repeatedly). Returns the
/// number of rows actually inserted.
pub fn seed(db: &Db) -> usize {
    let mut inserted = 0;
    for e in builtin() {
        match db.execute(
            "INSERT OR IGNORE INTO material(id,name,style_instruction,negative_prompt,scene_prefix,lighting,is_builtin) \
             VALUES(?1,?2,?3,?4,?5,?6,1)",
            &[&e.id, &e.name, &e.style_instruction, &e.negative_prompt, &e.scene_prefix, &e.lighting],
        ) {
            Ok(n) => inserted += n,
            Err(err) => eprintln!("seed material {:?}: {err}", e.id),
        }
    }
    inserted
}

/// Upsert every built-in, overwriting rows with the same id. Returns the
/// number of rows written.
pub fn restore(db: &Db) -> usize {
    let mut updated = 0;
    for e in builtin() {
        match db.execute(
            "INSERT INTO material(id,name,style_instruction,negative_prompt,scene_prefix,lighting,is_builtin) \
             VALUES(?1,?2,?3,?4,?5,?6,1) \
             ON CONFLICT(id) DO UPDATE SET \
               name=excluded.name, \
               style_instruction=excluded.style_instruction, \
               negative_prompt=excluded.negative_prompt, \
               scene_prefix=excluded.scene_prefix, \
               lighting=excluded.lighting, \
               is_builtin=1",
            &[&e.id, &e.name, &e.style_instruction, &e.negative_prompt, &e.scene_prefix, &e.lighting],
        ) {
            Ok(_) => updated += 1,
            Err(err) => eprintln!("restore material {:?}: {err}", e.id),
        }
    }
    updated
}

/// Import materials from a request body: either `{"path": "..."}` pointing at
/// a JSON file on disk, or a raw `{id: entry}` map. Insert-or-ignore; returns
/// `(inserted, skipped)`.
pub fn import(db: &Db, body: &[u8]) -> Result<(usize, usize), String> {
    let mut source: Vec<u8> = body.to_vec();
    if let Ok(v) = serde_json::from_slice::<Value>(body) {
        if let Some(p) = v.get("path").and_then(|x| x.as_str()) {
            if !p.trim().is_empty() {
                source = std::fs::read(p.trim()).map_err(|e| format!("read path failed: {e}"))?;
            }
        }
    }
    let raw: HashMap<String, Entry> =
        serde_json::from_slice(&source).map_err(|e| format!("invalid JSON: {e}"))?;
    let mut inserted = 0;
    let mut skipped = 0;
    for (key, e) in raw {
        let e = with_defaults(key, e);
        let n = db
            .execute(
                "INSERT OR IGNORE INTO material(id,name,style_instruction,negative_prompt,scene_prefix,lighting) \
                 VALUES(?1,?2,?3,?4,?5,?6)",
                &[&e.id, &e.name, &e.style_instruction, &e.negative_prompt, &e.scene_prefix, &e.lighting],
            )
            .map_err(|err| format!("insert {:?}: {err}", e.id))?;
        if n > 0 {
            inserted += 1;
        } else {
            skipped += 1;
        }
    }
    Ok((inserted, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_parses() {
        let v = builtin();
        assert!(v.iter().any(|e| e.id == "realistic"));
        assert!(v.iter().all(|e| !e.lighting.is_empty()));
    }

    #[test]
    fn seed_then_restore() {
        let db = Db::open_memory().unwrap();
        let n = seed(&db);
        assert!(n > 0);
        assert_eq!(seed(&db), 0); // idempotent
        assert_eq!(restore(&db), n);
    }

    #[test]
    fn import_raw_map() {
        let db = Db::open_memory().unwrap();
        let body = br#"{"custom": {"name": "Custom", "style_instruction": "x"}}"#;
        let (ins, skip) = import(&db, body).unwrap();
        assert_eq!((ins, skip), (1, 0));
        let (ins, skip) = import(&db, body).unwrap();
        assert_eq!((ins, skip), (0, 1));
    }
}
