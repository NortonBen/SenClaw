# Shopee — SenClaw Space App 🛒

Kết nối agent SenClaw với **shop Shopee của bạn** qua **[Shopee Open Platform
CHÍNH THỨC](https://open.shopee.com)** — OAuth per-shop để lấy access token,
quản lý đơn hàng, và trả lời khách qua **Chat API**. CSKH theo kiểu **draft-first**:
mọi câu trả lời được soạn sẵn vào hàng chờ, chỉ gửi khi bạn **Duyệt**.

> **Chỉ đi cổng chính thức.** App không trộm session token của web, không né
> anti-bot, không nhắn tin hàng loạt. Đây cũng chính là con đường **không bị
> Shopee chặn** — xem [`docs/shopee-app-research.md`](../../docs/shopee-app-research.md).
> `partner_key` và token chỉ nằm trong SQLite cục bộ, chỉ gửi tới host Shopee.

Chạy trên **port 4492**.

## Có gì trong này (Phase 1 + 2 — đã có, compile + test + chạy thật)

| Layer | File | Ghi chú |
|---|---|---|
| Shopee REST client | `src/shopee.rs` | OAuth (authorize link + đổi/refresh token), **ký HMAC-SHA256**, shop info / order list / Chat (conversations, message, send) / **Product** (item list, base info, update stock/price) |
| Local store | `src/db.rs` | settings (partner_id/key/shop_id/host + autonomy), tokens + expiry, **hàng đợi draft**, activity log |
| REST API | `src/api.rs` | status / settings / oauth link+callback / account / orders / chat / drafts (+ autonomy gate observe·draft·live) |
| LLM bridge | `src/llm.rs` | soạn trả lời CSKH qua daemon bridge (không gọi provider trực tiếp) |
| **Heartbeat** | `src/engine.rs` | mỗi 180s đọc hội thoại **chưa đọc** → SOẠN nháp trả lời (dedup theo `conversation_id`); tôn trọng autonomy; `POST /api/engine/tick` chạy tay |
| **MCP server** | `src/mcp.rs` | `shopee-mcp` (HTTP+SSE) **15 tool** `shopee_*` (status/oauth_link/shop_info/orders/**order_detail**/conversations/draft_reply/list_drafts/approve/reject/tick + products/product_info/update_stock/update_price); mọi write qua đúng cổng draft-approve, **không có tool gửi hàng loạt** |
| **Order grounding** | `api.rs` | `draft_reply`/`shopee_draft_reply` nhận `order_sn` → tự tra `get_order_detail` (trạng thái/sản phẩm/vận đơn) và nhét vào context để AI trả lời đúng số liệu, không bịa |
| **Web UI** | `web/` | React 19 + Ant Design 6 (dark): Kết nối (Settings + Authorize + autonomy) · Đơn hàng · **Sản phẩm** (list + sửa tồn/giá) · Hội thoại + soạn trả lời · **Hàng chờ duyệt** (badge + Duyệt/Bỏ) · Hoạt động |

Test: `cargo test -p shopee` (8/8: ký sign, item_id_list/order_sn join, rò rỉ partner_key, vòng đời draft/token).
Build web: `cd web && npm install && npm run build`. Chạy: `PORT=4492 ./target/debug/shopee`.

## Kết nối (một lần)

1. Đăng ký Partner App tại <https://open.shopee.com> → lấy `partner_id` +
   `partner_key`. **Bạn tự đăng ký** (cần tài khoản seller).
2. Nhập `partner_id` / `partner_key` ở Settings: `POST /api/settings`.
3. `GET /api/oauth/link?redirect=<callback>` → mở link cho seller bấm đồng ý
   (link sống 5 phút).
4. Shopee redirect `GET /api/oauth/callback?code=...&shop_id=...` → app lưu token.

## Chưa làm (roadmap tiếp)

- Logistics API (`ship_order`, tracking) để trả lời "đơn tới đâu rồi" trực tiếp từ vận đơn.
- Voucher/Discount API.
- Chrome extension: hứng OAuth redirect (không cần cho OAuth Shopee vì redirect
  thẳng về localhost là đủ) — **chỉ hợp lệ**, không né-chặn/trộm token.

## KHÔNG làm (ranh giới cố định)

Trộm `SPC_*` token gọi internal `api/v4`, kỹ thuật né anti-bot ("không bị chặn"),
DM/spam hàng loạt tới user bất kỳ, crawler toàn sàn. Lý do & giải thích kỹ thuật:
[`docs/shopee-app-research.md`](../../docs/shopee-app-research.md).
