# 🔮 Siêu Dự Đoán — SenClaw Space App

Dự báo AI đa lĩnh vực **có kiểm chứng**: bóng đá, xổ số miền Bắc (thống kê),
thời tiết, giá vàng/tỷ giá — và một **Sổ dự đoán** ghi lại mọi forecast, tự đối
chiếu kết quả thật rồi chấm điểm Brier/accuracy/calibration công khai.

- Port: **4600** · MCP: **`predict-mcp`** (`mcp__predict-mcp__predict_*`, 15 tools)
- Thiết kế chi tiết: [docs/sieu-du-doan-app-design.md](../../docs/sieu-du-doan-app-design.md)

## Pipeline Siêu Dự Báo (Superforecasting — Tetlock)

`predict_ask` không phải một lời gọi LLM trần mà là pipeline 5 bước theo sách
**Siêu Dự Báo**: (1) Fermi phân rã câu hỏi; (2) nền tảng dữ liệu — thống kê chủ
đề + quy luật + bài học + track record; (3) **tổng hợp tin ngoài qua MCP động** (xem dưới); (4) tổng hợp theo checklist
Tetlock — outside view/base rate trước, bằng chứng thuận–nghịch, điều chỉnh từng
bước, premortem, p mịn + độ tin cậy + điều kiện cập nhật (trace đầy đủ trả về và
lưu vào sổ); (5) khi resolve, tự **rút bài học postmortem** về chủ đề (điều răn
8). Nền tảng tri thức (11 điều răn + kỹ thuật) nằm ở [src/methodology.rs](src/methodology.rs),
xem được ở tab **Tri thức** / tool `predict_method`.

## Giao diện & cấu hình

- **Theme**: nút Sáng / Tối / Theo hệ thống ở góc phải header — lưu localStorage
  (áp dụng tức thì) và đồng bộ vào settings để máy khác vẫn nhớ.
- **Nguồn tìm kiếm = MCP động, không khai địa chỉ**: app hỏi daemon
  (`GET /api/mcp-servers`) xem đang có MCP server nào, chấm điểm các công cụ tra
  cứu (ưu tiên tin tức/web/nghiên cứu, loại tool ghi/xoá) và gọi chúng qua
  JSON-RPC. Chế độ **Tự động** dùng nguồn điểm cao nhất đang chạy; hoặc chọn tay
  nhiều nguồn trong tab Cài đặt. Tham số truy vấn của từng tool được tự dò từ
  `inputSchema`, kết quả trích theo nhiều shape (`evidence`/`results`/`articles`…).
- **Nguồn dữ liệu nằm trong từng chủ đề**, không phải cài đặt chung: mở chủ đề →
  thẻ *Nguồn dữ liệu của chủ đề này* để đổi **địa điểm bất kỳ** (gõ tên, toạ độ
  lấy qua Open-Meteo Geocoding keyless) hoặc **giải bóng đá bất kỳ** theo id
  TheSportsDB. Engine chỉ fetch đúng những nguồn các chủ đề đang dùng. Đổi nguồn
  mà tên chủ đề còn ở dạng mặc định thì tên tự đổi theo (kéo cả domain sổ điểm).
  Tab Cài đặt chỉ còn chọn nguồn MCP tìm kiếm + bảng nguồn đang hoạt động.
- **Tri thức sửa được**: tab Tri thức mặc định seed từ sách Siêu Dự Báo, bấm
  *Sửa tri thức* để đổi nguồn, **checklist bơm vào mọi lần tổng hợp dự đoán**,
  pipeline, nguyên tắc, kỹ thuật; *Về mặc định* khôi phục bản gốc. Phần để trống
  tự lấy lại mặc định nên tri thức không bao giờ rỗng. Agent dùng
  `predict_method {update|reset}`.

## Chủ đề = phần TĨNH + phần ĐỘNG

Mỗi chủ đề gồm hai phần tách bạch:

- **TĨNH — bối cảnh cố định**: vị trí/thành phố, đối tượng theo dõi, thông số
  không đổi… lưu dạng `{tên: giá trị}`; kèm **tài liệu hướng dẫn phân tích
  (`guide`)** — chính là prompt riêng của chủ đề, được bơm vào mọi lần AI phân
  tích, rút quy luật và dự đoán. Chủ đề connector tự ghi `vị trí` / `giải` vào
  đây khi tạo hoặc khi bạn đổi nguồn.
- **ĐỘNG — dữ liệu đầu vào để dự đoán**: ngày, giờ, nhiệt độ, gió, giá… chính là
  `fields` (kind: text/number/date/bool), nhập tay hoặc import theo thời gian.
- **TÀI LIỆU — thông tin ngoài số liệu**: ghi chú, tin tức, giải thích bối cảnh…
  mỗi tài liệu có thể gắn **theo ngày** (`date`, khớp bản ghi cùng ngày) và/hoặc
  **theo giá trị/trường** (`ref`). Tài liệu được đưa vào mọi lần AI phân tích,
  rút quy luật và dự đoán — model được dặn coi chúng là bằng chứng ngang hàng số
  liệu và dùng để giải thích bất thường (ví dụ: lưu tin "sương giá Brazil
  26/07" → dự báo giá cafe cộng thêm +15% và trích đúng tin đó làm lý do).

Chế độ ✨ Tự do sẽ để AI tự tách hai phần này từ mô tả của bạn (ví dụ "trồng rau
ở Đà Lạt, ghi nhiệt độ/độ ẩm/gió mỗi sáng để dự đoán sương muối" → tĩnh: Địa
điểm=Đà Lạt, Thời điểm đo=Buổi sáng; động: Ngày/Nhiệt độ/Độ ẩm/Sức gió/Có sương
muối; guide: điều kiện hình thành sương muối và cạm bẫy khi phân tích).

## Build chủ đề — tự do, không gò bó

Nút **Build chủ đề** có hai chế độ: **✨ Tự do** (mặc định) — mô tả mong muốn bằng
lời thường ("theo dõi doanh số shop, dự đoán ngày bán chạy"), AI thiết kế tên +
trường dữ liệu + câu hỏi mẫu, bạn **sửa thoải mái** trước khi tạo (hoặc bỏ qua AI
và tự điền trường); và **Template có sẵn** — lối tắt cho 4 chủ đề có connector tự
nạp dữ liệu (vàng/tỷ giá, thời tiết, XSMB, bóng đá). Chủ đề đã tạo **sửa được**
bất cứ lúc nào (nút Sửa trên dashboard): đổi tên, mô tả, thêm/bớt/đổi kiểu trường —
đổi tên tự chuyển domain của các dự đoán cũ nên sổ điểm không đứt gãy.

## Chủ đề tùy chỉnh — "form chung" tự thiết lập

Ngoài các lĩnh vực dựng sẵn, tab **Chủ đề** cho phép dự đoán *bất kỳ thứ gì có
dữ liệu*: tự định nghĩa chủ đề + trường (chữ/số/ngày/có-không) → nhập tay hoặc
import CSV/JSON → tìm kiếm → **AI phân tích** dữ liệu → **AI rút quy luật** siêu
dự đoán từ lịch sử (kèm độ tin cậy, quy luật user thêm tay được giữ riêng) →
hỏi **"điều X có xảy ra không?"** nhận `p_yes` + lý do kiểu superforecaster
(base rate → điều chỉnh theo quy luật, ít dữ liệu thì giữ gần 0.5). Mọi câu hỏi
tự vào sổ với domain `topic:<tên>` — nghĩa là mỗi chủ đề có accuracy/Brier riêng.

## Bốn lĩnh vực dựng sẵn + sổ điểm

| Lĩnh vực | Cách dự đoán | Tự resolve |
|---|---|---|
| ⚽ Bóng đá | Elo (ClubElo, ~600 CLB) + Poisson → 1X2, tỷ số, Tài/Xỉu 2.5, BTTS; AI chỉ *diễn giải* số model | Kết quả trận từ TheSportsDB |
| 🎰 Xổ số | **Thống kê trung thực** (tần suất, lô gan, đầu–đuôi trên ~7500 kỳ); "chốt số" là giải trí, disclaimer cứng trong code, xác suất trúng thật ~24%/số | Kỳ quay hàng ngày |
| 🌦 Thời tiết | Open-Meteo 7 ngày, 10 thành phố VN + lời khuyên AI | Archive Open-Meteo (mưa thật ≥1mm) |
| 🪙 Vàng & tỷ giá | Chuỗi giá tích lũy mỗi giờ → SMA 1d/7d, momentum, nhãn xu hướng; **không phải lời khuyên đầu tư** | (mô tả xu hướng, không ledger tự động) |

**Brier score**: 0 = hoàn hảo, 2 = sai hoàn toàn. Tab *Sổ dự đoán* có bảng
calibration — nhóm dự đoán 70% tự tin lý tưởng phải đúng ~70%.

## Nguồn dữ liệu (Phase 1 — 100% keyless, verified 2026-07-27)

ClubElo · TheSportsDB (key test `3`) · [dataset XSMB](https://github.com/khiemdoan/vietnam-lottery-xsmb-analysis)
(cập nhật daily qua GitHub Actions) · Open-Meteo · gold-api.com · open.er-api.com.

Chưa có (cần key miễn phí, phase sau): football-data.org (fixtures chính thống),
API-Football (V-League), vnappmob (vàng SJC nội địa).

## Chạy dev

```bash
cargo run -p predict                 # backend :4600 (tự backfill dữ liệu sau ~3s)
cd apps/predict/web && npm run dev   # UI dev :5173 (proxy /api → :4600)
cargo test -p predict                # 38 unit tests
```

## Đóng gói cài vào SenClaw

```bash
apps/predict/scripts/pack.sh         # → apps/predict/predict-app.zip
```

## Kiến trúc

```
src/
  main.rs      axum :4600, serve web_dist, spawn scheduler
  engine.rs    ensure_* fetchers (staleness-aware) + auto-resolver + loop 10'
  fetch.rs     ClubElo/XSMB/Open-Meteo/gold/fx/TheSportsDB (keyless)
  football.rs  Elo→1X2 + Poisson score matrix (thuần logic, có test)
  lottery.rs   parse CSV + tần suất/lô gan/đầu-đuôi + disclaimer cứng
  market.rs    SMA/momentum/trend + disclaimer đầu tư
  ledger.rs    Brier đa lớp + argmax correctness
  llm.rs       diễn giải qua bridge llm.request (không bịa số, chặn finish=length)
  api.rs       REST + value builders dùng chung
  mcp.rs       predict-mcp 15 tools (JSON-RPC + SSE)
  db.rs        SQLite ~/.senclaw/apps/predict/predict.db
  timeutil.rs  civil date ↔ unix, giờ VN (UTC+7)
```

Scheduler: mỗi 10' chạy `ensure_*` (Elo 7 ngày/lần, fixtures 6h, gold 1h, thời
tiết 3h, XSMB sau 18:35 VN retry 20') + resolve sổ. `POST /api/tick` chạy ngay.
