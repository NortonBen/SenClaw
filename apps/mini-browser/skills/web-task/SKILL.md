---
name: web-task
description: >-
  Carry out a multi-step ACTION on a live web page with the SenClaw Browser — log
  in, fill and submit a form, search-and-open a result, add to cart / check out,
  book or order, register / sign up / subscribe, download a file, post or comment,
  apply, or click through any flow. Use when the user wants the AI to DO something
  on the web, not just read (browse-web) or extract data (web-extract). Triggers:
  "đăng nhập/đăng ký", "điền/gửi form", "đặt/mua/thanh toán", "đặt vé/phòng", "tải
  file về", "đăng bài/bình luận", "nộp đơn/ứng tuyển", "làm giúp tôi trên web",
  "tự động thao tác", "log in", "fill/submit the form", "sign up", "place an order",
  "add to cart", "check out", "book", "download from", "apply for", "click through".
---

# web-task

Drive a real browser to accomplish a goal on the web, through the
`mini-browser-mcp` MCP server. The user watches the same live browser you drive.

## When to use this skill

- Any goal that needs several actions on a page or across pages: **log in**, **fill
  and submit a form**, **register / sign up / subscribe**, **place an order / buy /
  add to cart / check out**, **book a ticket or room**, **download a file**, **post
  or comment**, **apply / submit an application**, or **click through a flow**
  (search → open result → do something).

## When NOT to use it

- Just reading/summarizing → **browse-web**. Just extracting data → **web-extract**.

## Two ways to do it

**A. Autonomous agent loop (preferred for open-ended goals).**
Call `mcp__mini-browser-mcp__browser_act` with a clear `instruction` (and optional
`max_steps`, 1–12). It runs an observe→decide→act loop on the live page and returns
a log of every step. Read the log; if the goal isn't done, call it again with a
refined instruction or finish manually below.

**B. Drive the primitives yourself (for precise control).**
1. `browser_navigate` to the starting page.
2. `browser_snapshot` — read the numbered elements (each `idx`).
3. `browser_type` into a field `idx` (set `submit: true` to press Enter after), and/or
   `browser_click` a button `idx`.
4. `browser_snapshot` again to see the result; repeat until done.
5. Use `browser_press_key`, `browser_scroll`, `browser_new_tab`, `browser_switch_tab`
   as needed.

## Recipes

- **Log in** → navigate to the login page → snapshot → `browser_type` the username
  field, `browser_type` the password field → `browser_click` the submit button →
  snapshot to confirm you're signed in. Only use credentials the user provided.
- **Fill & submit a form** → snapshot → `browser_type` each field by `idx` → for
  dropdowns/checkboxes `browser_click` the option → `browser_click` Submit →
  snapshot the confirmation. Read the values back to the user before submitting if
  the form is consequential.
- **Search and open the first result** → `browser_navigate` the query →
  `browser_snapshot` → `browser_click` the first result's `idx`.
- **Add to cart / check out / buy** → navigate to the product → click "Add to cart"
  → go to cart → **STOP and confirm with the user before paying** → then proceed
  only on an explicit yes.
- **Book a ticket / room / appointment** → follow the flow with snapshot-between-steps
  → confirm dates/price with the user before the final booking step.
- **Download a file** → navigate → `browser_click` the download link; report where
  it went.
- **Post / comment / apply** → fill the field → **confirm the exact text with the
  user** → then `browser_click` submit.

## Safety (important)

- **Confirm before anything irreversible or consequential**: payments, sending
  money, placing orders, booking, public posts/comments, deletions, or submitting
  personal data the user didn't explicitly authorize. Describe what you're about to
  do and wait for a clear yes.
- Never enter credentials or personal info the user hasn't given you.
- Respect each site's terms and rate limits; don't use the stealth capability to
  spam, mass-register, scrape abusively, or evade access controls. Input is already
  human-paced — don't flood a site with rapid repeated actions.
