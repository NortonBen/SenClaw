# SenClaw Warehouse — Kho Hàng

Space App quản lý kho hàng, dữ liệu 100% local (SQLite tại
`~/.senclaw/apps/warehouse/warehouse.db`). App chỉ **ghi sổ kho** — không kết nối
sàn TMĐT, không tạo đơn hàng thật.

## Tính năng

- **Danh mục sản phẩm** — SKU (duy nhất), đơn vị, nhóm hàng, barcode, giá vốn tham
  khảo, giá bán, **tồn tối thiểu** (cảnh báo sắp hết hàng), ngừng bán bằng
  `status=inactive`.
- **Nhiều kho / chi nhánh** — mỗi kho có giá trị tồn và số mặt hàng riêng; kho mặc
  định "Kho chính" được tạo ở lần chạy đầu.
- **Phiếu kho** (một phiếu nhiều dòng hàng, mã tự sinh):
  - `receipt` NK- — nhập kho, đơn giá là giá vốn nhập
  - `issue` XK- — xuất kho, đơn giá là giá bán; **chặn xuất quá tồn**
  - `transfer` CK- — chuyển kho (trừ kho đi, cộng kho đến)
  - `adjust` DC- — điều chỉnh sau kiểm kê, số lượng là **delta có dấu**
  - Xoá phiếu bị từ chối nếu làm tồn âm (giữ sổ nhất quán).
- **Tồn kho suy ra từ phiếu** — không lưu cột tồn, sổ không bao giờ lệch chứng từ.
- **Giá vốn bình quân gia quyền** theo phiếu nhập (fallback giá vốn khai báo).
- **Thẻ kho** — ledger từng sản phẩm với số dư luỹ kế, theo từng kho hoặc toàn công ty.
- **Báo cáo nhập-xuất theo tháng** + dashboard (giá trị tồn, hàng sắp hết/hết,
  nhập-xuất 30 ngày, top sản phẩm, phiếu gần đây).
- **Phân tích hiệu suất sản phẩm** (`/api/insight/products`, tab "Phân tích SP") —
  phân loại tự động theo cửa sổ N ngày: `potential` (tiềm năng — bán tốt, tồn chỉ
  đủ ≤45 ngày), `steady`, `slow` (tồn đủ >180 ngày), `dead` (tồn đọng — không bán
  được đơn nào), `idle`; kèm tốc độ bán/30 ngày, ngày tồn còn lại, biên lãi,
  sell-through, lần bán cuối, giá trị vốn chôn trong hàng tồn đọng.
- **AI phân tích** qua bridge SenClaw (`llm.request`) — không gọi thẳng provider
  nào: `wh_analyze` (tồn kho tổng quan) + `wh_analyze_products` (đánh giá danh mục:
  nên nhập thêm gì, xả hàng gì).

## Chạy dev

```bash
cargo run -p warehouse                  # backend :4630 (đổi bằng PORT)
cd apps/warehouse/web && npm run dev    # Vite dev, proxy /api → :4630
```

Test: `cargo test -p warehouse` (23 test: stock logic, phiếu, thẻ kho, phân loại
hiệu suất sản phẩm, MCP schema).

## Đóng gói

```bash
apps/warehouse/scripts/pack.sh          # → apps/warehouse/warehouse-app.zip
```

## MCP

Server `warehouse-mcp` (HTTP + SSE tại `/api/mcp/sse`), 23 tool prefix `wh_`
(`mcp__warehouse-mcp__wh_*`): `wh_status`, `wh_dashboard`, `wh_product_*` (4),
`wh_warehouse_*` (3), `wh_partner_*` (2), `wh_move_*` (4), `wh_stock_onhand`,
`wh_stock_card`, `wh_report_inout`, `wh_low_stock`, `wh_product_insight`,
`wh_analyze`, `wh_analyze_products`, `wh_activity`.
Mọi tool gọi chung `api::*_value` với REST nên agent và UI luôn thấy cùng số liệu.
