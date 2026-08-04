# Sentinel — Giám sát & Điều tra Bảo mật AI Agent

Space App cổng **4680** · MCP `sentinel-mcp` (27 tool, prefix `sen_`) · 114 test

Lớp **phát hiện** (detective) cho chính SenClaw. SenClaw đã có nhiều lớp phòng
thủ — permission gate, HITL bridge, SSRF guard, shell-safety, workspace
containment — nhưng không có gì phát hiện khi một lớp trong số đó bị tắt hoặc bị
vượt qua, và `grep -ri 'audit' src/` không trả về bảng audit nào.

Sentinel lấp bốn khoảng trống đã kiểm chứng:

| Khoảng trống trong daemon | Sentinel làm gì |
|---|---|
| Lịch sử bị FIFO xoá dần (`tool_executions` trim theo `groups.max_messages`) | Chép một chiều sang kho **chỉ-thêm**, bảo toàn phần daemon vứt đi |
| Không cột `created_by` ở bất kỳ bảng nào | Suy ra đối tượng từ `group_folder`, `chat_jid`, `owner_kind` và nói rõ đâu là suy đoán |
| Cấu hình ghi đè tại chỗ, không lịch sử | **Chụp ảnh 9 nhóm** mỗi 15 phút rồi so sánh |
| Auto-approve không để lại bản ghi | Đo gián tiếp bằng khoảng cách "tool đã chạy" vs "lần được hỏi" |

Tài liệu thiết kế đầy đủ: [docs/sentinel-app-design.md](../../docs/sentinel-app-design.md).
Lớp **phòng ngừa** tương ứng: [docs/prompt-injection-security.md](../../docs/prompt-injection-security.md).

## Ba ranh giới cố ý

**Chỉ đọc.** Mở `~/.senclaw/senclaw.db` bằng `file:…?mode=ro` + `PRAGMA
query_only=ON`. Không có đường ghi nào tới daemon trong toàn bộ mã. MCP surface
cũng không có tool nào tạm dừng lịch hay tắt server — nếu chính agent đang bị
chiếm quyền, nó không được phép dùng công cụ điều tra để tự dọn dấu vết. Một bài
test bất biến sẽ gãy nếu ai đó thêm tool ghi.

**Bind loopback.** `127.0.0.1`, khác với 39 Space App còn lại đang bind
`0.0.0.0`. App này nắm toàn bộ lịch sử hoạt động agent nên không được nghe ngoài
máy. Ghi đè bằng `SENTINEL_BIND` nếu chạy trong container.

**Luật là Rust, AI chỉ diễn giải.** 32 luật tất định có kiểm thử. AI được giải
thích phát hiện, dựng giả thuyết, viết báo cáo — **không** chấm mức, không đóng
phát hiện, không sinh truy vấn. Mọi nội dung agent-sinh đưa vào prompt đều bọc
giữa `BEGIN_UNTRUSTED_EVIDENCE`/`END_UNTRUSTED_EVIDENCE`, vì dữ liệu app phân
tích chính là thứ có thể chứa prompt injection.

## Cấu trúc

```
src/
  main.rs      boot axum + vòng nền 60s (ingest → chụp ảnh → quét)
  db.rs        kho riêng: events (chuỗi băm SHA-256), snapshots, findings, cases
  source.rs    đầu đọc chỉ-đọc: SQLite daemon, REST 18788, llm_logs
  ingest.rs    chuẩn hoá 4 nguồn → events, con trỏ theo khoá tăng dần
  redact.rs    lọc bí mật TRƯỚC khi ghi — bản gốc không bao giờ được lưu
  rules.rs     32 luật, 6 nhóm, ánh xạ OWASP LLM + Agentic
  snapshot.rs  chụp 9 nhóm cấu hình + diff theo khoá
  llm.rs       AI qua bridge, có hàng rào untrusted
  api.rs       REST; mọi handler đi qua *_value dùng chung với MCP
  mcp.rs       27 tool JSON-RPC + SSE
web/           React 19 + AntD 6 + Vite 8, 6 tab
  theme.tsx    sáng / tối / theo hệ thống — AntD algorithm + biến CSS cho phần tự vẽ
```

Dữ liệu app: `~/.senclaw/apps/sentinel/sentinel.db` (đổi bằng `SENCLAW_DATA_DIR`).

## Nhóm luật

| Nhóm | Ví dụ |
|---|---|
| `PERSIST` (6) | Lịch chạy `bash -c` không qua kiểm tra; lịch tạo qua MCP chứ không qua UI; lịch bị xoá còn nhật ký; lịch `isolated` báo success nhưng là stub |
| `CTRL` (8) | HITL tắt toàn cục; khoảng cách phê duyệt; auto-accept wildcard; mở-cửa-rồi-đi-qua; bị từ chối vẫn chạy; hook đổi |
| `EXFIL` (4) | Đọc nhạy cảm rồi gửi ra ngoài; `send_file`; `curl`/`base64` trong shell; bí mật đi qua ngữ cảnh |
| `INJECT` (5) | Kết quả tool chứa chỉ thị; tool poisoning; rug pull manifest; memory poisoning; tin nhắn injection dẫn tới hành động |
| `ANOM` (4) | Ngoài giờ (theo giờ **địa phương**); bùng nổ vs nền; lặp lỗi; một phiên dùng nhiều họ tool |
| `POSTURE` (5) | Bề mặt tool; app mở ra LAN (kiểm chứng bằng TCP thật); skill/plugin mới; workdir quá rộng; llm_logs |

## Chạy & đóng gói

```bash
cargo run -p sentinel                      # backend, cổng 4680
cd apps/sentinel/web && npm run dev        # UI dev, proxy sang 4680
cargo test -p sentinel                     # 114 test
apps/sentinel/scripts/pack.sh              # → sentinel-app.zip (3.3M)
```

Cài vào daemon:

```bash
curl -X POST http://127.0.0.1:18788/api/space/apps/install-zip -F file=@apps/sentinel/sentinel-app.zip
```

## Điều Sentinel KHÔNG thấy

Nói ra để không tạo cảm giác an toàn giả:

- **Đối số tool không có trong DB daemon.** Chỉ khôi phục được từ
  `~/.senclaw/llm_logs` qua `sen_tool_args`, và log chỉ giữ 30 ngày.
- **Lệnh shell của lịch `script` không vào nhật ký chạy** — chỉ nằm trong định
  nghĩa lịch.
- **Lần auto-approve không để lại bản ghi nào** — chỉ đo được gián tiếp.
- **Hành động chi tiết trong trình duyệt không được ghi** (`browser_server.rs`
  không ghi DB).
- **Việc xoá lịch không được ghi lại** — chỉ suy ra từ nhật ký mồ côi.
- Chạy theo mẻ mỗi phút, nên luôn đến **sau** sự việc.

Cách khắc phục thật sự cho mục 1 là thêm hook `PostToolUse` ghi `tool_input` —
việc đó thuộc về core, không thuộc app này.
