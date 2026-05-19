---
name: agent-browser
description: Drive the connected browser via the `senclaw-browser` MCP server — search the web, navigate sites, extract structured data, fill forms, take screenshots. Use whenever a task needs live web content (current prices, news, docs, web research) or interaction with a web app.
version: 1.0.0
when-to-use: any request that needs fresh web content, web automation, or page screenshots (e.g. "tìm giá vàng hôm nay", "screenshot github trending", "fill this form", "extract product list from amazon")
---

# Agent Browser Skill

A connected Chrome instance is exposed through the `senclaw-browser` MCP server (~30 tools). All tools are prefixed `mcp__browser__*`. The browser runs in the user's actual session — pages they're logged in to are accessible.

Tools start **deferred** to save tokens — discover them with `ToolSearch { query: "<keywords>" }` before first use. After ToolSearch returns matches, call the tool directly in the same conversation.

## Core workflows

### 1. Quick web search (preferred for "find/search X")

Use **`mcp__browser__search`** — returns ranked SERP results without opening a tab. Cheapest path for fact-finding.

```
mcp__browser__search {
  "query": "giá bạc hôm nay vnđ",
  "engine": "google",            // or "bing"
  "num_results": 10
}
```

Returns: `[{ title, url, snippet, position }]`. Use snippets directly if they answer the question. If you need full content, `browser_navigate` to the top URL.

### 2. Open a page + read content

```
mcp__browser__navigate { "url": "https://example.com/article" }
↓
mcp__browser__snapshot { "tab_id": "<from navigate response>" }
↓ pick element indices from snapshot
mcp__browser__extract_text { "tab_id": "...", "element_index": <n> }
```

`snapshot` returns the accessibility tree with **numbered indices** for every visible element. Those indices are the addresses passed to `click`, `type_text`, `hover`, `extract_text`, etc.

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
  }
}
```

For HTML tables specifically use `mcp__browser__extract_table` — faster, no schema needed.

### 4. Fill forms / interact

```
mcp__browser__fill_form {
  "tab_id": "...",
  "fields": [
    { "label": "Email", "value": "alice@example.com" },
    { "label": "Password", "value": "..." }
  ],
  "submit": true
}
```

Auto-matches by label / placeholder / name / CSS selector. For single-field interactions use `browser_type` + `browser_click` directly.

For navigation-triggering clicks (login, submit), prefer **`browser_click_and_wait`** so the next step doesn't race the new page load.

### 5. Screenshot

```
mcp__browser__screenshot {
  "tab_id": "...",
  "mode": "viewport"   // or "full_page" / element_selector
}
```

Returns image data via the workbench (not inlined to save tokens). Tell the user "screenshot saved to workbench".

### 6. Crawl multiple pages

```
mcp__browser__crawl {
  "start_url": "https://blog.example.com",
  "max_pages": 20,
  "extract_type": "markdown",
  "include_pattern": ".*/posts/.*"
}
```

Returns a `task_id`. Poll with `mcp__browser__crawl_status { task_id }`. Stop early with `mcp__browser__stop_task`.

## Decision tree

```
User asks for…
├── A specific fact / list / "current X today"
│       → browser_search (fastest)
├── Content of a known URL
│       → browser_navigate + browser_extract_text
├── Structured data from a page
│       → browser_extract_structured (schema) or browser_extract_table
├── Multi-page sweep of a site
│       → browser_crawl + browser_crawl_status
├── Login / sign-up / form submission
│       → browser_fill_form (submit: true)
├── Visual proof / share with user
│       → browser_screenshot
└── Page state debug
        → browser_snapshot
```

## Rules

- **Always call `browser_search` first** for general questions. Don't navigate blindly.
- **Single tab per task** — open with `new_tab`, close with `close_tab` when done.
- **Indices come from snapshot** — never invent element numbers. If you need to click and don't have a snapshot, run `snapshot` first.
- **Wait when navigating** — use `click_and_wait` for nav-triggering clicks; `wait` after manual navigation if page is slow.
- **Respect login boundaries** — the browser uses the user's real cookies. Don't post / pay / send messages without explicit user confirmation.
- **Report the URL** in the final response so the user can verify the source.
- **No emojis** in tool output relays unless the user asks.

## Failure handling

| Symptom | Cause | Fix |
|---|---|---|
| `Extension not connected` | Chrome extension offline | Ask user to open the SenClaw browser tab |
| `Tab not found` after navigate | Tab closed mid-task | Re-create with `new_tab` |
| `Element not found` | Index stale after DOM change | Re-`snapshot` then retry |
| Search returns empty | Engine rate-limited | Switch `engine` to the other (google ↔ bing) |
| Crawl stuck | Task hung | `browser_stop_task { task_id }` then narrow `include_pattern` |

## Examples

**Query**: "tìm giá bạc hôm nay"

```
1. ToolSearch { query: "browser search web" }    ← discover search tool
2. mcp__browser__search {
     "query": "giá bạc hôm nay vnđ",
     "num_results": 5
   }
3. Read top snippets — answer the user with the figure + source URL.
```

**Query**: "screenshot github.com/trending"

```
1. mcp__browser__navigate { "url": "https://github.com/trending" }
   → { tab_id: "tab-1" }
2. mcp__browser__screenshot { "tab_id": "tab-1", "mode": "viewport" }
   → "screenshot saved to workbench"
3. Reply: "Screenshot of github.com/trending saved. [workbench link]"
```

**Query**: "extract top 10 hacker news front-page posts as JSON"

```
1. mcp__browser__navigate { "url": "https://news.ycombinator.com" }
2. mcp__browser__extract_structured {
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
     }
   }
3. Return the JSON to the user.
```

## Tools available (full list)

Navigation: `navigate`, `new_tab`, `close_tab`, `list_tabs`, `switch_tab`, `go_back`, `go_forward`, `reload`

Interaction: `click`, `type_text`, `select_option`, `scroll`, `hover`, `press_key`, `upload_file`, `fill_form`, `click_and_wait`, `execute_js`, `wait`

Inspection: `snapshot`, `screenshot`, `get_status`

Extraction: `extract_text`, `extract_links`, `extract_table`, `extract_structured`

Research: `search`, `crawl`, `crawl_status`, `stop_task`
