---
name: crm-log-interaction
description: >-
  Write side of the SenClaw CRM: create/update/delete contacts, manage their
  contact channels, log interactions (call/email/meeting/note/task), manage
  deals and their service line items, link contacts to organizations, maintain
  the service catalogue, create/complete/delete tasks, add/delete relationships,
  run AI graph extraction, and trigger calendar sync. Use for "ghi lại cuộc
  gọi", "thêm khách mới", "cập nhật avatar", "lưu Zalo của khách", "chuyển deal
  X sang won", "thêm dịch vụ vào deal", "X làm ở công ty Y", "nhắc tôi gọi lại
  tuần sau", "Anna do Tuấn Anh giới thiệu", "phân tích ai giới thiệu ai cho
  khách X", "đồng bộ lịch", "log a call", "add a customer", "move deal to won",
  "link contact to company", "add task". Always resolve identity BEFORE writing
  — never guess ids. NOT for sending anything to a customer — every outbound
  message goes through sale_send (use crm-sale-followup).
---

# crm-log-interaction

Perform any write operation on the SenClaw CRM via **`crm-mcp`**. Every write
takes effect immediately in the app.

## Tool catalogue — 28 write tools

### Contacts

- **`mcp__crm-mcp__crm_create_customer`** — only `name` is required. Fields:
  `email, phone, company, title, avatar_url` (https:// OR
  `data:image/…;base64,…` inline), `notes`, `tags` (array), `role`, `source`,
  `address`, `birthday` (YYYY-MM-DD).
  - **`role`** — `lead | prospect | customer | vip | contact | partner |
    referrer | supplier | investor | employee | former | paused | lost`.
    Defaults to `lead`.
  - **Creating a `lead` can enrol the welcome sequence** — but only when the
    `auto_welcome` setting is on, and it is **OFF by default**. Filing a contact
    must not message them. Do not rely on it either way; if outreach is wanted,
    hand off to `crm-sale-followup`.
  - Prefer `crm_link_organization` over typing `company` by hand — see
    Organizations below.
- **`mcp__crm-mcp__crm_update_customer`** — patch by `id`. Omitted fields
  untouched. Empty string clears a scalar. Pass a replacement array to overwrite
  tags (read-merge-write; do not blind-overwrite).
- **`mcp__crm-mcp__crm_delete_customer`** — deletes the contact **and every
  interaction logged against them**. Irreversible — confirm first.

### Contact channels (their handles)

- **`mcp__crm-mcp__crm_add_channel`** — `customer_id`, `kind`, `value`,
  `label`. `kind`: `phone | email | zalo | facebook | linkedin | instagram | x |
  tiktok | youtube | github | telegram | whatsapp | signal | line | wechat |
  skype | viber | discord | messenger | website`. `label` is free-form shorthand
  ("Công việc", "Cá nhân").
- **`mcp__crm-mcp__crm_update_channel`** — patch `id` (`kind`/`value`/`label`).
- **`mcp__crm-mcp__crm_delete_channel`** — by `id`. Confirm first.

These are the **customer's** handles. Our own connected inbox accounts are a
different thing entirely — see `crm-inbox`.

### Interactions

- **`mcp__crm-mcp__crm_add_interaction`** — `customer_id` + `summary` required.
  `kind` in `call | email | meeting | note | task` (default `note`). `details`
  for a longer body. `occurred_at` (Unix seconds) defaults to now.
- **`mcp__crm-mcp__crm_delete_interaction`** — hard-delete one log entry by `id`.

### Deals

- **`mcp__crm-mcp__crm_add_deal`** — `customer_id`, `title` required. `stage`:
  `qualifying | proposal | negotiation | won | lost` (default `qualifying`).
  `amount`, `currency` (default VND), `probability` (0..100, default 50),
  `expected_close_at` (Unix seconds), `notes`.
- **`mcp__crm-mcp__crm_move_deal`** — patch by `id`. Setting `stage=won|lost`
  stamps `closed_at` automatically.
- **`mcp__crm-mcp__crm_delete_deal`** — irreversible.

**`deals.stage` is not the nurture pipeline.** It tracks ONE opportunity. How
warm the *person* is lives in `customers.sale_stage` and moves with
`sale_update_stage` (`crm-sale-followup`).

### Deal line items

- **`mcp__crm-mcp__crm_attach_service`** — `deal_id`, `service_id`, `quantity`
  (default 1), `unit_amount`, `note`. `unit_amount` defaults to the catalogue
  price and is then **frozen** on the line. **Recomputes the deal's total.**
- **`mcp__crm-mcp__crm_detach_service`** — `deal_id`, `service_id`. Recomputes.

Once a deal has line items, its `amount` is **derived** from them — setting
`amount` via `crm_move_deal` on such a deal is fighting the recompute. Change
the lines instead.

### Organizations

- **`mcp__crm-mcp__crm_link_organization`** — `customer_id` +
  `organization_id` **or** `organization_name` (resolve-or-create by exact
  name). `role_title`, `is_primary`. **`is_primary: true` also updates the
  contact's `company` text** — this is the right way to set `company`.
- **`mcp__crm-mcp__crm_unlink_organization`** — `customer_id`,
  `organization_id`. Neither record is deleted.
- **`mcp__crm-mcp__crm_create_organization`** — `name` required, `kind`
  (`direct_customer | affiliated_company | partner | supplier | prospect`,
  default `direct_customer`), `website`, `domain`, `industry`, `size`,
  `address`, `notes`, `tags`. **Call `crm_find_organization` first** — never
  create a second row for a company that already exists.
- **`mcp__crm-mcp__crm_update_organization`** — patch by `id`.
- **`mcp__crm-mcp__crm_delete_organization`** — contacts and deals are
  **UNLINKED, not deleted**. Confirm first.

For anything beyond a quick link, use **`crm-organizations`** — it carries the
resolve-before-create rules in full.

### Service catalogue

- **`mcp__crm-mcp__crm_create_service`** — `name` required, `kind`
  (`service | hardware`, default `service`), `amount`, `currency` (default VND),
  `pricing_model` (`fixed | hourly | daily | monthly | yearly`, default
  `fixed`), `unit`, `sku`, `description`.
- **`mcp__crm-mcp__crm_update_service`** — patch by `id`. **`active: false`
  retires an entry** without deleting it.
- **`mcp__crm-mcp__crm_delete_service`** — **FAILS if it prices any deal.**
  Deactivate instead; those line items are what a customer was quoted.

### Tasks

- **`mcp__crm-mcp__crm_add_task`** — `title` required. Optional `customer_id`
  (a task can be unattached), `details`, `due_at` (Unix seconds).
- **`mcp__crm-mcp__crm_complete_task`** — `done=true|false` (reopen).
- **`mcp__crm-mcp__crm_delete_task`** — hard-delete. Different from completing.

### Relationships (network)

- **`mcp__crm-mcp__crm_add_relationship`** — `from_id --(kind)--> to_id`, read
  as "from is <kind> to". Kinds: `referred_by, introduced_by, colleague_of,
  spouse_of, family_of, friend_of, reports_to, partner_of, supplier_of,
  competitor_of, contact_of`. Optional `note`, `confidence` (0..1, default 1.0).
  Duplicate (from, to, kind) triples upsert.
- **`mcp__crm-mcp__crm_delete_relationship`** — remove by `id`.

### AI-driven writes

- **`mcp__crm-mcp__crm_extract_graph`** — `customer_id`. The LLM reads the
  profile + notes + interactions and extracts every OTHER person mentioned +
  the implied relationship. Each is saved as a `mention`; when the name matches
  an existing contact, a directional relationship (`source='ai'`) is created
  automatically. Requires the daemon's LLM.

### Integrations

- **`mcp__crm-mcp__crm_sync_calendar`** — `space_calendar` (bool, default true).
  Pushes open tasks + upcoming birthdays to the Space Calendar app. Upsert
  semantics: events the user edited or deleted on the calendar side are
  preserved. Returns `{pushed_tasks, pushed_birthdays, targets, warnings, note}`.

## Workflow rules

1. **RESOLVE identity FIRST.** For every write on an existing contact, look them
   up via `crm_list_customers(q=…)` or `crm_find_by_email` and confirm the id
   with the user if more than one matches. Same for organizations:
   `crm_find_organization` before `crm_create_organization`, always.
2. **Choose the right interaction kind.** `call` for phone, `email` for email,
   `meeting` for in-person/video, `note` for observation, `task` for a to-do.
3. **Summary should scan.** One line ("Alo hỏi thăm sau đơn hàng #42"). Context
   goes in `details`.
4. **Patch minimally.** For role/tag changes, send just the changed field. Don't
   overwrite `notes` with a partial — read, append, write.
5. **Confirm every write.** Report the id + what changed so the user can verify.
6. **Deletes need confirmation.** Show what will be deleted (name / summary /
   amount) and wait for a clear yes.
7. **Read back a derived total.** After attaching/detaching a line item, call
   `crm_deal_services({deal_id})` and report the recomputed total.

## Composed workflows

**New contact via referral**
1. `crm_create_customer({name: "Anna", role: "lead", …})`
2. `crm_list_customers({q: "Tuấn"})` → the referrer's id
3. `crm_add_relationship({from_id: anna.id, to_id: tuan.id, kind: "referred_by", note: "Sự kiện startup 2026"})`

**New contact at a company**
1. `crm_find_organization({name: "Shop Co"})` → id, or create after checking
   `crm_list_organizations({q: "shop"})` for near-misses
2. `crm_create_customer({name: "Anna", role: "lead"})`
3. `crm_link_organization({customer_id, organization_id, role_title: "Marketing Lead", is_primary: true})`
   — this sets `company` for you; do not also type it.

**Log a call that closed a deal**
1. `crm_add_interaction({customer_id, kind: "call", summary: "Chốt yearly"})`
2. `crm_move_deal({id, stage: "won"})`
3. `crm_add_task({customer_id, title: "Gửi hợp đồng", due_at: <tomorrow>})`

**Price a deal from the catalogue**
1. `crm_list_services({q: "yearly", active_only: true})` → service id
2. `crm_attach_service({deal_id, service_id, quantity: 1})`
3. `crm_deal_services({deal_id})` → report the recomputed total

**AI enrichment after adding a contact**
1. `crm_extract_graph({customer_id})`
2. Report which mentions were auto-linked vs left unresolved.

## Do not

- Do not fabricate emails, phone numbers, birthdays, or prices.
- Do not delete without an explicit user confirmation.
- Do not create a duplicate organization — resolve first.
- Do not delete a service that prices a deal — deactivate it.
- **Do not send anything.** No tool here reaches a customer. Outbound goes
  through `sale_send` and its guardrail — hand off to **`crm-sale-followup`**.
  A logged interaction records that something happened; it does not make it
  happen.
