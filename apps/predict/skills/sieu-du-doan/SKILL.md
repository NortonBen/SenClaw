---
name: sieu-du-doan
description: Chủ đề dự đoán tùy chỉnh (form + nhập/import/tìm dữ liệu + AI phân tích + rút quy luật + hỏi "X có xảy ra không?"), dự đoán bóng đá (Elo+Poisson), kết quả & thống kê xổ số miền Bắc, dự báo thời tiết, giá vàng/tỷ giá và Sổ dự đoán tự chấm điểm Brier. Dùng khi người dùng muốn theo dõi & dự đoán bất kỳ thứ gì có dữ liệu, hỏi kèo/tỷ số, xổ số/lô gan/chốt số, thời tiết, giá vàng, hoặc ghi & kiểm chứng một dự đoán.
---

# Siêu Dự Đoán — dự báo AI có kiểm chứng

App chạy trên `http://127.0.0.1:4600`. MCP server: `predict-mcp`
(công cụ `mcp__predict-mcp__predict_*`). Nếu tool chưa thấy trong roster, tra bằng
ToolSearch `select:mcp__predict-mcp__predict_status` — KHÔNG tự chế tên ngắn.

Dữ liệu Phase 1 hoàn toàn keyless: ClubElo (Elo bóng đá), TheSportsDB (lịch/kết
quả), dataset XSMB cập nhật hàng ngày, Open-Meteo (thời tiết), gold-api.com (XAU),
open.er-api.com (tỷ giá). Lần đầu mở app sẽ tự backfill ~7500 kỳ XSMB.

## Chủ đề tùy chỉnh — "form chung" (dự đoán bất kỳ thứ gì có dữ liệu)

Luồng chuẩn khi người dùng muốn theo dõi & dự đoán một thứ riêng (giá cafe, doanh
số, cân nặng, kèo giải phủi…):

1. Tạo chủ đề — **ba cách, ưu tiên cách tự do**:
   - **Tự do (khuyên dùng)**: `predict_topic_create {wish: "<mô tả mong muốn bằng lời>"}`
     — AI tự thiết kế tên + trường + câu hỏi mẫu rồi tạo luôn. Không ép người dùng
     nghĩ ra schema.
   - **Template có connector tự nạp dữ liệu**: `predict_topic_create {template, params}`
     — `gold` · `weather {city}` · `lottery` · `football {league}` · `blank`.
   - **Tự tay**: `{name, description, fields, static?, guide?}`.
     `fields` = trường **ĐỘNG** (dữ liệu theo thời gian):
     `[{"name":"ngày","kind":"date"},{"name":"nhiệt độ","kind":"number"}]`
     (kind: text|number|date|bool; date là YYYY-MM-DD).
     `static` = bối cảnh **TĨNH** cố định: `{"vị trí":"Đà Lạt","độ cao":"1500m"}`.
     `guide` = tài liệu hướng dẫn phân tích — prompt riêng của chủ đề, AI tuân
     thủ mỗi lần phân tích / rút quy luật / dự đoán.
   Sửa chủ đề sau đó: `POST /api/topics/:key` `{name?, description?, fields?}` —
   đổi tên tự chuyển domain sổ điểm của các dự đoán cũ, không đứt gãy track record.
2. Nạp dữ liệu: `predict_topic_data_add {topic, data}` từng bản ghi, hoặc
   `predict_topic_import {topic, csv|records}` hàng loạt (CSV dòng đầu = tên trường).
3. `predict_topic_search {topic, q}` khi cần tra lại dữ liệu.
3b. **Tài liệu ngoài số liệu**: `predict_topic_doc_add {topic, title, content, date?, ref?}`
   lưu tin tức/ghi chú/giải thích — `date` gắn với một ngày, `ref` gắn với một
   trường/giá trị. `predict_topic_docs {topic, q?}` để tra. Tài liệu tự động vào
   mọi lần phân tích, rút quy luật và dự đoán của chủ đề → khi người dùng kể một
   tin liên quan, HÃY lưu lại bằng tool này thay vì chỉ nhắc trong câu trả lời.
4. `predict_topic_analyze {topic}` — AI đánh giá bức tranh, xu hướng, chất lượng dữ liệu.
5. `predict_topic_rules {topic, derive:true}` — AI rút quy luật siêu dự đoán từ
   lịch sử (kèm độ tin cậy; rút lại sẽ thay quy luật AI cũ, quy luật user giữ nguyên).
6. `predict_ask {topic, question, due_days}` — "điều X có xảy ra không?" → `p_yes`
   + lý do, TỰ ghi sổ domain `topic:<tên>`; khi biết kết quả → `predict_resolve`.

`predict_ask` không có `topic` = dự đoán tự do (vẫn ghi sổ, domain generic).
Ít dữ liệu → AI phải giữ p gần 0.5 và nói rõ — đừng ép nó tự tin.

## Pipeline Siêu Dự Báo (sách Superforecasting — Tetlock)

`predict_ask` chạy pipeline 5 bước, KHÔNG phải một lời gọi LLM trần:

1. **Fermi phân rã** câu hỏi → câu hỏi con + truy vấn tìm tin.
2. **Nền tảng dữ liệu**: thống kê số học của chủ đề + quy luật + bài học + track record.
3. **Tổng hợp tin ngoài**: **khám phá MCP động** — hỏi daemon danh sách MCP server
   đang chạy, chấm điểm công cụ tra cứu và gọi nguồn tốt nhất (hoặc nguồn người dùng
   chọn tay ở Cài đặt). Không có nguồn nào → vẫn chạy, ghi rõ trong `evidence_note`.
   Mỗi bằng chứng có trường `source` = `<server>.<tool>` cho biết lấy từ đâu.
4. **Tổng hợp Tetlock**: outside view (base rate) → inside view (bằng chứng thuận/nghịch)
   → điều chỉnh từng bước → premortem → p mịn + độ tin cậy + điều kiện cập nhật.
   Trace đầy đủ nằm trong `trace` của kết quả — hãy trình bày lại cho người dùng
   (nhất là base rate, premortem và điều kiện cập nhật), đừng chỉ đọc mỗi con số p.
5. **Ghi sổ + postmortem**: khi `predict_resolve`, app tự rút **bài học** quy trình
   (điều răn 8) lưu vào chủ đề (tag `lesson`) và dùng cho các dự đoán sau.

`mode: "simple"` trong kết quả nghĩa là bước tổng hợp cấu trúc fail và app đã rơi
về dự đoán nhanh một-lời-gọi — vẫn hợp lệ nhưng không có trace đầy đủ.

`predict_method` trả về nền tảng tri thức (11 điều răn + kỹ thuật + pipeline +
checklist) — dùng khi người dùng hỏi "vì sao dự đoán như vậy / app dự đoán kiểu
gì". Tri thức **sửa được**: `predict_method {update: {checklist|principles|
techniques|pipeline|source}}` để cập nhật (phần bỏ trống giữ mặc định), hoặc
`{reset: true}` để về bản gốc từ sách. Checklist đã sửa áp dụng ngay cho mọi
`predict_ask` sau đó.

## Nguồn dữ liệu = cấu hình CỦA TỪNG CHỦ ĐỀ

Không có setting toàn cục cho địa điểm/giải. Mỗi chủ đề connector tự khai nguồn:
`POST /api/topics/:key/source {city}` (weather — tên bất kỳ, tự geocode qua
Open-Meteo keyless) hoặc `{league, league_name?}` (football — id TheSportsDB).
Engine chỉ fetch đúng các nguồn mà chủ đề đang dùng. Đổi nguồn khi tên chủ đề
còn ở dạng mặc định → tên tự đổi theo và domain sổ điểm được chuyển kèm.
Tab Cài đặt chỉ còn: chọn nguồn MCP tìm kiếm (tự động / thủ công) + bảng nguồn
dữ liệu đang hoạt động (chỉ để xem).

## Chọn tool theo câu hỏi

| Người dùng hỏi | Tool |
|---|---|
| "lưu tin này vào chủ đề / ghi chú bối cảnh" | `predict_topic_doc_add` / `predict_topic_docs` |
| "tạo chủ đề theo dõi X / nhập dữ liệu / import" | `predict_topic_create` / `predict_topic_data_add` / `predict_topic_import` |
| "phân tích dữ liệu X / có quy luật gì" | `predict_topic_analyze` / `predict_topic_rules {derive:true}` |
| "X có xảy ra không / khả năng bao nhiêu" | `predict_ask {topic?, question, due_days}` |
| "kèo hôm nay / trận nào hôm nay / dự đoán vòng này" | `predict_football_today {days?}` |
| "dự đoán Arsenal vs Chelsea / nhận định trận X" | `predict_football_match {home, away, article:true}` |
| "đội nào mạnh nhất / bảng Elo" | `predict_football_elo {limit?}` |
| "kết quả xổ số / XSMB hôm nay" | `predict_lottery_results` |
| "thống kê xổ số / lô gan / số nào hay về" | `predict_lottery_stats {days?}` |
| "chốt số / soi cầu" | `predict_lottery_suggest {n?, note:true}` |
| "mai có mưa không / thời tiết X" | `predict_weather {city, advice:true}` |
| "giá vàng hôm nay / tỷ giá đô" | `predict_gold_price` |
| "vàng lên hay xuống / xu hướng vàng" | `predict_gold_trend {note:true}` |
| "bản tin sáng / tổng hợp hôm nay" | `predict_brief {narrate:true}` |
| "ghi dự đoán: X, tôi tin 70%" | `predict_make {subject, p:0.7, due_days}` |
| "các dự đoán của tôi / sổ dự đoán" | `predict_list {status?}` |
| "dự đoán #id đã đúng/sai (kết quả X)" | `predict_resolve {id, outcome}` |
| "độ chính xác dự đoán / điểm Brier" | `predict_score` |

## Nguyên tắc trả lời (bắt buộc)

1. **Không bịa số.** Mọi xác suất/tỷ số/thống kê lấy nguyên từ tool. Bài nhận định
   AI (article/note) cũng chỉ diễn giải số của model.
2. **Giữ nguyên disclaimer** trong output tool:
   - Xổ số: "xổ số là ngẫu nhiên… chỉ thống kê & giải trí" — luôn chuyển tiếp cho
     người dùng, kể cả khi họ chỉ hỏi chốt số. Nêu cả `p_hit_honest` (xác suất
     trúng thật ~24%/số) khi chốt số.
   - Vàng/tỷ giá: "không phải lời khuyên đầu tư". Không khuyên mua/bán.
3. **Bóng đá nói bằng xác suất**, không khẳng định chắc thắng. `elo_matched=false`
   nghĩa là đội không có trong ClubElo (dùng Elo 1600 mặc định) — phải nói rõ độ
   tin cậy thấp.
4. **Khoe sổ điểm khi được hỏi về độ tin cậy**: `predict_score` trả accuracy +
   Brier + calibration theo domain — đó là bằng chứng, dùng nó thay vì tự nhận.
5. Dự đoán bóng đá/xổ số/thời tiết **tự resolve** khi có kết quả thật; chỉ
   `predict_resolve` tay cho dự đoán generic do người dùng tự ghi.

## Cài đặt chung

Chỉ còn URL **Search app** (`/api/settings` `{search_app_url}`) và theme. Địa điểm
thời tiết / giải bóng đá KHÔNG nằm ở đây — xem mục "Nguồn dữ liệu = cấu hình của
từng chủ đề" ở trên. Vàng SJC nội địa cần API key miễn phí — chưa có ở Phase 1.
