---
name: crm-quick-lookup
description: >-
  Read side of the SenClaw CRM: look up customers, browse deals & tasks,
  explore relationships & the network graph, run full-text search (FTS5, Vietnamese
  diacritic-folded), fetch AI briefings, discover similar customers, find the
  path between two people, extract shared themes, and query dashboard stats. Use
  for "khách hàng của tôi", "tìm khách tên X", "ai giống khách Y", "kết nối giữa
  A và B", "ai có điểm chung với X", "tổng hợp CRM hôm nay", "sắp tới có gì",
  "ai nhắc đến sản phẩm Z", "who is customer X", "list VIP customers", "AI
  briefing for X". Everything read-only.
---

# crm-quick-lookup

Answer any read-only question about the SenClaw CRM via the **`crm-mcp`** server.
Every fact must come from a tool — do not fabricate names, contact info, or
relationships. The read tools below cover 100% of the CRM's data surface.

## Tool catalogue — 20 read tools

### Customers

- **`mcp__crm-mcp__crm_list_customers`** — free-text + tag + role filter, limit.
  Roles: `lead|prospect|customer|vip|contact|partner|referrer|supplier|investor|
  employee|former|paused|lost`.
- **`mcp__crm-mcp__crm_get_customer`** — full profile + 20 most-recent interactions.
- **`mcp__crm-mcp__crm_find_by_email`** — case-insensitive email lookup.
- **`mcp__crm-mcp__crm_all_tags`** — every tag in use, sorted.

### Interactions & tasks

- **`mcp__crm-mcp__crm_list_interactions`** — full timeline for a customer (call,
  email, meeting, note, task, profile_update, deal_update).
- **`mcp__crm-mcp__crm_list_tasks`** — open (default) or all tasks, optional
  customer filter.
- **`mcp__crm-mcp__crm_upcoming`** — tasks due + birthdays in the next N days
  (Monica-style feed).
- **`mcp__crm-mcp__crm_recent_activity`** — global feed of every interaction
  across all customers, newest first.

### Deals

- **`mcp__crm-mcp__crm_list_deals`** — full pipeline or by `stage`
  (`qualifying|proposal|negotiation|won|lost`) or by `customer_id`. Includes
  customer_name so the agent can talk about the deal without a second lookup.

### Relationships & network graph

- **`mcp__crm-mcp__crm_list_relationships`** — every relationship involving a
  customer (or the entire CRM if `customer_id` omitted). Kinds: `referred_by,
  introduced_by, colleague_of, spouse_of, family_of, friend_of, reports_to,
  partner_of, supplier_of, competitor_of, contact_of`.
- **`mcp__crm-mcp__crm_customer_network`** — a customer's direct connections as
  a subgraph (focus node + neighbours + edges).
- **`mcp__crm-mcp__crm_expand_network`** — subgraph within N hops from a focus.
- **`mcp__crm-mcp__crm_find_path`** — BFS shortest path between two customers
  through the (undirected) relationship graph. Returns id path + name path.
- **`mcp__crm-mcp__crm_similar_customers`** — deterministic Jaccard blend on
  tags, company, 1-hop neighbours, extracted-mention overlap. Returns each match
  with a `score` (0..~3) and human-readable `reasons` in Vietnamese ("chung tag
  #vip, cùng công ty Shop Co, cùng biết Tuấn Anh").
- **`mcp__crm-mcp__crm_list_mentions`** — AI-extracted people mentioned in a
  customer's notes/interactions who aren't (yet) customers themselves. Filter
  by `unresolved_only`.

### AI analysis

- **`mcp__crm-mcp__crm_summarize`** — AI briefing for ONE customer (who they are +
  latest activity + next-step). Grounded in profile + interactions.
- **`mcp__crm-mcp__crm_aggregate_report`** — executive briefing across the WHOLE
  CRM: totals, pipeline by stage, top deals, most-active customers, upcoming
  events, overdue tasks. Ends with a single recommended next action.
- **`mcp__crm-mcp__crm_find_common`** — LLM finds every meaningful theme the
  focus customer shares with others (industry, project, mediating person,
  hobby, market). Returns themes + participant IDs + a de-duped
  `highlight_ids` list for graph rendering.
- **`mcp__crm-mcp__crm_ai_path`** — LLM-driven connection search between TWO
  people. Reasons about shared interests, common markets, mediating people,
  weak ties. Returns typed connections (`shared_interest`, `common_market`,
  `possible_bridge`, `explicit_path`, `weak_tie`, `shared_person`) + strength.

### FTS5 search

- **`mcp__crm-mcp__crm_search`** — full-text search across ALL customer profiles,
  interactions, and extracted mentions. Vietnamese diacritic-folded — "khach"
  matches "khách", "anna" matches "Anna Nguyễn". Returns entity_type
  (customer|interaction|mention), entity_id, snippet, customer link.

### Overview

- **`mcp__crm-mcp__crm_stats`** — dashboard totals: customers, interactions,
  open_tasks, overdue_tasks, open_deals, pipeline_value, won_value,
  by_role (13 roles), by_stage (5 stages).

## How to answer

1. **Simple lookups** ("khách nào tên X") → `crm_list_customers(q=…)` then
   `crm_get_customer(id)` to confirm.
2. **AI briefings** ("hồ sơ khách X") → `crm_summarize(id)` + 2–3 lines from the
   detail's interactions for grounding.
3. **Who's similar** ("khách nào giống X") → `crm_similar_customers(id)` — read
   out `reasons` verbatim; they're already user-facing Vietnamese.
4. **Connection between 2 people** — try `crm_find_path(from, to)` first (fast,
   deterministic). If no path OR user wants qualitative reasoning, follow with
   `crm_ai_path` for shared-interest style analysis.
5. **Common ground across many** → `crm_find_common(id)` — surface `themes` as
   named clusters + `customer_ids` per theme.
6. **Broad "what's happening"** → `crm_recent_activity(limit=10)` or
   `crm_aggregate_report()` for a full executive briefing.
7. **Sub-string / typo-tolerant search** → `crm_search(q)` — handles missing
   tone marks and returns the entity type so you can drill in.
8. **Time-based** → `crm_upcoming(days=…)` for tasks + birthdays.

## Do not

- Do not invent contact info (email/phone/birthday). Say "chưa có" instead.
- Do not use this skill to WRITE — hand off to `crm-log-interaction`.
- Do not substitute any browser MCP for `crm_search` — the CRM's FTS5 index is
  the authoritative full-text search over this data.

## Style

- Reply in the user's language (default Vietnamese).
- Include `id` when disambiguation matters: "#3 Tuấn Anh".
- Preserve avatars/URLs/emails **verbatim** so they render/click correctly.
