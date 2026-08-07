# `senclaw-sdk/` — SDK viết Space App, bản publish được

Mỗi thư mục ở đây là **một package độc lập, publish lên registry công khai**, để
người viết Space App ngoài monorepo chỉ cần `npm install` / `pip install` chứ
không phải clone cả repo SenClaw.

| Thư mục | Package | Registry |
|---|---|---|
| [`senclaw-app-sdk/`](senclaw-app-sdk) | `@senclaw/space-sdk` | npm |
| [`senclaw-app-sdk-python/`](senclaw-app-sdk-python) | `senclaw-space-sdk` | PyPI |

Bản **Rust** vẫn ở [`../app-space-sdk`](../app-space-sdk): nó là workspace member
của chính repo này, và repo ngoài trỏ tới nó bằng git dependency chứ không qua
crates.io — xem [docs/space-app-sdk-publish-guide.md](../docs/space-app-sdk-publish-guide.md).

Cả ba nói cùng một hợp đồng manifest: chế độ vòng đời (`background` / `session`),
`requires`, `sandbox`, `runner` — [docs/space-app-lifecycle.md](../docs/space-app-lifecycle.md).

## Ngang tính năng với bản Rust

Cột Rust là chuẩn — `app-space-sdk` là bản đầy đủ nhất vì phần lớn app trong
`apps/*` viết bằng Rust. Hai bản kia bám theo.

| | Rust | Node | Python |
|---|:--:|:--:|:--:|
| `llm.request` (system/prompt/maxTokens/**profile**) | ✅ | ✅ | ✅ |
| Trả về đủ `text` + `model` + `finish` + `usage` | `llm_request_usage` | `llmDetailed` | `llm_detailed` |
| `agent.run` (agent đủ tool, nhiều bước) | — ¹ | ✅ | ✅ |
| `knowledge.save` / `.search` / `.recall` | ✅ | ✅ | ✅ |
| `usage.report` (app tự cầm key provider) | ✅ | ✅ | ✅ |
| Liệt kê / đổi model đang hoạt động | ✅ | ✅ | ✅ |
| `capabilities` — hỏi daemon nó làm được gì | — ¹ | ✅ | ✅ |
| Config + SQLite riêng của app | — ¹ | ✅ | ✅ |
| Đăng ký MCP server | — ¹ | ✅ | ✅ |
| Máy chủ MCP dựng sẵn | — ² | `/mcp` | `McpServer` |
| Dispatch (poll/heartbeat/reclaim/finalize) | ✅ | `/dispatch` | `dispatch.py` |
| Manifest: định nghĩa + kiểm + CLI | — ³ | `/lifecycle` + `senclaw-manifest` | `manifest.py` + `-m` |
| `bind_host` / `PORT` / tắt êm | thủ công | `/lifecycle` | `serve()` |

¹ App Rust gọi thẳng `POST /api/space/apps/<id>/bridge` bằng `reqwest` —
`SpaceClient::bridge_action` là private nên chưa có wrapper công khai. Không
phải thiếu năng lực, chỉ là chưa bọc.
² App Rust dùng `rmcp` trực tiếp, không cần lớp bọc.
³ Manifest của app Rust do người viết tay; test
[`space_app_lifecycle_manifests.rs`](../tests/space_app_lifecycle_manifests.rs)
là thứ bắt lỗi chính tả cho toàn repo.

`events` / `fs` / `net` của bản Rust **không có bản tương ứng, và không cần**:
chúng chỉ là lớp mô phỏng `EventEmitter`, `fs.readFile`, `net.createServer` của
Node cho Rust. Node và Python đã có sẵn trong thư viện chuẩn.

Mỗi SDK mang theo app mẫu chạy được của chính nó trong `examples/` — cài bằng
`register-local` là daemon chạy được ngay, không cần build gì thêm.

## Publish

```bash
# npm
cd senclaw-app-sdk && npm publish        # `prepare` tự build lại dist/ trước khi đóng gói

# PyPI
cd senclaw-app-sdk-python && python -m build && python -m twine upload dist/*
```

Cả hai đều bump version bằng tay trong `package.json` / `pyproject.toml` — không
có bước sinh version tự động, và một version đã publish là không sửa được.
