# Tra cứu tên MCP tool & skill (Tool/Skill Name Lookup)

Tài liệu chuẩn để tìm **đúng** tên tool MCP và skill trong SenClaw — cho cả người viết skill lẫn agent đang chạy. Viết sai tên (rút gọn, đoán mò) là nguyên nhân số 1 của lỗi "No such tool available".

## 1. Hai họ tên MCP server

### a) Server bundled trong daemon (`senclaw-*`)

```
mcp__senclaw-<domain>__<prefix>_<verb>[_<modifier>]
```

Ví dụ: `mcp__senclaw-browser__browser_navigate`, `mcp__senclaw-memory__memory_search`, `mcp__senclaw-cognitive__cog_search` (cognitive là ngoại lệ duy nhất: prefix `cog_`).

- **Nguồn sự thật tên server:** các builder `*_mcp_config()` trong `src/mcp/helper.rs`.
- **Nguồn sự thật tên tool:** các `#[rmcp::tool] async fn <name>` trong `src/mcp/<domain>_server.rs`.
- Bảng registry đầy đủ nằm trong `CLAUDE.md` (mục "SenClaw MCP naming convention").

### b) Server của Space App (`<app>-mcp`)

```
mcp__<mcp.name trong senclaw-manifest.json>__<tool>
```

Tên server **không** suy ra từ id app — nó là giá trị `mcp.name` khai báo trong manifest. Ví dụ app `ssh-manager`:

```json
"mcp": { "name": "ssh-manager-mcp", "transport": "http", "path": "/api/mcp/sse", "autoRegister": true }
```

→ tool đầy đủ: `mcp__ssh-manager-mcp__ssh_list_hosts`, `mcp__ssh-manager-mcp__ssh_execute_command`, …

- **Nguồn sự thật tên server:** `apps/<app>/senclaw-manifest.json` → trường `mcp.name`.
- **Nguồn sự thật tên tool:** danh sách `tools/list` trong `apps/<app>/src/mcp.rs` (grep `"name": "` trong JSON tools).
- Một số tên server không theo mẫu `<id>-mcp` (vd. luna-calendar → `luna-mcp`) — luôn đọc manifest, đừng đoán.

## 2. Tra cứu lúc runtime

### Server nào đang đăng ký & trạng thái

```bash
curl -s http://127.0.0.1:18788/api/mcp-servers
```

Trả về từng server với `status: connected | error` và `url`. Nếu server của app không có mặt: app chưa chạy hoặc `autoRegister` thất bại — mở Space Apps UI hoặc restart daemon.

### Nạp tool trong phiên agent (ToolSearch)

- **Nạp đích danh, MỘT lệnh:** `ToolSearch` query
  `select:mcp__ssh-manager-mcp__ssh_list_hosts,mcp__ssh-manager-mcp__ssh_execute_command`
- **Tìm theo từ khóa:** `ToolSearch` query `ssh connect` — kết quả gồm cả tool (deferred) lẫn **skill** (resolver with_skills).
- ToolSearch **không phân biệt `-` và `_`** ở tên server: `mcp__ssh_manager_mcp__ssh_list_hosts` vẫn resolve về server `ssh-manager-mcp` (xem `src/tools/tool_search.rs::canonicalize`).
- **Không tồn tại dạng rút gọn**: `mcp__browser__*`, `mcp__ssh__*`, `mcp__ssh-manager__*` đều KHÔNG resolve.

### Khi ToolSearch trả 0 kết quả

Đọc kỹ output: nếu `deferred_total: 0` thì phiên này **không có tool MCP nào cả** — vấn đề không phải sai tên. Kiểm tra theo thứ tự:

1. **Whitelist `allowed_tools` của group** (bẫy phổ biến nhất): nếu cột `groups.allowed_tools` khác rỗng, phiên chỉ thấy đúng các tool trong danh sách đó.
   ```bash
   sqlite3 ~/.senclaw/senclaw.db "SELECT jid, allowed_tools FROM groups WHERE allowed_tools IS NOT NULL AND allowed_tools != '';"
   ```
   Lịch sử: trước bản vá tháng 7/2026, bấm "Always allow" ở permission prompt sẽ append tên tool vào chính cột này → session sau chỉ còn đúng tool đó (vd. `["Skill"]` tước sạch mọi MCP tool của phiên schedule SSH). Từ bản vá, lựa chọn "Always allow" lưu vào cột riêng `approved_tools`; `allowed_tools` chỉ còn là whitelist do người dùng chủ đích cấu hình. Log daemon in `set_use_tools: [...]` khi whitelist được áp.
2. **App chưa chạy / MCP chưa đăng ký:** kiểm tra `/api/mcp-servers` như trên.
3. **Daemon chưa restart** sau khi đăng ký/cài mới.

Agent gặp tình huống này phải **báo người dùng và dừng** — không thay thế bằng Bash/ssh cục bộ, không đoán tên khác.

## 3. Tra cứu skill

- **Skill đã cài (bản thật được nạp vào phiên):** `~/.senclaw/managed/skills/<name>/SKILL.md`.
- **Skill nguồn của Space App:** `apps/<app>/skills/<name>/SKILL.md` + khai báo trong `senclaw-manifest.json` → `skills[]` (name, path, triggers). Sau khi sửa nguồn phải đồng bộ sang bản đã cài (và bản deploy trong `<workspace>/space-apps/<app>/skills/` nếu có).
- **Tắt/bật:** `~/.senclaw/disabled-skills.json`.
- Trong phiên agent, ToolSearch theo từ khóa cũng trả về skill (trường `skills` trong kết quả) kèm cách invoke: `Skill { "skill": "<name>" }`.

## 4. Quy tắc khi viết/sửa SKILL.md có nhắc tool MCP

1. Ghi **tên đầy đủ** `mcp__<server>__<tool>` — copy nguyên văn từ nguồn sự thật (mục 1), không gõ tay theo trí nhớ.
2. Thêm mục "Tool names & availability" đầu skill: liệt kê lệnh `ToolSearch select:...` gộp tất cả tool cần dùng trong một lần gọi, và chỉ dẫn xử lý khi 0 kết quả (báo user, không fallback).
3. Không trộn server: skill của SenClaw/Space App không được trỏ sang Playwright hay MCP browser khác.
4. Kiểm chứng nhanh trước khi commit:
   ```bash
   grep -o 'mcp__[a-z0-9_-]*__[a-z0-9_]*' SKILL.md | sort -u   # tên nhắc trong skill
   curl -s http://127.0.0.1:18788/api/mcp-servers               # server có thật + connected?
   ```
