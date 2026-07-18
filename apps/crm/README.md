# SenClaw CRM

A CRM Space App for SenClaw that runs the whole customer lifecycle in one
process: who they are, who they work for, what you sell them, what they said,
and the proactive sales motion on top — behind a guardrail.

It absorbed the standalone **AI Sale** app. That app is gone; its engine,
skills and personas live here now. The merge was not cosmetic — see
[Proactive sales](#proactive-sales) for what changed.

## What's in it

- **Contacts** — name, avatar (URL or inline base64 upload), email, phone,
  company, title, address, birthday, source, notes, free-form tags, and a
  `role` (`lead|prospect|customer|vip|contact|partner|referrer|supplier|
  investor|employee|former|paused|lost`).
- **Contact channels** — their extra phones, secondary emails and social
  handles (zalo, facebook, linkedin, telegram, whatsapp, …). *Their* handles —
  not to be confused with the inbox's connected accounts below.
- **Organizations** — accounts. A contact belongs to **0..n** organizations via
  the `customer_organizations` join, **one flagged primary**; the primary also
  syncs the legacy `customers.company` text, so `company` is a projection of the
  primary link rather than an independent fact. Kinds: `direct_customer |
  affiliated_company | partner | supplier | prospect`.
- **Services** — the sellable catalogue. `kind`: `service | hardware`, with
  `amount`, `currency`, and `pricing_model`: `fixed | hourly | daily | monthly |
  yearly`.
- **Deals** — opportunities across the Kanban pipeline (`qualifying → proposal →
  negotiation → won | lost`). Services attach as **line items**
  (`deal_services`) with a quantity and a **frozen** `unit_amount`; **a deal's
  `amount` is recomputed from its line items when it has any**, so changing the
  catalogue price never silently re-prices a deal that was already quoted.
- **Interactions** — chronological `call` / `email` / `meeting` / `note` /
  `task` log per contact.
- **Tasks** + an upcoming feed (due tasks + birthdays).
- **Network graph** — directed relationships (11 kinds), BFS shortest path,
  similar-contact ranking, AI extraction of people mentioned in notes, AI
  common-theme discovery.
- **Inbox** — real multi-channel messaging. See below.
- **Proactive sales** — the AI Sale engine. See below.
- **Dynamic dashboard** — user-defined charts over an ad-hoc analytics surface.
  See below.
- **Search** — FTS5 with Vietnamese diacritic folding across profiles,
  interactions and mentions ("khach" matches "khách").
- **AI** — per-contact briefing, whole-CRM executive report, revenue breakdown
  by organization and service kind. Uses the daemon's active LLM.
- **Calendar sync** — pushes open tasks + upcoming birthdays to the Space
  Calendar app; upsert semantics preserve edits made on the calendar side.

## Inbox

`channels` are **our** connected accounts (our Telegram bot, our Zalo OA):
`telegram | zalo | facebook | tiktok | websocket`. They are **polled — there are
no webhooks**, so a thread lands on the next poll.

`conversations` are threads (`open | snoozed | closed`). An inbound message's
`external_id` is resolved against `customer_channels` to auto-link a contact.
**`customer_id = 0` means the thread is unlinked** — nobody has been identified
yet.

Linking a thread (`crm_link_conversation`) is two writes in one: it attaches the
thread *and* records the platform identity on that contact, so future messages
from that handle resolve automatically. That is why a wrong link is expensive —
it mis-routes that person from then on.

Channel credentials are configured in the app's Settings UI and come back
**redacted** from the API and MCP.

## Proactive sales

**There is no `leads` table, no `lead_id`, and no `sale_capture_lead` tool.** A
lead *is* a contact row. Sales state lives on that row: `sale_stage`,
`temperature`, `lead_score`, `unsubscribed`, `last_inbound_at`, `checkin_count`.
**Everything keys on `customer_id`.**

Two pipelines, deliberately distinct:

| Field | Values | Tracks |
|---|---|---|
| `customers.sale_stage` | `new_lead, engaged, qualified, consult_scheduled, consult_done, closed_won, churned` | how warm the **person** is (`sale_update_stage`) |
| `deals.stage` | `qualifying, proposal, negotiation, won, lost` | where **one opportunity** stands (`crm_move_deal`) |

Temperature: `cold | warm | hot | churned`.

Merged in-process, the engine reads the CRM through plain `Db` calls. The
standalone app reached it over `SENCLAW_CRM_URL` and fell back to id 0 when the
other side was down — which is what let the same person be captured twice.
That hop, and that failure mode, are gone.

### The guardrail

**`sale_send` is the only path to a customer's inbox.** No raw channel send is
exposed to the agent, so the rules cannot be talked around by a clever prompt.
Enforced in Rust (`src/guardrail.rs`), fail-closed, **first match wins**:

1. **Unsubscribed → Blocked.** Never sent, never queued. **No override** — not
   even via approve-from-review. A standing instruction from the customer that
   no operator should be able to click past.
2. **Rate limit → Review.** More than `max_messages_per_customer_24h`
   (default 3) *delivered* touches in 24h. Queued drafts don't burn the budget.
3. **Risky wording → Review.** Price/discount/contract/payment/deposit/
   commitment vocabulary. A **reply** trips on ≥1 keyword, a **broadcast** on ≥2
   — a proactive message that mentions "giá" in passing is less alarming than a
   direct reply about it. Matched diacritic-folded, so "bao gia" is caught too.

Approving from the review queue waives **only** rule 3 (a human read the words);
1 and 2 still apply.

Complaint detection runs separately against the **customer's** text on inbound
and escalates to a human before any draft is written.

Two human queues: **review** (approve/edit/reject a held draft) and
**escalation** (a case the agent refused to automate).

### `auto_welcome` is OFF by default

Welcome enrolment fires only for `role == "lead"`, and only when the
`auto_welcome` setting is on — **it defaults to off**. In the standalone app,
enrolment hung off `sale_capture_lead`, an explicit sales act. Here the
equivalent is `create_customer`, the everyday record-keeping call. Defaulting it
on would mean **adding a contact silently messages them**. Operators opt in from
Settings when they actually want the proactive motion.

## Dynamic dashboard

The dashboard is not a fixed set of cards. It is a list of **user-defined charts**
(`dashboard_charts`), each one a saved query — and the same query engine is
exposed directly as **`crm_query`**, so an agent can answer "how many / how much
/ broken down by X" without listing rows and counting them by hand. That
distinction matters: a list is capped by its `limit`, so a hand-tallied total is
quietly short. `crm_query` sees every row.

A query is four choices (`src/db_dashboard.rs`):

| Part | Meaning |
|---|---|
| **`element`** | What one row *is*: `contact \| organization \| deal \| service \| task`. |
| **`metric`** | `count` \| `dealValue` (summed money of related deals) \| `dealQuantity` (summed service line-item quantity). |
| **`grouping`** | A field key to split by. Omit for a single total. |
| **`filters`** | `[{field, op, values[]}]`, ANDed. |

**Not every element has every metric** — `task` has only `count`;
`contact`/`organization` have no `dealQuantity`. Counts are `COUNT(DISTINCT pk)`,
so a metric that reaches across a join never inflates them.

### The registry is the schema

`ELEMENTS` in `src/db_dashboard.rs` is the single authoritative list of elements,
metrics and fields; **`crm_dashboard_schema` serves it verbatim** rather than
keeping a second copy in the UI.

| Element | Metrics | Groupable | Filter-only |
|---|---|---|---|
| `contact` | `count`, `dealValue` | `role`, `sale_stage`, `temperature`, `source`, `unsubscribed`, `organization` | `lead_score`, `created_at`, `updated_at` |
| `organization` | `count`, `dealValue` | `kind`, `industry`, `size` | `created_at`, `updated_at` |
| `deal` | `count`, `dealValue`, `dealQuantity` | `stage`, `currency`, `organization`, `contact` | `amount`, `probability`, `expected_close_at`, `created_at`, `updated_at` |
| `service` | `count`, `dealValue`, `dealQuantity` | `kind`, `pricing_model`, `currency`, `active` | `amount`, `created_at`, `updated_at` |
| `task` | `count` | `status`, `contact` | `due_at`, `created_at` |

Operators come from the field's kind — enum/text/relation: `in | notIn | isNull |
isNotNull`; bool: `in` (`0 | 1`); number/date: `gt | gte | lt | lte | between |
inLastDays`. **An unknown field key or a mismatched operator is rejected, not
ignored** — you get an error, not a wrong number.

**Dates are deliberately not groupable.** There is no "by month" grouping; bound
the window with a date filter and group by something categorical. Dates are Unix
seconds, and **`inLastDays` takes a day count evaluated at query time**, so a
saved chart keeps meaning "the last 30 days" instead of freezing today's date
into itself.

### Why this can't be SQL injected

`element`, `metric`, `grouping` and `filter.field` are looked up in `ELEMENTS` by
**exact key**; the only SQL fragments ever concatenated are `&'static str`
literals written in that file. Every user-supplied value is bound as a parameter.
User text picks *which* literal, never *what* the literal says. The operator is
matched against a closed set for the field's kind, so an unrecognised one is an
error rather than something spliced into the statement.

### Charts

```
crm_query            -- run a spec, get {rows:[{bucket,value}], total, groups,
                     --   is_money, filter_summary}
crm_dashboard_schema -- the registry: elements, metrics, groupable/filterable
                     --   fields, operators, value vocabularies, display types
crm_list_charts      -- saved charts, each WITH its numbers already computed
crm_create_chart     -- persist a spec (+ name, display_type, size)
crm_delete_chart     -- remove one
```

Buckets sort **value DESC, then bucket**; an empty `bucket` string means the
value is unset (a contact with no primary org), and an ungrouped query returns a
single row with `bucket: ""`.

`crm_create_chart` **validates by compiling the spec** before storing it
(`Db::validate` → `run_chart`), so a chart that cannot run fails at save time,
where the author can see why — instead of rendering an error card on the
dashboard forever after.

Prefer the purpose-built reports where they already fit: `crm_stats`,
`crm_revenue_breakdown`, `sale_pipeline_report`. `crm_query` is for the questions
those don't cover.

## MCP

`crm-mcp` at `/api/mcp/sse` — **83 tools**, 100% of the app's read and write
surface. Defined in `src/mcp.rs` (40 core tools) and `src/mcp_ext.rs` (43:
dashboard, organizations, services, inbox, sale). They are two arrays because the
`json!` literal in `mcp.rs` already needs `#![recursion_limit = "512"]`.

## Skills

| Skill | Covers |
|---|---|
| `crm-quick-lookup` | 33 read tools — contacts, orgs, deals & line items, catalogue, tasks, graph, search, AI briefings, stats |
| `crm-analytics` | 5 tools — `crm_query` + the dashboard charts: aggregates, funnels, splits, revenue by X |
| `crm-log-interaction` | 28 write tools — contacts, channels, interactions, deals, line items, org links, catalogue, tasks, relationships, calendar sync |
| `crm-organizations` | 18 tools — accounts + catalogue + deal line items; resolve-before-create, deactivate-don't-delete |
| `crm-inbox` | 4 tools — read threads, link an unlinked thread, inspect connected channels |
| `crm-sale-followup` | 12 tools — the proactive motion: read, draft, send through the guardrail, sequences, scheduling |
| `crm-sale-inbox` | 7 tools — the review + escalation queues and the funnel report |

## Personas

`crm-assistant` (data accuracy across contacts/orgs/catalogue/inbox),
`sale-closer` (proactive selling), `sale-manager` (queues, quality, pipeline).

## Data

SQLite at `~/.senclaw/space-apps/crm/crm.db` (WAL). Schema in `src/db.rs`,
`src/db_org.rs`, `src/db_inbox.rs`, `src/db_sale.rs`, `src/db_dashboard.rs`.

```
customers(…, role, sale_stage, temperature, lead_score, unsubscribed,
          last_inbound_at, checkin_count, …)   -- sales state lives HERE
organizations(id, name, kind, website, domain, industry, size, address, …)
customer_organizations(customer_id, organization_id, role_title, is_primary)
services(id, name, kind, amount, currency, pricing_model, unit, sku, active, …)
deal_services(deal_id, service_id, quantity, unit_amount, note)  -- frozen price
deals(…) tasks(…) interactions(…) relationships(…) mentions(…)
channels(id, kind, name, enabled, config, last_status, last_error, last_sync_at)
conversations(id, channel_id, customer_id, external_id, status, …)  -- 0 = unlinked
customer_channels(customer_id, kind, value, label)   -- THEIR handles
dashboard_charts(id, name, element, metric, grouping, filters_json, display_json,
                 size, sort, is_template, …)   -- a saved crm_query spec;
                       -- ELEMENTS in db_dashboard.rs is the authoritative schema
settings(key, value)   -- auto_welcome, risky_keywords, max_messages_per_customer_24h, …
```

Avatars are stored as a URL — either an external `https://…` or an inline
`data:image/…;base64,…`. No blob column, no CDN, no orphan files.

## Daemon bridge

`src/senclaw.rs`. Long-term memory is the daemon's cognitive store, scoped per
customer (`crm:sale:<customer_id>`) — the CRM owns no vector store of its own.
Product knowledge for grounding a draft comes from the shared wiki.
Manifest capabilities: `space.rest`, `llm.request`, `knowledge.save`,
`knowledge.recall`, `knowledge.search`.

## Ports

`PORT=4390` by default. Manifest: `runtime.port = 4390`.

## Dev

```
cargo run -p crm                                  # backend on :4390
(cd apps/crm/web && npm install && npm run dev)   # web on :5173 → proxy /api
cargo test -p crm                                 # guardrail tests live in src/guardrail.rs
```

Then point the SenClaw daemon at this Space App via
`apps/crm/senclaw-manifest.json`.

## Pack

```
bash apps/crm/scripts/pack.sh
```

Produces `apps/crm/crm-app.zip` — the installer you upload in
**SenClaw → Apps → Install**.
