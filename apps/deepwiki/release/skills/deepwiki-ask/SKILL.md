---
name: deepwiki-ask
description: Answer questions about a codebase with source-grounded evidence from the DeepWiki App — explain how something works, find where logic lives, citing real file paths and line numbers.
---

# DeepWiki Ask Skill

Use this skill when the user asks a **question about an indexed codebase** — "how does
auth work here", "where is the request router", "giải thích cách X hoạt động" — and wants
an answer grounded in the actual source rather than a guess.

The DeepWiki App exposes these MCP tools (server `deepwiki-mcp`):

- `deepwiki_context` — source-grounded evidence for a `query`: matching symbols
  (signatures/docs/line numbers), callers/callees, and relevant file outlines. Optional `depth`.
- `deepwiki_outline` — high-level structural map of the repo (use to orient a broad question).
- `deepwiki_list_pages` / `deepwiki_get_page` — reuse existing generated wiki pages when they
  already cover the question.

## Instructions

1. **Check for an existing page first.** Call `deepwiki_list_pages`; if a page clearly covers
   the question, `deepwiki_get_page` and answer from it (still cite sources).

2. **Otherwise gather evidence.** Call `deepwiki_context` with the user's question reduced to
   the key symbol/concept. For broad questions, start with `deepwiki_outline` to pick the
   right entry points, then `deepwiki_context` on them.

3. **Answer only from evidence.** Synthesize a clear explanation and cite `path:line` for each
   concrete claim. If the evidence is insufficient, say so and state which `deepwiki_context`
   queries would help — do not fabricate.

4. **Offer to persist.** If the answer is reusable, offer to save it as a wiki page via the
   deepwiki-generate skill (`deepwiki_save_page`).

## Notes

- If the repo isn't indexed yet (`deepwiki_context` returns no matches), tell the user to
  index it first (deepwiki-generate skill / `deepwiki_index`).
- Supported languages (17): Rust, Python, JavaScript, TypeScript/TSX, Go, Java, C, C++, C#, Ruby, PHP, Scala, Bash, Julia, Haskell, OCaml.
