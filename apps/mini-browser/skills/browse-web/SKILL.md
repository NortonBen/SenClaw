---
name: browse-web
description: >-
  Open web pages, search the web, and READ / summarize / translate / answer
  questions about a page using the SenClaw Browser app (a real, stealth Chromium
  the user watches live). Use for any read-only "look it up / open / read / what
  does it say" request. NOT for pulling out structured data (use web-extract) and
  NOT for doing multi-step actions like logging in or filling forms (use web-task).
  Triggers: "mở trang/web", "tìm trên web/google", "xem/đọc trang này", "tóm tắt
  trang", "trang này nói gì", "dịch trang", "open this site", "search the web for",
  "summarize this page", "what does this page say", "read this article".
---

# browse-web

Open, search, and read the web through the **SenClaw Browser** app via the
`mini-browser-mcp` MCP server. The user sees the same live browser you drive, so
keep a short running narration of what you're doing.

## When to use this skill

- Open a specific URL, or search the web for something and open a result.
- Read, summarize, or translate the page the user is looking at.
- Answer a question whose answer is on a web page.
- Check what a site says / read the news / follow a link.

## When NOT to use it

- The user wants **structured data pulled out** (a table, a list, prices, emails,
  all links) → use **web-extract**.
- The user wants the browser to **do something** (log in, fill a form, buy, book,
  download, post) → use **web-task**.

## Core loop

1. **Go somewhere.** `mcp__mini-browser-mcp__browser_navigate` with a URL *or* a
   search phrase (a bare domain gets `https://`; a phrase becomes a Google search).
2. **See the page.** `mcp__mini-browser-mcp__browser_snapshot` → the page as an
   accessibility tree: every element with its role, name, state and a `[ref=eN]`.
   Refs are what click/type take, they stay valid across re-renders on the same
   page, and a `*` marks anything that appeared since your last snapshot. On a
   large page `browser_find` returns just the matching lines and is much cheaper.
3. **Read / answer.** For a question or a summary, prefer
   `mcp__mini-browser-mcp__browser_extract` (`request`) — it reads the page text and
   answers grounded in it. For raw text use `browser_extract_text` (optional CSS
   `selector`).
4. **Move around.** Snapshot, then `browser_click` an element `ref`, or
   `browser_navigate` to a known URL; `browser_scroll` (`direction` up/down) to
   reveal more; `browser_back` / `browser_forward` / `browser_reload` as needed.
5. **Report.** Give a concise answer and cite the page title + URL.

## Recipes

- **Search the web** → `browser_navigate` with the query → `browser_snapshot` →
  `browser_click` the result you want → `browser_extract` your question.
- **Nothing seems to happen after a click** → check `browser_list_tabs` (it may have
  opened a tab) and `browser_console_messages` (the page may have thrown).
- **Summarize this page** → `browser_extract` with request "Summarize this page in
  5 bullet points" (it already has the page text).
- **Translate this page** → `browser_extract` with "Translate the main content to
  Vietnamese" (short pages) — for long pages, summarize+translate section by section.
- **Answer a question from a site** → navigate to the likely source, snapshot,
  `browser_extract` the exact question; if not found, follow a link and repeat.
- **Open a link the user pasted** → `browser_navigate` to it, snapshot, tell them
  what's there.

## Notes

- The browser is **stealth** and shares one live session with the user — your reads
  look like a real person browsing. Never announce yourself as a bot on a page.
- Keep answers grounded: only state what the page actually contains; cite title/URL.
