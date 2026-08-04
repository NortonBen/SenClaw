use tree_sitter::Language;

/// Map a file path's extension to a supported language name, if any.
pub fn lang_for_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let name = match ext.as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "go" => "go",
        "sh" | "bash" => "bash",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "java" => "java",
        "rb" => "ruby",
        "php" | "phtml" => "php",
        "scala" | "sc" => "scala",
        "ml" | "mli" => "ocaml",
        "hs" => "haskell",
        "jl" => "julia",
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
        "bash" => tree_sitter_bash::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "csharp" => tree_sitter_c_sharp::LANGUAGE.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "ruby" => tree_sitter_ruby::LANGUAGE.into(),
        "php" => tree_sitter_php::LANGUAGE_PHP.into(),
        "scala" => tree_sitter_scala::LANGUAGE.into(),
        "ocaml" => tree_sitter_ocaml::LANGUAGE_OCAML.into(),
        "haskell" => tree_sitter_haskell::LANGUAGE.into(),
        "julia" => tree_sitter_julia::LANGUAGE.into(),
        _ => return None,
    };
    Some(lang)
}

pub const SUPPORTED: &[&str] = &[
    "rust",
    "python",
    "javascript",
    "typescript",
    "tsx",
    "go",
    "bash",
    "c",
    "cpp",
    "csharp",
    "java",
    "ruby",
    "php",
    "scala",
    "ocaml",
    "haskell",
    "julia",
];
