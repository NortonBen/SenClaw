# MCP Tool Alias — định danh lại & ghi đè MCP tool

Tính năng cho phép **đặt tên mới cho một MCP tool** (định danh lại / rename) hoặc
**ghi đè một tool đang có bằng tool khác** (override), quản lý tại
**Web UI → Plugins → Alias**, lưu trong bảng SQLite `mcp_tool_aliases`.

Một alias là một ánh xạ `alias → target`:

| Tình huống | Hành vi |
|---|---|
| `alias` là tên **chưa tồn tại** | **Định danh mới**: roster gửi cho LLM hiển thị tool đích dưới tên `alias` (mô tả có thể thay bằng mô tả của alias). Tên gốc *vẫn gọi được* (qua `Tool::renamed_from`), nên transcript cũ, skill docs, whitelist cũ không hỏng. |
| `alias` **trùng tên một tool đang đăng ký** | **Ghi đè**: mọi lệnh gọi tên đó bị chuyển hướng sang `target` *trước* bước exact-match, tool gốc bị che. Roster không đổi. |
| `target` không tồn tại (app tắt, gõ sai) | Fallback về tool gốc + log warning — alias không bao giờ làm chết một tool đang chạy. |
| Chuỗi alias `a → b → c` | Được duyệt đến đích cuối, có chống vòng lặp (cap 8 bước; vòng lặp thoái hoá về tên gốc). |

## Điểm cắm trong runtime

- **Dispatch / resolve** — stage 0 của `resolve_tool_by_name`
  ([`src/tools/tool_search.rs`](../src/tools/tool_search.rs)): mọi đường gọi tool
  (`run_tools`, `ToolSearch select:`, whitelist `use_tools`, isolated runner) đều đi qua đây.
- **Roster (tên LLM nhìn thấy)** — `apply_alias_names` decorate trong
  `tools_for_main_agent` + `deferred_tools`
  ([`src/zen_core/engine.rs`](../src/zen_core/engine.rs)); các funnel còn lại
  (`available_tools`, `tools_for_subagent`, `get_tool_infos`) dẫn xuất từ hai cái này.
- **Registry process-wide** — [`src/tools/tool_alias.rs`](../src/tools/tool_alias.rs)
  (`OnceLock<RwLock<HashMap>>`), nạp lúc boot (`run_daemon`) và nạp lại sau **mỗi**
  mutation REST / import app — đổi alias có hiệu lực từ turn kế tiếp, không cần restart.
- **Phân loại read-only** (chạy song song hay tuần tự) dùng đúng resolver trên —
  ghi đè tool read-only bằng tool có side-effect sẽ **không** lọt vào nhánh chạy song song.
- Permission key lấy theo **tool thực thi sau resolve** (override → key của target;
  rename → key là tên alias).

## REST API

```
GET    /api/tool-aliases                  → { aliases: [...] }
POST   /api/tool-aliases                  { alias, target, description? }   # source=user, enabled mặc định
PUT    /api/tool-aliases/:alias           { target, description? }          # chỉ alias source=user
POST   /api/tool-aliases/:alias/enabled   { enabled }                       # cổng phê duyệt alias app
DELETE /api/tool-aliases/:alias
```

Handlers: [`src/gateway/ui_server/tool_aliases.rs`](../src/gateway/ui_server/tool_aliases.rs);
DB: [`src/db/tool_aliases.rs`](../src/db/tool_aliases.rs).

## Space App khai báo alias trong manifest

`senclaw-manifest.json` → khối `mcp`:

```json
"mcp": {
  "name": "ssh-manager-mcp",
  "transport": "http",
  "path": "/api/mcp/sse",
  "autoRegister": true,
  "toolAliases": [
    { "alias": "mcp__ssh__run", "tool": "ssh_execute_command", "description": "Chạy lệnh trên host đã lưu" },
    { "alias": "mcp__senclaw-browser__browser_navigate", "target": "mcp__ssh-manager-mcp__ssh_open_url" }
  ]
}
```

- `tool` (hoặc `target`): tên bare sẽ được nở thành `mcp__<mcp.name>__<tool>`;
  tên đầy đủ `mcp__*` giữ nguyên (cho phép app ghi đè tool của server khác).
- `alias` **bắt buộc** dạng `mcp__*` — chặn app giả mạo tool builtin (Bash, Read, ...).
  Entry sai bị bỏ qua kèm warning, không chặn app.
- Import chạy trong `run_and_register` ([`src/gateway/ui_server/space_mcp.rs`](../src/gateway/ui_server/space_mcp.rs))
  — mọi đường install / update / boot / supervisor / restart đều qua đây, và cần
  `mcp.autoRegister: true`.

**Quy tắc phê duyệt (an toàn):**

1. Alias do app khai báo được nhập ở trạng thái **tắt** (`enabled = 0`).
   Người dùng phải vào **Plugins → Alias** bật lên thì mới có hiệu lực.
2. Re-import (app restart / update) chỉ refresh `target`/`description`,
   **không bao giờ** đụng `enabled` — opt-in của người dùng sống sót qua update.
3. App không chiếm được alias thuộc source khác (user hoặc app khác) —
   upsert có guard `WHERE source = excluded.source`.
4. Alias app không còn khai báo trong manifest sẽ bị prune; gỡ app xoá toàn bộ
   alias nguồn `app:<id>` và nạp lại registry ngay.

## Desktop app (Flutter)

Màn hình tương đương nằm ở **Plugins → Alias** trong `desktop_app`
([lib/features/plugins/plugins_screen.dart](../desktop_app/lib/features/plugins/plugins_screen.dart):
`_AliasTab` / `_AliasRow` / `_AliasEditor`, model `ToolAlias`, provider
`toolAliasesProvider` + `knownToolNamesProvider`). Nhãn tiếng Anh theo các
section anh em trong Plugins; badge `override` / `new name` suy từ danh sách
tool của `/api/mcp-servers`. Widget tests:
[test/alias_tab_test.dart](../desktop_app/test/alias_tab_test.dart).
Chạy thử desktop app trỏ vào harness (không đụng daemon thật):

```bash
cd desktop_app && flutter run -d macos --dart-define=SENCLAW_UI_PORT=18988
```

## Dev harness

```bash
npm run build:web
cargo build --example alias_ui_harness
./target/debug/examples/alias_ui_harness   # http://127.0.0.1:18988/plugins?nav=alias
```

Chạy router UI thật (REST mới + SPA `web/dist`) trên DB tạm có sẵn 2 alias mẫu —
không đụng daemon desktop đang chạy hay dữ liệu `~/.senclaw`.

## Tests

- `cargo test --lib tool_alias` — registry, chuỗi/vòng lặp, parse manifest, DB CRUD,
  bảo toàn `enabled` khi re-import, prune, rename/override resolution + roster decoration.
- `cargo test --test tool_alias_api` — REST end-to-end trên router thật
  (create → 409 → validate → import app → PUT bị chặn → enable gate → update → disable → delete).
