---
name: tiktok-activity
description: Điều khiển hoạt động TikTok qua app tiktok-activity — liệt kê account/flow, chạy một flow trên một account, theo dõi run, và sinh flow bằng AI. Dùng khi người dùng muốn tự động hoá tương tác TikTok (search/xem/like/comment/share/follow) theo account hoặc theo lịch.
---

# TikTok Activity

App `tiktok-activity` (App Space, port 4580) tự động hoá TikTok: mỗi account gắn proxy + browser profile riêng, các "flow" kéo-thả (chuỗi action) chạy thủ công hoặc theo lịch. Engine là graph-walker điều khiển Chromium thật qua CDP (khi bật `TIKTOK_USE_PLAYWRIGHT=1`), có nhánh success/error/alt, vòng lặp và template `{{param.key}}` / `{{prev.key}}` / `{{step.<id>.key}}`.

## MCP `tiktok-mcp`

Dùng các tool sau (canonical: `mcp__tiktok-mcp__<tool>` khi đã đăng ký):

- `tiktok_list_accounts` — liệt kê account (id, username), không lộ mật khẩu.
- `tiktok_list_flows` — liệt kê flow đã lưu (id, name, số bước).
- `tiktok_run_flow {account_id, flow_id, params?}` — khởi chạy flow, trả `run_id`.
- `tiktok_run_status {run_id}` — trạng thái + log của run.
- `tiktok_generate_flow {prompt, actions_catalog, account_id?, page_url?}` — sinh flow bằng AI từ mục tiêu ngôn ngữ tự nhiên; `actions_catalog` là danh sách action trong palette (mỗi phần tử có `paletteId`), LLM chỉ chọn trong danh sách đó.

## Quy trình gợi ý

1. `tiktok_list_accounts` + `tiktok_list_flows` để biết tài nguyên hiện có.
2. Nếu chưa có flow phù hợp: `tiktok_generate_flow` với mục tiêu + catalog, xem lại, rồi lưu qua REST `POST /api/flows`.
3. `tiktok_run_flow` để chạy, `tiktok_run_status` để theo dõi log tới khi `status` = `done`/`failed`.

## Lưu ý

- Không có `TIKTOK_USE_PLAYWRIGHT=1` → engine chạy StubDriver (mô phỏng, không mở trình duyệt) — hữu ích để kiểm thử luồng nhánh nhưng KHÔNG thực sự thao tác TikTok.
- Selector TikTok đổi theo A/B test; flow nên chèn `random_delay` giữa các bước nhạy cảm.
- LLM đi qua SenClaw bridge (`llm.request`): không nhận `temperature`, có trần output — giữ input gọn.
