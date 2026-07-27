# Rewrite Story

SenClaw Space App for rewriting stories with AI. Port of the Go/Gin + Postgres
[`re-write-story`](../../../re-write-story) backend.

Nhập truyện gốc → cắt chunk bằng bộ chia hybrid hiểu tiếng Việt → viết lại từng
chunk qua LLM chung của SenClaw → ghép thành một phiên bản mới, giữ nguyên bản
gốc. Mỗi chunk viết xong được lưu ngay, nên hỏng giữa chừng thì chạy tiếp được.

- Port **4470**, MCP server `rewrite-story-mcp` (tools `rs_*`).
- Dev: `cargo run -p rewrite-story` (repo root) + `cd web && npm run dev` (5175).
- Đóng gói: `scripts/pack.sh` → `rewrite-story-app.zip`.

## Layout

| File | Vai trò |
|---|---|
| `src/text.rs` | Bộ chia hybrid tiếng Việt (port `pkg/text/hybrid_splitter.go`) |
| `src/process.rs` | Poller + watchdog + vòng lặp viết lại (port `service/process`) |
| `src/llm.rs` | Cầu nối `llm.request` + prompt viết lại |
| `src/db.rs` | SQLite, struct có kiểu |
| `src/api.rs` / `src/mcp.rs` | REST + MCP |

## Khác gì bản Go

Những chỗ port lệch có chủ đích. Ghi lại để sau này không ai "sửa ngược".

**Bỏ hẳn**
- JWT, user, per-user settings, quota theo user — Space App là single-user, chạy local.
- Postgres → SQLite. Toàn bộ `service/llm/provider` (Gemini/DeepSeek/OpenRouter,
  API-key pool, rate-limit bookkeeping) → cầu nối `llm.request` của daemon.
- Gemini context cache. Thay bằng system prompt gửi thẳng mỗi chunk.
- Lorebook, `PlanVersion`, sinh ảnh nhân vật, cognee — ngoài phạm vi v1.

**Sửa lỗi khi port**
- `lengthReq` đếm **ký tự**, không phải byte. Go nội suy `len(chunkText)` (byte)
  vào câu nói "ký tự"; trên tiếng Việt sai ~33% (đo trên chính corpus của dự án),
  trong khi dung sai đi kèm chỉ vài %.
- **System instruction luôn tới model.** Ở Go nó chỉ đi qua Gemini context cache,
  nên mỗi lần tạo cache lỗi (đường đi "log rồi chạy tiếp") là toàn bộ vai trò hệ
  thống bị bỏ im lặng.
- **`creativity_ratio` diễn đạt thành chỉ dẫn trong prompt**, không map sang
  temperature. Cầu nối không có tham số temperature, nên map như Go sẽ khiến núm
  chính của app không có tác dụng gì.
- **Công thức tiến độ theo dải stage.** Go dùng `((currentChunk-1)+sub)/total`,
  ra số âm mỗi khi gọi với `currentChunk = 0` (stage `pending` và `analyzing` đều gọi thế).
- **Lưu chunk lỗi là fatal.** Go chỉ log cảnh báo rồi chạy tiếp, tạo một lỗ hổng
  vĩnh viễn trong dãy chunk khiến resume dừng ở đó mãi mãi.
- **Bảng `rewrite_chunks`** (Go: `rewrite_store_chunk` — typo, số ít) và có
  `UNIQUE(process_id, chunk_index)`; Go dùng `db.Save` với ID rỗng nên luôn INSERT,
  chunk trùng chỉ được chặn bằng logic resume.
- **Claim job nguyên tử** (`WHERE status='queued'`), Go UPDATE vô điều kiện.
- **Continuity dùng một hàm duy nhất.** Go dùng `ExtractLastParagraph` trong vòng
  lặp nhưng `ExtractLastSentences(_, 2)` khi resume, nên lượt chạy tiếp đưa cho
  model một gợi ý khác hình dạng so với lượt chạy liền mạch.
- **Cắt chuỗi theo char boundary.** Go cắt byte và có thể sinh UTF-8 hỏng ở nhánh
  cắt cứng; Rust sẽ panic.
- **`vite base` phải là `/`, không phải `./`.** Binary phục vụ SPA ở gốc origin và
  fallback index.html cho route client-side; base tương đối khiến `/stories/4` xin
  asset ở `/stories/assets/index-*.js`, fallback trả index.html kèm mã 200, và
  trang trắng hoàn toàn. Mọi deep link và mọi lần refresh đều hỏng. (video-flow
  đang dùng `./` — nhiều khả năng dính đúng lỗi này.)
- **Kích thước chunk tính bằng KÝ TỰ.** Go so `len()` — byte — với `min/max_size`,
  nên trên tiếng Việt (~1.4 byte/ký tự) chunk luôn ngắn hơn ~30% so với con số
  người dùng gõ vào ô ghi "ký tự". Cả `is_chapter_start` (trần 100) và bộ lọc
  từ 1 chữ trong `build_tf` cũng đổi sang đếm ký tự.
- **`max_size` bị chặn bởi trần output của model** (`llm::MAX_CHUNK_CHARS`), không
  phải bởi con số trong config Go. Chunk mà bản viết lại không lọt 8192 token thì
  hỏng ngay chunk đầu của mọi truyện.
- **Guard trạng thái nằm trong `WHERE` của SQL**, không phải đọc-rồi-ghi trong Rust.
  Đọc và ghi là hai lần khoá riêng, nên một lệnh huỷ rơi vào giữa vẫn bị ghi đè,
  tạo ra dòng `processing` mà lại mang `completed_at` và "Bị hủy bởi người dùng".
- **Đọc tiến độ đã lưu là fatal, không `unwrap_or_default`.** Một lỗi DB thoáng qua
  mà bị hiểu thành "chưa có chunk nào" sẽ viết lại cả tiểu thuyết và
  `INSERT OR REPLACE` xoá luôn bằng chứng.
- **Ghép chỉ chấp nhận đúng dãy `0..total`.** Nối "những dòng nào có" sẽ âm thầm
  giao một cuốn truyện thiếu chương dưới trạng thái `completed`.
- **`GET /api/stories/:id` trả một cửa sổ ký tự**, cắt bằng `substr` trong SQL.
  Trả nguyên `original_text` là ~15MB JSON mỗi lần mở trang chi tiết để hiển thị
  20.000 ký tự.

## Bàn giao sang app làm video

Đích là `apps/video-flow`. Nó nhận **screenplay markdown**: một chuỗi heading
`# Cảnh N`, mỗi heading là một cảnh. Đó là *hợp đồng giao tiếp*, không phải kiểu
trình bày — `parse_blocks` của nó cắt tài liệu theo heading.

| Đường | Cách dùng |
|---|---|
| REST | `GET /api/stories/:id/export?format=screenplay&scene_chars=900` (kèm `Content-Disposition`, tải về file) |
| UI | Trang chi tiết truyện → “Xuất sang app làm video” |
| MCP | `rs_story_export` — phân trang theo cảnh, đồng thời ghi bản đầy đủ ra `~/.senclaw/space-app-data/rewrite-story/exports/` |

Rồi bên Video Flow: `vf_project_create` → `vf_video_create` (chốt orientation) →
`vf_pipeline_create(script=<screenplay>, mode="production")`.

Format khác: `json` (cảnh có cấu trúc, cho mini app bất kỳ), `markdown` (bản đọc),
`txt` (toàn văn).

**Hai app không gọi trực tiếp được nhau.** Bridge của daemon khai báo `mcp.call`
và `space.rest` nhưng chưa implement — `mcp.call` trả thẳng `status: "pending"`,
"not enabled yet". Nên cầu nối là *file* hoặc *agent* (agent giữ cả hai MCP
server). Skill `rewrite-story-to-video` viết sẵn quy trình đó.

Cảnh được cắt bằng chính bộ chia hybrid tiếng Việt, ở kích thước nhỏ hơn nhiều
(`scene_chars`, mặc định 900 ≈ 8 giây video) — bộ chia vốn ngắt ở chỗ chuyển
cảnh, đúng chỗ một cảnh nên kết thúc.

Đã kiểm thật: xuất 3 cảnh đầu của một truyện đã viết lại → `POST /api/script/parse`
của video-flow → 3 scene, tự trích nhân vật (`Lâm Bắc`, `Instructor`…), sinh
prompt ảnh + shot type + thời lượng.

## Kích thước chunk quyết định app có thực sự viết lại hay không

Đây là điều quan trọng nhất trong repo này. Đo trên bridge của SenClaw
(`ag/gemini-pro-agent`):

| chunk nguồn | bản trả về | tỉ lệ |
|---|---|---|
| 5531 ký tự | 2246 | 0,41 |
| 4143 ký tự | 2277 | 0,55 |
| 2261 ký tự | 2337 | **1,03** |

Model trả về **một lượng văn gần như cố định (~2300 ký tự)** bất kể đưa vào bao
nhiêu. Cho nên chunk lớn hơn ngưỡng đó **không** cho bản viết lại dài hơn — nó
cho một bản **tóm tắt**, âm thầm, với `finish = "stop"` nên không có lỗi nào báo.

Hai tham số phải đi cùng nhau:

- `llm::MAX_CHUNK_CHARS = 2000` — trần đo được, không phải suy đoán.
- `max_output_tokens = 32000` — độ dài trả về bám gần tuyến tính theo giá trị
  này cho tới ~32000 rồi bão hoà. Đặt 8192 cho ra tỉ lệ 0,28.

Kết quả toàn cục trên 26.000 ký tự truyện thật: **0,18 → 1,02** (chunk tệ nhất 0,98).

Nếu đổi model, hãy đo lại: nhập một truyện ngắn, chạy, rồi so `original_content`
với `rewritten_content` trong `GET /api/processes/:id/chunks`. Tỉ lệ tụt sâu dưới
1,0 nghĩa là chunk đang lớn hơn trần của model đó.

## Tốc độ

Một truyện được viết lại **tuần tự từng chunk** vì chunk *i* cần đuôi bản đã viết
lại của *i-1* để mạch văn liền. Truyện vài trăm chunk vì thế mất hàng giờ.

`parallel_chunks` (1–8, mặc định 1) đổi điều đó: chunk trong cùng một lô chạy
song song, và chunk nào có "hàng xóm trước" đang bay cùng lô thì dùng đuôi bản
**gốc** làm cầu nối thay vì đuôi bản đã viết lại. Nhanh gần tuyến tính, đổi lại
mối nối trong lô hơi kém mượt. Ở mức 1, nhánh dự phòng đó không bao giờ chạy tới,
nên kết quả giống hệt đường tuần tự — có test khoá điều đó.

`max_concurrent_processes` là số **truyện** song song, không giúp gì cho một truyện.

## Bẫy đã gặp khi chạy thật
- Huỷ một job rồi bấm "Chạy tiếp" ngay lập tức: job cũ có thể còn kẹt trong lời
  gọi model hàng chục giây. Nếu poller được phép claim lúc đó, một process sẽ có
  **hai** worker và cái sắp chết ghi đè trạng thái `cancelled` lên lượt chạy mới.
  Chặn bằng hai lớp: `Core::register_job` giữ slot **trước khi** claim row, và
  worker chỉ được ghi khi row còn là `processing`
  (`update_progress_guarded(..., only_if_running = true)`).
  Hệ quả nhìn thấy được: sau khi bấm "Chạy tiếp", tiến trình nằm ở `queued` cho
  tới khi worker cũ thoát. Đó là đúng, không phải treo.
