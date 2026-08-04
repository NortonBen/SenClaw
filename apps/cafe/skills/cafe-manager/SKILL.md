---
name: cafe-manager
description: >-
  Quản lý quán cafe / đồ uống qua app Quán Cafe: kho nguyên liệu theo gram / ml / cái
  (nhập kg / lít tự quy đổi), phiếu nhập hàng với giá vốn bình quân gia quyền, kiểm kê
  + thẻ kho, thực đơn món kèm cách pha chế và giá bán, công thức định lượng để tính giá
  vốn / lãi gộp từng món, bán hàng trừ kho tự động theo công thức, báo cáo nhập hàng và
  doanh thu – lãi gộp, dự đoán lượng bán / doanh thu / nguyên liệu sắp hết, đề xuất
  nhập hàng và AI phân tích kinh doanh + gợi ý công thức món mới. Dùng khi người dùng
  nói về quán cafe, pha chế, công thức, nguyên liệu, thực đơn, bán hàng, doanh thu,
  nhập hàng hay dự đoán. Mọi con số lấy từ tool — không tự cộng trừ tồn kho, giá vốn
  hay doanh thu.
triggers:
  - quán cafe
  - quán cà phê
  - cà phê
  - đồ uống
  - pha chế
  - công thức pha chế
  - công thức món
  - nguyên liệu
  - định lượng
  - thực đơn
  - giá vốn món
  - lãi gộp
  - bán hàng
  - doanh thu
  - báo cáo doanh thu
  - nhập hàng
  - báo cáo nhập hàng
  - đề xuất nhập hàng
  - sắp hết nguyên liệu
  - dự đoán doanh thu
  - dự báo nguyên liệu
  - thẻ kho
  - kiểm kê
  - coffee shop
  - barista
  - recipe
---

# cafe-manager

Dùng MCP server `cafe-mcp` của app **Quán Cafe**. App chỉ **ghi sổ cục bộ** — không kết
nối máy POS, không bán hàng online. "Bán hàng" nghĩa là *ghi nhận* đơn đã bán tại quầy;
kho nguyên liệu tự trừ theo công thức của từng món.

## Mô hình dữ liệu

- **Nguyên liệu** có đơn vị gốc `g` / `ml` / `cái`; tồn kho và công thức luôn tính bằng
  đơn vị gốc. Giá vốn nguyên liệu = bình quân gia quyền theo phiếu nhập.
- **Món** có giá bán + cách pha chế (văn bản) + **công thức** (danh sách nguyên liệu và
  định lượng). Giá vốn món = Σ định lượng × giá vốn nguyên liệu; lãi gộp = giá bán − giá vốn.
- **Đơn bán** trừ kho theo công thức tại thời điểm bán và chốt giá vốn ngay lúc đó —
  báo cáo lãi không đổi khi giá nhập sau này thay đổi.

## Chọn công cụ

- **`mcp__cafe-mcp__cafe_dashboard`** — LUÔN gọi trước khi trả lời câu hỏi tổng quan
  ("quán hôm nay thế nào", "doanh thu bao nhiêu"): doanh thu / lãi hôm nay và 7 ngày,
  biểu đồ 14 ngày, top món, nguyên liệu sắp hết, kho âm, món chưa có công thức.
- **`mcp__cafe-mcp__cafe_ingredient_add` / `cafe_ingredient_update` /
  `cafe_ingredient_list`** — danh mục nguyên liệu. `unit` là đơn vị gốc (`g`|`ml`|`cái`),
  `min_stock` = ngưỡng cảnh báo sắp hết (theo đơn vị gốc). `cafe_ingredient_list` với
  `low_only: true` = danh sách cần nhập thêm. Ngừng dùng: `active: false`.
- **`mcp__cafe-mcp__cafe_purchase_create`** — phiếu nhập hàng nhiều dòng. Mỗi dòng:
  `ingredient_id`, `qty`, `unit` (`g`|`kg`|`ml`|`l`|`lít`|`cái` — kg/lít tự quy đổi về
  gốc), `unit_price` = giá cho MỘT `unit` vừa khai (nhập 5 kg giá 90000 đ/kg thì
  qty=5, unit="kg", unit_price=90000). Giá vốn bình quân gia quyền tự cập nhật. Mã
  phiếu NH- tự sinh.
- **`mcp__cafe-mcp__cafe_purchase_list` / `cafe_purchase_get`** — tra cứu phiếu nhập.
- **`mcp__cafe-mcp__cafe_report_purchases`** — báo cáo nhập hàng theo `group_by`:
  `supplier` (nhà cung cấp) | `ingredient` | `day`, kèm tổng tiền.
- **`mcp__cafe-mcp__cafe_purchase_suggest`** — đề xuất nhập hàng cho N ngày tới từ dự
  báo tiêu hao + tồn tối thiểu − tồn hiện tại, kèm chi phí ước tính. Dùng khi Sếp hỏi
  "tuần tới cần mua gì".
- **`mcp__cafe-mcp__cafe_stock_adjust`** — điều chỉnh kiểm kê MỘT nguyên liệu: `delta`
  có dấu (đếm thừa dương, thiếu âm) HOẶC `set_qty` (đặt thẳng số đếm thực tế). Không
  dùng để "lách" khi thiếu nguyên liệu.
- **`mcp__cafe-mcp__cafe_stock_card`** — thẻ kho một nguyên liệu với số dư luỹ kế
  (nhập / bán / điều chỉnh / hoàn kho).
- **`mcp__cafe-mcp__cafe_menu_add` / `cafe_menu_update` / `cafe_menu_list` /
  `cafe_menu_get`** — thực đơn: giá bán (`price`), nhóm (`category`), cách pha chế
  (`instructions`). `cafe_menu_list` trả kèm giá vốn, lãi gộp, margin % và cờ
  `has_recipe`. Ngừng bán: `active: false`.
- **`mcp__cafe-mcp__cafe_recipe_set`** — đặt CÔNG THỨC cho món (thay thế toàn bộ):
  `items: [{ingredient_id, qty}]`, `qty` theo đơn vị gốc của nguyên liệu (ví dụ 25 g
  cafe, 30 ml sữa đặc). Sau khi đặt, giá vốn món tự tính lại.
- **`mcp__cafe-mcp__cafe_sale_create`** — ghi đơn bán nhiều dòng:
  `lines: [{menu_id, qty, unit_price?}]` (bỏ `unit_price` = lấy giá thực đơn). Kho tự
  trừ theo công thức; kết quả có thể kèm `warnings` (món chưa có công thức, nguyên
  liệu bị âm) — PHẢI nhắc lại các cảnh báo đó cho Sếp. Mã đơn BH- tự sinh.
- **`mcp__cafe-mcp__cafe_sale_list` / `cafe_sale_get` / `cafe_sale_void`** — tra cứu
  đơn và huỷ đơn ghi nhầm (huỷ = hoàn nguyên liệu về kho + loại khỏi báo cáo).
- **`mcp__cafe-mcp__cafe_report_revenue`** — báo cáo doanh thu – giá vốn – lãi gộp
  theo `group_by`: `day` | `item` (từng món) | `category` (nhóm món), trong khoảng
  `from`..`to`.
- **`mcp__cafe-mcp__cafe_report_inventory`** — tồn kho hiện tại: giá trị tồn từng
  nguyên liệu, tổng giá trị, danh sách sắp hết / âm kho.
- **`mcp__cafe-mcp__cafe_forecast_sales`** — dự đoán N ngày tới (mặc định 7): lượng
  bán từng món + doanh thu từng ngày, dựa trung bình cùng thứ 4 tuần gần nhất.
- **`mcp__cafe-mcp__cafe_forecast_ingredients`** — dự báo tiêu hao nguyên liệu N ngày
  tới + số ngày còn cầm cự + ngày dự kiến hết.
- **`mcp__cafe-mcp__cafe_ai_analyze`** — AI phân tích kinh doanh (doanh thu, món lãi
  tốt / kém, kho) qua bridge SenClaw. Kết quả luôn kèm dòng lưu ý "phân tích tham
  khảo…" — giữ nguyên dòng đó.
- **`mcp__cafe-mcp__cafe_ai_menu_suggest`** — AI gợi ý công thức món mới từ nguyên
  liệu sẵn có (định lượng g/ml, giá vốn ước tính, gợi ý giá bán). Chỉ là gợi ý — chốt
  với Sếp rồi mới `cafe_menu_add` + `cafe_recipe_set`.
- **`mcp__cafe-mcp__cafe_status`** — health check nhanh (số món, nguyên liệu, doanh
  thu hôm nay).

## Cách trả lời

- **Kết luận trước**: "Hôm nay 42 đơn / 3,2 triệu, lãi gộp 1,9 triệu; sữa đặc dưới
  ngưỡng" — rồi mới tới bảng chi tiết.
- **Số nào cũng từ tool.** Không tự cộng doanh thu từ danh sách đơn, không tự tính giá
  vốn món — gọi `cafe_report_revenue` / `cafe_menu_list`.
- **Ghi sổ xong xác nhận lại** mã chứng từ + tổng tiền + cảnh báo từ kết quả tool.
- **Có nguyên liệu dưới tồn tối thiểu thì nói ngay đầu câu trả lời**, kể cả khi Sếp
  hỏi chuyện khác.
- **Kiểm kê**: hỏi số ĐẾM THỰC TẾ từng nguyên liệu, so với tồn sổ
  (`cafe_ingredient_list`), rồi `cafe_stock_adjust` với `set_qty` = số đếm được cho
  từng nguyên liệu lệch. Không tạo phiếu nhập / đơn bán giả để "cân sổ".
- Ngày dùng định dạng `YYYY-MM-DD` khi gọi tool; hiển thị cho người dùng dạng
  dd/mm/yyyy. Tiền hiển thị có phân tách nghìn (ví dụ 1.250.000 đ).
