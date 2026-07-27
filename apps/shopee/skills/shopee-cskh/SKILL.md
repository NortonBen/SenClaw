---
name: shopee-cskh
description: Kết nối shop Shopee (OAuth), xem đơn hàng, và trả lời khách qua Chat API — draft-first, chỉ gửi khi được duyệt. Dùng khi người dùng nói về tin nhắn/đơn hàng Shopee hoặc muốn kết nối shop.
---

# Shopee — CSKH (draft-first)

App Shopee chạy trên `http://127.0.0.1:4490`. Chỉ dùng **Shopee Open Platform
CHÍNH THỨC**. Không bao giờ trộm cookie/session token, không né anti-bot, không
nhắn tin hàng loạt.

## Kết nối shop (một lần)

1. Người dùng đăng ký Partner App tại <https://open.shopee.com> → lấy
   `partner_id` + `partner_key` → nhập ở tab **Settings** của app.
2. Gọi `GET /api/oauth/link?redirect=<callback>` → mở link cho seller **tự bấm
   đồng ý** (link sống 5 phút).
3. Shopee redirect về `GET /api/oauth/callback?code=...&shop_id=...` → app đổi
   lấy access/refresh token và lưu cục bộ.

## Trả lời khách

- `GET /api/chat/conversations` — danh sách hội thoại buyer↔seller.
- `POST /api/chat/reply` `{conversation_id, to_id, customer_msg, context}` —
  SOẠN một bản nháp (LLM viết nếu không truyền `content`). **Không gửi ngay.**
- Trình cho người dùng bản nháp. Chỉ khi được đồng ý:
  `POST /api/drafts/:id/approve` → đây là cổng duy nhất thực sự gọi Shopee
  `send_message`. `POST /api/drafts/:id/reject` để bỏ.

## Nguyên tắc

- Trả lời dựa trên **dữ liệu shop thật** (đơn/sản phẩm/chính sách). Không bịa
  giá, tồn kho, chính sách. Không chắc thì hẹn kiểm tra lại.
- Chỉ nhắn cho **khách của shop này**. Không có (và không thêm) API nhắn hàng loạt.
