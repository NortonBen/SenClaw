---
name: crm-log-interaction
description: >-
  Write side of the SenClaw CRM: create/update/delete customers, log interactions
  (call/email/meeting/note/task), manage deals (create, move stage, delete),
  create/complete/delete tasks, add/delete relationships between customers, run
  AI graph extraction, and trigger calendar sync with luna-calendar &
  event-space. Use for "ghi lại cuộc gọi", "thêm khách mới", "cập nhật avatar",
  "chuyển deal X sang won", "nhắc tôi gọi lại tuần sau", "Anna do Tuấn Anh giới
  thiệu", "phân tích ai giới thiệu ai cho khách X", "đồng bộ lịch". Always
  resolve identity BEFORE writing — never guess ids.
---

# crm-log-interaction

Perform any write operation on the SenClaw CRM via **`crm-mcp`**. Every write
takes effect immediately in the app (the timeline picks up profile/deal edits
as `profile_update` / `deal_update` entries automatically).

## Tool catalogue — 13 write tools

### Customers

- **`mcp__crm-mcp__crm_create_customer`** — only `name` is required. Fields:
  `email, phone, company, title, avatar_url` (https:// OR `data:image/…;base64,…`
  inline), `tags` (array), `role`
  (`lead|prospect|customer|vip|contact|partner|referrer|supplier|investor|
  employee|former|paused|lost`, default `lead`), `source`, `address`,
  `birthday` (YYYY-MM-DD).
- **`mcp__crm-mcp__crm_update_customer`** — patch by `id`. Omitted fields untouched.
  Empty string clears a scalar. Pass a replacement array to overwrite tags.
  Adds a `profile_update` interaction with the diff — optionally pass a
  `change_note` (free-form user note) that appears in the log.
- **`mcp__crm-mcp__crm_delete_customer`** — CASCADE deletes interactions,
  relationships, mentions. Irreversible — confirm first.

### Interactions

- **`mcp__crm-mcp__crm_add_interaction`** — `customer_id` + `summary` required.
  `kind` in `call|email|meeting|note|task` (default `note`). `details` for a
  longer body. `occurred_at` (Unix seconds) defaults to now.
- **`mcp__crm-mcp__crm_delete_interaction`** — hard-delete a single log entry
  by id.

### Deals

- **`mcp__crm-mcp__crm_add_deal`** — `customer_id`, `title` required. Stages:
  `qualifying|proposal|negotiation|won|lost` (default `qualifying`).
  `amount`, `currency` (default VND), `probability` (0..100, default 50),
  `expected_close_at` (Unix seconds), `notes`.
- **`mcp__crm-mcp__crm_move_deal`** — patch by `id`. Setting stage=won/lost
  stamps `closed_at` automatically. Logs a `deal_update` interaction with the
  diff on the customer.
- **`mcp__crm-mcp__crm_delete_deal`** — irreversible.

### Tasks

- **`mcp__crm-mcp__crm_add_task`** — `title` required. Optional
  `customer_id`, `details`, `due_at`.
- **`mcp__crm-mcp__crm_complete_task`** — mark done or reopen (`done=true|false`).
- **`mcp__crm-mcp__crm_delete_task`** — hard-delete.

### Relationships (network)

- **`mcp__crm-mcp__crm_add_relationship`** — link two customers.
  `from_id --(kind)--> to_id`. Kinds: `referred_by, introduced_by,
  colleague_of, spouse_of, family_of, friend_of, reports_to, partner_of,
  supplier_of, competitor_of, contact_of`. Duplicate (from, to, kind) upserts.
- **`mcp__crm-mcp__crm_delete_relationship`** — remove by id.

### AI-driven writes

- **`mcp__crm-mcp__crm_extract_graph`** — LLM reads a customer's profile + notes +
  interactions and extracts every OTHER person mentioned + implied relationship
  kind. Each extraction is saved as a `mention` (unresolved) OR
  auto-materialized into `relationships(source='ai')` when the name matches an
  existing customer (case-insensitive or token-match).

### Integrations

- **`mcp__crm-mcp__crm_sync_calendar`** — push open tasks + upcoming birthdays
  to `luna-calendar` (Vietnamese lunar) and `event-space`. Args: `luna` (bool,
  default true), `event` (bool, default false). Returns `pushed_tasks`,
  `pushed_birthdays`, `warnings` (per-target unreachable notices).

## Workflow rules

1. **RESOLVE identity FIRST.** For every write on an existing customer, look
   the person up via `crm_list_customers(q=…)` or `crm_find_by_email` and
   confirm the id with the user if more than one matches.
2. **Choose the right interaction kind.** `call` for phone, `email` for email,
   `meeting` for in-person/video, `note` for observation, `task` for a to-do.
3. **Summary should scan.** One line ("Alo hỏi thăm sau đơn hàng #42"). Put
   context in `details`.
4. **Patch minimally.** For status/tag/role changes, just send the changed
   field. Don't overwrite `notes` with a partial — append instead.
5. **Confirm every write.** Report back the id + what changed so the user can
   verify. Deals and profile edits already generate a diffed interaction entry,
   so referencing them in your reply reinforces trust.
6. **Deletes need confirmation.** For `delete_*` tools, always show what will
   be deleted (name / summary / amount) and wait for a clear yes.

## Composed workflows

- **New customer via referral**:
  1. `crm_create_customer({name: "Anna", ...})`
  2. Look up the referrer: `crm_list_customers(q="Tuấn")`
  3. `crm_add_relationship({from_id: Anna.id, to_id: Tuấn.id, kind: "referred_by", note: "Sự kiện startup 2026"})`

- **Log a call that closed a deal**:
  1. `crm_add_interaction({customer_id, kind: "call", summary: "Chốt yearly"})`
  2. `crm_move_deal({id, stage: "won", change_note: "Khách quyết trong cuộc gọi hôm nay"})`
  3. `crm_add_task({customer_id, title: "Gửi hợp đồng", due_at: <tomorrow>})`

- **AI enrichment after adding a customer**:
  1. `crm_extract_graph({customer_id})` — LLM parses their notes/interactions
     and adds `mentions` + auto-links relationships to existing customers.
  2. Report back which mentions were auto-linked vs left unresolved.

## Do not

- Do not fabricate emails, phone numbers, birthdays.
- Do not delete without an explicit user confirmation.
- Do not push to calendar without checking the sync settings first (`crm_sync_calendar`
  reads `luna`/`event` args; the settings are stored per-user via the UI).
