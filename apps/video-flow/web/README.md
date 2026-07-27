# Frontend React + TypeScript (Migration UI)

Giao diện shell: **Tạo project** (form + AI gợi ý, khớp skill) và **Pipeline** (3 bước: video → scenes → generate) gọi API Go + SQLite (`migration-golang/backend-go`).

## Chạy dev

1. Backend Go (cùng file `flow_agent.db`):

   ```bash
   cd ../backend-go
   export FLOWKIT_DB_PATH=/path/to/flowkit/flow_agent.db
   go run ./cmd/server
   ```

2. UI (port **5174**, proxy `/api` và `/health` → `8101`):

   ```bash
   npm install
   npm run dev
   ```

Mở `http://127.0.0.1:5174`.

## Cấu hình API trực tiếp (không proxy)

Nếu deploy tách host, set:

```bash
VITE_API_BASE=http://127.0.0.1:8101 npm run dev
```

## Build production

```bash
npm run build
npm run preview
```

## Gợi ý project bằng AI (langchaingo)

Ở trang **Tạo project**, khối **Gợi ý bằng AI**: nhập story/yêu cầu, chọn provider (OpenAI, Gemini, LM Studio, DeepSeek, OpenRouter), tùy chọn model / API key / base URL → **Điền form từ AI** gọi `POST /api/ai/suggest-project`. Sau khi **Tạo project**, UI chuyển sang Pipeline; ở bước **Scenes** có thể **Tạo N scene từ gợi ý AI** (scene hints được truyền từ bước tạo project).

Backend: [github.com/tmc/langchaingo](https://github.com/tmc/langchaingo/).

## Luồng UI

1. **Tạo project** (menu) — tên, material, story, entities, AI gợi ý → `POST /api/projects`, hints scene lưu cho Pipeline
2. **Pipeline — Video** — tiêu đề, orientation → `POST /api/videos`
3. **Pipeline — Scenes** — thêm scene hoặc import từ hints → `POST /api/scenes`
4. **Pipeline — Generate** — batch ref / scene image / video → `POST /api/requests/batch`, poll `GET /api/requests/batch-status` (ref ảnh chỉ lọc theo `project_id`)

Worker thực tế (Google Flow) vẫn cần agent Python + extension nếu dùng chung DB; UI chỉ enqueue và hiển thị trạng thái.
