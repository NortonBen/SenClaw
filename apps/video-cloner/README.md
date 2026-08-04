# Video Cloner (SenClaw Space App)

Phân tích một video có sẵn và sinh ra bộ prompt JSON kỹ thuật để **tái tạo lại
video đó bằng Veo 3**. Video được cắt thành các phân đoạn đúng 8 giây; mỗi phân
đoạn là một dòng JSON mô tả nhân vật, bối cảnh, máy quay, âm thanh và lời thoại,
với ID nhân vật/giọng nói bị khóa cố định để mọi đoạn đồng nhất tuyệt đối.

Port từ ứng dụng AI Studio "VeoPrompt Pro / từ 2 nhân vật" (React + Gemini chạy
hoàn toàn trong trình duyệt) sang Space App Rust + React.

- Cổng: **4480**
- MCP: `video-cloner-mcp` (11 tool, tiền tố `vc_`)
- Skills: `video-cloner-run`, `video-cloner-manage`
- Persona: `video-director`

## Làm được gì

- **Nguồn video: tải lên tay hoặc dán link YouTube.** Tab "YouTube" tải video
  về (qua `yt-dlp`) rồi tạo dự án như video tải tay.
- Giữ nguyên phong cách gốc, hoặc đổi sang phong cách khác (7 preset + tự nhập).
- Thay nhân vật chính bằng mô tả chữ **hoặc ảnh mẫu** (ảnh được gửi kèm cho model).
- Thay bối cảnh, hoặc để trống cho AI tự sáng tạo bối cảnh hợp phong cách.
- Thay lời thoại bằng câu viral tuỳ ý; không có thì app **không** tự trích phụ đề.
- Thanh **độ tương đồng hình ảnh** 0–100% và chế độ **"AI tự do sáng tạo"**.
- Sửa hàng loạt: đổi tên nhân vật và ép giọng nam/nữ, tự đồng bộ `voice_id` /
  `audio_markers` / `voice_marker` trên mọi đoạn.
- Xuất `.txt` một dòng JSON mỗi đoạn, dán thẳng vào Veo 3.
- **Lịch sử & khôi phục**: mỗi lượt bóc tách được lưu lại (model, temperature, số
  đoạn sinh ra, output thô nguyên vẹn), và app tự chụp một điểm khôi phục ngay
  **trước** mỗi thao tác ghi đè — lỡ tay vẫn lùi lại được.

## Khác biệt so với bản gốc

| | Bản AI Studio | Space App này |
|---|---|---|
| Gọi model | Từ trình duyệt | Từ backend Rust |
| Lưu trữ | Không — reload là mất | SQLite, video giữ trên đĩa |
| Video lớn | Base64 inline, hỏng khi >20 MB | Tự chuyển sang Gemini Files API, upload một lần rồi dùng lại |
| Chờ đợi | Request HTTP treo cho đến khi xong | Job nền + WebSocket + poll |
| Lỡ tay | Mất vĩnh viễn | Điểm khôi phục tự động trước mỗi thao tác ghi đè |
| Dùng bởi agent | Không | 14 MCP tool + 2 skill + persona |

## Dữ liệu được lưu những gì

| Bảng | Nội dung |
|---|---|
| `projects` | Phiên bóc tách: video, phong cách, nhân vật/bối cảnh/lời thoại thay thế, độ tương đồng |
| `scenes` | Nội dung đã bóc tách: từng đoạn JSON 8 giây, kèm `job_id` của lượt sinh ra nó |
| `jobs` | Từng lượt chạy: mode, model, temperature, số đoạn, lỗi, **output thô đầy đủ** |
| `snapshots` | Điểm khôi phục: bản sao toàn bộ scene ngay trước mỗi thao tác ghi đè (giữ 20 bản gần nhất/dự án) |

Điểm khôi phục được tạo tự động trước ba thao tác phá huỷ: phân tích lại từ đầu,
làm lại đoạn cuối, và sửa hàng loạt. Bản thân việc khôi phục cũng tạo một điểm
mới, nên quay ngược lại được. Video gốc nằm ở `media/`, không nằm trong DB.

## Xuất & bàn giao sang app sinh video

Kịch bản ở đây là prompt Veo 3: JSON lồng nhau, hợp với đúng một bộ sinh video.
Nên mọi đường xuất đều kèm **ba dạng** của cùng nội dung:

- `veo` — JSON gốc, mỗi đoạn một dòng, dán thẳng vào Veo 3;
- `image_prompt` / `video_prompt` — văn xuôi đã làm phẳng, cho bất kỳ bộ sinh
  video nào ăn prompt chữ. Khung hình và diễn biến tách riêng: bộ sinh ảnh mà
  bị kể về chuyển động sẽ render ra vệt mờ;
- Markdown — kịch bản cho người hoặc agent đọc.

| Đường ra | Cách dùng |
|---|---|
| Tải `.json` / `.md` | Nút trong panel **Xuất & Bàn giao**, hoặc `GET /api/projects/:id/export/bundle?download=true` |
| Thư mục chia sẻ | Ghi vào `~/.senclaw/exports/video-cloner/` để app khác tự đọc (`VIDEO_CLONER_EXPORT_DIR` để đổi) |
| Wiki SenClaw | Đăng thành trang git-backed, agent đọc lại bằng `wiki_read` / `wiki_search` |
| Bàn giao thẳng | Tạo project + nhân vật + video + toàn bộ scene bên **video-flow** |

### Bàn giao sang video-flow — ba cái bẫy

1. **Sau khi bàn giao, đừng chạy `pipeline/create` bên video-flow.** Agent
   `script_parser` của nó chạy `DELETE FROM scene WHERE video_id = ?` rồi dựng
   lại từ đầu bằng LLM — tức là vứt sạch đúng những đoạn vừa nhận. Hãy dùng
   `workflow` hoặc các bước `steps/*`.
2. **Tạo nhân vật qua REST, không qua MCP của video-flow.** `vf_character_create`
   viết hoa `entity_type`, trong khi cột có `CHECK` trên giá trị viết thường nên
   insert sẽ hỏng. `POST /api/projects/:id/characters` giữ nguyên giá trị.
3. **video-flow cần prompt tiếng Anh** (nó đưa thẳng cho Veo 3), còn app này cố
   ý sinh tiếng Việt. Bật `translate` để dịch phần hình ảnh; `narrator_text`
   luôn giữ nguyên tiếng gốc vì nó dùng để lồng tiếng.

Ánh xạ sang enum của video-flow (`shot_type`, `camera_movement`) nhận cả tiếng
Anh lẫn tiếng Việt và luôn rơi về giá trị an toàn khi không nhận ra — các cột đó
có ràng buộc `CHECK`, đoán bừa là insert hỏng.

## Vì sao app này không dùng bridge `llm.request`

Các Space App khác gọi model qua bridge của daemon. App này **không**, vì bridge
chỉ nhận `{system, prompt, maxTokens, profile}`:

- **Không có đường truyền video/ảnh.** Video chính là đầu vào của app này.
- **Không có `temperature`.** Thanh "độ tương đồng hình ảnh" được quy đổi trực
  tiếp thành temperature (`0.1 + (1 - similarity/100) * 0.7`); bỏ nó đi thì cái
  núm sáng tạo trở thành vô nghĩa.

Nên backend gọi thẳng Generative Language API bằng API key riêng của app.

Riêng phần **dịch prompt** thì vẫn đi qua bridge (`src/llm.rs`) — nó chỉ là văn
bản thuần, nên dùng model đã cấu hình sẵn trong daemon thay vì đốt quota Gemini.

Lưu ý: bridge quảng cáo `space.rest` nhưng **không có handler**, nên việc ghi
wiki gọi thẳng `PUT {SENCLAW_BASE_URL}/api/wiki/file` của daemon.

## Tải video từ YouTube

Tab **YouTube** ở khung "Nguồn Video" nhận link, tải video về `media/` bằng
`yt-dlp` rồi tạo dự án. Tải chạy nền (job trong bộ nhớ + WebSocket + poll dự
phòng), nên video dài không treo request.

- **Cần `yt-dlp`** (và `ffmpeg` để ghép luồng). Thiếu thì UI hiện hướng dẫn cài
  thay vì ô nhập:
  ```bash
  brew install yt-dlp
  ```
- Tải tối đa **720p** (`VIDEO_CLONER_YOUTUBE_MAX_HEIGHT`) và tối đa **500M**
  (`VIDEO_CLONER_YOUTUBE_MAX_FILESIZE`) để không đầy đĩa.
- **Chặn "confirm you're not a bot":** YouTube giới hạn tải ẩn danh sau vài
  lượt. Trỏ yt-dlp vào cookies trình duyệt đang đăng nhập để vượt:
  ```bash
  VIDEO_CLONER_YTDLP_COOKIES=chrome   # safari | firefox | edge | brave...
  ```
- yt-dlp hỗ trợ hàng nghìn trang, không chỉ YouTube — ô nhập nhận mọi URL
  http/https. URL được truyền dạng tham số (sau `--`), không qua shell.
- MCP: `vc_youtube_import` (trả `import_id`) + `vc_youtube_status`. `vc_status`
  có cờ `youtube_download` cho biết máy đã có yt-dlp chưa.

## Cấu hình

Nhập Gemini API key bằng nút **Cài đặt** trong giao diện web (lưu vào
`app_settings`, đổi được mà không cần khởi động lại daemon).

Biến môi trường dùng làm fallback: `VIDEO_CLONER_GEMINI_API_KEY`,
`GEMINI_API_KEY`, `GOOGLE_API_KEY`.

Dữ liệu nằm ở `~/.senclaw/space-app-data/video-cloner/` (`app.sqlite` +
`media/`) — **ngoài** thư mục cài đặt, vì cài lại app sẽ xoá sạch thư mục đó.
Đổi bằng `VIDEO_CLONER_DATA_DIR`.

## Chạy khi phát triển

```bash
# backend (từ gốc repo)
cargo run -p video-cloner

# frontend, hot reload, proxy sang 4480
cd apps/video-cloner/web && npm install && npm run dev
```

## Đóng gói

```bash
apps/video-cloner/scripts/pack.sh
# -> apps/video-cloner/video-cloner-app.zip
```

## Lưu ý vận hành

- **Mỗi lượt chạy chỉ sinh một đoạn 8 giây.** Video 40 giây cần 1 lượt `start`
  + 4 lượt `continue`.
- **`mode: "start"` xoá mọi đoạn đã tạo.** Muốn thêm đoạn thì dùng `continue`.
- **Bối cảnh để trống ≠ giữ bối cảnh gốc** — AI sẽ tự nghĩ bối cảnh mới. Muốn
  giữ nguyên thì phải mô tả lại nó.
- Video lớn được upload lên Gemini một lần và dùng lại trong 40 giờ, nên lượt
  đầu chậm hơn hẳn các lượt sau.
- Một dự án chỉ chạy được một lượt phân tích tại một thời điểm; các đoạn phải
  được nối theo thứ tự.
