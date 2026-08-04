# Google Workspace Space App

Space App **Rust** (thay bản Next.js cũ): một binary axum duy nhất phục vụ
web UI (React + antd), MCP server (`/api/mcp/sse`), health probe và toàn bộ
REST API. Gọi thẳng Gmail / Calendar / Drive REST v1–v3 bằng reqwest — không
SDK, không Node.

```
SenClaw daemon ──launch ./google-workspace (PORT=4310)──▶ axum
   │  ▲                                                    │ serves:
   │  │  auto-register MCP (http://127.0.0.1:4310/api/mcp/sse)
   │  │  install bundled skill + persona                   │  • /            (UI iframe)
   │  └── POST /api/space/sync/google-calendar ◀───────────┤  • /api/*       (REST + MCP)
   │        (calendar sync đẩy sự kiện vào Space Calendar) │  • /health
   └─ SENCLAW_BASE_URL env                                 ▼
                                              ~/.senclaw/apps/google-workspace/gworkspace.db
                                              (settings + tokens + sync log, SQLite cục bộ)
```

## Kết nối Google

Hai cách, đều nằm trong UI (nút **Kết nối Google**):

1. **OAuth chuẩn** (khuyên dùng — tự gia hạn): tạo OAuth client loại *Web
   application* trong Google Cloud Console, redirect URI
   `http://127.0.0.1:4310/api/auth/callback`, nhập Client ID/Secret vào
   Settings rồi bấm nút OAuth (mở tab mới vì Google chặn iframe).
2. **Dán access token** (`ya29.…` từ OAuth Playground) — hết hạn ~1 giờ trừ
   khi kèm refresh token.

Token chỉ nằm trong SQLite cục bộ và chỉ được gửi tới endpoint Google.
Scopes: `gmail.readonly`, `gmail.send`, `calendar.events`, `drive.file`,
`drive.readonly`.

## Tools (MCP server `google-workspace-mcp` — 10 tools)

| Nhóm | Tools |
|------|-------|
| Settings | `gworkspace_get_settings`, `gworkspace_set_settings` |
| Gmail | `gworkspace_list_emails` (hỗ trợ `q`), `gworkspace_read_email`, `gworkspace_send_email` |
| Calendar | `gworkspace_list_events`, `gworkspace_create_event` |
| Drive | `gworkspace_list_files` (hỗ trợ `q`), `gworkspace_upload_file` |
| Sync | `gworkspace_sync` — calendar đẩy vào Space Calendar qua daemon |

## Develop

```bash
cargo run -p google-workspace          # backend :4310
cd apps/google-workspace/web && npm run dev   # Vite dev server (proxy /api)
cargo test -p google-workspace         # 15 unit tests, không gọi mạng
```

## Build & pack ZIP

```bash
apps/google-workspace/scripts/pack.sh
# → apps/google-workspace/google-workspace-app.zip
```

Cài: Space → Apps → Cài từ ZIP, hoặc

```bash
curl -X POST http://127.0.0.1:18788/api/space/apps/install-zip \
  -F "file=@apps/google-workspace/google-workspace-app.zip"
```
