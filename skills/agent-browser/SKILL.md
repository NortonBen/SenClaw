---
name: agent-browser
description: Drive the connected browser via the `senclaw-browser` MCP server — search the web, navigate sites, extract structured data, fill forms, take screenshots. Use whenever a task needs live web content (current prices, news, docs, web research) or interaction with a web app.
version: 1.1.0
when-to-use: any request that needs fresh web content, web automation, or page screenshots (e.g. "tìm giá vàng hôm nay", "screenshot github trending", "fill this form", "extract product list from amazon")
---

# Agent Browser Skill

A connected Chrome instance is exposed through the **`senclaw-browser`** MCP server (~30 tools). The browser runs in the user's actual session — pages they're logged in to are accessible.

## Tool names

The bridge strips the `senclaw-` server prefix **and** the redundant `browser_` tool prefix, so the names you call are:

```
mcp__browser__<verb>      e.g. mcp__browser__search, mcp__browser__navigate, mcp__browser__snapshot
```

> The registered functions are `browser_search`, `browser_navigate`, … but agents call them as `mcp__browser__search`, `mcp__browser__navigate`, … (the `browser_` part is dropped once).
> If a `select:mcp__browser__*` load returns nothing, this deployment may expose them unstripped as `mcp__senclaw-browser__browser_<verb>` — in that case search the keyword (`"browser navigate snapshot"`) and call whatever exact name ToolSearch returns.

## Step 0 — Load the tools FIRST (required)

These tools are **deferred** to save tokens. Their schemas are NOT in the prompt, so **calling one before loading it fails with `InputValidationError`**. Always run `ToolSearch` first, in the same turn, before any `mcp__browser__*` call.

Bulk-load the common set by exact name (most reliable):

```
ToolSearch {
  "query": "select:mcp__browser__search,mcp__browser__navigate,mcp__browser__snapshot,mcp__browser__click,mcp__browser__type,mcp__browser__extract_text,mcp__browser__extract_structured,mcp__browser__screenshot,mcp__browser__fill_form,mcp__browser__click_and_wait,mcp__browser__wait,mcp__browser__new_tab,mcp__browser__close_tab",
  "max_results": 30
}
```

Need a tool not in that list (e.g. `crawl`, `extract_table`)? Load it the same way: `ToolSearch { query: "select:mcp__browser__crawl,mcp__browser__crawl_status" }`. Keyword search also works: `ToolSearch { query: "browser search web", max_results: 10 }`.

Only after ToolSearch returns the matched schemas may you call the tool directly.

## Core workflows

### 1. Quick web search (preferred for "find/search X")

Use **`mcp__browser__search`** — returns ranked SERP results without opening a tab. Cheapest path for fact-finding.

```
mcp__browser__search {
  "query": "giá bạc hôm nay vnđ",
  "engine": "google",            // or "bing"; default "google"
  "num_results": 10,             // default 10
  "language": "vi"               // optional
}
```

Returns structured results (`title`, `url`, `snippet`, …). Use snippets directly if they answer the question. If you need full content, `mcp__browser__navigate` to the top URL.

### 2. Open a page + read content

```
mcp__browser__navigate { "url": "https://example.com/article" }
↓ (response includes a tab_id)
mcp__browser__snapshot { "tab_id": "<from navigate response>" }
↓ pick element indices from the snapshot
mcp__browser__extract_text { "tab_id": "...", "selector": "article" }   // selector is optional CSS; omit for whole page
```

`mcp__browser__snapshot` returns a compact accessibility tree with **numbered indices** for every interactive element. Those indices are the `index` values passed to `click`, `type`, `hover`, `click_and_wait`, etc. `tab_id` is optional on most tools — omit it to act on the active tab.

### 3. Structured data extraction

When the page has tabular / list data, skip manual parsing — let the model map it via schema:

```
mcp__browser__extract_structured {
  "tab_id": "...",
  "schema": {
    "type": "array",
    "items": {
      "type": "object",
      "properties": {
        "title":  { "type": "string" },
        "price":  { "type": "number" },
        "url":    { "type": "string" }
      }
    }
  },
  "selector": "...",     // optional: scope to a container
  "max_items": 50        // optional cap
}
```

For HTML tables specifically use `mcp__browser__extract_table` — faster, no schema needed.

### 4. Fill forms / interact

```
mcp__browser__fill_form {
  "tab_id": "...",
  "fields": [
    { "target": "Email",    "value": "alice@example.com", "type": "text" },
    { "target": "Password", "value": "...",                "type": "password" }
  ],
  "submit": true
}
```

Each field's `target` auto-matches by label / placeholder / name / CSS selector. For a single field, use `mcp__browser__type { "tab_id": "...", "index": <n>, "text": "...", "submit": false }` after a snapshot.

For navigation-triggering clicks (login, submit), prefer **`mcp__browser__click_and_wait { "index": <n> }`** so the next step doesn't race the new page load.

### 5. Screenshot

```
mcp__browser__screenshot {
  "tab_id": "...",
  "full_page": false,                 // false = viewport (default); true = full page
  "element_selector": "#hero",        // optional: shoot just one element
  "format": "png"                     // or "jpeg" (+ optional "quality")
}
```

Returns image data via the workbench (not inlined, to save tokens). Tell the user "screenshot saved to workbench".

### 6. Crawl multiple pages

```
mcp__browser__crawl {
  "start_url": "https://blog.example.com",
  "max_pages": 20,                    // default 50
  "depth": 2,                         // default 2
  "extract_type": "markdown",         // "text" (default) | "markdown" | ...
  "link_patterns": [".*/posts/.*"],   // regexes a link must match to follow
  "exclude_patterns": ["/tag/.*"],    // optional
  "same_domain": true                 // default true
}
```

Returns a `job_id`. Poll with `mcp__browser__crawl_status { "job_id": "..." }`. Stop a stuck per-tab task with `mcp__browser__stop_task { "tab_id": "..." }`.

## Decision tree

```
User asks for…
├── A specific fact / list / "current X today"
│       → mcp__browser__search (fastest)
├── Content of a known URL
│       → mcp__browser__navigate + mcp__browser__extract_text
├── Structured data from a page
│       → mcp__browser__extract_structured (schema) or mcp__browser__extract_table
├── Multi-page sweep of a site
│       → mcp__browser__crawl + mcp__browser__crawl_status
├── Login / sign-up / form submission
│       → mcp__browser__fill_form (submit: true)
├── Visual proof / share with user
│       → mcp__browser__screenshot
└── Page state debug
        → mcp__browser__snapshot
```

## Rules

- **ToolSearch before the first call** (Step 0). A direct call to a not-yet-loaded `mcp__browser__*` tool fails with `InputValidationError`.
- **Search first** for general questions — use `mcp__browser__search` before navigating blindly.
- **Single tab per task** — open with `mcp__browser__new_tab`, close with `mcp__browser__close_tab` when done. `tab_id` is optional elsewhere (defaults to the active tab).
- **Indices come from the snapshot** — never invent element numbers. If you need to click and don't have a current snapshot, run `mcp__browser__snapshot` first; re-snapshot after the DOM changes.
- **Wait when navigating** — use `mcp__browser__click_and_wait` for nav-triggering clicks; `mcp__browser__wait` after manual navigation if the page is slow.
- **Respect login boundaries** — the browser uses the user's real cookies. Don't post / pay / send messages without explicit user confirmation.
- **Report the URL** in the final response so the user can verify the source.
- **No emojis** in tool-output relays unless the user asks.

## Failure handling

| Symptom | Cause | Fix |
|---|---|---|
| `InputValidationError` on a browser tool | Tool not loaded yet | Run the Step 0 `ToolSearch` first, then retry |
| `Extension not connected` | Chrome extension offline | Ask user to open the SenClaw browser tab |
| `Tab not found` after navigate | Tab closed mid-task | Re-create with `mcp__browser__new_tab` |
| `Element not found` | Index stale after DOM change | Re-run `mcp__browser__snapshot` then retry |
| Search returns empty | Engine rate-limited | Switch `engine` to the other (google ↔ bing) |
| Crawl stuck | Task hung | `mcp__browser__stop_task { tab_id }`, then narrow `link_patterns` and re-crawl |

## Examples

**Query**: "tìm giá bạc hôm nay"

```
1. ToolSearch { query: "select:mcp__browser__search" }        ← load the search tool
2. mcp__browser__search {
     "query": "giá bạc hôm nay vnđ",
     "num_results": 5
   }
3. Read top snippets — answer the user with the figure + source URL.
```

**Query**: "screenshot github.com/trending"

```
1. ToolSearch { query: "select:mcp__browser__navigate,mcp__browser__screenshot" }
2. mcp__browser__navigate { "url": "https://github.com/trending" }
   → { tab_id: "tab-1", ... }
3. mcp__browser__screenshot { "tab_id": "tab-1", "full_page": false }
   → "screenshot saved to workbench"
4. Reply: "Screenshot of github.com/trending saved. [workbench link]"
```

**Query**: "extract top 10 hacker news front-page posts as JSON"

```
1. ToolSearch { query: "select:mcp__browser__navigate,mcp__browser__extract_structured" }
2. mcp__browser__navigate { "url": "https://news.ycombinator.com" }
3. mcp__browser__extract_structured {
     "tab_id": "...",
     "schema": {
       "type": "array",
       "items": {
         "type": "object",
         "properties": {
           "rank":      { "type": "number" },
           "title":     { "type": "string" },
           "url":       { "type": "string" },
           "points":    { "type": "number" },
           "comments":  { "type": "number" }
         }
       }
     },
     "max_items": 10
   }
4. Return the JSON to the user.
```

## Tools available (full list)

All called as `mcp__browser__<name>`:

Navigation: `navigate`, `new_tab`, `close_tab`, `list_tabs`, `switch_tab`, `go_back`, `go_forward`, `reload`

Interaction: `click`, `type`, `select_option`, `scroll`, `hover`, `press_key`, `upload_file`, `fill_form`, `click_and_wait`, `execute_js`, `wait`

Inspection: `snapshot`, `screenshot`, `get_status`

Extraction: `extract_text`, `extract_links`, `extract_table`, `extract_structured`

Research: `search`, `crawl`, `crawl_status`, `stop_task`
