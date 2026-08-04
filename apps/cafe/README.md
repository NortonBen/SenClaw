# Quán Cafe — SenClaw Space App

Quản lý quán cafe / đồ uống 100% cục bộ (SQLite):

- **Kho nguyên liệu** theo đơn vị gốc `g` / `ml` / `cái`; nhập hàng khai `kg` / `lít`
  tự quy đổi; giá vốn **bình quân gia quyền** theo phiếu nhập; kiểm kê + thẻ kho.
- **Thực đơn & công thức pha chế**: mỗi món có giá bán, cách pha chế và công thức định
  lượng (25 g cafe, 30 ml sữa đặc…) → giá vốn món + lãi gộp tính tự động.
- **Bán hàng**: ghi đơn nhiều dòng, kho tự trừ theo công thức, chốt giá vốn tại thời
  điểm bán; huỷ đơn hoàn kho.
- **Báo cáo**: nhập hàng (theo NCC / nguyên liệu / ngày), doanh thu – giá vốn – lãi gộp
  (theo ngày / món / nhóm), tồn kho.
- **Dự đoán**: lượng bán + doanh thu theo trung bình cùng thứ 4 tuần gần nhất; dự báo
  tiêu hao nguyên liệu, ngày hết hàng và **đề xuất nhập hàng**.
- **AI qua bridge SenClaw**: phân tích kinh doanh + gợi ý công thức món mới.

## Chạy dev

```bash
cargo run -p cafe                 # backend http://127.0.0.1:4700
cd apps/cafe/web && npm run dev   # UI dev, proxy /api → :4700
```

DB tại `$SENCLAW_DATA_DIR` hoặc `~/.senclaw/apps/cafe/cafe.db`. Bind mặc định
loopback; đặt `SENCLAW_BIND_HOST=0.0.0.0` nếu cần truy cập từ máy khác.

## Test & đóng gói

```bash
cargo test -p cafe
apps/cafe/scripts/pack.sh         # → apps/cafe/cafe-app.zip
```

MCP server: `cafe-mcp` (`/api/mcp/sse`), 27 tools tiền tố `cafe_` — tên đầy đủ dạng
`mcp__cafe-mcp__cafe_dashboard`. Xem `skills/cafe-manager/SKILL.md`.
