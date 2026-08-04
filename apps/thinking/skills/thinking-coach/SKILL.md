---
name: thinking-coach
description: >-
  Phân tích, đánh giá vấn đề và ra quyết định qua app Tư Duy theo hai phương pháp:
  5W (Who/What/When/Where/Why — làm rõ bản chất, nguyên nhân gốc) và 6 Mũ Tư Duy
  de Bono (Trắng dữ kiện, Đỏ cảm xúc, Đen rủi ro, Vàng lợi ích, Xanh Lá sáng tạo,
  Xanh Dương tổng kết). Đề xuất giải pháp, chấm điểm 4 tiêu chí, so sánh bằng điểm
  tổng hợp hệ thống tính và chốt quyết định kèm lý do. Dùng khi người dùng muốn
  phân tích một vấn đề, cân nhắc phương án, brainstorm giải pháp hay hỏi "nên chọn
  cái nào". Điểm số lấy từ tool — không tự chấm lại điểm tổng.
triggers:
  - 6 mũ tư duy
  - sáu chiếc mũ
  - six thinking hats
  - mũ trắng
  - mũ đen
  - mũ vàng
  - 5w
  - phân tích vấn đề
  - đánh giá vấn đề
  - làm rõ vấn đề
  - nguyên nhân gốc
  - ra quyết định
  - nên chọn phương án nào
  - so sánh giải pháp
  - đề xuất giải pháp
  - brainstorm
  - động não
  - tư duy phản biện
  - de bono
  - root cause
  - decision making
---

# thinking-coach

Dùng MCP server `thinking-mcp` của app **Tư Duy — 6 Mũ & 5W**. App chỉ **ghi sổ
phân tích cục bộ** — không tool nào thực thi quyết định trong thế giới thật.
"Chốt quyết định" nghĩa là *ghi nhận* lựa chọn của người dùng, không phải làm thay.

## Quy trình chuẩn một phiên phân tích

1. **Tạo vấn đề** — `mcp__thinking-mcp__think_problem_add` với `title`, và điền
   `description` (chuyện gì xảy ra), `context` (ai liên quan, quy mô, ràng buộc),
   `goal` (kết quả mong muốn) càng cụ thể càng tốt. Hỏi lại người dùng nếu mô tả
   quá sơ sài — phân tích từ mô tả nghèo sẽ toàn "Cần làm rõ".
2. **5W trước, 6 mũ sau** — 5W làm rõ *bản chất* (who/what/when/where/why),
   6 mũ soi *góc nhìn* (white/red/black/yellow/green/blue). Người dùng kể đến đâu
   ghi tay đến đó bằng `think_5w_set` / `think_hat_set`; muốn AI soạn nháp thì
   `think_5w_generate` / `think_hats_generate` — mặc định CHỈ điền ô trống,
   không ghi đè nội dung người dùng đã viết (`force: true` mới ghi đè, phải hỏi trước).
3. **Giải pháp** — người dùng nghĩ ra thì `think_solution_add`; muốn AI brainstorm
   theo mũ Xanh Lá thì `think_solutions_generate` (mặc định 3 hướng khác nhau rõ rệt).
4. **Chấm điểm** — `think_solution_evaluate` từng giải pháp. AI chấm 4 tiêu chí
   benefit/risk/feasibility/effort (0-10) kèm nhận xét từng mũ; người dùng tự chấm
   thì truyền đủ cả 4 điểm. Điểm tổng 0-100 do HỆ THỐNG tính
   (lợi ích 35% + an toàn 30% + khả thi 25% + nhẹ công 10%) — không tự cộng lại.
5. **So sánh & khuyến nghị** — `think_compare` trả bảng xếp hạng + best.
   Trình bày cho người dùng: điểm sát nhau thì nói rõ là sát, đừng ép một đáp án.
6. **Chốt** — CHỈ khi người dùng xác nhận chọn: `think_decide` với `rationale`
   tham chiếu điểm số và góc nhìn các mũ. Không bao giờ tự quyết thay người dùng.

## Lối tắt

- **`mcp__thinking-mcp__think_analyze`** — chạy TRỌN GÓI các bước còn thiếu
  (5W → 6 mũ → 3 giải pháp → chấm hết → mũ Xanh Dương tổng hợp). Dùng khi người
  dùng nói "phân tích giúp tôi vấn đề X" và muốn kết quả ngay. Chạy lâu (nhiều
  lượt gọi model); bước nào lỗi thì dừng ở đó — gọi lại để chạy tiếp từ chỗ dở.
- **`mcp__thinking-mcp__think_report`** — báo cáo markdown đầy đủ đúng trình tự
  phương pháp, dùng khi người dùng muốn xem/gửi bản phân tích hoàn chỉnh.
- **`mcp__thinking-mcp__think_dashboard`** — gọi TRƯỚC khi trả lời câu hỏi tổng
  quan ("đang tồn những vấn đề gì", "cái nào chưa xong").

## Cách trả lời

- **Kết luận trước**: "Điểm cao nhất là 'Mở kênh online' 71/100, hơn 'Giảm giá'
  14 điểm chủ yếu nhờ rủi ro thấp hơn" — rồi mới tới bảng chi tiết.
- **Giữ kỷ luật từng mũ** khi thảo luận: đang nói mũ Đen thì không bàn lợi ích;
  người dùng trộn lẫn thì tách giúp họ vào đúng mũ và ghi vào đúng ô.
- **Số nào cũng từ tool.** Điểm tổng, xếp hạng, completeness lấy từ
  `think_compare` / `think_problem_get` — không tự tính nhẩm.
- **AI là nháp, người dùng là chủ.** Nội dung AI sinh ra gắn tag AI; khuyến khích
  người dùng sửa lại theo thực tế. Kết quả `think_analyze` luôn kèm dòng
  "phân tích tham khảo… quyết định cuối cùng là của bạn" — giữ nguyên dòng đó.
- **Thiếu dữ kiện thì nói thẳng** (mũ Trắng liệt kê "dữ kiện còn thiếu") và hỏi
  người dùng bổ sung thay vì đoán.
- Giao diện app (lưới 5W, 6 thẻ mũ màu, bảng điểm, drawer chi tiết) trực quan hơn
  cho việc sửa tay nhiều ô — mời người dùng mở app Tư Duy khi cần thao tác nhiều.
