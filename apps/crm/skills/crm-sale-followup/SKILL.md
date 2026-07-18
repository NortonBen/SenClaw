---
name: crm-sale-followup
description: >-
  Proactive sales side of the SenClaw CRM: work a contact through the nurture
  pipeline (new_lead → engaged → qualified → consult_scheduled → consult_done →
  closed_won), draft a personalised touch, enrol a follow-up sequence, schedule
  a check-in, and send — always through the guardrail. Use for "follow up khách
  này", "chăm sóc lead", "chốt sale khách X", "nuôi tệp khách cũ", "kéo lại
  khách im lặng", "gửi tin cho khách", "lên lịch chăm khách", "khách này bảo
  đừng nhắn nữa", "follow up a lead", "nurture this customer", "re-engage a
  quiet customer", "send a message to a customer". Everything keys on
  `customer_id` — there is no lead_id. NOT for approving a queued draft or
  handling an escalated complaint (use crm-sale-inbox). NOT for plain
  record-keeping like logging a call or editing a profile (use
  crm-log-interaction).
---

# crm-sale-followup

Run the proactive sales motion on a **contact already in the CRM**, via the
**`crm-mcp`** server. This is the same database the rest of the CRM reads, so a
draft is grounded in the real profile, organizations, deals and transcript —
not in whatever the model recalls.

## The one thing to get right first

**There is no `leads` table, no `lead_id`, and no `sale_capture_lead` tool.**
A lead *is* a customer row. Sales state (`sale_stage`, `temperature`,
`lead_score`, `unsubscribed`, `last_inbound_at`, `checkin_count`) lives on that
row. **Every tool below keys on `customer_id`.**

To bring someone new in: `crm_create_customer({name, role: "lead", …})`, then
work them with the `sale_*` tools using the `id` it returns.

## Two pipelines — do not confuse them

| | Field | Values | Tracks |
|---|---|---|---|
| **Nurture pipeline** | `customers.sale_stage` | `new_lead, engaged, qualified, consult_scheduled, consult_done, closed_won, churned` | How warm the **person** is. Moved with `sale_update_stage`. |
| **Deal pipeline** | `deals.stage` | `qualifying, proposal, negotiation, won, lost` | Where **one opportunity** stands. Moved with `crm_move_deal`. |

A contact has one `sale_stage` and 0..n deals. `sale_update_stage` never touches
a deal; `crm_move_deal` never touches the nurture stage.

Temperature: `cold | warm | hot | churned`. Lead score: `0..100`.

## Tool catalogue — 12 tools

### Read the situation

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_get_lead`** | `customer_id` | Customer 360 through the sales lens: profile + organizations + sales state + transcript + the agent's own reasoning replay + scheduled follow-ups. **Start every turn here.** |
| **`mcp__crm-mcp__sale_list_leads`** | `stage`, `temperature`, `q`, `limit` (200) | Contacts through the sales lens. "lead nóng", "khách nào sắp chốt". |

### Bring someone in

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__crm_create_customer`** | `name` (required), `role`, `email`, `phone`, `company`, `source`, `notes`, `tags`, … | Pass `role: "lead"`. Returns the created row with its `id`. |

Welcome enrolment is gated by the **`auto_welcome`** setting, which defaults to
**OFF** — adding a contact to a CRM is a filing action and must not message
them. When on, it fires **only** for `role == "lead"`. **There is no
`start_welcome` argument on any tool.** To welcome someone now, call
`sale_start_sequence({customer_id, sequence_key: "welcome"})` explicitly.

### Draft & send

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_draft_message`** | `customer_id`, `intent` | Drafts wording and returns it. **Sends nothing**; records no guardrail decision. Preview before committing. |
| **`mcp__crm-mcp__sale_next_action`** | `customer_id`, `intent`, `channel` | Runs ONE proactive turn end-to-end: builds context from the CRM, drafts, pushes through the guardrail. Ends **sent**, **queued for review**, or **blocked**. The normal way to touch someone. |
| **`mcp__crm-mcp__sale_send`** | `customer_id`, `text`, `channel`, `is_reply` | **THE ONLY SEND PATH.** See the guardrail section. |

Intents for `sale_next_action` / `sale_draft_message`: `welcome_and_value`,
`share_value_content` (default), `soft_offer_consultation`, `re_engage_soft`,
`check_in_value`, `winback_offer`, `reply_to_customer`.

`is_reply` matters: replies are held to a **stricter** risky-wording threshold
than proactive messages. Set it truthfully — it is not a tuning knob.

### Move the pipeline

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_update_stage`** | `customer_id` + any of `stage`, `temperature`, `lead_score` | Move the nurture pipeline. |
| **`mcp__crm-mcp__sale_escalate`** | `customer_id`, `reason`, `draft`, `context` | Hand to a human. `reason`: `complaint \| pricing_request \| asked_for_human \| hot_lead \| complex_question`. `draft` = what you *would* have said, for the human to adapt. |

### Sequences & scheduling

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_list_sequences`** | — | Available sequences and their steps. |
| **`mcp__crm-mcp__sale_start_sequence`** | `customer_id`, `sequence_key` (`welcome \| nurture \| re_engage \| winback`) | Enrol. The scheduler drives each step; wording is generated fresh per step, never templated. |
| **`mcp__crm-mcp__sale_schedule_followup`** | `customer_id`, `delay_hours`, `intent` | One ad-hoc touch in N hours ("nhắc chăm khách này sau 3 ngày"). |
| **`mcp__crm-mcp__sale_unsubscribe`** | `customer_id`, `on` (default true) | Record that they asked to stop being contacted. |

## The guardrail — read before you send anything

`sale_send` is **the only way a message reaches a customer**, and it is not a
convention you can route around: no raw channel send is exposed to you, and
`sale_next_action` / `sale_approve_review` funnel through the same gate. Every
draft is evaluated in Rust, fail-closed, **first match wins**:

1. **Unsubscribed → BLOCKED.** Not sent, not queued. **No override exists** —
   not for you, not for an operator approving from the review queue. Calling
   `sale_send` again will not change the answer.
2. **Rate limit → REVIEW.** More than `max_messages_per_customer_24h`
   (default **3**) *delivered* touches in the last 24h queues the draft for a
   human. Queued drafts do not burn the budget; only delivered ones do.
3. **Risky wording → REVIEW.** Price / discount / contract / payment / deposit /
   commitment vocabulary. A **reply** trips on **≥1** keyword; a **proactive**
   message on **≥2** — a broadcast that mentions "giá" in passing is less
   alarming than a direct reply about it. Matching is Vietnamese
   diacritic-folded, so "bao gia" is caught exactly like "báo giá".

`sale_send` returns `action`: `sent | review | blocked | failed`, plus `detail`.
**Report that outcome honestly.** "Queued for review" is not "sent"; "blocked"
is not a transient error to retry.

Complaint detection runs separately against the **customer's** words on inbound
and escalates to a human before any draft is written.

## Each turn

1. **READ** — `sale_get_lead({customer_id})`: profile, org, sales state,
   transcript, prior reasoning.
2. **ANALYSE** — their intent (asking price / interested / declining / needs
   support / greeting), temperature, stage, risk.
3. **DECIDE** — touch or not; what wording and tone; does this need escalating
   instead.
4. **ACT** — `sale_next_action` (normal path), `sale_send` for specific wording,
   or `sale_escalate`. Call `sale_update_stage` when something actually moved.
5. **REPORT** — the real outcome, including queued/blocked.

## Hard rules

- **Never invent** a price, discount, deadline, case study, or commitment. Price
  or contract question → `sale_escalate` with `reason: "pricing_request"`. Do
  not produce a number.
- **Never work around the guardrail.** Do not re-call `sale_send` to get past a
  block, do not split a risky message into pieces to duck the keyword count, do
  not reword solely to defeat the filter, do not flip `is_reply` to buy a looser
  threshold. A queue is the system working, not an obstacle.
- **Complaint / refund demand / legal threat** → `sale_escalate` with
  `reason: "complaint"` **immediately**. Do not answer it yourself.
- **Unsubscribed means stop.** Do not draft, do not schedule, do not "check just
  once more".
- **Always personalise** from what is actually stored — name, org, industry,
  behaviour, prior messages. Read before you write.
- **`customer_id` everywhere.** If you reach for `lead_id` or
  `sale_capture_lead`, you are working from the deleted AI Sale app's API.
- **Do not approve your own drafts.** The review queue belongs to a human, via
  `crm-sale-inbox`.

## Do not use this skill for

- Approving/rejecting a queued draft, resolving an escalation, reading the
  funnel report → **`crm-sale-inbox`**.
- Logging a call, editing a profile, managing deals/tasks → **`crm-log-interaction`**.
- Reading an inbox thread or linking one to a contact → **`crm-inbox`**.

## Style

- Reply in the customer's language (default Vietnamese).
- Warm, professional, concrete. Value first, no filler.
