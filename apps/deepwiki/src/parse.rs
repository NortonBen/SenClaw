use crate::lang;
use anyhow::{anyhow, Result};
use tree_sitter::{Node, Parser};

#[derive(Debug, Clone)]
pub struct ParsedSymbol {
    pub name: String,
    pub kind: String,
    pub parent: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub signature: String,
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedEdge {
    pub kind: String, // call | import
    pub src_symbol: Option<String>,
    pub target: String,
    pub line: i64,
}

#[derive(Debug, Default)]
pub struct ParseResult {
    pub symbols: Vec<ParsedSymbol>,
    pub edges: Vec<ParsedEdge>,
}

/// Parse a source file and extract its symbols and call/import edges.
pub fn parse(lang_name: &str, src: &str) -> Result<ParseResult> {
    let grammar = lang::grammar(lang_name).ok_or_else(|| anyhow!("unsupported language: {lang_name}"))?;
    let mut parser = Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|e| anyhow!("set_language failed: {e}"))?;
    let tree = parser.parse(src, None).ok_or_else(|| anyhow!("parse returned None"))?;

    let mut out = ParseResult::default();
    let bytes = src.as_bytes();
    let mut ctx = Ctx {
        lang: lang_name,
        bytes,
        enclosing_def: None,
        container: None,
    };
    walk(tree.root_node(), &mut ctx, &mut out);
    Ok(out)
}

struct Ctx<'a> {
    lang: &'a str,
    bytes: &'a [u8],
    /// Nearest enclosing function/method name (for edge attribution).
    enclosing_def: Option<String>,
    /// Nearest enclosing class/impl/struct/trait/interface name (symbol parent).
    container: Option<String>,
}

fn text<'a>(node: Node, bytes: &'a [u8]) -> &'a str {
    node.utf8_text(bytes).unwrap_or("")
}

/// Symbol kind for a definition node, or None if not a definition.
fn def_kind(lang: &str, kind: &str) -> Option<&'static str> {
    let k = match (lang, kind) {
        // Rust
        (_, "function_item") => "function",
        (_, "struct_item") | (_, "union_item") => "struct",
        (_, "enum_item") => "enum",
        (_, "trait_item") => "trait",
        (_, "impl_item") => "impl",
        (_, "mod_item") => "module",
        (_, "const_item") | (_, "static_item") => "const",
        (_, "type_item") => "type",
        (_, "macro_definition") => "macro",
        // Python
        (_, "function_definition") => "function",
        (_, "class_definition") => "class",
        // JS/TS
        (_, "function_declaration") | (_, "generator_function_declaration") => "function",
        (_, "method_definition") | (_, "method_declaration") => "method",
        (_, "class_declaration") => "class",
        (_, "interface_declaration") => "interface",
        (_, "type_alias_declaration") => "type",
        (_, "enum_declaration") => "enum",
        // Go
        (_, "type_spec") => "type",
        _ => return None,
    };
    Some(k)
}

/// True if this definition kind is a container that other symbols nest under.
fn is_container(kind: &str) -> bool {
    matches!(kind, "class" | "impl" | "struct" | "trait" | "interface" | "enum" | "module")
}

/// Extract the defined name from a definition node.
fn def_name(lang: &str, node: Node, bytes: &[u8]) -> Option<String> {
    // impl blocks key off the type they implement.
    if node.kind() == "impl_item" {
        if let Some(t) = node.child_by_field_name("type") {
            return Some(text(t, bytes).to_string());
        }
    }
    if let Some(n) = node.child_by_field_name("name") {
        return Some(text(n, bytes).to_string());
    }
    // JS/TS arrow/function assigned to a variable: variable_declarator name + function value.
    if node.kind() == "variable_declarator" {
        let val = node.child_by_field_name("value")?;
        if matches!(val.kind(), "arrow_function" | "function_expression") {
            if let Some(n) = node.child_by_field_name("name") {
                return Some(text(n, bytes).to_string());
            }
        }
        let _ = lang;
    }
    None
}

/// For a JS/TS `variable_declarator`, only treat it as a definition when the
/// value is a function. Returns the kind if so.
fn var_decl_kind(node: Node) -> Option<&'static str> {
    if node.kind() != "variable_declarator" {
        return None;
    }
    let val = node.child_by_field_name("value")?;
    if matches!(val.kind(), "arrow_function" | "function_expression") {
        Some("function")
    } else {
        None
    }
}

/// Extract a callee name from a call node, if this node is a call.
fn call_target(node: Node, bytes: &[u8]) -> Option<String> {
    match node.kind() {
        "call_expression" | "call" => {
            let f = node.child_by_field_name("function")?;
            callee_name(f, bytes)
        }
        "macro_invocation" => node
            .child_by_field_name("macro")
            .map(|n| text(n, bytes).to_string()),
        _ => None,
    }
}

fn callee_name(f: Node, bytes: &[u8]) -> Option<String> {
    match f.kind() {
        "identifier" => Some(text(f, bytes).to_string()),
        // a.b() / obj.method()
        "field_expression" => f
            .child_by_field_name("field")
            .map(|n| text(n, bytes).to_string()),
        "member_expression" => f
            .child_by_field_name("property")
            .map(|n| text(n, bytes).to_string()),
        "selector_expression" => f
            .child_by_field_name("field")
            .map(|n| text(n, bytes).to_string()),
        "attribute" => f
            .child_by_field_name("attribute")
            .map(|n| text(n, bytes).to_string()),
        "scoped_identifier" => f
            .child_by_field_name("name")
            .map(|n| text(n, bytes).to_string()),
        _ => None,
    }
}

/// Extract an import target from an import node, if this node is an import.
fn import_target(lang: &str, node: Node, bytes: &[u8]) -> Option<String> {
    let raw = match (lang, node.kind()) {
        ("rust", "use_declaration") => node
            .child_by_field_name("argument")
            .map(|n| text(n, bytes).to_string()),
        ("python", "import_from_statement") => node
            .child_by_field_name("module_name")
            .map(|n| text(n, bytes).to_string()),
        ("python", "import_statement") => node
            .child_by_field_name("name")
            .map(|n| text(n, bytes).to_string()),
        (_, "import_statement") => node
            .child_by_field_name("source")
            .map(|n| text(n, bytes).to_string()),
        ("go", "import_spec") => node
            .child_by_field_name("path")
            .map(|n| text(n, bytes).to_string()),
        _ => None,
    }?;
    Some(raw.trim_matches(|c| c == '"' || c == '\'' || c == '`').to_string())
}

fn signature(node: Node, bytes: &[u8]) -> String {
    let full = text(node, bytes);
    let first = full.lines().next().unwrap_or("").trim();
    let mut s = first.trim_end_matches('{').trim().to_string();
    if s.len() > 200 {
        s.truncate(200);
        s.push('…');
    }
    s
}

/// Leading line/block comment immediately preceding the node, if any.
fn doc_comment(node: Node, bytes: &[u8]) -> Option<String> {
    let mut sib = node.prev_sibling()?;
    // Skip attributes/decorators to find a comment.
    let mut guard = 0;
    while guard < 4 {
        let k = sib.kind();
        if k.contains("comment") {
            let t = text(sib, bytes).trim().to_string();
            if t.is_empty() {
                return None;
            }
            return Some(if t.len() > 400 { t[..400].to_string() } else { t });
        }
        if k == "attribute_item" || k == "decorator" {
            sib = sib.prev_sibling()?;
            guard += 1;
            continue;
        }
        break;
    }
    None
}

fn walk(node: Node, ctx: &mut Ctx, out: &mut ParseResult) {
    // Edges (calls / imports) attributed to the current enclosing symbol.
    if let Some(target) = call_target(node, ctx.bytes) {
        out.edges.push(ParsedEdge {
            kind: "call".into(),
            src_symbol: ctx.enclosing_def.clone(),
            target,
            line: node.start_position().row as i64 + 1,
        });
    }
    if let Some(target) = import_target(ctx.lang, node, ctx.bytes) {
        out.edges.push(ParsedEdge {
            kind: "import".into(),
            src_symbol: None,
            target,
            line: node.start_position().row as i64 + 1,
        });
    }

    // Definitions.
    let kind = def_kind(ctx.lang, node.kind()).or_else(|| var_decl_kind(node));
    if let Some(kind) = kind {
        if let Some(name) = def_name(ctx.lang, node, ctx.bytes) {
            let parent = if matches!(kind, "function" | "method" | "const" | "type") {
                ctx.container.clone()
            } else {
                None
            };
            out.symbols.push(ParsedSymbol {
                name: name.clone(),
                kind: kind.to_string(),
                parent,
                start_line: node.start_position().row as i64 + 1,
                end_line: node.end_position().row as i64 + 1,
                signature: signature(node, ctx.bytes),
                doc: doc_comment(node, ctx.bytes),
            });

            // Recurse with this symbol as the new context.
            let prev_def = ctx.enclosing_def.clone();
            let prev_container = ctx.container.clone();
            if matches!(kind, "function" | "method") {
                ctx.enclosing_def = Some(name.clone());
            }
            if is_container(kind) {
                ctx.container = Some(name.clone());
            }
            recurse_children(node, ctx, out);
            ctx.enclosing_def = prev_def;
            ctx.container = prev_container;
            return;
        }
    }

    recurse_children(node, ctx, out);
}

fn recurse_children(node: Node, ctx: &mut Ctx, out: &mut ParseResult) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, ctx, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_symbols_and_calls() {
        let src = r#"
/// adds two numbers
fn add(a: i32, b: i32) -> i32 { a + b }

struct Calc;
impl Calc {
    fn run(&self) -> i32 { add(1, 2) }
}
"#;
        let r = parse("rust", src).unwrap();
        let names: Vec<_> = r.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"add"));
        assert!(names.contains(&"Calc"));
        assert!(names.contains(&"run"));
        // call to add from inside run
        assert!(r
            .edges
            .iter()
            .any(|e| e.kind == "call" && e.target == "add" && e.src_symbol.as_deref() == Some("run")));
        // doc comment captured
        let add = r.symbols.iter().find(|s| s.name == "add").unwrap();
        assert!(add.doc.as_deref().unwrap_or("").contains("adds two"));
    }

    #[test]
    fn python_symbols() {
        let src = "def foo():\n    return bar()\n\nclass A:\n    def m(self):\n        return foo()\n";
        let r = parse("python", src).unwrap();
        let names: Vec<_> = r.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"A"));
        assert!(names.contains(&"m"));
        let m = r.symbols.iter().find(|s| s.name == "m").unwrap();
        assert_eq!(m.parent.as_deref(), Some("A"));
        assert!(r.edges.iter().any(|e| e.target == "bar" && e.kind == "call"));
    }

    #[test]
    fn typescript_symbols() {
        let src = "export function greet(n: string){ return hi(n); }\nconst add = (a:number,b:number)=> a+b;\ninterface P { x: number }\n";
        let r = parse("typescript", src).unwrap();
        let names: Vec<_> = r.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"add"));
        assert!(names.contains(&"P"));
    }

    #[test]
    fn go_symbols() {
        let src = "package main\nfunc Add(a int, b int) int { return a + b }\nfunc main(){ Add(1,2) }\n";
        let r = parse("go", src).unwrap();
        let names: Vec<_> = r.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Add"));
        assert!(names.contains(&"main"));
        assert!(r.edges.iter().any(|e| e.target == "Add" && e.kind == "call"));
    }
}
