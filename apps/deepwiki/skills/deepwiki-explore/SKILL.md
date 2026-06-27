---
name: deepwiki-explore
description: Understand a codebase fast using DeepWiki's pre-indexed symbol/call graph — explore symbols, callers/callees, blast-radius impact, and file outlines instead of grep/glob/read crawling.
triggers: ["explore code", "khám phá code", "call graph", "blast radius", "ai gọi hàm", "trace callers"]
---

# DeepWiki Explore Skill

Use this skill when the user wants to **understand code structure**, **trace who calls what**,
or **find where a symbol is defined** — e.g. "how does X work", "what calls `foo`", "outline
this file". DeepWiki is one app: the same index powers both the wiki and this code graph.

The DeepWiki App exposes these MCP tools (server `deepwiki-mcp`):

- `deepwiki_index` — index/re-index a repo by absolute `path`. Run once per codebase.
- `deepwiki_status` — indexed root + file/symbol/edge counts + language breakdown.
- `deepwiki_explore` — **preferred first call.** Given a `query` (symbol name or keyword),
  returns matching definitions (file/line, signature, doc), callers, callees, and the
  transitive `blast_radius`, all in one shot. Optional `depth` (default 3).
- `deepwiki_search` — full-text search over symbol names/signatures/docs.
- `deepwiki_symbol` — exact-name lookup: definitions + direct callers + callees.
- `deepwiki_impact` — transitive callers (blast radius) of a `name`.
- `deepwiki_file_outline` — every symbol in a file (`path`) plus its imports.
- `deepwiki_snippet` — read the real source of a symbol (`name`) or `path`+`start`/`end`.

## Instructions

1. **Ensure the repo is indexed.** Call `deepwiki_status`; if `root` is null or wrong, call
   `deepwiki_index` with the absolute project path (it re-indexes automatically on changes).
2. **"How does X work" / "where is X".** `deepwiki_explore` → lead with the definition
   (file:line + signature), then callees (what it does) and callers (who uses it). Read with
   `deepwiki_snippet` before describing behavior. Cite every claim as `path:line`.
3. **"What calls X" / "what does X call".** Use `deepwiki_symbol` for the one-hop view.
4. **Outline a file.** Use `deepwiki_file_outline`.

## Notes

- Supported languages (17): Rust, Python, JavaScript, TypeScript/TSX, Go, Java, C, C++, C#, Ruby, PHP, Scala, Bash, Julia, Haskell, OCaml.
- Call edges are name-resolved (static), so external/overloaded calls may be approximate.
- Prefer DeepWiki over grep/glob/read for structure questions; it is far cheaper in tokens.
- For an autonomous pass, delegate to the **`code-explorer`** sub-agent via `run_persona`.
