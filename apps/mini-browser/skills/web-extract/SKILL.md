---
name: web-extract
description: >-
  Pull STRUCTURED data out of web pages with the SenClaw Browser — tables, lists,
  prices, contact info (emails/phones), links, or any fields into JSON/CSV, and
  compare data across several pages. Use when the user wants the actual data
  extracted or scraped, not just a summary. NOT for casual reading (use browse-web)
  and NOT for actions that change a site (use web-task). Triggers: "lấy dữ liệu từ
  trang", "trích xuất bảng", "lấy bảng giá", "so sánh giá", "lấy danh sách", "lấy
  email/liên hệ", "lấy tất cả link", "cào/scrape trang", "extract the table",
  "scrape this page", "get all links", "extract contacts", "compare prices".
---

# web-extract

Extract structured data from web pages through the **SenClaw Browser** app via the
`mini-browser-mcp` MCP server, and shape it into whatever the user asked for
(JSON, a table, CSV-style text).

## When to use this skill

- Extract a **table**, **list**, or repeated records from a page.
- Collect **contact info** (emails, phone numbers), **prices**, **product specs**,
  **links**, or specific fields.
- **Compare** the same data across multiple pages/products.
- "Scrape" / "pull" / "grab the data" from one or more pages.

## When NOT to use it

- Just reading or summarizing → **browse-web**.
- Doing something on the site (login, form, purchase, download) → **web-task**.

## Tools you'll use

- `mcp__mini-browser-mcp__browser_navigate` — go to the page.
- `mcp__mini-browser-mcp__browser_snapshot` — the page as an accessibility tree.
  Worth reading before extracting: it preserves table rows, list structure and link
  targets, which flat text throws away.
- `mcp__mini-browser-mcp__browser_extract` (`request`, optional `schema`) — AI
  extraction: describe the fields you want, and pass `schema` to pin the JSON shape,
  e.g. `schema: "[{name, price, rating}]"`. Best for messy or semantic data.
- `mcp__mini-browser-mcp__browser_find` — locate one thing on a big page without
  paying for the whole snapshot.
- `mcp__mini-browser-mcp__browser_extract_links` — every link as `{href, text}`.
- `mcp__mini-browser-mcp__browser_extract_text` — raw text (optional CSS `selector`).
- `mcp__mini-browser-mcp__browser_execute_js` — precise DOM scraping when structure
  is regular (return a JSON-serializable value with `return`).
- `mcp__mini-browser-mcp__browser_scroll` — lazy-loaded lists only render what has
  been scrolled to. Scroll and re-snapshot until the count stops growing, or you
  will confidently report the first twenty of two hundred rows. The snapshot tells
  you when to stop: it ends with `[end of page]` rather than `[more below]`.

## Recipes

- **Extract a table** → navigate → `browser_extract` "Return the table as JSON array
  of row objects using the header cells as keys." (or `browser_execute_js` querying
  `table tr`/`td` for exact control).
- **Prices / product list** → navigate to the listing → `browser_extract` "Return
  JSON [{title, price, url}] for each item." Scroll + repeat if the list is long or
  lazy-loaded (`browser_scroll` down, snapshot, extract again, merge).
- **Contacts (emails/phones)** → `browser_execute_js` with a regex over
  `document.body.innerText`, or `browser_extract` "List every email and phone number
  on the page as JSON."
- **All links (optionally filtered)** → `browser_extract_links`, then filter by
  pattern in your answer.
- **Compare across pages** → for each URL: navigate → extract the same fields →
  collect → present a single comparison table, citing each source URL.
- **Paginated data** → extract page 1, click "Next"/increment the page URL with
  `browser_navigate`, extract again, until no new rows; then merge and dedupe.

## Notes

- Extraction is **read-only** — don't submit anything. Say which URL(s) each row
  came from.
- Respect each site's terms and rate limits; don't hammer a site — input is already
  human-paced, and heavy scraping should be modest and considerate.
- If the data isn't present (JS-rendered later), scroll / wait and re-snapshot before
  concluding it's missing.
