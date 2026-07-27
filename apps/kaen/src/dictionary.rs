//! Dictionary lookup with a local SQLite cache — port of kaizen's `dictionary`
//! module. kaizen scraped Cambridge; Kaen uses the free dictionaryapi.dev for
//! IPA/definition/audio and Google's public translate endpoint for the
//! word translation. Both results are cached forever, so each word costs at
//! most one network round-trip per source.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::db::Db;
use crate::srs;

fn cached_entry(db: &Db, word: &str) -> Result<Option<Value>> {
    db.with(|c| {
        c.query_row(
            "SELECT word, ipa, part_of_speech, definition, examples, audio_url, audio_us, audio_uk
             FROM dictionary_entries WHERE word = ?1",
            params![word],
            |r| {
                let examples: Option<String> = r.get(4)?;
                Ok(json!({
                    "word": r.get::<_, String>(0)?,
                    "ipa": r.get::<_, Option<String>>(1)?,
                    "partOfSpeech": r.get::<_, Option<String>>(2)?,
                    "definition": r.get::<_, Option<String>>(3)?,
                    "examples": examples
                        .and_then(|e| serde_json::from_str::<Value>(&e).ok())
                        .unwrap_or(json!([])),
                    "audioUrl": r.get::<_, Option<String>>(5)?,
                    "audioUs": r.get::<_, Option<String>>(6)?,
                    "audioUk": r.get::<_, Option<String>>(7)?,
                }))
            },
        )
        .optional()
    })
    .map_err(Into::into)
}

fn save_entry(db: &Db, entry: &Value) -> Result<()> {
    db.with(|c| {
        c.execute(
            "INSERT OR REPLACE INTO dictionary_entries
               (word, ipa, part_of_speech, definition, examples, audio_url, audio_us, audio_uk, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry["word"].as_str().unwrap_or(""),
                entry["ipa"].as_str(),
                entry["partOfSpeech"].as_str(),
                entry["definition"].as_str(),
                entry["examples"].to_string(),
                entry["audioUrl"].as_str(),
                entry["audioUs"].as_str(),
                entry["audioUk"].as_str(),
                srs::fmt(Utc::now()),
            ],
        )
    })?;
    Ok(())
}

fn cached_translation(db: &Db, word: &str, lang: &str) -> Result<Option<String>> {
    db.with(|c| {
        c.query_row(
            "SELECT translation FROM dictionary_translations WHERE word = ?1 AND target_lang = ?2",
            params![word, lang],
            |r| r.get(0),
        )
        .optional()
    })
    .map_err(Into::into)
}

fn save_translation(db: &Db, word: &str, lang: &str, translation: &str) -> Result<()> {
    db.with(|c| {
        c.execute(
            "INSERT OR REPLACE INTO dictionary_translations (word, target_lang, translation) VALUES (?1, ?2, ?3)",
            params![word, lang, translation],
        )
    })?;
    Ok(())
}

/// Parse one dictionaryapi.dev response into kaizen's entry shape.
pub fn parse_dictionaryapi(word: &str, body: &Value) -> Option<Value> {
    let first = body.as_array()?.first()?;
    let ipa = first["phonetic"].as_str().map(String::from).or_else(|| {
        first["phonetics"]
            .as_array()?
            .iter()
            .find_map(|p| p["text"].as_str().map(String::from))
    });
    let mut audio_us = None;
    let mut audio_uk = None;
    let mut audio_any = None;
    if let Some(phonetics) = first["phonetics"].as_array() {
        for p in phonetics {
            let Some(url) = p["audio"].as_str().filter(|u| !u.is_empty()) else {
                continue;
            };
            if url.contains("-us.") && audio_us.is_none() {
                audio_us = Some(url.to_string());
            } else if url.contains("-uk.") && audio_uk.is_none() {
                audio_uk = Some(url.to_string());
            }
            audio_any.get_or_insert_with(|| url.to_string());
        }
    }
    let meaning = first["meanings"].as_array()?.first()?;
    let part_of_speech = meaning["partOfSpeech"].as_str().map(String::from);
    let defs = meaning["definitions"].as_array()?;
    let definition = defs.first()?["definition"].as_str().map(String::from);
    let examples: Vec<String> = defs
        .iter()
        .filter_map(|d| d["example"].as_str().map(String::from))
        .take(3)
        .collect();
    Some(json!({
        "word": word,
        "ipa": ipa,
        "partOfSpeech": part_of_speech,
        "definition": definition,
        "examples": examples,
        "audioUrl": audio_any,
        "audioUs": audio_us,
        "audioUk": audio_uk,
    }))
}

async fn fetch_entry(word: &str) -> Option<Value> {
    let url = format!(
        "https://api.dictionaryapi.dev/api/v2/entries/en/{}",
        urlencode(word)
    );
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    parse_dictionaryapi(word, &body)
}

/// Google's public single-translate endpoint (same one kaizen leaned on) —
/// keyless; on any failure the lookup simply has no translation.
async fn fetch_translation(word: &str, lang: &str) -> Option<String> {
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl={}&dt=t&q={}",
        urlencode(lang),
        urlencode(word)
    );
    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    let body: Value = resp.json().await.ok()?;
    let translated: String = body[0]
        .as_array()?
        .iter()
        .filter_map(|seg| seg[0].as_str())
        .collect();
    (!translated.trim().is_empty()).then(|| translated.trim().to_string())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// GET /dictionary/lookup?word&targetLang — cache-first on both sources.
pub async fn lookup(db: &Db, word: &str, target_lang: &str) -> Result<Value> {
    let word = word.trim().to_lowercase();
    let is_long_sentence = word.split_whitespace().count() > 5;

    let mut entry = match cached_entry(db, &word)? {
        Some(e) => e,
        None => {
            let fetched = if is_long_sentence { None } else { fetch_entry(&word).await };
            match fetched {
                Some(e) => {
                    save_entry(db, &e)?;
                    e
                }
                None => json!({
                    "word": word,
                    "ipa": Value::Null,
                    "partOfSpeech": if is_long_sentence { json!("sentence") } else { Value::Null },
                    "definition": Value::Null,
                    "examples": [],
                    "audioUrl": Value::Null,
                    "audioUs": Value::Null,
                    "audioUk": Value::Null,
                }),
            }
        }
    };

    let mut translation = Value::Null;
    if !target_lang.is_empty() && target_lang != "en" {
        translation = match cached_translation(db, &word, target_lang)? {
            Some(t) => json!(t),
            None => match fetch_translation(&word, target_lang).await {
                Some(t) => {
                    save_translation(db, &word, target_lang, &t)?;
                    json!(t)
                }
                None => Value::Null,
            },
        };
    }
    entry
        .as_object_mut()
        .unwrap()
        .insert("translation".into(), translation);
    Ok(entry)
}

/// GET /dictionary/audio?word.
pub async fn audio_url(db: &Db, word: &str) -> Result<Value> {
    let entry = lookup(db, word, "en").await?;
    let url = entry["audioUrl"]
        .as_str()
        .or_else(|| entry["audioUs"].as_str())
        .or_else(|| entry["audioUk"].as_str());
    Ok(json!({ "word": entry["word"], "audioUrl": url }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dictionaryapi_shape() {
        let body = json!([{
            "word": "apple",
            "phonetic": "/ˈæp.əl/",
            "phonetics": [
                { "text": "/ˈæp.əl/", "audio": "https://x/apple-us.mp3" },
                { "text": "/ˈæp.əl/", "audio": "https://x/apple-uk.mp3" }
            ],
            "meanings": [{
                "partOfSpeech": "noun",
                "definitions": [
                    { "definition": "A round fruit.", "example": "I ate an apple." },
                    { "definition": "A tree." }
                ]
            }]
        }]);
        let e = parse_dictionaryapi("apple", &body).unwrap();
        assert_eq!(e["ipa"], "/ˈæp.əl/");
        assert_eq!(e["partOfSpeech"], "noun");
        assert_eq!(e["definition"], "A round fruit.");
        assert_eq!(e["audioUs"], "https://x/apple-us.mp3");
        assert_eq!(e["audioUk"], "https://x/apple-uk.mp3");
        assert_eq!(e["examples"][0], "I ate an apple.");
    }

    #[test]
    fn cache_round_trips_entries_and_translations() {
        let db = Db::open_memory().unwrap();
        let entry = json!({
            "word": "run", "ipa": "/rʌn/", "partOfSpeech": "verb",
            "definition": "To move fast.", "examples": ["Run home."],
            "audioUrl": Value::Null, "audioUs": Value::Null, "audioUk": Value::Null,
        });
        save_entry(&db, &entry).unwrap();
        let back = cached_entry(&db, "run").unwrap().unwrap();
        assert_eq!(back["definition"], "To move fast.");
        assert_eq!(back["examples"][0], "Run home.");

        save_translation(&db, "run", "vi", "chạy").unwrap();
        assert_eq!(cached_translation(&db, "run", "vi").unwrap().unwrap(), "chạy");
        assert!(cached_translation(&db, "run", "jp").unwrap().is_none());
    }

    #[test]
    fn urlencode_handles_spaces_and_unicode() {
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("vi"), "vi");
        assert!(urlencode("chạy").contains('%'));
    }
}
