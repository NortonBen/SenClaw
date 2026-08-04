# Tin Tức — SenClaw News Space App

Trung tâm tin tức cá nhân chạy hoàn toàn local: thu thập bài từ nhiều nguồn
RSS/Atom — **và cả những trang không có RSS**, bằng cách quét link bài viết ngay
trong nội dung trang — gán chủ đề theo từ khóa, phát hiện xu hướng, gom bài cùng
sự kiện thành **dòng sự kiện** với timeline, và AI phân tích qua bridge SenClaw.

- Port: **4660** · MCP: **`news-mcp`** (`mcp__news-mcp__news_*`, 27 tools)
- Dữ liệu: `~/.senclaw/apps/news/news.db` (SQLite, FTS5)
- Outbound duy nhất: các feed người dùng khai + bridge LLM của daemon.
  App **chỉ đọc tin** — không đăng/gửi gì đi đâu.

## Kiến trúc

```
src/
  main.rs     — axum server (:4660) + scheduler quét feed định kỳ
  fetch.rs    — HTTP + parser RSS 2.0/Atom (quick-xml), conditional GET
                (ETag/Last-Modified), trích xuất toàn văn trang bài, và
                SCRAPER cho trang không có RSS (scan_page_articles: lọc
                <a> theo độ dài tiêu đề + dạng slug + cùng host;
                parse_page_meta: đọc Open Graph của từng bài mới)
  cluster.rs  — tokenize tiếng Việt (GIỮ dấu), gom dòng sự kiện (token
                profile overlap), phát hiện trend (n-gram spike vs kỳ trước),
                bản đồ liên kết sự kiện (trùng CỤM 2 âm tiết + lọc IDF)
  db.rs       — SQLite: sources, articles (+FTS5 remove_diacritics 2 →
                tìm không dấu), topics, stories, analyses, activity, settings
  llm.rs      — bridge AI: đánh giá bài (JSON), tóm tắt dòng sự kiện,
                điểm tin, nhận định xu hướng
  api.rs      — REST + các *_value helper dùng chung với MCP
  mcp.rs      — MCP server news-mcp (HTTP + SSE)
web/          — React 19 + AntD 6, sáng/tối/theo hệ thống (7 tab: Tổng quan /
                Tin tức / Xu hướng / Dòng sự kiện / Điểm tin AI / Chủ đề /
                Nguồn tin). Tab Dòng sự kiện có 2 chế độ: danh sách + timeline,
                hoặc BẢN ĐỒ graph (force layout, kéo-thả node, kéo nền, zoom).
```

## Hai kiểu nguồn

| | `kind='feed'` | `kind='scrape'` |
|---|---|---|
| URL trỏ vào | tài liệu RSS/Atom | trang chuyên mục/danh sách bài bình thường |
| Lấy bài bằng | parser XML | lọc `<a>` trong HTML: tiêu đề ≥24 ký tự & ≥4 từ, href dạng slug bài viết, cùng host, bỏ `<nav>/<header>/<footer>` |
| Ngày đăng, tóm tắt, ảnh | feed cung cấp sẵn | mở từng bài MỚI đọc Open Graph (`og:*`, `article:published_time`), tối đa 25 bài/chu kỳ |
| Chi phí một chu kỳ | 1 request | 1 + số bài mới (≤25), 5 request song song |

Ưu tiên feed khi trang có cả hai — rẻ hơn, ngày đăng chuẩn hơn, không phải đoán.
`news_source_discover` tự làm việc đó: dò feed trước, chỉ khi không có feed nào
mới đề xuất `kind='scrape'`. Cả hai kiểu đi tiếp cùng một đường ống (dedup theo
URL → gán chủ đề → gom dòng sự kiện), nên mọi tính năng phía sau không phân biệt
nguồn đến từ đâu.

Giới hạn thật thà: scraper đọc HTML server-render, **không chạy JavaScript**.
Trang chỉ dựng danh sách bài bằng JS sẽ quét ra rỗng — app báo lỗi rõ ở
`last_error` thay vì im lặng coi như "không có tin mới".

## Hai tầng liên kết sự kiện

| | Máy (thống kê) | AI (ngữ nghĩa) |
|---|---|---|
| Cách làm | trùng cụm 2 âm tiết giữa tiêu đề, lọc cụm phổ thông bằng IDF | đọc cả bản đồ qua bridge, map lại |
| Cho ra | cạnh xám, kèm cụm chung + % trùng | mạch chuyện (tô màu node), cạnh nét đứt tím "AI nối thêm", cạnh nghi nhiễu |
| Bắt được | cùng chủ đề, chung tên riêng | nhân quả, cùng chủ thể — thứ không trùng chữ nào |

Ngưỡng máy đặt chặt (≥2 cụm chung, hoặc 1 cụm nhưng chiếm ≥20%) để bản đồ không
thành mạng nhện; phần mở rộng ngữ nghĩa là việc của AI. Mọi id AI trả về đều
được đối chiếu với graph thật trước khi vẽ — id bịa bị loại.

Nguyên tắc phân vai: **máy đếm, AI diễn giải**. Gom sự kiện + trend là thuần
thống kê, deterministic; AI chỉ tóm tắt/diễn giải trên số liệu đã tính và mọi
prompt đều cấm bịa thêm.

## Chạy dev

```bash
cargo run -p news                    # backend :4660 (tự seed nguồn VN + quốc tế)
cd apps/news/web && npm run dev      # UI dev, proxy /api → :4660
cargo test -p news                   # 79 unit tests
```

## Đóng gói

```bash
apps/news/scripts/pack.sh            # → apps/news/news-app.zip
```

## MCP nhanh

| Nhóm | Tools |
|---|---|
| Thu thập | `news_fetch`, `news_source_discover` (AI/URL), `news_source_add/list/update/delete` |
| Đọc tin | `news_latest`, `news_search`, `news_article_get`, `news_article_content` |
| Chủ đề | `news_topic_add/list/update/delete` |
| Xu hướng | `news_trends`, `news_analyze_trends` |
| Dòng sự kiện | `news_story_list`, `news_story_get`, `news_story_brief`, `news_story_graph`, `news_analyze_graph` |
| AI | `news_analyze_article`, `news_digest`, `news_digest_history` |
| Khác | `news_status`, `news_dashboard`, `news_activity` |
