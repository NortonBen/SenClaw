---
name: deepwiki-generate
description: Generate a source-grounded wiki for a local codebase using the DeepWiki App — plan pages from the repo's structural outline, write each page from grounded evidence, and save them as browsable Markdown.
---

# DeepWiki Generate Skill

Use this skill when the user wants to **document a codebase**, **generate a wiki**, or
**produce an architecture overview** — e.g. "tạo wiki cho dự án này", "document this repo",
"generate a deepwiki for /path/to/project".

The DeepWiki App exposes these MCP tools (server `deepwiki-mcp`):

- `deepwiki_index` — index/re-index a repo by absolute `path`. Run first.
- `deepwiki_outline` — structural map: stats, top-level directories, largest files,
  architectural types (classes/structs/traits/interfaces), most-called symbols. Use to PLAN.
- `deepwiki_context` — source-grounded evidence for a `query`: matching symbols (with
  signatures/docs/line numbers), callers/callees, and relevant file outlines. Use to WRITE.
- `deepwiki_save_page` — create/update a page (`slug` kebab-case, `title`, `content` Markdown,
  optional `parent` slug for nesting, optional `ord`).
- `deepwiki_list_pages`, `deepwiki_get_page`, `deepwiki_delete_page` — manage pages.

## Workflow

1. **Index.** Confirm the target path with the user, then call `deepwiki_index`.

2. **Plan.** Call `deepwiki_outline`. From it, decide a page set. A good default tree:
   - `overview` — what the project is, top-level layout, how to build/run.
   - `architecture` — major components and how they fit together (use the directories + types).
   - One page per major subsystem/module (parent: `architecture`).
   - `data-model` — key structs/classes and their relationships, if applicable.
   - `glossary` — important symbols and terms.

3. **Write each page from evidence.** For every page, call `deepwiki_context` one or more
   times with focused queries (a module name, a key type, a concept). Write the Markdown
   **only from what the evidence returns** — cite `path:line` for concrete claims. Do not
   invent APIs, parameters, or behavior the evidence doesn't show.

4. **Save.** Call `deepwiki_save_page` for each page. Set `parent` to build the sidebar tree
   and `ord` to control order (overview first).

5. **Summarize.** Tell the user which pages were created and that they can browse them in the
   DeepWiki App (or ask follow-up questions via the deepwiki-ask skill).

## Notes

- Supported languages: Rust, Python, JavaScript, TypeScript/TSX, Go.
- Keep pages focused and skimmable: short intro, a structure section, and grounded detail
  with file/line citations. Prefer linking related pages over duplicating content.
- Re-running on an updated repo: re-index, then update the affected pages with fresh context.
