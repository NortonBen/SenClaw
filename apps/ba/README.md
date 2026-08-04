# BA Studio — Trợ lý Business Analyst (Space App)

Space App mô phỏng quy trình **BA-Kit** (nghiên cứu từ ai4ba.com/ba-kit — xem
[docs/ba-app-design.md](../../docs/ba-app-design.md)): quản lý dự án → tính
năng → tài liệu BA **chia theo 9 giai đoạn làm việc**, chạy theo **workflow**,
mọi bước **xuất ra tài liệu** có ID truy vết.

- Port **4740** · MCP **`ba-mcp`** (tool `mcp__ba-mcp__ba_*`, 30 tool)
- DB: `~/.senclaw/space-app-data/ba/app.sqlite` (override `BA_DATA_DIR`)
- LLM qua bridge daemon `llm.request` (không khoá riêng, maxTokens ≤ 32000)

## Tính năng chính

- **31 loại tài liệu** sinh bằng AI theo template chuẩn (`src/templates.rs` là
  source of truth): PRD/roadmap/discover · brainstorm (phỏng vấn làm rõ —
  interview mode) · URD/BRD/PRD-epic · **SRS 11 mục** (FR/NFR/BR/Error
  Matrix/SC) · SRS tái lập từ văn bản/code (kèm mức tin cậy) · 9 loại sơ đồ
  (Mermaid render sống; BPMN kèm XML 2.0, DBML, PlantUML/D2 đính kèm) · use
  case Cockburn · user story + AC Given/When/Then · user flow ·
  wireframe ASCII/HTML · prototype HTML bấm được · bộ API (assess/doc/design/
  map 3 tầng/checklist/**bộ test Bruno collection**/readiness) · test
  checklist/case/Playwright · gap report · doc drift · userguide · biên bản
  họp · overview dùng chung. ERD/activity/architecture kèm khối mã D2.
- **Workflow**: 3 biến thể BA-Kit (trọn vòng đời / story trước / prototype
  trước) + tuỳ biến; bước `run` sinh tài liệu, gợi ý bước kế.
- **Truy vết deterministic** (`src/trace.rs`): parse ID bằng regex — coverage
  FR↔US↔AC↔UC↔TC, pipeline 8 chặng, staleness theo đồ thị upstream, dashboard
  việc gấp + kanban lifecycle (draft → in_review → revisions → approved →
  shipped).
- **Change Request đồng bộ**: `CR-YYYYMMDD-NNN`, AI phân tích tác động thành
  impacts trên tài liệu thật → apply từng cái (draft-first, giữ version).
- **Knowledge Graph** (`/api/projects/:id/kg`, tool `ba_kg`): node = tài liệu,
  cạnh upstream + cạnh tham chiếu ID thật, vẽ mermaid — bản /kg deterministic.
- **Hỏi đáp có trích dẫn** trên FTS5 (fold đ→d) · **Export** md/HTML tự chứa ·
  **Preview** `/api/preview` giống srs-preview của BA-Kit (render mermaid,
  dark/light, nút quay lại app).

## Chạy dev

```bash
cargo build -p ba
PORT=4740 BA_DATA_DIR=/tmp/badata ./target/debug/ba
cd apps/ba/web && npm install && npm run build   # hoặc npm run dev (proxy :4740)
cargo test -p ba
apps/ba/scripts/pack.sh                          # -> ba-app.zip
```

Đăng ký dev với daemon:

```bash
curl -X POST http://127.0.0.1:18788/api/space/apps/register-local \
  -H 'Content-Type: application/json' \
  -d '{"path":"/Users/benji/Projects/SemaClaw/apps/ba"}'
```

## Ngoài phạm vi

Jira/Confluence sync, Figma, prototype Next.js scaffold, chạy test thật (dùng
app AutoTest), đọc Word/PDF/ảnh trực tiếp (dán text hoặc dùng OCR app).
