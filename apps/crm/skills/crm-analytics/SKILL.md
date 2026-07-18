---
name: crm-analytics
description: >-
  Aggregate questions about the SenClaw CRM — "how many", "how much", "broken
  down by X" — answered with crm_query instead of listing rows and counting by
  hand, plus the saved dashboard charts. Use for "doanh thu theo công ty", "bao
  nhiêu khách mỗi giai đoạn", "phễu theo stage", "bán dịch vụ hay phần cứng
  nhiều hơn", "khách mới 30 ngày qua", "tổng giá trị deal chưa tính lost", "thêm
  biểu đồ vào dashboard", "dashboard đang có gì", "revenue by organization",
  "funnel by stage", "service vs hardware split", "new contacts last 30 days",
  "deal value excluding lost", "create a chart", "list charts". NOT for fetching
  an individual record — that is crm-quick-lookup. NOT for the review /
  escalation queues or the sales funnel report — that is crm-sale-inbox.
---

# crm-analytics

Answer **aggregate** questions over the SenClaw CRM via the **`crm-mcp`** server.
One rule sits above the rest: **`crm_query` computes the number; you do not.**
Listing rows and tallying them yourself is slow, silently truncated by `limit`,
and wrong as soon as the data outgrows the page.

Everything keys on **`customer_id`** — there is no `leads` table and no
`lead_id`. Sales state (`sale_stage`, `temperature`, `lead_score`) is a field on
the contact row, which is why you group `contact` by `sale_stage`.

## Tool catalogue — 5 tools

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__crm_query`** | `element` (required), `metric`, `grouping`, `filters` | Ad-hoc analytics. Returns buckets with numbers. **The workhorse.** |
| **`mcp__crm-mcp__crm_dashboard_schema`** | — | Which elements exist, their metrics, and which fields are groupable / filterable with which operators. **Read before guessing a field key.** |
| **`mcp__crm-mcp__crm_list_charts`** | — | The saved dashboard charts, each with its current numbers already computed. |
| **`mcp__crm-mcp__crm_create_chart`** | `name`, `element` (required), `metric`, `grouping`, `filters`, `display_type`, `size` | Persist a query as a dashboard chart. |
| **`mcp__crm-mcp__crm_delete_chart`** | `id` | Remove a chart. Confirm with the user first. |

## The model

A query is four choices. Only `element` is required.

- **`element`** — what one row *is*: `contact | organization | deal | service |
  task`.
- **`metric`** — what to measure:
  - `count` — how many. Always `COUNT(DISTINCT …)`, so a join never inflates it.
  - `dealValue` — summed money of the related deals.
  - `dealQuantity` — summed service line-item quantity.
- **`grouping`** — a field key to split by. **Omit for a single total** (you get
  one row with an empty `bucket`).
- **`filters`** — `[{field, op, values[]}]`, ANDed together.

**Not every element has every metric.** `task` has only `count`; `contact` and
`organization` have no `dealQuantity`. Asking for one it doesn't have is an
error, not a zero.

### Elements at a glance

| Element | Metrics | Groupable fields | Filter-only fields |
|---|---|---|---|
| `contact` | `count`, `dealValue` | `role`, `sale_stage`, `temperature`, `source`, `unsubscribed`, `organization` | `lead_score`, `created_at`, `updated_at` |
| `organization` | `count`, `dealValue` | `kind`, `industry`, `size` | `created_at`, `updated_at` |
| `deal` | `count`, `dealValue`, `dealQuantity` | `stage`, `currency`, `organization`, `contact` | `amount`, `probability`, `expected_close_at`, `created_at`, `updated_at` |
| `service` | `count`, `dealValue`, `dealQuantity` | `kind`, `pricing_model`, `currency`, `active` | `amount`, `created_at`, `updated_at` |
| `task` | `count` | `status`, `contact` | `due_at`, `created_at` |

Value vocabularies: `role` = the 13 contact roles; `sale_stage` = `new_lead |
engaged | qualified | consult_scheduled | consult_done | closed_won | churned`;
`temperature` = `cold | warm | hot | churned`; deal `stage` = `qualifying |
proposal | negotiation | won | lost`; org `kind` = `direct_customer |
affiliated_company | partner | supplier | prospect`; service `kind` = `service |
hardware`; `pricing_model` = `fixed | hourly | daily | monthly | yearly`; `task`
`status` = `open | done`; bool fields (`unsubscribed`, `active`) take `0 | 1`.
`crm_dashboard_schema` is the live copy — prefer it over this table.

### Operators, by field kind

| Kind | Operators |
|---|---|
| enum / text / relation | `in`, `notIn`, `isNull`, `isNotNull` |
| bool | `in` (values `0` / `1`) |
| number / date | `gt`, `gte`, `lt`, `lte`, `between`, `inLastDays` |

An operator that doesn't match the field's kind is **rejected**, not coerced —
`{field: "created_at", op: "in"}` is an error.

**Dates are Unix seconds.** `inLastDays` takes a **day count** and is evaluated
at query time, so a saved chart keeps meaning "the last 30 days" instead of
freezing today's date into itself. Prefer it over computing a timestamp.

## The common questions

| Question | Query |
|---|---|
| Revenue by organization | `{element: "deal", metric: "dealValue", grouping: "organization"}` |
| …excluding lost deals | add `filters: [{field: "stage", op: "notIn", values: ["lost"]}]` |
| Funnel by deal stage | `{element: "deal", metric: "count", grouping: "stage"}` |
| Nurture funnel (people) | `{element: "contact", metric: "count", grouping: "sale_stage"}` |
| Service vs hardware — money | `{element: "service", metric: "dealValue", grouping: "kind"}` |
| Service vs hardware — units | `{element: "service", metric: "dealQuantity", grouping: "kind"}` |
| New contacts in the last 30 days | `{element: "contact", metric: "count", filters: [{field: "created_at", op: "inLastDays", values: [30]}]}` |
| …and where they came from | same, plus `grouping: "source"` |
| Total pipeline, lost excluded | `{element: "deal", metric: "dealValue", filters: [{field: "stage", op: "notIn", values: ["lost"]}]}` |
| Open tasks per person | `{element: "task", metric: "count", grouping: "contact", filters: [{field: "status", op: "in", values: ["open"]}]}` |
| Hot leads by owner org | `{element: "contact", metric: "count", grouping: "organization", filters: [{field: "temperature", op: "in", values: ["hot"]}]}` |
| Contacts we may still message | `{element: "contact", metric: "count", filters: [{field: "unsubscribed", op: "in", values: [0]}]}` |

## Reading the result

`{rows: [{bucket, value}], total, groups, is_money, filter_summary}`.

- Buckets are sorted **value DESC**, then bucket name. Ungrouped queries return a
  single row whose `bucket` is `""`.
- An **empty `bucket` string means the value is unset** — a contact with no
  primary organization, a service with no currency. Report it as "chưa có" /
  "unset", never as a real bucket named "".
- **`is_money: true` → say the currency.** Money buckets are summed across
  whatever currencies the rows carry, so group or filter by `currency` when the
  data is mixed.
- `filter_summary` is a human-readable echo of the filters. Quote it when you
  present a number so the user can see what was counted.

## Saving a chart

`crm_create_chart` takes the same spec plus `name`, `display_type`
(`verticalBarChart` — the default — `horizontalBarChart`,
`verticalBarChartWithLabels`, `horizontalBarChartWithLabels`, `doughnutChart`,
`radarChart`) and `size` (`small | medium | large`).

The spec is **compiled before it is stored**: an invalid combination fails at
save time with a reason, so a broken card never lands on the dashboard.

**A chart is a query someone will want repeatedly.** A one-off question is
answered with `crm_query` and nothing is saved. Do not litter the dashboard with
the residue of a conversation.

**There is no `crm_update_chart` and no reorder tool.** Editing and re-ordering a
chart are UI/REST operations only. To change a saved chart from here you would
have to delete it and create it again — so confirm with the user first, and say
plainly that tweaking it in the dashboard UI is usually the better move.

## Hard rules

- **Call `crm_dashboard_schema` before guessing a field key.** An unknown key is
  **rejected, not ignored** — you get an error, not a wrong number. There is no
  `assignee`, no `owner`, no `lead_id`; do not invent them.
- **Never count rows by hand.** Do not `crm_list_customers` / `crm_list_deals`
  and tally the result to answer "how many" — the list is capped by `limit` and
  your total will be quietly short. `crm_query` sees every row.
- **Dates are not groupable, deliberately.** There is no "by month" grouping.
  Use a date **filter** to bound the window, and group by something categorical.
  Say so plainly instead of faking a time series.
- **`notIn` includes the blanks on purpose.** "stage not in lost" returns rows
  with a NULL/empty stage too — that is the human reading, not a bug.
- **An empty `in` / `notIn` value list is treated as no filter.** If you meant to
  filter, pass values; a half-built filter will not narrow anything.
- **`contact.organization` is the PRIMARY organization only.** A contact in three
  orgs counts once, under their primary. It is not "any org they touch".
- **To filter deal money by deal stage, use `element: "deal"`.** `contact` +
  `dealValue` sums *all* their deals and has no `stage` field to filter on —
  group `deal` by `contact` instead.
- **Report the number the tool returned.** Do not round it into a story, do not
  add up buckets to a total the tool already computed (`total` is right there).
- **Confirm before `crm_delete_chart`.** Someone else built it.

## Prefer the purpose-built report when there is one

`crm_query` is for questions nothing else answers. Do not rebuild these:

- **`crm_stats`** — dashboard totals (customers, open/overdue tasks, open deals,
  pipeline value, won value, by_role, by_stage).
- **`crm_revenue_breakdown`** — value by organization + by service kind, ready-made.
- **`sale_pipeline_report`** — the sales funnel, win rate, queue depths.

Reach for `crm_query` when the question bends away from those: a different
filter, a different split, a window nobody pre-baked.

## Do not use this skill for

- Fetching an individual record — who someone is, what a deal contains, the
  price list → **`crm-quick-lookup`**.
- The review / escalation queues or the funnel report → **`crm-sale-inbox`**.
- Writing anything that isn't a chart → **`crm-log-interaction`**,
  **`crm-organizations`**, **`crm-sale-followup`**.

## Style

- Reply in the user's language (default Vietnamese).
- Lead with the number, then what was counted: "Doanh thu theo công ty (chưa
  tính deal lost): Shop Co 240tr, …".
- Money always with its currency; counts without.
- When a bucket is blank, name it: "(chưa gán công ty): 12 khách".
