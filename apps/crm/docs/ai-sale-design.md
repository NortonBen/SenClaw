# AI Sale — Design Doc (rev 1)

> **AI chốt sale** — lớp *chủ động* bán hàng & chăm sóc tệp trong hệ sinh thái SenClaw:
> **CRM (dữ liệu) + AI Office (điều phối) + AI Chat (kênh & CSKH phản ứng) + AI Sale (chốt & nuôi tệp)**.
>
> Rút tính năng từ reference `auto-followup` (Mastra + Postgres/pgvector, Node) và tái kiến trúc
> theo khuôn Space App của SenClaw (Rust axum + rusqlite, reuse daemon bridge).

**Trạng thái**: thiết kế, chưa code. Port dự kiến **4450**. MCP `ai-sale-mcp`.

---

## Mục lục
1. [Triết lý & định vị](#1-triết-lý--định-vị)
2. [Ranh giới hệ sinh thái (ai làm gì)](#2-ranh-giới-hệ-sinh-thái)
3. [Quyết định kiến trúc đã chốt](#3-quyết-định-kiến-trúc-đã-chốt)
4. [Data model (SQLite)](#4-data-model-sqlite)
5. [Engine — vòng lặp agentic 5 bước](#5-engine--vòng-lặp-agentic-5-bước)
6. [Guardrails (port nguyên khối)](#6-guardrails)
7. [Human-in-the-loop 2 tầng](#7-human-in-the-loop-2-tầng)
8. [Sequences + Scheduler](#8-sequences--scheduler)
9. [Lead scoring / temperature / intent](#9-lead-scoring--temperature--intent)
10. [Upsell / Loyalty / Referral](#10-upsell--loyalty--referral)
11. [Integration contract (CRM + AI Chat)](#11-integration-contract)
12. [MCP `ai-sale-mcp` — tool specs](#12-mcp-ai-sale-mcp)
13. [REST API](#13-rest-api)
14. [Personas & Skills](#14-personas--skills)
15. [Web UI](#15-web-ui)
16. [Config / Settings](#16-config--settings)
17. [Lộ trình + tiêu chí nghiệm thu](#17-lộ-trình)
18. [Rủi ro & câu hỏi mở](#18-rủi-ro--câu-hỏi-mở)

---

## 1. Triết lý & định vị

Triết lý sản phẩm (từ yêu cầu gốc):

> Bán được hàng không do tiếp cận nhiều/quảng cáo nhiều, mà **focus vào hiệu quả, chăm sóc tệp
> đang có, liên tục gia tăng giá trị – niềm tin – mạng lưới** → win rate cao & bền vững.

Cụ thể hoá thành 4 năng lực AI Sale phải có:

| Năng lực | Nghĩa | Cơ chế trong app |
|---|---|---|
| **Follow-up chủ động** | Không để lead nguội; chạm đúng lúc, đúng nội dung | Sequences (welcome/nurture/re-engage/winback) + scheduler |
| **Nuôi tệp đang có** | Gia tăng giá trị & niềm tin cho khách cũ | Sequence `nurture` + value-content, không spam (guardrail) |
| **Nhận biết & upsell** | Phát hiện nhu cầu mua thêm từ hành vi + lịch sử | `sale_upsell_suggest` dựa deal-won (CRM) + intent signals |
| **Mạng lưới & giới thiệu** | Tận dụng quan hệ để có lead ấm | Referral program + **relationship graph của CRM** |

**Một câu định nghĩa**: AI Sale là "sale chăm sale giỏi" — chủ động, kiên trì, an toàn (không bịa
giá/cam kết, luôn có người duyệt việc rủi ro), và luôn cá nhân hoá theo ngữ cảnh thật từ CRM.

Phân biệt với AI Chat: **AI Chat = phản ứng** (khách nhắn → trả lời theo wiki, escalate nội dung khó).
**AI Sale = chủ động** (tự quyết chạm ai, khi nào, nội dung gì, để tiến pipeline & tăng doanh thu tệp).

---

## 2. Ranh giới hệ sinh thái

| Trách nhiệm | Chủ sở hữu | AI Sale gọi qua |
|---|---|---|
| Khách hàng, deal, interaction, task, kênh liên hệ, mạng lưới, FTS5 | **CRM** | `crm-mcp` (qua `agent.run` allowlist) |
| Hội thoại + kênh (Telegram/Zalo/FB/Web…), handoff/inbox | **AI Chat** | `ai-chat-mcp` (`chat_send`, handoff) |
| CSKH phản ứng (trả lời theo wiki) | **AI Chat** | — (song song, không chồng lấn) |
| LLM + agent tool-calling | **Daemon** | bridge `agent.run` / `llm.request` |
| Trí nhớ dài hạn (semantic recall) | **Daemon cognitive** | bridge `knowledge.save/recall/search`, scope `ai-sale:<crm_id>` |
| Kho tri thức sản phẩm (grounding) | **Daemon wiki** | `/api/wiki/search` |
| Điều phối đội / DAG | **AI Office** | persona `ai-sale__sale-*` được dispatch |
| **Chủ động chốt + nuôi tệp + upsell + loyalty + referral** | 🆕 **AI Sale** | (net-new) |

**Nguyên tắc vàng**: AI Sale **không** lưu lại customer master, **không** dựng pgvector, **không**
dựng LLM provider, **không** dựng channel poller. Nó chỉ giữ *sales-motion state* và điều phối.

---

## 3. Quyết định kiến trúc đã chốt

1. **Master data = tham chiếu CRM.** AI Sale lưu `crm_customer_id`, mọi hồ sơ/deal/network đọc từ CRM.
   → CRM app là dependency (cài kèm). Không đồng bộ 2 chiều, không nhân bản.
2. **Kênh = dùng lại AI Chat.** Gửi qua `chat_send`, nhận inbound qua handoff-target `ai-sale` (Phase 1).
   → Không dựng lại Telegram/Zalo/FB poller.
3. **LLM/memory = daemon bridge.** `agent.run` cho reasoning+draft, `knowledge.*` cho memory (thay pgvector).
4. **Guardrail = enforce app-side Rust**, tại chokepoint `sale_send` — agent không có đường gửi nào khác.
5. **Engine hybrid**: phần *xác định* (guardrail, rate-limit, scoring, scheduling, advance step) là Rust
   thuần; phần *suy luận + soạn tin* là `agent.run`. Giống hệt cách auto-followup tách guardrail (code)
   khỏi decision (LLM).

---

## 4. Data model (SQLite)

`~/.senclaw/space-apps/ai-sale/ai-sale.db` (WAL). Chỉ *sales-motion state*.

```sql
-- Overlay bán hàng, 1:1 với 1 customer trong CRM (không lưu lại tên/email/… — đọc từ CRM khi cần).
CREATE TABLE leads (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  crm_customer_id   INTEGER NOT NULL UNIQUE,          -- tham chiếu CRM customers.id
  stage             TEXT NOT NULL DEFAULT 'new_lead',  -- new_lead|engaged|qualified|consult_scheduled|consult_done|closed_won|churned
  temperature       TEXT NOT NULL DEFAULT 'cold',      -- cold|warm|hot|churned
  lead_score        INTEGER NOT NULL DEFAULT 0,        -- 0..100 (agent + heuristics)
  intent_signals    TEXT NOT NULL DEFAULT '[]',        -- ["asked_pricing","hot_lead_signal","complaint","upsell_interest"]
  source            TEXT NOT NULL DEFAULT '',          -- landing|ads|dm|referral|manual|telegram|zalo|...
  utm               TEXT NOT NULL DEFAULT '{}',        -- {source,campaign,medium}
  unsubscribed      INTEGER NOT NULL DEFAULT 0,
  unsubscribed_at   INTEGER,
  last_interaction_at INTEGER,
  owner             TEXT NOT NULL DEFAULT '',          -- persona/nhân sự phụ trách (tuỳ chọn)
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL
);
CREATE INDEX idx_leads_stage ON leads(stage);
CREATE INDEX idx_leads_temp  ON leads(temperature);
CREATE INDEX idx_leads_crm   ON leads(crm_customer_id);

-- Định nghĩa chuỗi follow-up. steps_json = [{ delay_hours, intent, channel?, template? }, ...]
CREATE TABLE sequences (
  key         TEXT PRIMARY KEY,          -- welcome|nurture|re_engage|winback|upsell|onboarding
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  steps_json  TEXT NOT NULL DEFAULT '[]',
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  INTEGER NOT NULL
);

-- 1 instance sequence đang chạy cho 1 lead. Nhiều sequence song song được.
CREATE TABLE sequence_runs (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  lead_id      INTEGER NOT NULL,
  sequence_key TEXT NOT NULL,
  current_step INTEGER NOT NULL DEFAULT 0,
  status       TEXT NOT NULL DEFAULT 'active',  -- active|completed|stopped|failed
  started_at   INTEGER NOT NULL,
  completed_at INTEGER,
  last_sent_at INTEGER
);
CREATE INDEX idx_seqrun_lead ON sequence_runs(lead_id);
CREATE INDEX idx_seqrun_status ON sequence_runs(status);

-- Hàng đợi job đến hạn (scheduler tokio quét). Mọi thứ "làm sau X giờ" đều là 1 row ở đây.
CREATE TABLE followup_jobs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  lead_id     INTEGER NOT NULL,
  job_type    TEXT NOT NULL,     -- sequence_step|check_inactive|follow_up_after_demo|review_reminder|weekly_report
  run_at      INTEGER NOT NULL,
  payload     TEXT NOT NULL DEFAULT '{}',   -- {sequence_run_id, step, intent, channel, ...}
  status      TEXT NOT NULL DEFAULT 'pending', -- pending|running|done|failed|cancelled
  executed_at INTEGER,
  error       TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL
);
CREATE INDEX idx_jobs_runat ON followup_jobs(run_at) WHERE status='pending';
CREATE INDEX idx_jobs_lead  ON followup_jobs(lead_id);

-- Draft rủi ro chờ Founder duyệt trước khi gửi (guardrail chokepoint điền vào đây).
CREATE TABLE reviews (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  lead_id      INTEGER NOT NULL,
  draft        TEXT NOT NULL,
  channel      TEXT NOT NULL,      -- telegram|email|zalo|...
  subject      TEXT NOT NULL DEFAULT '',
  risk_reason  TEXT NOT NULL,      -- mentioned_price|risky_keywords|rate_limit_exceeded
  status       TEXT NOT NULL DEFAULT 'pending',  -- pending|approved|rejected|edited
  edited       TEXT NOT NULL DEFAULT '',
  approved_by  TEXT NOT NULL DEFAULT '',
  approved_at  INTEGER,
  created_at   INTEGER NOT NULL
);
CREATE INDEX idx_reviews_status ON reviews(status);

-- Case cần Founder xử lý TRỰC TIẾP (không chỉ duyệt draft): complaint/hot-lead/hỏi giá/cần người.
CREATE TABLE escalations (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  lead_id     INTEGER NOT NULL,
  reason      TEXT NOT NULL,       -- hot_lead|complaint|asked_for_human|complex_question|pricing_request
  context     TEXT NOT NULL DEFAULT '{}',  -- snapshot hội thoại + reasoning + draft
  draft       TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'open', -- open|resolved
  resolved_by TEXT NOT NULL DEFAULT '',
  resolved_at INTEGER,
  created_at  INTEGER NOT NULL
);
CREATE INDEX idx_esc_status ON escalations(status);

-- Log MỌI hành động agent: reasoning + tool calls + tokens + cost. Cho audit/replay/cost dashboard.
CREATE TABLE agent_actions (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  lead_id     INTEGER,
  action_type TEXT NOT NULL,   -- next_action|draft|send|schedule|escalate|queue_review|update_stage|upsell|...
  reasoning   TEXT NOT NULL DEFAULT '',
  tool_calls  TEXT NOT NULL DEFAULT '[]',
  tokens      INTEGER NOT NULL DEFAULT 0,
  cost        REAL NOT NULL DEFAULT 0,
  needs_review INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL
);
CREATE INDEX idx_actions_lead ON agent_actions(lead_id, created_at DESC);

-- Loyalty / tích điểm.
CREATE TABLE loyalty (
  lead_id    INTEGER PRIMARY KEY,
  points     INTEGER NOT NULL DEFAULT 0,
  tier       TEXT NOT NULL DEFAULT 'bronze',  -- bronze|silver|gold|vip
  updated_at INTEGER NOT NULL
);
CREATE TABLE loyalty_ledger (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  lead_id    INTEGER NOT NULL,
  delta      INTEGER NOT NULL,
  reason     TEXT NOT NULL,       -- purchase|referral|engagement|redeem
  created_at INTEGER NOT NULL
);

-- Chương trình giới thiệu.
CREATE TABLE referrals (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  referrer_lead_id  INTEGER NOT NULL,
  referred_lead_id  INTEGER,          -- null cho tới khi người được giới thiệu thành lead
  referred_name     TEXT NOT NULL DEFAULT '',
  code              TEXT NOT NULL DEFAULT '',
  status            TEXT NOT NULL DEFAULT 'pending',  -- pending|joined|rewarded
  reward            TEXT NOT NULL DEFAULT '',
  created_at        INTEGER NOT NULL
);

CREATE TABLE settings ( key TEXT PRIMARY KEY, value TEXT NOT NULL );
```

**Vì sao overlay chứ không copy CRM**: khách 1 người, deal 1 nguồn. `leads` chỉ là "trạng thái vận
động bán hàng" gắn vào customer CRM. Xoá customer ở CRM → `leads` mồ côi (giữ để audit, giống ai-chat
giữ session mồ côi).

---

## 5. Engine — vòng lặp agentic 5 bước

Mỗi **trigger** khởi động 1 lượt. 5 bước từ auto-followup (`ĐỌC→PHÂN TÍCH→QUYẾT ĐỊNH→HÀNH ĐỘNG→GHI NHỚ`)
được giữ nguyên tinh thần, chạy bằng `agent.run`.

**Triggers**:
- `lead_captured` — có lead mới (từ contact form/ads/DM/CRM) → khởi động sequence `welcome`.
- `inbound_reply` — khách phản hồi (AI Chat handoff sang, Phase 1).
- `job_due` — scheduler bắn job đến hạn (sequence step, check-inactive…).
- `manual` — Founder/AI Office gọi `sale_next_action(lead)`.

**Luồng 1 lượt** (pseudocode Rust):
```
fn handle(trigger, lead):
  # 1. ĐỌC — build context (Rust, deterministic)
  crm      = crm_get_customer(lead.crm_customer_id)   # qua agent.run allowlist hoặc bridge
  history  = ai_chat history (nếu có session)
  memory   = knowledge.recall("ai-sale:{crm_id}", query, 5)
  product  = wiki.search(query)                        # grounding sản phẩm/giá tham khảo nội bộ
  guardrail_state = { rate_24h, unsubscribed, last_sent }

  # 2+3. PHÂN TÍCH + QUYẾT ĐỊNH + soạn draft — agent.run (LLM)
  #   persona = sale-closer/nurturer tuỳ trigger; tools allowlist = [sale_*, crm read-only]
  #   TRẢ VỀ JSON: { intent, sentiment, temperature, stage, action, draft, channel, delay_hours }
  decision = agent.run(system=persona+context, tools=[sale_send, sale_schedule_followup,
                        sale_escalate, sale_update_stage, crm_get_customer, crm_list_deals])

  # 4. HÀNH ĐỘNG — mọi "gửi" đi qua sale_send (chokepoint guardrail)
  #    agent tự gọi tool; guardrail bên trong sale_send quyết định gửi / review / escalate

  # 5. GHI NHỚ — Rust
  agent_actions.insert(reasoning, tool_calls, tokens, cost)
  knowledge.save("ai-sale:{crm_id}", summary_of_turn)
  leads.update(stage, temperature, lead_score, last_interaction_at)
```

**Điểm mấu chốt**: allowlist của agent **chỉ có `sale_send`** làm đường gửi (không cho `chat_send` trần)
→ guardrail không thể bị bypass. `sale_send` bên trong mới gọi `chat_send`.

Prompt lõi (persona `sale-closer`) = port `SYSTEM_PROMPT` của auto-followup: brand voice ấm/chuyên nghiệp,
xưng "mình – anh/chị", **NEVER-BREAK**: không bịa giá/case study, không cam kết giá/hợp đồng → escalate;
không spam (≥24h giữa 2 lần chủ động trừ khi khách vừa reply); không gửi khi unsubscribed; complaint →
escalate ngay; reply chứa giá/offer → queue review. Brand voice để chỉnh trong Settings.

---

## 6. Guardrails

Port nguyên `src/agent/guardrails.ts`. Enforce trong Rust, tại `sale_send` — **fail-closed** (không chắc → review).

Thứ tự kiểm tra trong `sale_send(lead, channel, text, is_reply)`:
1. **unsubscribed** → chặn, không gửi, không queue (throw).
2. **rate limit**: đếm outbound đã gửi trong 24h ≥ `MAX_MESSAGES_PER_CUSTOMER_24H` (mặc định 3) → queue review `rate_limit_exceeded`.
3. **complaint** (chỉ khi is_reply): text khách chứa `COMPLAINT_KEYWORDS` → **escalate** `complaint` (không auto-reply).
4. **risky content**: chứa `RISKY_KEYWORDS` (giá/giảm giá/hợp đồng/thanh toán/đặt cọc/discount…).
   - is_reply=true: ≥1 keyword → queue review.
   - broadcast (welcome/value): ≥2 keyword → queue review (ngưỡng cao hơn).
5. else → gửi thật qua AI Chat.

Keyword mặc định (VN) bootstrap sẵn, chỉnh trong Settings:
- RISKY: `giá, giảm giá, hợp đồng, thanh toán, đặt cọc, chiết khấu, discount, báo giá`
- COMPLAINT: `khiếu nại, lừa đảo, hoàn tiền, refund, kiện, tệ, huỷ, cancel`

> Đây là phần **quan trọng nhất** để doanh nghiệp thật dám để "AI tự gửi". Test coverage phải cao
> (mỗi rule 1 unit test, như bản gốc).

---

## 7. Human-in-the-loop 2 tầng

| Tầng | Bảng | Khi nào | Founder làm gì |
|---|---|---|---|
| **Review** (duyệt draft) | `reviews` | Draft rủi ro (giá/rate-limit) trước khi gửi | Approve / Edit-then-send / Reject |
| **Escalation** (xử lý trực tiếp) | `escalations` | Complaint, hỏi giá, đòi gặp người, hot-lead, câu khó | Tự trả lời / giao người / resolve |

- `POST /api/reviews/:id/approve {edited?, by}` → nếu edited: gửi bản edit (status `edited`), else gửi
  draft gốc (status `approved`). Gửi qua `sale_send` (vẫn qua guardrail — nhưng đã được người duyệt nên
  bỏ qua rule risky, giữ rule unsubscribed).
- `POST /api/reviews/:id/reject {by, reason}`.
- Escalation resolve: `POST /api/escalations/:id/resolve {by}`.
- **Thông báo Founder** (bản gốc để TODO): bắn qua **AI Chat tới admin session** hoặc daemon push
  notification / desktop toast. → tận dụng hạ tầng notify sẵn có thay vì email riêng.

UI = **Sales Inbox** (xem §15): 2 tab Review + Escalation, badge số lượng pending.

---

## 8. Sequences + Scheduler

**Sequence** = chuỗi step. Ví dụ `welcome_v1`:
```json
[
  { "delay_hours": 0,  "intent": "welcome_and_value" },
  { "delay_hours": 24, "intent": "share_value_content" },
  { "delay_hours": 72, "intent": "soft_offer_consultation" }
]
```
Bootstrap sẵn 4 sequence: `welcome`, `nurture` (khách cũ/ấm), `re_engage` (im lặng), `winback` (churned nhẹ).
Mỗi step khi đến hạn → tạo trigger `job_due` → engine chạy 1 lượt (agent soạn tin theo `intent` + ngữ cảnh
mới nhất, **không** dùng template cứng — nội dung do LLM cá nhân hoá).

**Scheduler**: 1 tokio loop trong app (giống `engine` các app khác), poll `followup_jobs` mỗi ~30–60s:
- Lấy job `pending` có `run_at <= now`, set `running`, chạy engine, set `done|failed`.
- Job định kỳ (weekly_report, check_inactive toàn tệp) tự re-enqueue.
- Guard: sau 2 lần check-in không phản hồi → `stage=churned`, dừng sequence (port từ `CHECK_IN_PROMPT`).

> Reference `auto-followup` **chưa code** scheduler (chỉ `.gitkeep`) — phần này ta viết mới hoàn toàn,
> nhưng thiết kế job types của nó (`check_inactive`, `review_reminder`, `weekly_report`,
> `follow_up_after_demo`) là kim chỉ nam.

---

## 9. Lead scoring / temperature / intent

- **lead_score 0–100**: heuristic Rust (reply gần đây, mở email/click, số lần chạm, stage) + điều chỉnh
  bởi agent sau mỗi lượt. Dùng để sort review queue & ưu tiên chăm.
- **temperature** cold/warm/hot/churned: agent cập nhật theo hành vi (reply nhanh + hỏi mua = hot).
- **intent_signals**: mảng nhãn agent gắn (`asked_pricing`, `hot_lead_signal`, `complaint`,
  `upsell_interest`, `ready_to_buy`) — feed cho escalation rule & upsell.
- **stage machine**: `new_lead → engaged → qualified → consult_scheduled → consult_done → closed_won | churned`.
  Khi `closed_won` → ghi deal-won vào CRM (`crm_move_deal`/`crm_add_deal`) + cộng loyalty.
- **Similar won-deal → playbook** (port từ customer-360): tìm khách đã thắng "giống" lead hiện tại
  (qua `knowledge.recall` thay pgvector) → gợi ý cách chốt đã hiệu quả.

---

## 10. Upsell / Loyalty / Referral

Phần mở rộng theo triết lý "tăng doanh thu từ tệp đã có" (auto-followup chưa có).

- **Upsell** `sale_upsell_suggest(lead)`: đọc `crm_list_deals` (đã won) + wiki sản phẩm → phát hiện
  sản phẩm bổ trợ/nâng cấp phù hợp → tạo `nurture`/`upsell` sequence hoặc draft gợi ý. Chỉ chạy khi
  intent cho phép (không ép khi khách vừa complaint).
- **Loyalty**: `sale_loyalty_award(lead, points, reason)` cộng điểm + ledger; tier tự lên theo ngưỡng.
  Điểm cho: mua (purchase), giới thiệu (referral), tương tác (engagement). Hiển thị trong Customer 360.
- **Referral**: `sale_referral_create(referrer, referred_name|code)`. Khi người được giới thiệu thành
  lead → link `referred_lead_id`, thưởng cả 2. **Tận dụng CRM relationship graph**: khi referral joined,
  ghi quan hệ `referred_by` vào CRM (`crm_add_relationship`) để mạng lưới liền mạch.

---

## 11. Integration contract

### 11.1 CRM (qua `agent.run` allowlist `mcp__crm-mcp__*`)
| Mục đích | Tool CRM | Tham số chính |
|---|---|---|
| Tìm/tạo lead | `crm_find_by_email(email)` → nếu trống `crm_create_customer(...)` | role=`lead` |
| Đọc hồ sơ | `crm_get_customer(id)` | |
| Deal | `crm_list_deals(customer_id)`, `crm_add_deal`, `crm_move_deal(id, stage)` | stage: qualifying→won/lost |
| Ghi touchpoint | `crm_add_interaction(customer_id, summary, kind, details)` | kind=`note`/`call`/… |
| Follow-up task | `crm_add_task(customer_id, title, due_at)` | |
| Mạng lưới/referral | `crm_customer_network(id)`, `crm_add_relationship(...)` | |

Khi `sale_capture_lead` chạy: dedupe theo email/phone → `crm_find_by_email`; nếu chưa có tạo customer
role=lead; lưu `leads.crm_customer_id`. **Một nguồn sự thật, không nhân bản.**

### 11.2 AI Chat (qua `mcp__ai-chat-mcp__*`)
- **Outbound**: `chat_send(sessionId, text)`. ⚠️ **Gap**: `chat_send` cần `sessionId` (session đã tồn tại,
  tức khách đã từng nhắn). Cho **first-touch chủ động** (lead chưa nhắn bao giờ) chưa có session.
  → **Cần bổ sung nhỏ ở AI Chat**: `chat_send_to(botKey, channel, externalId, text)` tự tạo session rồi gửi
  (hoặc `sale_send` gọi thẳng channel adapter). Ghi nhận là task tích hợp Phase 1.
  → **Phase 0** né gap này: chỉ chạm lead đã có session (đã inbound qua AI Chat) + kênh Web/preview.
- **Inbound**: thêm handoff-target `ai-sale`. Khi AI Chat bot thấy tín hiệu mua → `chat_handoff(session,
  "ai-sale")` → AI Chat `POST http://127.0.0.1:4450/api/inbound {session, customer, text, history}` →
  engine AI Sale chạy `inbound_reply`. (Phase 1.)

### 11.3 Daemon bridge (đã có, dùng như ai-chat)
`agent.run` (reasoning+tools), `llm.request` (draft đơn), `knowledge.save/recall/search`
(scope `ai-sale:<crm_id>`), `/api/wiki/search`, `/api/mcp-servers` + `/api/skills` (inventory cho allowlist),
`/api/tts|whisper` (nếu cần voice follow-up).

---

## 12. MCP `ai-sale-mcp`

Transport HTTP+SSE `/api/mcp/sse`, `autoRegister: true`. Tool prefix `sale_`.

| Tool | Input | Tác dụng / side-effect |
|---|---|---|
| `sale_capture_lead` | `{name, email?, phone?, channel?, external_id?, source?, utm?}` | Dedupe→CRM customer(lead)→tạo `leads`→khởi động `welcome` |
| `sale_get_lead` | `{lead_id}` | Trả overlay + CRM profile ghép (Customer 360) |
| `sale_list_leads` | `{stage?, temperature?, q?, limit?}` | Danh sách + lọc |
| `sale_score_lead` | `{lead_id}` | Tính lại lead_score + temperature |
| `sale_next_action` | `{lead_id}` | Chạy 1 lượt engine (agent quyết + hành động) |
| `sale_start_sequence` | `{lead_id, sequence_key}` | Tạo `sequence_run` + enqueue step 0 |
| `sale_stop_sequence` | `{run_id}` | Dừng chuỗi |
| `sale_schedule_followup` | `{lead_id, delay_hours, intent}` | Tạo `followup_jobs` |
| `sale_draft_message` | `{lead_id, intent}` | Chỉ soạn draft (không gửi) — cho UI xem trước |
| `sale_send` ⭐ | `{lead_id, channel, text, is_reply?}` | **Guardrail chokepoint** → gửi qua `chat_send` / review / escalate |
| `sale_queue_review` | `{lead_id, draft, channel, risk_reason}` | Đẩy vào `reviews` |
| `sale_escalate` | `{lead_id, reason, draft?, context?}` | Đẩy vào `escalations` + notify |
| `sale_list_inbox` | `{kind: review\|escalation, status?}` | Cho Sales Inbox |
| `sale_approve_review` | `{review_id, edited?, by}` | Duyệt & gửi |
| `sale_update_stage` | `{lead_id, stage, temperature?}` | Cập nhật pipeline |
| `sale_upsell_suggest` | `{lead_id}` | Gợi ý upsell từ deal-won + wiki |
| `sale_loyalty_award` | `{lead_id, points, reason}` | Cộng điểm + ledger |
| `sale_referral_create` | `{referrer_lead_id, referred_name?, code?}` | Tạo referral |
| `sale_pipeline_report` | `{}` | Funnel theo stage + giá trị |
| `sale_win_rate` | `{period?}` | Win rate + trend |

Để AI Office manager điều phối: các tool này + persona `sale-*` cho phép "giao chiến dịch bán" như 1 task.

---

## 13. REST API

Phục vụ web UI (React) + AI Chat inbound. Mọi thứ dưới `/api`.
```
GET  /api/status                      # health {ok, llm-info}
GET  /api/leads?stage&temperature&q   # list (ghép CRM profile)
GET  /api/leads/:id                   # Customer 360 (overlay + CRM + engagement + loyalty + timeline)
POST /api/leads                       # capture lead (idempotent) → welcome
POST /api/leads/:id/next-action       # chạy engine 1 lượt
POST /api/leads/:id/sequence          # start sequence {sequence_key}
GET  /api/sequences                   # list + edit định nghĩa
GET  /api/reviews?status               # review queue
POST /api/reviews/:id/approve|reject
GET  /api/escalations?status
POST /api/escalations/:id/resolve
GET  /api/jobs                         # scheduler queue (debug)
GET  /api/stats                        # dashboard: funnel, win-rate, pending reviews, tokens/cost
GET  /api/actions?lead_id              # agent reasoning replay
POST /api/inbound                      # AI Chat handoff → xử lý reply (Phase 1)
GET  /api/mcp/sse                      # MCP server
GET  /api/settings  / PUT /api/settings
```

---

## 14. Personas & Skills

**Personas** (cài khi install; cowork-ready `persona:ai-sale__sale-*`):
- `sale-closer` — chốt sale; tuân guardrail; escalate giá/hợp đồng. (prompt = port SYSTEM_PROMPT)
- `sale-nurturer` — nuôi tệp; gia tăng giá trị/niềm tin; nhịp chậm, không spam.
- `sale-upsell-advisor` — phát hiện nhu cầu mua thêm từ lịch sử; gợi ý đúng thời điểm.
- `sale-manager` — giám sát; duyệt review; đọc win-rate; điều phối.

**Skills** (manifest triggers, song ngữ VN/EN):
- `ai-sale-followup` — chạy/ lên lịch sequence follow-up. Triggers: "follow up khách", "chăm khách này",
  "gửi nurture", "lên lịch chốt sale", "start follow-up sequence"…
- `ai-sale-inbox` — duyệt review / xử lý escalation. Triggers: "duyệt tin chờ gửi", "hàng chờ duyệt sale",
  "xử lý khiếu nại", "review queue", "approve draft"…
- `ai-sale-pipeline` — báo cáo pipeline / win-rate. Triggers: "win rate", "báo cáo bán hàng", "phễu sale",
  "pipeline report"…
- `ai-sale-loyalty` — tích điểm / referral. Triggers: "cộng điểm", "chương trình giới thiệu", "loyalty"…

---

## 15. Web UI

React + Vite + Tailwind, song ngữ (i18n VN/EN như ai-office/ai-chat), light/dark. Màn hình:
1. **Pipeline** — cột theo stage (kanban), thẻ lead có temperature + score; kéo đổi stage.
2. **Sales Inbox** — 2 tab Review + Escalation; approve/edit/reject inline; badge pending.
3. **Customer 360** — ghép CRM profile + sales-state + engagement (sent/replied/opened) + loyalty +
   timeline `agent_actions` (reasoning replay) + similar won-deal.
4. **Sequences** — xem/sửa định nghĩa chuỗi; bật/tắt.
5. **Dashboard** — funnel, win-rate trend, pending reviews, tokens/cost.
6. **Settings** — brand voice, guardrail keywords, MAX/24h, model, ngôn ngữ.

Widget cho SenClaw dashboard: `sale-overview` (pipeline + reviews chờ), `sale-winrate`.

---

## 16. Config / Settings

Bảng `settings` (k/v) + màn Settings:
- `brand_voice` (text) — chèn vào system prompt.
- `risky_keywords`, `complaint_keywords` (CSV).
- `max_messages_per_customer_24h` (int, default 3).
- `model` (để trống = model mặc định daemon).
- `default_language` (vi/en).
- Env: `PORT=4450`, `SENCLAW_BASE_URL=http://127.0.0.1:18788`, `SENCLAW_SPACE_APP_ID=ai-sale`.

---

## 17. Lộ trình

### Phase 0 — MVP "chốt an toàn" (không cần sửa AI Chat)
Leads overlay + `sale_capture_lead` (dedupe CRM) · engine `sale_next_action` (agent.run, 5 bước) ·
**guardrail + review queue + escalation** (port §6, §7) · `sale_send` chokepoint (gửi kênh Web/session có sẵn) ·
`agent_actions` audit · Sales Inbox + Customer 360 (đọc CRM) · manifest + 2 persona + 1 skill · port 4450.
**Nghiệm thu**: tạo lead → agent soạn welcome → tin chứa "giá" bị chặn vào review → Founder approve → gửi;
complaint → escalation. `cargo build` xanh, pack zip cài được.

### Phase 1 — Follow-up tự động + inbound
Sequences engine + tokio scheduler (welcome/nurture/re-engage) · lead-score/temperature tự cập nhật ·
**inbound qua AI Chat handoff-target `ai-sale`** + `chat_send_to` (sửa nhỏ AI Chat) · pipeline funnel +
win-rate · MCP `sale_*` đầy đủ · 4 persona + 4 skill.
**Nghiệm thu**: lead im 3 ngày → scheduler tự gửi re-engage; khách reply "muốn mua" trên Telegram →
AI Chat handoff → AI Sale chốt/escalate.

### Phase 2 — Tăng doanh thu từ tệp
`sale_upsell_suggest` · loyalty (điểm/tier/ledger) + referral (link CRM graph) · mở Zalo/FB/Email ·
similar won-deal playbook · weekly report.

### Phase 3 — Hợp nhất
AI Office manager giao chiến dịch bán dạng DAG · A/B biến thể tin · cross-app analytics.

---

## 18. Rủi ro & câu hỏi mở

1. **`chat_send` session-based** → first-touch chủ động cần `chat_send_to` (Phase 1). Phase 0 tránh bằng
   chỉ chạm session có sẵn. *Cần chốt: sửa AI Chat hay AI Sale tự cắm 1 kênh outbound tối thiểu?*
2. **Ranh giới inbound AI Chat ↔ AI Sale**: khi nào bot AI Chat handoff sang AI Sale? Rule tín hiệu mua
   (intent) hay Founder gắn cờ session? → cần định nghĩa "sales-intent detection" ở AI Chat.
3. **Chi phí token**: mỗi trigger là 1 lượt agent.run. Với tệp lớn + sequence dày → cost tăng. Cần
   `cost` tracking (đã có `agent_actions.cost`) + trần rate.
4. **Nhân bản intent/temperature với CRM role**: CRM có `role` (lead/prospect/customer/vip…), AI Sale có
   `stage`. Cần map rõ để không lệch (đề xuất: CRM role = trạng thái quan hệ tổng thể; AI Sale stage =
   vị trí trong pipeline bán 1 deal).
5. **Guardrail tiếng Việt**: keyword match dấu/không dấu (dùng lower + normalize như CRM FTS5
   `remove_diacritics`). Cần test kỹ.
6. **Quyền gửi hàng loạt**: sequence chạm nhiều lead — có cần phê duyệt chiến dịch trước khi bật? (đề
   xuất: bật/tắt sequence ở cấp định nghĩa + trần MAX/24h/lead vẫn áp).

---

*Reference đã nghiên cứu: `~/Downloads/auto-followup` (Mastra + Postgres/pgvector). Các file lõi đã rút:
`guardrails.ts`, `memory.ts`, `prompts/system.ts` + `scenarios.ts`, `tools/*`, `routes/{leads,review,customer-360}.ts`,
`prisma/schema.prisma`. Scheduler jobs của reference mới là skeleton — phần đó ta thiết kế/viết mới.*
