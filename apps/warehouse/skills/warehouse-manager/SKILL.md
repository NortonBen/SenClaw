---
name: warehouse-manager
description: >-
  Quản lý kho hàng qua app Kho Hàng: danh mục sản phẩm (SKU, giá vốn/giá bán, tồn tối
  thiểu), nhiều kho, phiếu nhập / xuất / chuyển kho / điều chỉnh kiểm kê, thẻ kho, giá
  vốn bình quân gia quyền, báo cáo nhập-xuất theo tháng, cảnh báo hàng sắp hết và nhờ
  AI phân tích tồn kho + phân tích sản phẩm (tiềm năng / bán chậm / tồn đọng không
  bán được). Dùng khi người dùng nói về kho hàng, tồn kho, nhập kho, xuất kho, chuyển
  kho, kiểm kê, thẻ kho, hàng sắp hết, sản phẩm bán chạy/tiềm năng hay hàng ế/tồn
  đọng. Mọi con số lấy từ tool — không tự cộng trừ tồn kho hay tính nhẩm giá vốn.
triggers:
  - kho hàng
  - quản lý kho
  - tồn kho
  - nhập kho
  - xuất kho
  - chuyển kho
  - kiểm kê
  - thẻ kho
  - phiếu nhập
  - phiếu xuất
  - sắp hết hàng
  - tồn tối thiểu
  - giá vốn
  - nhập xuất tồn
  - sản phẩm tiềm năng
  - sản phẩm bán chạy
  - hàng tồn đọng
  - hàng bán chậm
  - hàng ế
  - phân tích sản phẩm
  - inventory
  - stock card
  - warehouse
  - dead stock
---

# warehouse-manager

Dùng MCP server `warehouse-mcp` của app **Kho Hàng**. App chỉ **ghi sổ cục bộ** — không
kết nối sàn TMĐT, không tạo đơn hàng thật. "Xuất kho" nghĩa là *ghi nhận* hàng đã rời
kho, không phải bán hàng online.

## Chọn công cụ

- **`mcp__warehouse-mcp__wh_dashboard`** — LUÔN gọi trước khi trả lời câu hỏi tổng quan
  ("kho dạo này thế nào", "tồn bao nhiêu tiền hàng"): trả về giá trị tồn kho, hàng dưới
  tồn tối thiểu, hàng hết, nhập/xuất 30 ngày, biểu đồ 12 tháng, top sản phẩm, các kho
  và phiếu gần đây.
- **`mcp__warehouse-mcp__wh_product_add` / `wh_product_update` / `wh_product_list` /
  `wh_product_get`** — danh mục sản phẩm. `min_stock` = tồn tối thiểu để cảnh báo;
  `wh_product_list` với `low_stock: true` (hoặc `wh_low_stock`) = danh sách cần nhập
  thêm. Ngừng bán bằng `status: "inactive"`.
- **`mcp__warehouse-mcp__wh_warehouse_add` / `wh_warehouse_list` /
  `wh_warehouse_update`** — các kho/chi nhánh, kèm giá trị tồn từng kho.
- **`mcp__warehouse-mcp__wh_partner_add` / `wh_partner_list`** — nhà cung cấp
  (`supplier`) / khách hàng (`customer`), gắn vào phiếu bằng `partner_id`.
- **`mcp__warehouse-mcp__wh_move_create`** — tạo phiếu kho, một phiếu nhiều dòng hàng.
  `kind`: `receipt` (nhập — `unit_price` là giá vốn nhập) · `issue` (xuất — `unit_price`
  là giá bán; tồn không đủ sẽ bị TỪ CHỐI, đừng cố lách bằng adjust) · `transfer`
  (chuyển kho — cần `to_warehouse_id` khác kho đi) · `adjust` (điều chỉnh sau kiểm kê —
  `qty` là DELTA có dấu: đếm thừa ghi dương, thiếu ghi âm). Mã phiếu tự sinh
  NK-/XK-/CK-/DC-.
- **`mcp__warehouse-mcp__wh_move_list` / `wh_move_get` / `wh_move_delete`** — tra cứu
  và huỷ phiếu ghi nhầm. Xoá bị từ chối nếu làm tồn kho âm (ví dụ xoá phiếu nhập khi
  hàng đã xuất rồi) — khi đó phải xoá phiếu xuất liên quan trước hoặc dùng adjust.
- **`mcp__warehouse-mcp__wh_stock_onhand`** — tồn hiện tại theo từng cặp (sản phẩm,
  kho) kèm giá vốn bình quân và giá trị. Đây là nguồn số tồn duy nhất — không tự cộng
  trừ từ danh sách phiếu.
- **`mcp__warehouse-mcp__wh_stock_card`** — thẻ kho một sản phẩm với số dư luỹ kế.
  Có `warehouse_id` → thẻ của riêng kho đó; không có → toàn công ty.
- **`mcp__warehouse-mcp__wh_report_inout`** — báo cáo nhập-xuất theo tháng
  (in_qty/in_value, out_qty/out_value, adjust_qty).
- **`mcp__warehouse-mcp__wh_product_insight`** — hiệu suất từng sản phẩm trong cửa sổ
  N ngày (mặc định 90) với phân loại TỰ ĐỘNG: `potential` (tiềm năng — bán tốt, tồn
  chỉ đủ ≤45 ngày, nên nhập thêm) · `steady` · `slow` (bán chậm — tồn đủ >180 ngày) ·
  `dead` (tồn đọng — không bán được đơn nào) · `idle`. Kèm tốc độ bán/30 ngày, ngày
  tồn còn lại, biên lãi, lần bán cuối và giá trị vốn chôn trong hàng tồn đọng. Dùng
  tool này khi được hỏi "sản phẩm nào bán chạy / tiềm năng / ế / không bán được".
- **`mcp__warehouse-mcp__wh_analyze_products`** — AI đánh giá danh mục dựa trên số
  liệu trên: nên nhập thêm gì (ước lượng bao nhiêu), hàng tồn đọng xử lý ra sao (xả
  giảm giá / gộp combo / ngừng nhập). KHÔNG tự phân loại lại bằng tay — phân loại đã
  do máy tính, AI chỉ nhận định và ưu tiên hành động.
- **`mcp__warehouse-mcp__wh_analyze`** — AI phân tích tồn kho tổng quan qua bridge
  SenClaw. Kết quả luôn kèm lưu ý "phân tích tham khảo… hãy kiểm kê thực tế" — giữ
  nguyên lưu ý đó.

## Cách trả lời

- **Kết luận trước**: "Tồn kho 128 triệu / 42 mặt hàng, 3 mặt hàng dưới tồn tối thiểu"
  — rồi mới tới bảng chi tiết.
- **Số nào cũng từ tool.** Không tự cộng tồn từ các phiếu, không tự tính giá vốn bình
  quân — gọi `wh_stock_onhand` / `wh_dashboard` để máy tính.
- **Ghi sổ xong phải xác nhận lại con số** (đọc lại tồn mới từ kết quả tool).
- **Có hàng dưới tồn tối thiểu thì nói ngay đầu câu trả lời**, kể cả khi người dùng
  hỏi chuyện khác.
- **Kiểm kê**: hỏi số ĐẾM THỰC TẾ, lấy tồn sổ từ `wh_stock_onhand`, rồi tạo MỘT phiếu
  `adjust` với delta = thực tế − sổ cho từng mặt hàng lệch. Không tạo cặp phiếu
  nhập/xuất giả để "cân sổ".
- Ngày dùng định dạng `YYYY-MM-DD` khi gọi tool; hiển thị cho người dùng dạng dd/mm/yyyy.
