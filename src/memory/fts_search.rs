//! FTS5 + hybrid (vector+FTS) search. Mirrors `src-old/memory/fts-search.ts`.
//!
//! Retrieval strategy (progressive fallback):
//!   1. Embedding available → hybrid (vector 0.7 + FTS 0.3)
//!   2. No embedding → FTS5 (BM25)
//!   3. No FTS results → keyword substring fallback

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::db::Db;
use crate::memory::embedding::EmbeddingProvider;
use crate::memory::query_rewrite::{expand_query_tokens, smart_rewrite_query};
use crate::memory::tokenizer::{generate_2gram, tokenize_optimized};

// ===== Types =====

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
    pub score: f32,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub max_results: usize,
    pub min_score: f32,
    pub source: Option<String>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            max_results: 6,
            min_score: 0.25,
            source: None,
        }
    }
}

// ===== Main entry =====

pub async fn hybrid_search(
    db: &Db,
    folder: &str,
    query: &str,
    embedding_provider: Option<&dyn EmbeddingProvider>,
    options: SearchOptions,
) -> Result<Vec<SearchResult>> {
    let max_results = options.max_results;
    let min_score = options.min_score;
    let source_filter = options.source.as_deref().unwrap_or("all");

    if let Some(provider) = embedding_provider {
        match mixed_search(db, folder, query, provider, source_filter, max_results).await {
            Ok(results) if !results.is_empty() => {
                return Ok(results
                    .into_iter()
                    .filter(|r| r.score >= min_score)
                    .take(max_results)
                    .collect());
            }
            Err(e) => {
                tracing::warn!("[MemorySearch] Embedding search failed, falling back to FTS: {e}");
            }
            _ => {}
        }
    }

    let fts = db.with_conn(|c| fts_search(c, folder, query, source_filter, max_results * 2))?;
    if !fts.is_empty() {
        return Ok(fts.into_iter().take(max_results).collect());
    }

    db.with_conn(|c| keyword_fallback(c, folder, query, source_filter, max_results))
}

// ===== FTS5 search =====

fn fts_search(
    conn: &Connection,
    folder: &str,
    query: &str,
    source_filter: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let rewritten = smart_rewrite_query(query);
    let tokens = tokenize_optimized(&rewritten, true);
    if tokens.is_empty() {
        return Ok(vec![]);
    }

    let expanded = expand_query_tokens(&tokens);
    let sanitize = |t: &str| -> String {
        t.chars()
            .filter(|c| !matches!(c, '"' | '\'' | '`' | '(' | ')' | '*' | '^' | '-'))
            .collect()
    };
    let fts_query = expanded
        .iter()
        .map(|t| sanitize(t))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" OR ");
    if fts_query.is_empty() {
        return Ok(vec![]);
    }

    let rows: Vec<FtsRow> = if source_filter != "all" {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.path, c.start_line, c.end_line, c.text, c.source, \
             bm25(memory_chunks_fts) AS rank \
             FROM memory_chunks_fts f JOIN memory_chunks c ON c.id = f.chunk_id \
             WHERE f.text MATCH ?1 AND c.folder = ?2 AND c.source = ?3 \
             ORDER BY rank LIMIT ?4",
        )?;
        let mapped = stmt.query_map(
            params![fts_query, folder, source_filter, limit as i64],
            row_to_fts,
        )?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.path, c.start_line, c.end_line, c.text, c.source, \
             bm25(memory_chunks_fts) AS rank \
             FROM memory_chunks_fts f JOIN memory_chunks c ON c.id = f.chunk_id \
             WHERE f.text MATCH ?1 AND c.folder = ?2 \
             ORDER BY rank LIMIT ?3",
        )?;
        let mapped = stmt.query_map(params![fts_query, folder, limit as i64], row_to_fts)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };

    if rows.is_empty() {
        return Ok(vec![]);
    }

    let ranks: Vec<f64> = rows.iter().map(|r| r.rank).collect();
    let min_rank = ranks.iter().cloned().fold(f64::INFINITY, f64::min);
    let max_rank = ranks.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max_rank - min_rank;

    Ok(rows
        .into_iter()
        .map(|r| SearchResult {
            id: r.id,
            path: r.path,
            start_line: r.start_line,
            end_line: r.end_line,
            text: r.text,
            source: r.source,
            score: if range == 0.0 {
                1.0
            } else {
                ((max_rank - r.rank) / range) as f32
            },
        })
        .collect())
}

fn row_to_fts(row: &rusqlite::Row<'_>) -> rusqlite::Result<FtsRow> {
    Ok(FtsRow {
        id: row.get(0)?,
        path: row.get(1)?,
        start_line: row.get(2)?,
        end_line: row.get(3)?,
        text: row.get(4)?,
        source: row.get(5)?,
        rank: row.get(6)?,
    })
}

struct FtsRow {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    text: String,
    source: String,
    rank: f64,
}

// ===== Keyword substring fallback =====

fn keyword_fallback(
    conn: &Connection,
    folder: &str,
    query: &str,
    source_filter: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let rewritten = smart_rewrite_query(query);
    let tokens = tokenize_optimized(&rewritten, true);
    let ngrams = generate_2gram(&rewritten);
    let all_tokens: Vec<String> = tokens.into_iter().chain(ngrams).collect();
    if all_tokens.is_empty() {
        return Ok(vec![]);
    }

    let rows: Vec<ChunkRow> = if source_filter != "all" {
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, text, source FROM memory_chunks WHERE folder = ?1 AND source = ?2",
        )?;
        let mapped = stmt.query_map(params![folder, source_filter], row_to_chunk)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, text, source FROM memory_chunks WHERE folder = ?1",
        )?;
        let mapped = stmt.query_map(params![folder], row_to_chunk)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut results: Vec<SearchResult> = Vec::new();
    for row in &rows {
        let text_lower = row.text.to_lowercase();
        let row_tokens: std::collections::HashSet<String> = tokenize_optimized(&row.text, false)
            .into_iter()
            .chain(generate_2gram(&row.text))
            .collect();
        let match_count = all_tokens
            .iter()
            .filter(|t| row_tokens.contains(t.as_str()) || text_lower.contains(&t.to_lowercase()))
            .count();
        if match_count > 0 {
            results.push(SearchResult {
                id: row.id.clone(),
                path: row.path.clone(),
                start_line: row.start_line,
                end_line: row.end_line,
                text: row.text.clone(),
                source: row.source.clone(),
                score: match_count as f32 / all_tokens.len() as f32,
            });
        }
    }
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(results.into_iter().take(limit).collect())
}

fn row_to_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkRow> {
    Ok(ChunkRow {
        id: row.get(0)?,
        path: row.get(1)?,
        start_line: row.get(2)?,
        end_line: row.get(3)?,
        text: row.get(4)?,
        source: row.get(5)?,
    })
}

struct ChunkRow {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    text: String,
    source: String,
}

// ===== Mixed/hybrid search =====

async fn mixed_search(
    db: &Db,
    folder: &str,
    query: &str,
    provider: &dyn EmbeddingProvider,
    source_filter: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let emb = provider
        .embed(&[query.to_string()])
        .await
        .context("embed query")?;
    let q_emb = emb.into_iter().next().context("embed() returned empty")?;

    let vec_results = db.with_conn(|c| vec_search(c, folder, &q_emb, source_filter, limit * 2))?;
    if vec_results.is_empty() {
        return Ok(vec![]);
    }

    let fts_results = db.with_conn(|c| fts_search(c, folder, query, source_filter, limit * 2))?;

    let mut combined: std::collections::HashMap<String, SearchResult> =
        std::collections::HashMap::new();
    for r in vec_results {
        combined.insert(
            r.id.clone(),
            SearchResult {
                score: r.score * 0.7,
                ..r
            },
        );
    }
    for r in fts_results {
        if let Some(e) = combined.get_mut(&r.id) {
            // chunk found by both vector AND fts — combine weights
            e.score += r.score * 0.3;
        } else {
            // fts-only result gets 0.3 weight (not the vector weight of 0.7)
            combined.insert(
                r.id.clone(),
                SearchResult {
                    score: r.score * 0.3,
                    ..r
                },
            );
        }
    }
    let mut merged: Vec<SearchResult> = combined.into_values().collect();
    merged.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(merged.into_iter().take(limit).collect())
}

// ===== Vector search =====

fn vec_search(
    conn: &Connection,
    folder: &str,
    query_embedding: &[f32],
    source_filter: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    if let Ok(results) = try_vec0(conn, folder, query_embedding, source_filter, limit) {
        if !results.is_empty() {
            return Ok(results);
        }
    }
    try_blob_scan(conn, folder, query_embedding, source_filter, limit)
}

fn try_vec0(
    conn: &Connection,
    folder: &str,
    query_embedding: &[f32],
    source_filter: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let query_buf: Vec<u8> = query_embedding
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM memory_chunks_vec", [], |r| r.get(0))
        .unwrap_or(0);
    let k = total.max((limit * 2) as i64);

    let rows: Vec<VecDistanceRow> = if source_filter != "all" {
        let mut stmt = conn.prepare(
            "SELECT v.chunk_id, c.path, c.start_line, c.end_line, c.text, c.source, v.distance \
             FROM memory_chunks_vec v JOIN memory_chunks c ON c.id = v.chunk_id \
             WHERE v.embedding MATCH ?1 AND k = ?2 AND c.folder = ?3 AND c.source = ?4",
        )?;
        let mapped = stmt.query_map(
            params![query_buf, k, folder, source_filter],
            row_to_vec_dist,
        )?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut stmt = conn.prepare(
            "SELECT v.chunk_id, c.path, c.start_line, c.end_line, c.text, c.source, v.distance \
             FROM memory_chunks_vec v JOIN memory_chunks c ON c.id = v.chunk_id \
             WHERE v.embedding MATCH ?1 AND k = ?2 AND c.folder = ?3",
        )?;
        let mapped = stmt.query_map(params![query_buf, k, folder], row_to_vec_dist)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(normalize_vec_results(rows))
}

fn row_to_vec_dist(row: &rusqlite::Row<'_>) -> rusqlite::Result<VecDistanceRow> {
    Ok(VecDistanceRow {
        id: row.get(0)?,
        path: row.get(1)?,
        start_line: row.get(2)?,
        end_line: row.get(3)?,
        text: row.get(4)?,
        source: row.get(5)?,
        distance: row.get(6)?,
    })
}

fn try_blob_scan(
    conn: &Connection,
    folder: &str,
    query_embedding: &[f32],
    source_filter: &str,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let rows: Vec<ChunkEmbRow> = if source_filter != "all" {
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, text, source, embedding \
             FROM memory_chunks WHERE folder = ?1 AND source = ?2 AND embedding IS NOT NULL",
        )?;
        let mapped = stmt.query_map(params![folder, source_filter], row_to_chunk_emb)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, text, source, embedding \
             FROM memory_chunks WHERE folder = ?1 AND embedding IS NOT NULL",
        )?;
        let mapped = stmt.query_map(params![folder], row_to_chunk_emb)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let mut with_dist: Vec<VecDistanceRow> = rows
        .into_iter()
        .filter_map(|row| {
            let emb = row.embedding?;
            let dist = cosine_distance(query_embedding, &emb)?;
            Some(VecDistanceRow {
                id: row.id,
                path: row.path,
                start_line: row.start_line,
                end_line: row.end_line,
                text: row.text,
                source: row.source,
                distance: dist,
            })
        })
        .collect();

    with_dist.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    with_dist.truncate(limit * 2);
    Ok(normalize_vec_results(with_dist))
}

fn row_to_chunk_emb(row: &rusqlite::Row<'_>) -> rusqlite::Result<ChunkEmbRow> {
    Ok(ChunkEmbRow {
        id: row.get(0)?,
        path: row.get(1)?,
        start_line: row.get(2)?,
        end_line: row.get(3)?,
        text: row.get(4)?,
        source: row.get(5)?,
        embedding: row.get(6)?,
    })
}

fn cosine_distance(a: &[f32], b_blob: &[u8]) -> Option<f32> {
    if b_blob.len() % 4 != 0 {
        return None;
    }
    let b: Vec<f32> = b_blob
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if a.len() != b.len() {
        return None;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return Some(1.0);
    }
    Some(1.0 - dot / (na * nb))
}

fn normalize_vec_results(rows: Vec<VecDistanceRow>) -> Vec<SearchResult> {
    if rows.is_empty() {
        return vec![];
    }
    let distances: Vec<f32> = rows.iter().map(|r| r.distance).collect();
    let min_dist = distances.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_dist = distances.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let dist_range = max_dist - min_dist;
    if dist_range < 0.05 || min_dist > 0.6 {
        return vec![];
    }
    rows.into_iter()
        .map(|r| SearchResult {
            id: r.id,
            path: r.path,
            start_line: r.start_line,
            end_line: r.end_line,
            text: r.text,
            source: r.source,
            score: (max_dist - r.distance) / dist_range,
        })
        .collect()
}

struct VecDistanceRow {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    text: String,
    source: String,
    distance: f32,
}
struct ChunkEmbRow {
    id: String,
    path: String,
    start_line: u32,
    end_line: u32,
    text: String,
    source: String,
    embedding: Option<Vec<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_distance_identical() {
        let a: Vec<f32> = vec![1.0, 0.0, 0.0];
        let b: Vec<u8> = a.iter().flat_map(|f| f.to_le_bytes()).collect();
        let d = cosine_distance(&a, &b).unwrap();
        assert!(d < 0.001, "distance {d}");
    }

    #[test]
    fn cosine_distance_orthogonal() {
        let a = vec![1.0, 0.0];
        let b: Vec<u8> = vec![0.0f32, 1.0f32]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let d = cosine_distance(&a, &b).unwrap();
        assert!((d - 1.0).abs() < 0.001, "distance {d}");
    }
}
