---
name: crm-quick-lookup
description: >-
  Read side of the SenClaw CRM: look up contacts, their organizations and
  contact channels, browse deals with their line items and the service
  catalogue, read tasks & the upcoming feed, explore relationships & the network
  graph, run full-text search (FTS5, Vietnamese diacritic-folded), fetch AI
  briefings, discover similar contacts, find the path between two people,
  extract shared themes, and query dashboard + revenue stats. Use for "khách
  hàng của tôi", "tìm khách tên X", "ai giống khách Y", "kết nối giữa A và B",
  "ai có điểm chung với X", "khách X làm ở công ty nào", "bảng giá", "deal này
  gồm gì", "doanh thu theo công ty", "tổng hợp CRM hôm nay", "sắp tới có gì",
  "ai nhắc đến sản phẩm Z", "who is customer X", "list VIP customers", "AI
  briefing for X", "what does company Y have". Everything read-only. For
  aggregate questions — "how many / how much / broken down by X", funnels,
  splits, dashboard charts — use crm-analytics instead.
---

# crm-quick-lookup

Answer any read-only question about the SenClaw CRM via the **`crm-mcp`**
server. Every fact must come from a tool — do not fabricate names, contact info,
prices, or relationships.

## Tool catalogue — 33 read tools

### Contacts

- **`mcp__crm-mcp__crm_list_customers`** — free-text `q` + `tag` + `status`
  filter + `limit` (default 50, max 500).
  - The `status` argument filters on the contact's **role**. Roles: `lead |
    prospect | customer | vip | contact | partner | referrer | supplier |
    investor | employee | former | paused | lost`.
- **`mcp__crm-mcp__crm_get_customer`** — `id`. Full profile + the 20 most-recent
  interactions.
- **`mcp__crm-mcp__crm_find_by_email`** — `email`. Case-insensitive.
- **`mcp__crm-mcp__crm_all_tags`** — every tag in use, sorted.
- **`mcp__crm-mcp__crm_list_channels`** — `customer_id`. **Their** contact
  channels: extra phones, secondary emails, social handles (zalo, facebook,
  linkedin, instagram, x, tiktok, youtube, github, telegram, whatsapp, signal,
  line, wechat, skype, viber, discord, messenger, website). Not to be confused
  with our connected inbox accounts (`crm-inbox`).

### Organizations

- **`mcp__crm-mcp__crm_list_organizations`** — `q`, `kind`
  (`direct_customer | affiliated_company | partner | supplier | prospect`),
  `limit`. Includes contact/deal counts + open pipeline value.
- **`mcp__crm-mcp__crm_get_organization`** — `id`. The org **with its contacts
  and its deals**.
- **`mcp__crm-mcp__crm_find_organization`** — `name`. Exact, case-insensitive.
- **`mcp__crm-mcp__crm_customer_organizations`** — `customer_id`. Which orgs a
  contact belongs to, **primary first**. A contact belongs to 0..n orgs; their
  `company` text is a projection of the primary one.

### Interactions & tasks

- **`mcp__crm-mcp__crm_list_interactions`** — `customer_id`, `limit`. The
  timeline (call, email, meeting, note, task).
- **`mcp__crm-mcp__crm_list_tasks`** — `open_only` (default true),
  `customer_id`, `limit`.
- **`mcp__crm-mcp__crm_upcoming`** — `days` (default 14). Tasks due + birthdays.
- **`mcp__crm-mcp__crm_recent_activity`** — `limit`. Global interaction feed,
  newest first.

### Deals, services & revenue

- **`mcp__crm-mcp__crm_list_deals`** — `stage`
  (`qualifying | proposal | negotiation | won | lost`) or `customer_id`.
  Includes `customer_name`, so no second lookup to talk about a deal.
- **`mcp__crm-mcp__crm_deal_services`** — `deal_id`. The deal's **line items**:
  quantity, frozen unit amount, line totals, deal total. A deal's `amount` is
  recomputed from its line items when it has any.
- **`mcp__crm-mcp__crm_list_services`** — `q`, `kind` (`service | hardware`),
  `active_only`, `limit`. The catalogue: name, kind, amount, currency,
  `pricing_model` (`fixed | hourly | daily | monthly | yearly`), deal-use count.
- **`mcp__crm-mcp__crm_get_service`** — `id`.
- **`mcp__crm-mcp__crm_revenue_breakdown`** — `limit` (top N orgs, default 20).
  Value by organization, value by service kind, org counts by type.

### Relationships & network graph

- **`mcp__crm-mcp__crm_list_relationships`** — `customer_id`, or the whole CRM
  if omitted. Kinds: `referred_by, introduced_by, colleague_of, spouse_of,
  family_of, friend_of, reports_to, partner_of, supplier_of, competitor_of,
  contact_of`.
- **`mcp__crm-mcp__crm_customer_network`** — `customer_id`. Direct connections
  as a subgraph (focus + neighbours + edges).
- **`mcp__crm-mcp__crm_expand_network`** — `focus`, `hops` (default 1).
- **`mcp__crm-mcp__crm_find_path`** — `from`, `to`. BFS shortest path through
  the undirected relationship graph. Returns id path + name path.
- **`mcp__crm-mcp__crm_similar_customers`** — `id`, `limit` (default 8, max 50).
  Deterministic Jaccard blend on tags, company, 1-hop neighbours, extracted
  mentions. Returns `score` + human-readable Vietnamese `reasons`.
- **`mcp__crm-mcp__crm_list_mentions`** — `unresolved_only`, `limit`.
  AI-extracted people who aren't (yet) contacts themselves.

### AI analysis

- **`mcp__crm-mcp__crm_summarize`** — `id`. AI briefing for ONE contact: who
  they are + latest activity + next step. Grounded in profile + interactions.
- **`mcp__crm-mcp__crm_aggregate_report`** — executive briefing across the WHOLE
  CRM: totals, pipeline by stage, top open deals, most-active contacts, recent
  activity, upcoming birthdays, overdue tasks.
- **`mcp__crm-mcp__crm_find_common`** — `id`. LLM finds every theme the focus
  contact shares with others (industry, project, mediating person, market).
  Returns `themes` + `customer_ids` + a de-duped `highlight_ids`.
- **`mcp__crm-mcp__crm_ai_path`** — `from`, `to`. LLM connection search between
  TWO people: shared interests, common markets, mediating people, weak ties.
  Typed connections (`shared_interest`, `common_market`, `possible_bridge`,
  `explicit_path`, `weak_tie`, `shared_person`) + strength. Includes the BFS
  path as grounding when one exists.

*The four AI tools require the daemon's LLM. If it is unavailable, say so — do
not fall back to your own guesswork.*

### Search & overview

- **`mcp__crm-mcp__crm_search`** — `q`, `limit` (default 20, max 100). FTS5
  across ALL profiles, interactions and extracted mentions. Vietnamese
  diacritic-folded — "khach" matches "khách", "anna" matches "Anna Nguyễn".
  Returns entity_type (`customer|interaction|mention`), entity_id, customer
  link, 12-word snippet.
- **`mcp__crm-mcp__crm_stats`** — dashboard totals: customers, interactions,
  open_tasks, overdue_tasks, open_deals, pipeline_value, won_value, by_role,
  by_stage.

### Aggregates & the dashboard

- **`mcp__crm-mcp__crm_query`** — `element`
  (`contact | organization | deal | service | task`), `metric`
  (`count | dealValue | dealQuantity`), `grouping`, `filters`. Ad-hoc analytics:
  "how many / how much / broken down by X" answered as buckets, without reading
  rows. **Use this instead of listing and counting by hand** — a list is capped
  by `limit`, this is not.
- **`mcp__crm-mcp__crm_dashboard_schema`** — which elements exist, their
  metrics, and which fields are groupable/filterable with which operators. Call
  it before guessing a field key; an unknown key is rejected, not ignored.
- **`mcp__crm-mcp__crm_list_charts`** — the saved dashboard charts, each with
  its current numbers already computed.

*For anything beyond a one-line total — funnels, splits, revenue by
organization, saving a chart — use **`crm-analytics`**, which covers the query
model properly.*

## How to answer

1. **Simple lookups** ("khách nào tên X") → `crm_list_customers(q=…)` then
   `crm_get_customer(id)` to confirm.
2. **AI briefings** ("hồ sơ khách X") → `crm_summarize(id)` + 2–3 lines from the
   interactions for grounding.
3. **"Ai làm ở công ty X"** → `crm_get_organization(id)` — it already returns
   contacts + deals in one call.
4. **"Khách X ở công ty nào"** → `crm_customer_organizations(customer_id)`.
   Primary first; mention it when they belong to more than one.
5. **"Deal này bao nhiêu / gồm gì"** → `crm_deal_services(deal_id)`. Read the
   **line items**, not just `deals.amount` — the total is derived from them.
6. **"Bảng giá"** → `crm_list_services({active_only: true})`. Always say the
   `pricing_model`; "12tr" and "12tr/tháng" are different answers.
7. **Who's similar** → `crm_similar_customers(id)` — read `reasons` verbatim,
   they are already user-facing Vietnamese.
8. **Connection between 2 people** → `crm_find_path(from, to)` first (fast,
   deterministic). If no path, or the user wants qualitative reasoning, follow
   with `crm_ai_path`.
9. **Common ground across many** → `crm_find_common(id)`.
10. **Broad "what's happening"** → `crm_recent_activity(limit=10)`, or
    `crm_aggregate_report()` for the full briefing.
11. **Typo-tolerant search** → `crm_search(q)` — handles missing tone marks.
12. **Time-based** → `crm_upcoming(days=…)`.
13. **"Bao nhiêu / tổng bao nhiêu / theo X"** → `crm_query`, not a list you count
    yourself. `crm_stats` if a plain total already covers it.

## Do not

- Do not invent contact info (email/phone/birthday) or prices. Say "chưa có".
- Do not quote a price from memory — `crm_list_services` / `crm_deal_services`
  are the source of truth.
- Do not use this skill to WRITE — hand off to `crm-log-interaction`
  (contacts/deals/tasks), `crm-organizations` (orgs/catalogue/line items), or
  `crm-sale-followup` (anything outbound).
- Do not substitute any browser MCP for `crm_search` — the CRM's FTS5 index is
  the authoritative full-text search over this data.
- Do not read the sales lens from here — `sale_list_leads` / `sale_get_lead`
  belong to `crm-sale-followup`.
- Do not answer an aggregate by paging a list and tallying it — `crm_query` is
  the tool, `crm-analytics` is the skill. A list is capped by `limit`; a total
  you counted off the page is quietly wrong.
- Do not create or delete dashboard charts from here — that is `crm-analytics`.

## Style

- Reply in the user's language (default Vietnamese).
- Include `id` when disambiguation matters: "#3 Tuấn Anh".
- Preserve avatars/URLs/emails **verbatim** so they render/click correctly.
- Format money with its currency and pricing model.
