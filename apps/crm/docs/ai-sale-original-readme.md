# AI Sale — SenClaw Space App 🎯

**AI chốt sale** — lớp *chủ động* bán hàng & chăm sóc tệp trong hệ sinh thái SenClaw:
**CRM (dữ liệu) + AI Office (điều phối) + AI Chat (kênh & CSKH phản ứng) + AI Sale (chốt & nuôi tệp)**.

Thiết kế đầy đủ (schema, tool specs, lộ trình): [`DESIGN.md`](DESIGN.md).

## Điểm chính

- **Tái dùng, không dựng lại**: khách hàng đọc từ **CRM** (`crm_customer_id`), LLM + trí nhớ từ **daemon**
  (`llm.request` + `knowledge.*` scope `ai-sale:<crm_id>`, thay pgvector), kênh gửi từ **AI Chat**
  (Phase 1). AI Sale chỉ giữ *sales-motion state* trong SQLite (`~/.senclaw/space-apps/ai-sale/ai-sale.db`).
- **Guardrail bắt buộc** (port từ reference `auto-followup`), enforce Rust tại chokepoint `sale_send` —
  đường gửi DUY NHẤT nên agent không lách được:
  1. khách đã hủy nhận tin → chặn;
  2. quá tần suất (mặc định 3 tin/24h) → hàng chờ **duyệt**;
  3. nội dung nhạy (giá/hợp đồng…) → hàng chờ **duyệt** (reply ≥1 từ, broadcast ≥2 từ);
  4. inbound có từ khiếu nại → **escalate** lên người ngay (không auto-reply).
- **Human-in-the-loop 2 tầng**: `reviews` (duyệt draft) + `escalations` (Founder xử lý trực tiếp).
- **Vòng lặp agentic 5 bước**: Phase 0 soạn tin qua `llm.request` + guardrail Rust; Phase 1 chuyển sang
  `agent.run` với allowlist `sale_*` + `crm_*`.
- **Luồng TỰ ĐỘNG đầu-cuối** (scheduler tokio là nhịp tim): lead mới → **tự vào chuỗi `welcome`** →
  các bước chạy theo lịch (soạn tin qua guardrail) → nếu khách **im lặng** quá ngưỡng → **tự check-in**
  (re-engage) → sau `max_checkins` lần không hồi đáp → **tự churn**; khách **reply** → reset đồng hồ im
  lặng + streak. Knob: `auto_welcome` (setting), `SALE_INACTIVE_MS` (3 ngày), `SALE_CHECKIN_COOLDOWN_MS`
  (7 ngày), `SALE_MAX_CHECKINS` (2), `SALE_SCHED_INTERVAL_SECS` (30s).
- **MCP `ai-sale-mcp`**: để daemon/AI Office điều khiển (capture lead, next-action, send-qua-guardrail,
  review/escalation, pipeline report).

## Chạy khi phát triển

```bash
cargo run -p ai-sale               # http://127.0.0.1:4450 (cần daemon SenClaw ở 18788 cho LLM)
npm --prefix apps/ai-sale/web run dev   # Vite dev server cho web UI
```

Cấu hình qua env: `PORT` (4450), `SENCLAW_BASE_URL` (`http://127.0.0.1:18788`),
`SENCLAW_SPACE_APP_ID` (`ai-sale`), `SENCLAW_CRM_URL` (`http://127.0.0.1:4390`).

## Đóng gói

```bash
apps/ai-sale/scripts/pack.sh       # build web + binary → release/ + ai-sale-app.zip
```

## Trạng thái

**Phase 0 (MVP)** — leads overlay + guardrail + review/escalation + Customer 360 + MCP + web skeleton.
Đã verify guardrail routing (risky→review, safe→sent, approve→sent, complaint→escalate) không cần daemon.
Follow-up sequences + scheduler + nhận inbound qua AI Chat handoff + upsell/loyalty/referral thuộc
Phase 1–2 (xem DESIGN.md §17).
