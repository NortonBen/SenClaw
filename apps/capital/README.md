# Nguồn Vốn — SenClaw Capital Space App

Quản lý **nguồn vốn** hoàn toàn cục bộ (SQLite, không kết nối ngân hàng, không chuyển
tiền thật): các nguồn vốn, sổ cái giải ngân/trả nợ, lịch trả nợ tự sinh, phân bổ vốn
theo mục đích, dashboard và AI phân tích cơ cấu vốn qua bridge SenClaw.

## Tính năng

- **Nguồn vốn** — 8 loại: vốn chủ sở hữu (`equity`), vốn góp NĐT (`investor`), vay ngân
  hàng (`bank_loan`), hạn mức tín dụng quay vòng (`credit_line`), vay cá nhân
  (`personal_loan`), trái phiếu (`bond`), tài trợ (`grant`), khác (`other`). Mỗi nguồn có
  tổng cam kết/hạn mức, lãi suất %/năm (cố định/thả nổi), ngày bắt đầu/đáo hạn.
- **Sổ cái giao dịch** — `disburse` (giải ngân/nhận vốn), `repay_principal` (trả gốc),
  `repay_interest` (trả lãi), `fee` (phí). Dư nợ = disburse − repay_principal;
  `credit_line` trả gốc thì hạn mức khả dụng hồi lại (quay vòng).
- **Lịch trả nợ** — sinh tự động theo 3 phương pháp: **niên kim** (annuity — tổng trả mỗi
  kỳ bằng nhau), **gốc chia đều** (equal_principal), **lãi định kỳ + gốc cuối kỳ**
  (interest_only). Chu kỳ tháng/quý/6 tháng/năm; kỳ cuối tự hấp thụ sai số làm tròn nên
  tổng gốc khớp tuyệt đối. Đánh dấu "đã trả" tự ghi giao dịch gốc + lãi vào sổ cái. Sinh
  lại lịch chỉ thay các kỳ chưa trả.
- **Phân bổ vốn** — gắn giải ngân vào mục đích/dự án, theo dõi đã rót so với ngân sách.
- **Dashboard** — dư nợ vay, vốn chủ đã góp, còn rút được, lãi đã trả, lãi suất nợ bình
  quân gia quyền, hệ số D/E, kỳ trả nợ 30 ngày tới, kỳ quá hạn, dòng tiền 12 tháng.
- **AI phân tích** — `capital_analyze` gửi số liệu dashboard qua bridge SenClaw
  (`llm.request`), trả về nhận định rủi ro thanh khoản/cơ cấu vốn kèm lưu ý "phân tích
  tham khảo".

## Chạy

```bash
cargo run -p capital            # backend + UI tĩnh trên http://localhost:4620
cd apps/capital/web && npm run dev   # UI dev (proxy /api → :4620)
cargo test -p capital           # 34 unit tests (finance + db + insight + goals + mcp)
```

Dữ liệu: `~/.senclaw/apps/capital/capital.db` (đổi bằng `SENCLAW_DATA_DIR`). Port đổi
bằng `PORT`.

## Smart: đánh giá & phân tích hỗ trợ

- **Đánh giá sức khoẻ vốn** (`/api/insight`, tool `capital_evaluate`) — rule engine
  cục bộ, tức thời, giải thích được: điểm 0–100 + hạng A/B/C/D + phát hiện theo mức độ
  (good/warn/crit) trên 8 tiêu chí: kỳ quá hạn, thanh khoản 30 ngày so với nguồn còn rút
  được, đòn bẩy D/E, chi phí vốn + khoản vay đắt bất thường (ứng viên đảo nợ), tập trung
  chủ nợ, áp lực đáo hạn 90 ngày, hạn mức dùng >90%, nợ chưa có lịch trả.
- **Mô phỏng what-if** (`/api/simulate`, tool `capital_simulate`) — không ghi sổ:
  `new_loan` (vay thêm → kỳ trả đầu, tổng lãi, so sánh trước/sau dư nợ · D/E · lãi suất
  bq · điểm sức khoẻ · nghĩa vụ theo tháng 12 tháng) và `early_repay` (trả trước hạn →
  lãi tiết kiệm ước tính + trước/sau).
- **AI phân tích** nhận cả dashboard lẫn kết quả rule engine — LLM diễn giải phát hiện
  theo thứ tự nghiêm trọng, không tự tính lại số.
- **Mục tiêu & kế hoạch** (`/api/goals*`, tools `capital_goal_*`) — 5 loại mục tiêu đo
  TỰ ĐỘNG từ sổ cái (giảm dư nợ, tất toán khoản vay, tăng vốn chủ, huy động vốn, tăng dự
  phòng); baseline chụp lúc tạo; đánh giá phát triển liên tục: % tiến độ so với % thời
  gian đã trôi → on_track/behind/at_risk/achieved/overdue + tốc độ cần mỗi tháng. Lên kế
  hoạch: AI soạn bước qua bridge (JSON strict) với fallback chia mốc tháng/quý tự động;
  bước tay + bước đã xong không bị ghi đè khi sinh lại.
- **Phân tích sử dụng nguồn tiền** (`/api/report/usage`, tool `capital_usage`) — tiền đã
  dùng vào đâu: theo phân bổ (share %, so ngân sách, cờ vượt), phần chưa phân loại, mức
  tận dụng và vốn nhàn rỗi từng nguồn, kèm tín hiệu cảnh báo.
- **Đánh giá từng nguồn tiền** (`/api/report/source-ratings`, tool `capital_source_rate`)
  — scorecard 0–100 + hạng A/B/C/D + verdict cho mỗi nguồn: chi phí so mặt bằng sổ, kỷ
  luật trả đúng hạn (từ lịch sử `paid_date`), đáo hạn gần, lãi thả nổi, room hạn mức,
  thiếu lịch trả; nguồn vốn chủ chấm mức thực hiện cam kết góp.

## MCP — `capital-mcp` (25 tools, prefix `capital_`)

`capital_status` · `capital_dashboard` · `capital_source_add/list/get/update` ·
`capital_tx_add/list` · `capital_schedule_generate/list/pay` ·
`capital_alloc_add/list` · `capital_report_cashflow` ·
`capital_goal_add/list/update/plan/steps` · `capital_usage` · `capital_source_rate` ·
`capital_evaluate` · `capital_simulate` · `capital_analyze` · `capital_activity`

Full identifier từ Claude Code: `mcp__capital-mcp__capital_<verb>` (HTTP+SSE tại
`/api/mcp/sse`, message tại `/api/mcp/message`).

## Đóng gói

```bash
apps/capital/scripts/pack.sh    # → release/ + capital-app.zip (cài trong SenClaw)
```
