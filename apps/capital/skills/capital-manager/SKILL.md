---
name: capital-manager
description: >-
  Quản lý nguồn vốn qua app Nguồn Vốn: ghi nhận các nguồn vốn (vốn chủ, vốn góp NĐT,
  vay ngân hàng, hạn mức tín dụng, vay cá nhân, trái phiếu, tài trợ), ghi sổ giải ngân /
  trả gốc / trả lãi / phí, sinh lịch trả nợ (niên kim, gốc đều, lãi định kỳ + gốc cuối kỳ),
  phân bổ vốn theo dự án, xem dashboard dư nợ / D-E / dòng tiền và nhờ AI phân tích cơ cấu
  vốn. Dùng khi người dùng nói về nguồn vốn, khoản vay, giải ngân, trả nợ, dư nợ, lịch trả
  nợ hay cơ cấu vốn. Mọi con số lấy từ tool — không tự tính nhẩm lãi hay dư nợ.
triggers:
  - nguồn vốn
  - quản lý vốn
  - khoản vay
  - vay ngân hàng
  - hạn mức tín dụng
  - giải ngân
  - trả nợ
  - lịch trả nợ
  - dư nợ
  - trả gốc
  - trả lãi
  - lãi vay
  - vốn chủ sở hữu
  - vốn góp
  - phân bổ vốn
  - cơ cấu vốn
  - capital source
  - loan schedule
  - amortization
---

# capital-manager

Dùng MCP server `capital-mcp` của app **Nguồn Vốn**. App chỉ **ghi sổ cục bộ** — không
kết nối ngân hàng, không chuyển tiền thật. "Thanh toán" một kỳ trả nợ nghĩa là *ghi nhận*
việc con người đã trả, không phải thực hiện giao dịch.

## Chọn công cụ

- **`mcp__capital-mcp__capital_dashboard`** — LUÔN gọi trước khi trả lời câu hỏi tổng
  quan ("tình hình vốn thế nào", "còn nợ bao nhiêu"): trả về vốn chủ đã góp, dư nợ vay,
  hạn mức khả dụng, lãi đã trả, lãi suất bình quân gia quyền, hệ số D/E, kỳ trả nợ 30
  ngày tới, kỳ quá hạn, dòng tiền 12 tháng và toàn bộ nguồn.
- **`mcp__capital-mcp__capital_source_add`** — thêm nguồn vốn. `kind`: `equity` (vốn chủ)
  · `investor` (vốn góp NĐT) · `bank_loan` (vay ngân hàng) · `credit_line` (hạn mức tín
  dụng quay vòng — trả gốc thì rút lại được) · `personal_loan` · `bond` · `grant` ·
  `other`. `total_amount` = tổng cam kết/hạn mức, `interest_rate` = %/năm.
- **`mcp__capital-mcp__capital_source_list` / `capital_source_get` /
  `capital_source_update`** — xem và sửa nguồn. Đóng nguồn bằng `status: "closed"`
  (nguồn đóng bị loại khỏi mọi chỉ số dashboard).
- **`mcp__capital-mcp__capital_tx_add`** — ghi sổ cái. `kind`: `disburse` (giải ngân /
  nhận vốn về — với vốn chủ đây là "góp vốn") · `repay_principal` (trả gốc) ·
  `repay_interest` (trả lãi) · `fee` (phí). Gắn `alloc_id` khi giải ngân cho một dự án.
  Dư nợ = tổng disburse − tổng repay_principal, tự tính — đừng cộng tay.
- **`mcp__capital-mcp__capital_schedule_generate`** — sinh lịch trả nợ cho một nguồn.
  `method`: `annuity` (niên kim — tổng trả mỗi kỳ bằng nhau, mặc định) ·
  `equal_principal` (gốc chia đều, lãi giảm dần) · `interest_only` (trả lãi định kỳ,
  gốc trả một cục cuối kỳ). `freq_months`: 1 = tháng, 3 = quý. Bỏ trống `principal` =
  dư nợ hiện tại; bỏ trống `annual_rate` = lãi suất của nguồn. Sinh lại lịch chỉ thay
  các kỳ CHƯA trả.
- **`mcp__capital-mcp__capital_schedule_list`** — xem lịch; `status`: `upcoming` ·
  `overdue` · `paid`. Dùng để nhắc "sắp phải trả gì".
- **`mcp__capital-mcp__capital_schedule_pay`** — CHỈ khi người dùng xác nhận đã trả một
  kỳ. Mặc định tự ghi giao dịch trả gốc + lãi vào sổ cái.
- **`mcp__capital-mcp__capital_alloc_add` / `capital_alloc_list`** — phân bổ vốn theo
  mục đích/dự án, theo dõi đã rót (`used`) so với dự kiến (`target_amount`).
- **`mcp__capital-mcp__capital_report_cashflow`** — dòng tiền theo tháng (vào = giải
  ngân, ra = gốc + lãi + phí).
- **`mcp__capital-mcp__capital_evaluate`** — ĐÁNH GIÁ sức khoẻ vốn bằng rule engine
  (tức thời, không LLM): điểm 0–100 + hạng A/B/C/D + phát hiện good/warn/crit trên 8
  tiêu chí (quá hạn, thanh khoản 30 ngày, D/E, chi phí vốn & khoản vay đắt, tập trung
  chủ nợ, đáo hạn 90 ngày, hạn mức cạn, nợ chưa có lịch). Khi người dùng hỏi "vốn có ổn
  không / rủi ro gì" — gọi tool này TRƯỚC, trình bày phát hiện `crit` đầu tiên.
- **`mcp__capital-mcp__capital_simulate`** — MÔ PHỎNG what-if, KHÔNG ghi sổ. "Nếu vay
  thêm X thì sao?" → `scenario: "new_loan"` (amount, annual_rate, periods, method) trả
  về kỳ trả đầu, tổng lãi, và so sánh trước/sau (dư nợ, D/E, lãi suất bq, điểm sức khoẻ,
  nghĩa vụ theo tháng). "Trả trước hạn có lợi không?" → `scenario: "early_repay"`
  (source_id, amount) trả về lãi tiết kiệm ƯỚC TÍNH — nói rõ đây là ước tính đơn giản,
  số thật phụ thuộc điều khoản hợp đồng.
- **`mcp__capital-mcp__capital_analyze`** — AI phân tích cơ cấu vốn qua bridge SenClaw
  (đã kèm sẵn kết quả rule engine trong ngữ cảnh). Kết quả luôn kèm lưu ý "phân tích
  tham khảo, không phải tư vấn tài chính chuyên nghiệp" — giữ nguyên lưu ý đó khi trả lời.
- **`mcp__capital-mcp__capital_goal_add` / `capital_goal_list` / `capital_goal_update`**
  — mục tiêu tài chính đo TỰ ĐỘNG từ sổ. `kind`: `reduce_debt` (dư nợ về ≤ target) ·
  `payoff_source` (tất toán 1 nguồn, cần source_id) · `raise_equity` · `raise_funding` ·
  `build_reserve`. `capital_goal_list` trả về đánh giá phát triển sẵn: `eval_status`
  (on_track/behind/at_risk/achieved/overdue), % tiến độ vs % thời gian, còn thiếu bao
  nhiêu, cần bao nhiêu mỗi tháng. Người dùng hỏi "mục tiêu đến đâu rồi" → gọi tool này,
  báo mục tiêu at_risk/overdue TRƯỚC. Chỉ set status=done khi người dùng xác nhận.
- **`mcp__capital-mcp__capital_goal_plan`** — LÊN KẾ HOẠCH cho mục tiêu (AI soạn bước;
  ai=false thì chia mốc tự động). `capital_goal_steps` để thêm/đánh dấu/xoá bước.
- **`mcp__capital-mcp__capital_usage`** — PHÂN TÍCH SỬ DỤNG tiền: đã dùng vào đâu theo
  phân bổ, phần chưa phân loại, tận dụng/nhàn rỗi từng nguồn + tín hiệu. Hỏi "tiền dùng
  vào đâu / có hiệu quả không" → tool này.
- **`mcp__capital-mcp__capital_source_rate`** — ĐÁNH GIÁ TỪNG NGUỒN: scorecard 0–100 +
  hạng A–D + yếu tố cộng/trừ (chi phí, kỷ luật trả hạn, đáo hạn, thả nổi, room hạn mức).
  Hỏi "nguồn nào tốt / nên bỏ nguồn nào / nên đảo nợ khoản nào" → tool này; trình bày
  verdict + 1-2 yếu tố chính, không đọc cả danh sách.

## Cách trả lời

- **Kết luận trước**: "Dư nợ hiện tại 1,2 tỷ trên 3 khoản vay, kỳ gần nhất 15/08 phải trả
  45 triệu" — rồi mới tới bảng chi tiết.
- **Số nào cũng từ tool.** Không tự tính lãi niên kim, không tự cộng dư nợ — gọi
  `capital_dashboard` / `capital_schedule_generate` để máy tính. Câu hỏi "nếu… thì sao"
  → `capital_simulate`, đừng tự ước lượng.
- **Ghi sổ xong phải xác nhận lại con số** (đọc lại từ kết quả tool, ví dụ dư nợ mới).
- **Có kỳ quá hạn thì nói ngay đầu câu trả lời**, kể cả khi người dùng hỏi chuyện khác.
- Ngày dùng định dạng `YYYY-MM-DD` khi gọi tool; hiển thị cho người dùng dạng dd/mm/yyyy.
