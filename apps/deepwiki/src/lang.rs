use tree_sitter::Language;

/// Map a file path's extension to a supported language name, if any.
pub fn lang_for_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let name = match ext.as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        _ => return None,
    };
    Some(name)
}

/// Build the tree-sitter grammar for a language name.
pub fn grammar(name: &str) -> Option<Language> {
    let lang: Language = match name {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        _ => return None,
    };
    Some(lang)
}

pub const SUPPORTED: &[&str] = &["rust", "python", "javascript", "typescript", "tsx", "go"];
