//! Title search across wiki markdown files.

use std::path::Path;

use anyhow::Result;

use super::manager::WikiManager;
use super::types::SearchResult;

impl WikiManager {
    /// Search wiki documents by filename, H1 title, or tags.
    /// When query is empty, returns all documents (useful for tag-only filtering).
    pub fn search(
        &self,
        query: &str,
        filter_tags: Option<&[String]>,
        limit: Option<usize>,
    ) -> Result<Vec<SearchResult>> {
        let limit = limit.unwrap_or(20);
        let query_lower = query.to_lowercase();
        let filter_tags = filter_tags.unwrap_or(&[]);
        let mut results: Vec<SearchResult> = Vec::new();

        for (rel_path, content) in self.collect_md_files() {
            let (fm, _) = Self::parse_frontmatter(&content);
            let title = Self::extract_title(&content, &rel_path);
            let title_lower = title.to_lowercase();
            let filename_lower = Path::new(&rel_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let tags_lower: Vec<String> = fm.tags.iter().map(|t| t.to_lowercase()).collect();

            if !filter_tags.is_empty()
                && !filter_tags
                    .iter()
                    .any(|t| tags_lower.contains(&t.to_lowercase()))
            {
                continue;
            }

            let matches = query.is_empty()
                || filename_lower.contains(&query_lower)
                || title_lower.contains(&query_lower)
                || tags_lower.iter().any(|t| t.contains(&query_lower));

            if matches {
                results.push(SearchResult {
                    path: rel_path,
                    title,
                    tags: fm.tags.clone(),
                    updated: fm.updated.clone(),
                });
            }
        }

        results.sort_by(|a, b| b.updated.cmp(&a.updated));
        Ok(results.into_iter().take(limit).collect())
    }
}
