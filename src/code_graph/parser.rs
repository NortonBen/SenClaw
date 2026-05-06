//! Tree-sitter parser — extracts symbols (nodes) and relationships (edges) from source files.
//!
//! Languages with full symbol + edge extraction:
//!   Rust, C, C++, C#, Go, Java, Scala, TypeScript, JavaScript, Python, Ruby, PHP, Bash,
//!   Haskell, OCaml, Julia, Verilog, Agda
//!
//! Languages with minimal / no code-graph extraction (not useful for call graphs):
//!   HTML, CSS, JSON  — parsed but return empty graph

use anyhow::Result;
use tree_sitter::{Language, Node, Parser};

use super::types::{EdgeKind, Language as Lang, NodeKind, ParsedFile, RawEdge, RawNode};

// ─── Grammar dispatch ─────────────────────────────────────────────────────────

fn ts_language(lang: Lang) -> Option<Language> {
    match lang {
        Lang::Rust       => Some(tree_sitter_rust::LANGUAGE.into()),
        Lang::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Lang::JavaScript => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        Lang::Python     => Some(tree_sitter_python::LANGUAGE.into()),
        Lang::Bash       => Some(tree_sitter_bash::LANGUAGE.into()),
        Lang::C          => Some(tree_sitter_c::LANGUAGE.into()),
        Lang::Cpp        => Some(tree_sitter_cpp::LANGUAGE.into()),
        Lang::CSharp     => Some(tree_sitter_c_sharp::LANGUAGE.into()),
        Lang::Go         => Some(tree_sitter_go::LANGUAGE.into()),
        Lang::Haskell    => Some(tree_sitter_haskell::LANGUAGE.into()),
        Lang::Java       => Some(tree_sitter_java::LANGUAGE.into()),
        Lang::Ruby       => Some(tree_sitter_ruby::LANGUAGE.into()),
        Lang::Scala      => Some(tree_sitter_scala::LANGUAGE.into()),
        Lang::OCaml      => Some(tree_sitter_ocaml::LANGUAGE_OCAML.into()),
        Lang::PHP        => Some(tree_sitter_php::LANGUAGE_PHP.into()),
        Lang::Julia      => Some(tree_sitter_julia::LANGUAGE.into()),
        Lang::Verilog    => Some(tree_sitter_verilog::LANGUAGE.into()),
        Lang::Agda       => Some(tree_sitter_agda::LANGUAGE.into()),
        Lang::HTML       => Some(tree_sitter_html::LANGUAGE.into()),
        Lang::CSS        => Some(tree_sitter_css::LANGUAGE.into()),
        Lang::JSON       => Some(tree_sitter_json::LANGUAGE.into()),
        Lang::Unknown    => None,
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub struct CodeParser;

impl CodeParser {
    pub fn parse_file(file_path: &str, source: &str, lang: Lang) -> Result<ParsedFile> {
        let ts_lang = match ts_language(lang) {
            Some(l) => l,
            None => return Ok(ParsedFile { file_path: file_path.to_string(), language: lang, ..Default::default() }),
        };

        let mut parser = Parser::new();
        parser.set_language(&ts_lang)?;

        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return Ok(ParsedFile { file_path: file_path.to_string(), language: lang, ..Default::default() }),
        };

        // Languages where extraction isn't meaningful — just return empty
        if !lang.is_indexable() {
            return Ok(ParsedFile { file_path: file_path.to_string(), language: lang, ..Default::default() });
        }

        let root = tree.root_node();
        let src  = source.as_bytes();

        let mut result = ParsedFile {
            file_path: file_path.to_string(),
            language: lang,
            ..Default::default()
        };

        match lang {
            Lang::Rust       => { walk_rust_nodes(root, src, &mut result, None); walk_rust_edges(root, src, &mut result, ""); }
            Lang::TypeScript | Lang::JavaScript => { walk_ts_nodes(root, src, &mut result, None); walk_ts_edges(root, src, &mut result, ""); }
            Lang::Python     => { walk_python_nodes(root, src, &mut result, None); walk_python_edges(root, src, &mut result, ""); }
            Lang::C | Lang::Cpp => { walk_c_nodes(root, src, &mut result, lang); walk_c_edges(root, src, &mut result, ""); }
            Lang::CSharp     => { walk_csharp_nodes(root, src, &mut result, None); walk_csharp_edges(root, src, &mut result, ""); }
            Lang::Go         => { walk_go_nodes(root, src, &mut result); walk_go_edges(root, src, &mut result, ""); }
            Lang::Java       => { walk_java_nodes(root, src, &mut result, None); walk_java_edges(root, src, &mut result, ""); }
            Lang::Scala      => { walk_scala_nodes(root, src, &mut result, None); walk_scala_edges(root, src, &mut result, ""); }
            Lang::Ruby       => { walk_ruby_nodes(root, src, &mut result, None); walk_ruby_edges(root, src, &mut result, ""); }
            Lang::PHP        => { walk_php_nodes(root, src, &mut result, None); walk_php_edges(root, src, &mut result, ""); }
            Lang::Bash       => { walk_bash_nodes(root, src, &mut result); walk_bash_edges(root, src, &mut result, ""); }
            Lang::Haskell    => { walk_haskell_nodes(root, src, &mut result); walk_haskell_edges(root, src, &mut result, ""); }
            Lang::OCaml      => { walk_ocaml_nodes(root, src, &mut result, None); walk_ocaml_edges(root, src, &mut result, ""); }
            Lang::Julia      => { walk_julia_nodes(root, src, &mut result, None); walk_julia_edges(root, src, &mut result, ""); }
            Lang::Verilog    => { walk_verilog_nodes(root, src, &mut result); }
            Lang::Agda       => { walk_agda_nodes(root, src, &mut result); }
            _ => {}
        }

        Ok(result)
    }
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

fn node_text(node: Node, src: &[u8]) -> String {
    node.utf8_text(src).unwrap_or("").to_string()
}

fn has_child_kind(node: Node, kind: &str) -> bool {
    (0..node.child_count()).any(|i| node.child(i).map(|c| c.kind() == kind).unwrap_or(false))
}

fn push_node(out: &mut ParsedFile, name: String, kind: NodeKind, sig: Option<String>, node: Node) {
    if name.is_empty() { return; }
    out.nodes.push(RawNode {
        name,
        kind,
        signature: sig,
        start_line: node.start_position().row as u32 + 1,
        end_line:   node.end_position().row as u32 + 1,
    });
}

fn push_edge(out: &mut ParsedFile, from: &str, to: String, to_file: Option<String>, kind: EdgeKind, node: Node) {
    if to.is_empty() || to == from { return; }
    out.edges.push(RawEdge {
        from_name: from.to_string(),
        to_name:   to,
        to_file,
        kind,
        at_line: node.start_position().row as u32 + 1,
    });
}

fn line(node: Node) -> u32 { node.start_position().row as u32 + 1 }

// ─── Rust ─────────────────────────────────────────────────────────────────────

fn walk_rust_nodes(node: Node, src: &[u8], out: &mut ParsedFile, impl_ctx: Option<&str>) {
    match node.kind() {
        "function_item" => {
            if let Some(name_n) = node.child_by_field_name("name") {
                let name = node_text(name_n, src);
                let is_async = has_child_kind(node, "async");
                let kind = if impl_ctx.is_some() { NodeKind::Method }
                           else if is_async { NodeKind::AsyncFunction }
                           else { NodeKind::Function };
                push_node(out, name, kind, Some(rust_fn_sig(&node, src, impl_ctx)), node);
            }
            for i in 0..node.child_count() {
                walk_rust_nodes(node.child(i).unwrap(), src, out, impl_ctx);
            }
            return;
        }
        "struct_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("struct {name}")), node);
            }
        }
        "enum_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Enum, Some(format!("enum {name}")), node);
            }
        }
        "trait_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let tname = node_text(n, src);
                push_node(out, tname.clone(), NodeKind::Trait, Some(format!("trait {tname}")), node);
                if let Some(body) = node.child_by_field_name("body") {
                    for i in 0..body.child_count() {
                        let m = body.child(i).unwrap();
                        if matches!(m.kind(), "function_item" | "function_signature_item") {
                            if let Some(fn_n) = m.child_by_field_name("name") {
                                push_node(out, node_text(fn_n, src), NodeKind::Method,
                                    Some(rust_fn_sig(&m, src, Some(&tname))), m);
                            }
                        }
                    }
                }
            }
            return;
        }
        "type_alias_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let rhs = node.child_by_field_name("type").map(|t| node_text(t, src)).unwrap_or_default();
                push_node(out, name.clone(), NodeKind::Type, Some(format!("type {name} = {rhs}")), node);
            }
        }
        "const_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let ty = node.child_by_field_name("type").map(|t| node_text(t, src)).unwrap_or_default();
                push_node(out, name.clone(), NodeKind::Const, Some(format!("const {name}: {ty}")), node);
            }
        }
        "static_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let ty = node.child_by_field_name("type").map(|t| node_text(t, src)).unwrap_or_default();
                let m = if has_child_kind(node, "mutable_specifier") { "mut " } else { "" };
                push_node(out, name.clone(), NodeKind::Const, Some(format!("static {m}{name}: {ty}")), node);
            }
        }
        "mod_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("mod {name}")), node);
            }
        }
        "impl_item" => {
            let impl_type = node.child_by_field_name("type").map(|n| node_text(n, src));
            for i in 0..node.child_count() {
                walk_rust_nodes(node.child(i).unwrap(), src, out, impl_type.as_deref());
            }
            return;
        }
        "macro_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Function, Some(format!("macro_rules! {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_rust_nodes(node.child(i).unwrap(), src, out, impl_ctx); }
}

fn walk_rust_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "function_item" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                push_edge(out, &current, rust_callee(f, src), None, EdgeKind::Calls, node);
            }
        }
        "method_call_expression" => {
            if let Some(m) = node.child_by_field_name("name") {
                push_edge(out, &current, node_text(m, src), None, EdgeKind::Calls, node);
            }
        }
        "use_declaration" => {
            let text = node_text(node, src);
            let module = text.trim_start_matches("use ").trim_end_matches(';').trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        "extern_crate_declaration" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" {
                    let fp = out.file_path.clone();
                    push_edge(out, &fp, node_text(c, src), None, EdgeKind::Imports, node);
                    break;
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_rust_edges(node.child(i).unwrap(), src, out, &current); }
}

fn rust_callee(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier"        => node_text(node, src),
        "scoped_identifier" => node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| node_text(node, src)),
        "field_expression"  => node.child_by_field_name("field").map(|n| node_text(n, src)).unwrap_or_default(),
        "generic_function"  => node.child_by_field_name("function").map(|n| rust_callee(n, src)).unwrap_or_default(),
        _ => String::new(),
    }
}

fn rust_fn_sig(node: &Node, src: &[u8], impl_ctx: Option<&str>) -> String {
    let asyncness = if has_child_kind(*node, "async") { "async " } else { "" };
    let name = node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_default();
    let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_else(|| "()".to_string());
    let ret = node.child_by_field_name("return_type").map(|r| format!(" -> {}", node_text(r, src))).unwrap_or_default();
    match impl_ctx {
        Some(t) => format!("impl {t} {{ {asyncness}fn {name}{params}{ret} }}"),
        None    => format!("{asyncness}fn {name}{params}{ret}"),
    }
}

// ─── C / C++ ──────────────────────────────────────────────────────────────────

fn walk_c_nodes(node: Node, src: &[u8], out: &mut ParsedFile, lang: Lang) {
    match node.kind() {
        "function_definition" => {
            let name = c_fn_name(&node, src);
            let sig  = c_fn_sig(&node, src);
            push_node(out, name, NodeKind::Function, Some(sig), node);
        }
        "declaration" => {
            // function prototype: int foo(int x);
            if node.child_by_field_name("declarator")
                .map(|d| matches!(d.kind(), "function_declarator" | "pointer_declarator"))
                .unwrap_or(false)
            {
                let name = c_fn_name(&node, src);
                if !name.is_empty() {
                    push_node(out, name.clone(), NodeKind::Function, Some(format!("{name}(...)")), node);
                }
            }
        }
        "struct_specifier" | "union_specifier" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind_str = if node.kind() == "union_specifier" { "union" } else { "struct" };
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("{kind_str} {name}")), node);
            }
        }
        "enum_specifier" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Enum, Some(format!("enum {name}")), node);
            }
        }
        "type_definition" => {
            // typedef struct Foo Foo;  or  typedef int MyInt;
            if let Some(n) = node.child_by_field_name("declarator") {
                let name = node_text(n, src);
                if !name.is_empty() {
                    push_node(out, name.clone(), NodeKind::Type, Some(format!("typedef {name}")), node);
                }
            }
        }
        // C++ only
        "class_specifier" if lang == Lang::Cpp => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Class, Some(format!("class {name}")), node);
                for i in 0..node.child_count() {
                    walk_c_nodes(node.child(i).unwrap(), src, out, lang);
                }
                return;
            }
        }
        "namespace_definition" if lang == Lang::Cpp => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("namespace {name}")), node);
            }
        }
        "template_declaration" if lang == Lang::Cpp => {
            // template<T> class/function inside
            for i in 0..node.child_count() {
                walk_c_nodes(node.child(i).unwrap(), src, out, lang);
            }
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_c_nodes(node.child(i).unwrap(), src, out, lang); }
}

fn walk_c_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "function_definition" {
        c_fn_name(&node, src).if_empty_use(current_fn)
    } else { current_fn.to_string() };

    match node.kind() {
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                let callee = match f.kind() {
                    "identifier" => node_text(f, src),
                    "field_expression" => f.child_by_field_name("field").map(|n| node_text(n, src)).unwrap_or_default(),
                    _ => node_text(f, src),
                };
                push_edge(out, &current, callee, None, EdgeKind::Calls, node);
            }
        }
        "preproc_include" => {
            // #include <foo.h> or #include "foo.h"
            let path = node_text(node, src)
                .trim_start_matches("#include")
                .trim()
                .trim_matches(|c| c == '<' || c == '>' || c == '"')
                .to_string();
            if !path.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, path, None, EdgeKind::Imports, node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_c_edges(node.child(i).unwrap(), src, out, &current); }
}

fn c_fn_name(node: &Node, src: &[u8]) -> String {
    // Walk declarator recursively to find identifier
    fn find_ident(n: Node, src: &[u8]) -> String {
        if n.kind() == "identifier" { return node_text(n, src); }
        if let Some(d) = n.child_by_field_name("declarator") { return find_ident(d, src); }
        for i in 0..n.child_count() {
            let c = n.child(i).unwrap();
            let r = find_ident(c, src);
            if !r.is_empty() { return r; }
        }
        String::new()
    }
    node.child_by_field_name("declarator").map(|d| find_ident(d, src)).unwrap_or_default()
}

fn c_fn_sig(node: &Node, src: &[u8]) -> String {
    let ret = node.child_by_field_name("type").map(|t| node_text(t, src)).unwrap_or_default();
    let name = c_fn_name(node, src);
    format!("{ret} {name}(...)")
}

// ─── C# ───────────────────────────────────────────────────────────────────────

fn walk_csharp_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if class_ctx.is_some() { NodeKind::Method } else { NodeKind::Function };
                let ret = node.child_by_field_name("type").map(|t| node_text(t, src)).unwrap_or_default();
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                let ctx = class_ctx.map(|c| format!("{c}.")).unwrap_or_default();
                push_node(out, name.clone(), kind, Some(format!("{ret} {ctx}{name}{params}")), node);
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                push_node(out, name.clone(), NodeKind::Method, Some(format!("{name}{params}")), node);
            }
        }
        "class_declaration" | "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Class, Some(format!("class {name}")), node);
                for i in 0..node.child_count() {
                    walk_csharp_nodes(node.child(i).unwrap(), src, out, Some(&name.clone()));
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Interface, Some(format!("interface {name}")), node);
            }
        }
        "struct_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("struct {name}")), node);
            }
        }
        "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Enum, Some(format!("enum {name}")), node);
            }
        }
        "namespace_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("namespace {name}")), node);
            }
        }
        "delegate_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("delegate {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_csharp_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_csharp_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "method_declaration" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "invocation_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                let callee = match f.kind() {
                    "identifier" => node_text(f, src),
                    "member_access_expression" => f.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_default(),
                    _ => String::new(),
                };
                push_edge(out, &current, callee, None, EdgeKind::Calls, node);
            }
        }
        "object_creation_expression" => {
            if let Some(t) = node.child_by_field_name("type") {
                push_edge(out, &current, node_text(t, src), None, EdgeKind::Calls, node);
            }
        }
        "using_directive" => {
            let text = node_text(node, src);
            let module = text.trim_start_matches("using").trim().trim_end_matches(';').trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        "base_list" => {
            let child_class = node.parent()
                .and_then(|p| p.child_by_field_name("name"))
                .map(|n| node_text(n, src))
                .unwrap_or_else(|| current.clone());
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" || c.kind() == "generic_name" {
                    let base = node_text(c, src);
                    let kind = if base.starts_with('I') { EdgeKind::Implements } else { EdgeKind::Extends };
                    push_edge(out, &child_class, base, None, kind, node);
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_csharp_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Go ───────────────────────────────────────────────────────────────────────

fn walk_go_nodes(node: Node, src: &[u8], out: &mut ParsedFile) {
    match node.kind() {
        "function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                let result = node.child_by_field_name("result").map(|r| format!(" {}", node_text(r, src))).unwrap_or_default();
                push_node(out, name.clone(), NodeKind::Function, Some(format!("func {name}{params}{result}")), node);
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let recv = node.child_by_field_name("receiver").map(|r| node_text(r, src)).unwrap_or_default();
                push_node(out, name.clone(), NodeKind::Method, Some(format!("func ({recv}) {name}(...)")), node);
            }
        }
        "type_declaration" => {
            // type Foo struct { ... }  or  type MyInt int
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "type_spec" {
                    if let Some(n) = c.child_by_field_name("name") {
                        let name = node_text(n, src);
                        let ty = c.child_by_field_name("type");
                        let kind = match ty.map(|t| t.kind()) {
                            Some("struct_type")    => NodeKind::Struct,
                            Some("interface_type") => NodeKind::Interface,
                            _                      => NodeKind::Type,
                        };
                        push_node(out, name.clone(), kind, Some(format!("type {name} ...")), c);
                    }
                }
            }
        }
        "const_declaration" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "const_spec" {
                    if let Some(n) = c.child_by_field_name("name") {
                        push_node(out, node_text(n, src), NodeKind::Const, None, c);
                    }
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_go_nodes(node.child(i).unwrap(), src, out); }
}

fn walk_go_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = match node.kind() {
        "function_declaration" | "method_declaration" => {
            node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
        }
        _ => current_fn.to_string(),
    };

    match node.kind() {
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                let callee = match f.kind() {
                    "identifier" => node_text(f, src),
                    "selector_expression" => f.child_by_field_name("field").map(|n| node_text(n, src)).unwrap_or_default(),
                    _ => String::new(),
                };
                push_edge(out, &current, callee, None, EdgeKind::Calls, node);
            }
        }
        "import_declaration" => {
            // import ( "fmt"\n "os" )  or  import "fmt"
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "import_spec_list" {
                    for j in 0..c.child_count() {
                        let spec = c.child(j).unwrap();
                        if spec.kind() == "import_spec" {
                            if let Some(path_n) = spec.child_by_field_name("path") {
                                let path = node_text(path_n, src).trim_matches('"').to_string();
                                let fp = out.file_path.clone();
                                push_edge(out, &fp, path, None, EdgeKind::Imports, node);
                            }
                        }
                    }
                } else if c.kind() == "import_spec" {
                    if let Some(path_n) = c.child_by_field_name("path") {
                        let path = node_text(path_n, src).trim_matches('"').to_string();
                        let fp = out.file_path.clone();
                        push_edge(out, &fp, path, None, EdgeKind::Imports, node);
                    }
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_go_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Java ─────────────────────────────────────────────────────────────────────

fn walk_java_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "method_declaration" | "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if class_ctx.is_some() { NodeKind::Method } else { NodeKind::Function };
                let ret = node.child_by_field_name("type").map(|t| node_text(t, src)).unwrap_or_default();
                let params = node.child_by_field_name("formal_parameters").map(|p| node_text(p, src)).unwrap_or_default();
                push_node(out, name.clone(), kind, Some(format!("{ret} {name}{params}")), node);
            }
        }
        "class_declaration" | "enum_declaration" | "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if node.kind() == "enum_declaration" { NodeKind::Enum } else { NodeKind::Class };
                push_node(out, name.clone(), kind, Some(format!("class {name}")), node);
                for i in 0..node.child_count() {
                    walk_java_nodes(node.child(i).unwrap(), src, out, Some(&name.clone()));
                }
                return;
            }
        }
        "interface_declaration" | "annotation_type_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Interface, Some(format!("interface {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_java_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_java_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "method_declaration" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "method_invocation" => {
            if let Some(n) = node.child_by_field_name("name") {
                push_edge(out, &current, node_text(n, src), None, EdgeKind::Calls, node);
            }
        }
        "object_creation_expression" => {
            if let Some(t) = node.child_by_field_name("type") {
                push_edge(out, &current, node_text(t, src), None, EdgeKind::Calls, node);
            }
        }
        "import_declaration" => {
            let text = node_text(node, src);
            let module = text.trim_start_matches("import").trim().trim_end_matches(';').trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        "superclass" => {
            let child_class = node.parent().and_then(|p| p.child_by_field_name("name"))
                .map(|n| node_text(n, src)).unwrap_or_else(|| current.clone());
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "type_identifier" {
                    push_edge(out, &child_class, node_text(c, src), None, EdgeKind::Extends, node);
                }
            }
        }
        "super_interfaces" => {
            let child_class = node.parent().and_then(|p| p.child_by_field_name("name"))
                .map(|n| node_text(n, src)).unwrap_or_else(|| current.clone());
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "type_identifier" {
                    push_edge(out, &child_class, node_text(c, src), None, EdgeKind::Implements, node);
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_java_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Scala ────────────────────────────────────────────────────────────────────

fn walk_scala_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if class_ctx.is_some() { NodeKind::Method } else { NodeKind::Function };
                push_node(out, name.clone(), kind, Some(format!("def {name}(...)")), node);
            }
        }
        "class_definition" | "trait_definition" | "object_definition" | "enum_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = match node.kind() {
                    "trait_definition"  => NodeKind::Trait,
                    "enum_definition"   => NodeKind::Enum,
                    _                   => NodeKind::Class,
                };
                let kw = match node.kind() {
                    "trait_definition"  => "trait",
                    "object_definition" => "object",
                    "enum_definition"   => "enum",
                    _                   => "class",
                };
                push_node(out, name.clone(), kind, Some(format!("{kw} {name}")), node);
                for i in 0..node.child_count() {
                    walk_scala_nodes(node.child(i).unwrap(), src, out, Some(&name.clone()));
                }
                return;
            }
        }
        "type_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("type {name}")), node);
            }
        }
        "val_definition" | "var_definition" => {
            if let Some(n) = node.child_by_field_name("pattern") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Const, None, node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_scala_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_scala_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "function_definition" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                let callee = match f.kind() {
                    "identifier" => node_text(f, src),
                    "field_expression" => f.child_by_field_name("field").map(|n| node_text(n, src)).unwrap_or_default(),
                    _ => String::new(),
                };
                push_edge(out, &current, callee, None, EdgeKind::Calls, node);
            }
        }
        "import_declaration" => {
            let text = node_text(node, src);
            let module = text.trim_start_matches("import").trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_scala_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── TypeScript / JavaScript ──────────────────────────────────────────────────

fn walk_ts_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "export_statement" => {
            for i in 0..node.child_count() {
                let child = node.child(i).unwrap();
                if !matches!(child.kind(), "export" | "default" | "type" | ";" | "{" | "}") {
                    walk_ts_nodes(child, src, out, class_ctx);
                }
            }
            return;
        }
        "function_declaration" | "function_signature" | "generator_function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Function, ts_fn_sig(node, src, class_ctx), node);
            }
        }
        "method_definition" | "method_signature" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let is_async = has_child_kind(node, "async");
                let kind = if is_async { NodeKind::AsyncFunction } else { NodeKind::Method };
                push_node(out, name.clone(), kind, ts_fn_sig(node, src, class_ctx), node);
            }
        }
        "class_declaration" | "class" => {
            if let Some(n) = node.child_by_field_name("name") {
                let cname = node_text(n, src);
                push_node(out, cname.clone(), NodeKind::Class, Some(format!("class {cname}")), node);
                for i in 0..node.child_count() {
                    walk_ts_nodes(node.child(i).unwrap(), src, out, Some(&cname.clone()));
                }
                return;
            }
        }
        "abstract_class_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let cname = node_text(n, src);
                push_node(out, cname.clone(), NodeKind::Class, Some(format!("abstract class {cname}")), node);
                for i in 0..node.child_count() {
                    walk_ts_nodes(node.child(i).unwrap(), src, out, Some(&cname.clone()));
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let iname = node_text(n, src);
                push_node(out, iname.clone(), NodeKind::Interface, Some(format!("interface {iname}")), node);
                if let Some(body) = node.child_by_field_name("body") {
                    for i in 0..body.child_count() {
                        let m = body.child(i).unwrap();
                        if matches!(m.kind(), "method_signature" | "property_signature") {
                            if let Some(mn) = m.child_by_field_name("name") {
                                push_node(out, node_text(mn, src), NodeKind::Method, ts_fn_sig(m, src, Some(&iname)), m);
                            }
                        }
                    }
                }
            }
        }
        "type_alias_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("type {name}")), node);
            }
        }
        "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Enum, Some(format!("enum {name}")), node);
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            for i in 0..node.child_count() {
                let child = node.child(i).unwrap();
                if child.kind() == "variable_declarator" {
                    if let (Some(name_n), Some(val_n)) = (child.child_by_field_name("name"), child.child_by_field_name("value")) {
                        if matches!(val_n.kind(), "arrow_function" | "function_expression") {
                            let is_async = has_child_kind(val_n, "async");
                            let name = node_text(name_n, src);
                            let kind = if is_async { NodeKind::AsyncFunction } else { NodeKind::Function };
                            let params = val_n.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                            let ret = val_n.child_by_field_name("return_type").map(|r| format!(": {}", node_text(r, src))).unwrap_or_default();
                            let prefix = if is_async { "async " } else { "" };
                            push_node(out, name.clone(), kind, Some(format!("const {name} = {prefix}({params}){ret} =>")), child);
                        }
                    }
                }
            }
        }
        "namespace_declaration" | "module" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("namespace {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_ts_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_ts_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = match node.kind() {
        "function_declaration" | "function_signature" | "generator_function_declaration" | "method_definition" => {
            node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
        }
        "variable_declarator" => {
            if let Some(v) = node.child_by_field_name("value") {
                if matches!(v.kind(), "arrow_function" | "function_expression") {
                    node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
                } else { current_fn.to_string() }
            } else { current_fn.to_string() }
        }
        _ => current_fn.to_string(),
    };

    match node.kind() {
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                push_edge(out, &current, ts_callee(f, src), None, EdgeKind::Calls, node);
            }
        }
        "new_expression" => {
            if let Some(ctor) = node.child_by_field_name("constructor") {
                push_edge(out, &current, node_text(ctor, src), None, EdgeKind::Calls, node);
            }
        }
        "import_statement" => {
            let module = ts_import_source(&node, src);
            if !module.is_empty() {
                let to_file = resolve_ts_import(&out.file_path, &module);
                out.edges.push(RawEdge {
                    from_name: out.file_path.clone(), to_name: module, to_file,
                    kind: EdgeKind::Imports, at_line: line(node),
                });
            }
        }
        "extends_clause" => {
            let parent_class = node.parent().and_then(|p| p.parent())
                .and_then(|gp| gp.child_by_field_name("name"))
                .map(|n| node_text(n, src))
                .unwrap_or_else(|| current.clone());
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "identifier" {
                    push_edge(out, &parent_class, node_text(c, src), None, EdgeKind::Extends, node);
                }
            }
        }
        "implements_clause" => {
            let child_class = node.parent().and_then(|p| p.child_by_field_name("name"))
                .map(|n| node_text(n, src))
                .unwrap_or_else(|| current.clone());
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "type_identifier" {
                    push_edge(out, &child_class, node_text(c, src), None, EdgeKind::Implements, node);
                } else if c.kind() == "generic_type" {
                    if let Some(nn) = c.child_by_field_name("name") {
                        push_edge(out, &child_class, node_text(nn, src), None, EdgeKind::Implements, node);
                    }
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_ts_edges(node.child(i).unwrap(), src, out, &current); }
}

fn ts_fn_sig(node: Node, src: &[u8], class_ctx: Option<&str>) -> Option<String> {
    let name = node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_default();
    let is_async = has_child_kind(node, "async");
    let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_else(|| "()".to_string());
    let ret = node.child_by_field_name("return_type").map(|r| format!(": {}", node_text(r, src))).unwrap_or_default();
    let prefix = if is_async { "async " } else { "" };
    Some(match class_ctx {
        Some(c) => format!("{c}.{name}{params}{ret}"),
        None    => format!("{prefix}function {name}{params}{ret}"),
    })
}

fn ts_callee(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" => node_text(node, src),
        "member_expression" => node.child_by_field_name("property").map(|n| node_text(n, src)).unwrap_or_default(),
        "call_expression" => node.child_by_field_name("function").map(|n| ts_callee(n, src)).unwrap_or_default(),
        _ => String::new(),
    }
}

fn ts_import_source(node: &Node, src: &[u8]) -> String {
    for i in 0..node.child_count() {
        let c = node.child(i).unwrap();
        if c.kind() == "string" {
            return node_text(c, src).trim_matches(|ch| ch == '\'' || ch == '"' || ch == '`').to_string();
        }
    }
    String::new()
}

fn resolve_ts_import(from_file: &str, module: &str) -> Option<String> {
    if !module.starts_with('.') { return None; }
    let dir = std::path::Path::new(from_file).parent()?.to_str()?;
    let resolved = format!("{dir}/{module}");
    if resolved.ends_with(".ts") || resolved.ends_with(".tsx") || resolved.ends_with(".js") {
        return Some(resolved);
    }
    Some(format!("{resolved}.ts"))
}

// ─── Python ───────────────────────────────────────────────────────────────────

fn walk_python_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "decorated_definition" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if matches!(c.kind(), "function_definition" | "async_function_definition" | "class_definition") {
                    walk_python_nodes(c, src, out, class_ctx);
                }
            }
            return;
        }
        "function_definition" | "async_function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let is_async = node.kind() == "async_function_definition";
                let kind = match (class_ctx.is_some(), is_async) {
                    (true, true)  => NodeKind::AsyncFunction,
                    (true, false) => NodeKind::Method,
                    (_, true)     => NodeKind::AsyncFunction,
                    _             => NodeKind::Function,
                };
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                let ret = node.child_by_field_name("return_type").map(|r| format!(" -> {}", node_text(r, src))).unwrap_or_default();
                let prefix = if is_async { "async " } else { "" };
                push_node(out, name.clone(), kind, Some(format!("{prefix}def {name}{params}{ret}")), node);
            }
            for i in 0..node.child_count() { walk_python_nodes(node.child(i).unwrap(), src, out, class_ctx); }
            return;
        }
        "class_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let cname = node_text(n, src);
                let bases = node.child_by_field_name("superclasses").map(|a| node_text(a, src)).unwrap_or_default();
                let sig = if bases.is_empty() { format!("class {cname}") } else { format!("class {cname}({bases})") };
                push_node(out, cname.clone(), NodeKind::Class, Some(sig), node);
                for i in 0..node.child_count() { walk_python_nodes(node.child(i).unwrap(), src, out, Some(&cname.clone())); }
                return;
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_python_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_python_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if matches!(node.kind(), "function_definition" | "async_function_definition") {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "call" => {
            if let Some(f) = node.child_by_field_name("function") {
                push_edge(out, &current, python_callee(f, src), None, EdgeKind::Calls, node);
            }
        }
        "import_statement" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                match c.kind() {
                    "dotted_name" => { let fp = out.file_path.clone(); push_edge(out, &fp, node_text(c, src), None, EdgeKind::Imports, node); }
                    "aliased_import" => {
                        if let Some(orig) = c.child_by_field_name("name") {
                            let fp = out.file_path.clone(); push_edge(out, &fp, node_text(orig, src), None, EdgeKind::Imports, node);
                        }
                    }
                    _ => {}
                }
            }
        }
        "import_from_statement" => {
            let module = node.child_by_field_name("module_name").map(|n| node_text(n, src)).unwrap_or_default();
            if !module.is_empty() {
                let to_file = if module.starts_with('.') { None } else { resolve_python_import(&out.file_path, &module) };
                out.edges.push(RawEdge { from_name: out.file_path.clone(), to_name: module, to_file, kind: EdgeKind::Imports, at_line: line(node) });
            }
        }
        "class_definition" => {
            if let (Some(name_n), Some(args_n)) = (node.child_by_field_name("name"), node.child_by_field_name("superclasses")) {
                let cname = node_text(name_n, src);
                for i in 0..args_n.child_count() {
                    let arg = args_n.child(i).unwrap();
                    match arg.kind() {
                        "identifier" => push_edge(out, &cname, node_text(arg, src), None, EdgeKind::Extends, node),
                        "attribute"  => { if let Some(a) = arg.child_by_field_name("attribute") { push_edge(out, &cname, node_text(a, src), None, EdgeKind::Extends, node); } }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_python_edges(node.child(i).unwrap(), src, out, &current); }
}

fn python_callee(node: Node, src: &[u8]) -> String {
    match node.kind() {
        "identifier" => node_text(node, src),
        "attribute"  => node.child_by_field_name("attribute").map(|a| node_text(a, src)).unwrap_or_default(),
        "call"       => node.child_by_field_name("function").map(|n| python_callee(n, src)).unwrap_or_default(),
        _ => String::new(),
    }
}

fn resolve_python_import(from_file: &str, module: &str) -> Option<String> {
    if module.starts_with('.') { return None; }
    let dir = std::path::Path::new(from_file).parent()?.to_str()?;
    Some(format!("{dir}/{}.py", module.replace('.', "/")))
}

// ─── Ruby ─────────────────────────────────────────────────────────────────────

fn walk_ruby_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "method" | "singleton_method" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if class_ctx.is_some() { NodeKind::Method } else { NodeKind::Function };
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                push_node(out, name.clone(), kind, Some(format!("def {name}{params}")), node);
            }
        }
        "class" | "singleton_class" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Class, Some(format!("class {name}")), node);
                for i in 0..node.child_count() { walk_ruby_nodes(node.child(i).unwrap(), src, out, Some(&name.clone())); }
                return;
            }
        }
        "module" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("module {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_ruby_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_ruby_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "method" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "call" => {
            let callee = node.child_by_field_name("method")
                .map(|n| node_text(n, src))
                .unwrap_or_default();
            push_edge(out, &current, callee, None, EdgeKind::Calls, node);
        }
        "require" | "require_relative" => {
            // require 'foo'
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "string" {
                    let path = node_text(c, src).trim_matches(|ch| ch == '\'' || ch == '"').to_string();
                    let fp = out.file_path.clone();
                    push_edge(out, &fp, path, None, EdgeKind::Imports, node);
                    break;
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_ruby_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── PHP ──────────────────────────────────────────────────────────────────────

fn walk_php_nodes(node: Node, src: &[u8], out: &mut ParsedFile, class_ctx: Option<&str>) {
    match node.kind() {
        "function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                let kind = if class_ctx.is_some() { NodeKind::Method } else { NodeKind::Function };
                push_node(out, name.clone(), kind, Some(format!("function {name}{params}")), node);
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let params = node.child_by_field_name("parameters").map(|p| node_text(p, src)).unwrap_or_default();
                push_node(out, name.clone(), NodeKind::Method, Some(format!("function {name}{params}")), node);
            }
        }
        "class_declaration" | "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if node.kind() == "enum_declaration" { NodeKind::Enum } else { NodeKind::Class };
                push_node(out, name.clone(), kind, Some(format!("class {name}")), node);
                for i in 0..node.child_count() { walk_php_nodes(node.child(i).unwrap(), src, out, Some(&name.clone())); }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Interface, Some(format!("interface {name}")), node);
            }
        }
        "trait_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Trait, Some(format!("trait {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_php_nodes(node.child(i).unwrap(), src, out, class_ctx); }
}

fn walk_php_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if matches!(node.kind(), "function_definition" | "method_declaration") {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "function_call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                push_edge(out, &current, node_text(f, src), None, EdgeKind::Calls, node);
            }
        }
        "member_call_expression" | "nullsafe_member_call_expression" => {
            if let Some(n) = node.child_by_field_name("name") {
                push_edge(out, &current, node_text(n, src), None, EdgeKind::Calls, node);
            }
        }
        "object_creation_expression" => {
            if let Some(cn) = node.child_by_field_name("class_name") {
                push_edge(out, &current, node_text(cn, src), None, EdgeKind::Calls, node);
            }
        }
        "namespace_use_declaration" => {
            let text = node_text(node, src);
            let module = text.trim_start_matches("use").trim().trim_end_matches(';').trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_php_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Bash ─────────────────────────────────────────────────────────────────────

fn walk_bash_nodes(node: Node, src: &[u8], out: &mut ParsedFile) {
    match node.kind() {
        "function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Function, Some(format!("{name}()")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_bash_nodes(node.child(i).unwrap(), src, out); }
}

fn walk_bash_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "function_definition" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "command" => {
            if let Some(n) = node.child_by_field_name("name") {
                let callee = node_text(n, src);
                // Only track user-defined-looking calls (no builtins like echo, cd, etc.)
                if !matches!(callee.as_str(), "echo" | "cd" | "export" | "local" | "return" | "exit"
                    | "if" | "then" | "fi" | "for" | "do" | "done" | "while" | "case" | "esac"
                    | "true" | "false" | "test" | "[" | "[[" | "set" | "unset" | "shift" | "read")
                {
                    push_edge(out, &current, callee, None, EdgeKind::Calls, node);
                }
            }
        }
        "source_command" => {
            // source ./lib.sh  or  . ./lib.sh
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "word" {
                    let fp = out.file_path.clone();
                    push_edge(out, &fp, node_text(c, src), None, EdgeKind::Imports, node);
                    break;
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_bash_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Haskell ──────────────────────────────────────────────────────────────────

fn walk_haskell_nodes(node: Node, src: &[u8], out: &mut ParsedFile) {
    match node.kind() {
        "function" => {
            // (function name patterns = body)
            if let Some(n) = node.child(0) {
                if n.kind() == "variable" {
                    let name = node_text(n, src);
                    push_node(out, name.clone(), NodeKind::Function, Some(name), node);
                }
            }
        }
        "data_type" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("data {name}")), node);
            }
        }
        "newtype" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("newtype {name}")), node);
            }
        }
        "type_synomym" | "type_synonym" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("type {name}")), node);
            }
        }
        "class" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Trait, Some(format!("class {name}")), node);
            }
        }
        "instance" => {
            // instance Foo Bar — extract method definitions inside
            for i in 0..node.child_count() {
                walk_haskell_nodes(node.child(i).unwrap(), src, out);
            }
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_haskell_nodes(node.child(i).unwrap(), src, out); }
}

fn walk_haskell_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "function" {
        node.child(0).filter(|n| n.kind() == "variable")
            .map(|n| node_text(n, src))
            .unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "apply" | "apply_infix" => {
            if let Some(f) = node.child(0) {
                if f.kind() == "variable" || f.kind() == "constructor" {
                    push_edge(out, &current, node_text(f, src), None, EdgeKind::Calls, node);
                }
            }
        }
        "import" => {
            let text = node_text(node, src);
            // import Data.List (sort)  →  "Data.List"
            if let Some(module) = text.split_whitespace().nth(1) {
                let module = module.trim_end_matches('(').trim().to_string();
                if !module.is_empty() {
                    let fp = out.file_path.clone();
                    push_edge(out, &fp, module, None, EdgeKind::Imports, node);
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_haskell_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── OCaml ────────────────────────────────────────────────────────────────────

fn walk_ocaml_nodes(node: Node, src: &[u8], out: &mut ParsedFile, module_ctx: Option<&str>) {
    match node.kind() {
        "let_binding" => {
            // let foo x y = ... or let rec foo ...
            if let Some(n) = node.child_by_field_name("pattern") {
                if n.kind() == "value_name" || n.kind() == "variable_pattern" {
                    let name = node_text(n, src);
                    push_node(out, name.clone(), NodeKind::Function, Some(format!("let {name}")), node);
                }
            }
        }
        "type_binding" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("type {name}")), node);
            }
        }
        "module_binding" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("module {name}")), node);
                for i in 0..node.child_count() { walk_ocaml_nodes(node.child(i).unwrap(), src, out, Some(&name.clone())); }
                return;
            }
        }
        "class_binding" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Class, Some(format!("class {name}")), node);
            }
        }
        "exception_definition" => {
            for i in 0..node.child_count() {
                let c = node.child(i).unwrap();
                if c.kind() == "constructor_name" {
                    let name = node_text(c, src);
                    push_node(out, name.clone(), NodeKind::Type, Some(format!("exception {name}")), node);
                    break;
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_ocaml_nodes(node.child(i).unwrap(), src, out, module_ctx); }
}

fn walk_ocaml_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "let_binding" {
        node.child_by_field_name("pattern")
            .filter(|n| n.kind() == "value_name")
            .map(|n| node_text(n, src))
            .unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "application_expression" => {
            if let Some(f) = node.child(0) {
                let callee = match f.kind() {
                    "value_path" | "value_name" => node_text(f, src),
                    _ => String::new(),
                };
                push_edge(out, &current, callee, None, EdgeKind::Calls, node);
            }
        }
        "open_module" => {
            let text = node_text(node, src);
            let module = text.trim_start_matches("open").trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_ocaml_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Julia ────────────────────────────────────────────────────────────────────

fn walk_julia_nodes(node: Node, src: &[u8], out: &mut ParsedFile, struct_ctx: Option<&str>) {
    match node.kind() {
        "function_definition" | "short_function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                let kind = if struct_ctx.is_some() { NodeKind::Method } else { NodeKind::Function };
                push_node(out, name.clone(), kind, Some(format!("function {name}(...)")), node);
            }
        }
        "struct_definition" | "mutable_struct_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("struct {name}")), node);
                for i in 0..node.child_count() { walk_julia_nodes(node.child(i).unwrap(), src, out, Some(&name.clone())); }
                return;
            }
        }
        "abstract_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Type, Some(format!("abstract type {name}")), node);
            }
        }
        "module_definition" | "baremodule_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("module {name}")), node);
            }
        }
        "const_statement" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Const, Some(format!("const {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_julia_nodes(node.child(i).unwrap(), src, out, struct_ctx); }
}

fn walk_julia_edges(node: Node, src: &[u8], out: &mut ParsedFile, current_fn: &str) {
    let current = if node.kind() == "function_definition" {
        node.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_else(|| current_fn.to_string())
    } else { current_fn.to_string() };

    match node.kind() {
        "call_expression" => {
            if let Some(f) = node.child_by_field_name("function") {
                let callee = match f.kind() {
                    "identifier" => node_text(f, src),
                    "field_expression" => f.child_by_field_name("name").map(|n| node_text(n, src)).unwrap_or_default(),
                    _ => String::new(),
                };
                push_edge(out, &current, callee, None, EdgeKind::Calls, node);
            }
        }
        "using_statement" | "import_statement" => {
            let text = node_text(node, src);
            let kw = if node.kind() == "using_statement" { "using" } else { "import" };
            let module = text.trim_start_matches(kw).trim().split(':').next().unwrap_or("").trim().to_string();
            if !module.is_empty() {
                let fp = out.file_path.clone();
                push_edge(out, &fp, module, None, EdgeKind::Imports, node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_julia_edges(node.child(i).unwrap(), src, out, &current); }
}

// ─── Verilog ──────────────────────────────────────────────────────────────────

fn walk_verilog_nodes(node: Node, src: &[u8], out: &mut ParsedFile) {
    match node.kind() {
        "module_declaration" | "program_declaration" | "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("module_name")
                .or_else(|| node.child_by_field_name("name"))
            {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("module {name}")), node);
            }
        }
        "function_declaration" | "task_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Function, Some(format!("function {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_verilog_nodes(node.child(i).unwrap(), src, out); }
}

// ─── Agda ─────────────────────────────────────────────────────────────────────

fn walk_agda_nodes(node: Node, src: &[u8], out: &mut ParsedFile) {
    match node.kind() {
        "function_clause" => {
            if let Some(n) = node.child(0) {
                let name = node_text(n, src);
                if !name.is_empty() && n.kind() != "where" {
                    push_node(out, name.clone(), NodeKind::Function, Some(name), node);
                }
            }
        }
        "data_type" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("data {name}")), node);
            }
        }
        "record_type" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Struct, Some(format!("record {name}")), node);
            }
        }
        "module" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src);
                push_node(out, name.clone(), NodeKind::Module, Some(format!("module {name}")), node);
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() { walk_agda_nodes(node.child(i).unwrap(), src, out); }
}

// ─── Utility trait ────────────────────────────────────────────────────────────

trait IfEmptyUse {
    fn if_empty_use(self, fallback: &str) -> String;
}
impl IfEmptyUse for String {
    fn if_empty_use(self, fallback: &str) -> String {
        if self.is_empty() { fallback.to_string() } else { self }
    }
}
