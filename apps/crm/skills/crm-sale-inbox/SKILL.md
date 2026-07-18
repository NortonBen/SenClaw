---
name: crm-sale-inbox
description: >-
  The two human queues of the SenClaw CRM's sales engine, plus the funnel
  report. Review queue = a risky draft waiting for someone to approve, edit, or
  reject before it is sent. Escalation queue = a case the agent refused to
  answer and handed to a person (complaint, price demand, asked-for-a-human, hot
  lead). Use for "duyệt tin chờ gửi", "hàng chờ duyệt", "tin nhắn rủi ro chờ
  duyệt", "có gì cần tôi xử lý", "xử lý escalation", "khách khiếu nại", "báo cáo
  pipeline", "win rate", "phễu bán hàng", "review queue", "approve draft",
  "reject draft", "handle escalation", "pipeline report". NOT for proactively
  messaging or nurturing a customer — that is crm-sale-followup.
---

# crm-sale-inbox

Work the human side of the sales engine via the **`crm-mcp`** server: clear the
review queue, resolve escalations, read the funnel. Everything keys on
**`customer_id`** — there is no `lead_id`.

## The two queues are different things

| Queue | `kind` | What it means | What you do |
|---|---|---|---|
| **Review** | `review` | The agent wrote a draft and the **guardrail held it**. Nobody has been messaged. | Read the wording, approve (optionally edited) or reject. |
| **Escalation** | `escalation` | The agent **refused to answer** and handed the case to a person. | Handle the customer yourself, then mark resolved. |

A review is about *words we were going to say*. An escalation is about *a
situation we should not automate*. Approving a review sends a message; resolving
an escalation does not.

## Tool catalogue — 7 tools

### The queues

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_list_inbox`** | `kind` (required: `review \| escalation`), `status`, `limit` | Review statuses: `pending` (default) `\| approved \| rejected \| edited \| all`. Escalation statuses: `open` (default) `\| resolved \| all`. |
| **`mcp__crm-mcp__sale_approve_review`** | `review_id`, `edited`, `by` (default `"operator"`) | Approve a queued draft **and send it**. Pass `edited` to send different words instead. |
| **`mcp__crm-mcp__sale_reject_review`** | `review_id`, `by` | Reject. **Nothing is sent.** |
| **`mcp__crm-mcp__sale_resolve_escalation`** | `escalation_id`, `by` | Mark an escalated case handled. |

### Context before you decide

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_get_lead`** | `customer_id` | Customer 360: profile + organizations + sales state + transcript + reasoning replay. Read this before approving anything non-obvious. |
| **`mcp__crm-mcp__sale_send`** | `customer_id`, `text`, `channel`, `is_reply` | **THE ONLY SEND PATH.** Use when handling an escalation means actually replying to the customer. Still guardrailed. |

### The numbers

| Tool | Args | Use |
|---|---|---|
| **`mcp__crm-mcp__sale_pipeline_report`** | — | Funnel by stage, win rate, hot leads, pending reviews, open escalations, unsubscribes, token spend. |

## What approval actually waives

`sale_approve_review` waives **only the risky-wording rule** — a human read the
words, so that rule steps aside. **Two rules still apply and cannot be clicked
past:**

- **Unsubscribed → still BLOCKED.** A standing instruction from the customer.
  No operator can override it. If you approve a review for an unsubscribed
  customer, it will not send — and that is correct.
- **Rate limit → still applies.** It is about volume regardless of content.

So approving is not "force send". If the outcome comes back `blocked` or
`review`, report that, do not retry.

## Reading a review item

Each item carries the `draft` and a risk reason:

- **`risky_keywords`** — the draft used price/discount/contract/payment/deposit/
  commitment vocabulary. A reply trips on ≥1 keyword, a proactive message on ≥2.
  **Check the numbers and claims yourself.** The agent is forbidden from
  inventing a price; if a figure appears, verify it against
  `crm_list_services` / the deal's line items before approving.
- **`rate_limit_exceeded`** — more than `max_messages_per_customer_24h`
  (default 3) delivered touches in 24h. The wording may be fine; the question is
  whether this person should hear from us again today at all. Usually: reject,
  or schedule it for later via `crm-sale-followup`.

## Handling an escalation

Read `reason` + `context` + the `draft` the agent would have sent:

- **`complaint`** — acknowledge receipt first; do not promise a resolution you
  have not confirmed. Handle it as a person.
- **`pricing_request`** — the agent correctly refused to make up a number. Quote
  from the real catalogue (`crm_list_services`, `crm_deal_services`) or from an
  authorised price.
- **`asked_for_human`** — they asked for a person. Be one.
- **`hot_lead`** — move fast; check `sale_get_lead` for the full picture.
- **`complex_question`** — the agent could not ground an answer in stored
  context. Either answer it yourself or get the fact into the CRM/wiki so the
  next turn can be grounded.

Then `sale_resolve_escalation({escalation_id, by})`.

## Priorities

1. Open escalations with `reason: "complaint"` — a person is unhappy and waiting.
2. Pending reviews for **hot** leads or late-stage contacts
   (`consult_scheduled`, `consult_done`) — timing is the whole value.
3. Everything else, oldest first. Do not let the queue accumulate; a stale queue
   is a customer who never got answered.

## Hard rules

- **Reject is a real answer.** If the wording is wrong, reject it. Do not edit a
  bad draft into a mediocre one just to clear the row.
- **Verify every number before approving.** Ground it in the catalogue or the
  deal's line items. Approving a fabricated price is how a fabricated price
  reaches a customer — you are the check that stops it.
- **Do not loosen the guardrail to clear the queue faster.** Not the keyword
  list, not `max_messages_per_customer_24h`. The queue existing is the system
  working.
- **`sale_send` is the only send path**, including when you are handling an
  escalation yourself. There is no other way to reach a customer, and no reason
  to want one.
- **Record who acted.** Pass `by` so the log says who approved or resolved.

## Do not use this skill for

- Proactively touching / nurturing / drafting for a customer →
  **`crm-sale-followup`**.
- Reading raw inbox threads or linking a thread to a contact → **`crm-inbox`**.
- Editing profiles, deals, or tasks → **`crm-log-interaction`**.

## Style

- Reply in the operator's language (default Vietnamese).
- Lead with what needs a decision, then the context behind it.
- Name the customer and id: "#12 Anna Nguyễn — chờ duyệt, lý do: risky_keywords".
