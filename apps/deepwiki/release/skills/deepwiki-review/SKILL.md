---
name: deepwiki-review
description: Assess the impact and risk of changing a symbol using DeepWiki's call graph — direct callers, transitive blast radius, tests at risk, and a Low/Medium/High risk verdict.
triggers: ["review change", "đánh giá thay đổi", "impact review", "what breaks if I change", "rủi ro thay đổi", "change risk"]
---

# DeepWiki Review Skill

Use this skill when the user asks **"what breaks if I change X"**, **"is this change risky"**,
**"what depends on this"**, or wants an impact review before editing/refactoring a symbol.

The DeepWiki App exposes these MCP tools (server `deepwiki-mcp`):

- `deepwiki_symbol(name)` — definition(s) + direct callers + callees.
- `deepwiki_impact(name, depth?)` — transitive callers (blast radius).
- `deepwiki_snippet(name | path,start,end)` — read the real source.
- `deepwiki_list_files` / `deepwiki_file_outline(path)` — locate tests and related code.
- `deepwiki_explore(query, depth?)` — matches + call graph + blast radius in one shot.

## Instructions

1. **Resolve the target.** `deepwiki_symbol(name)`. If multiple definitions match, list them
   and confirm which one — impact differs per definition.
2. **Read the change surface.** `deepwiki_snippet(name)` to judge whether a change would be
   signature-breaking (callers must change) or internal-only.
3. **Compute blast radius.** `deepwiki_impact(name, depth=3..4)`. Group affected symbols by
   file with counts.
4. **Flag tests at risk.** From the affected files (and `deepwiki_list_files`), surface paths
   that look like tests (`test`, `spec`, `_test`, `tests/`).
5. **Deliver a verdict:**
   - **Direct callers** (one hop) — most likely to break, with `path:line`.
   - **Blast radius** — transitive, grouped by file.
   - **Tests to run** — the affected test files.
   - **Risk: Low / Medium / High** — one-line justification (surface size + signature-breaking?).

For a deeper, autonomous pass, delegate to the **`impact-analyst`** sub-agent via
`run_persona` — it runs this same analysis end-to-end.

## Notes

- Call edges are name-resolved (static): external/dynamic/overloaded calls may be missed, so
  present the blast radius as a strong signal, not an exhaustive guarantee.
- Supported languages: Rust, Python, JavaScript, TypeScript/TSX, Go.
- Advisory and read-only — it assesses risk; it does not make the change.
