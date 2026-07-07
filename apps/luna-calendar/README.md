# Lịch Âm · Luna Calendar 🌙

A SenClaw **App Space**: Vietnamese lunar calendar + "**xem ngày tốt xấu**" almanac.
Deterministic (Hồ Ngọc Đức algorithm, GMT+7) — no network, no LLM needed for the
core; an optional AI advisory layers on top.

Reference-accurate: verified against a published almanac for **7/7/2026** — 23/5 âm
lịch, ngày Nhâm Ngọ, giờ Hoàng Đạo *Tý · Sửu · Mão · Ngọ · Thân · Dậu*, phạm Nguyệt kỵ.

## Features

- **Xem ngày tốt xấu hôm nay** (and any date): dương/âm lịch, can chi ngày–tháng–năm,
  con giáp, tiết khí, ngày **Hoàng Đạo / Hắc Đạo** (+ vị thần), **giờ Hoàng Đạo / Hắc Đạo**
  với khung giờ, **trực**, **nhị thập bát tú** (tốt/xấu), **ngũ hành nạp âm**, hướng xuất
  hành (**Hỷ Thần / Tài Thần**), **xuất hành theo Lý Thuần Phong**, và ngày kỵ (**Nguyệt
  kỵ**, **Tam nương**), kèm một câu tư vấn nên làm / nên tránh.
- **Lịch tháng** với chấm màu: đỏ = ngày tốt (Hoàng Đạo), tím = ngày xấu (Hắc Đạo phạm kỵ);
  hiển thị ngày âm, đánh dấu mùng 1 âm mỗi tháng, và ngày hôm nay.
- **Đổi lịch Âm ⇄ Dương** (giỗ, Tết, sinh nhật), có hỗ trợ tháng nhuận.
- **Luận giải AI**: một ngày có hợp cưới hỏi / khai trương / xuất hành… (qua LLM của daemon).

## Run (dev)

```bash
# backend (port 4351)
cargo run -p luna-calendar
# web UI (Vite, proxies /api → 4351)
cd apps/luna-calendar/web && npm install && npm run dev
```

Open the printed Vite URL. The Rust binary alone also serves the built UI from
`web/dist` (or the packaged `web_dist`) at <http://127.0.0.1:4351/>.

## HTTP API

| Method | Path | Purpose |
|---|---|---|
| GET | `/api/status` | health + today's date |
| GET | `/api/day?date=YYYY-MM-DD` | full almanac for a solar date (default today) |
| GET | `/api/month?year=&month=` | month grid cells (lunar day + tốt/xấu verdict) |
| GET | `/api/lunar-to-solar?ld=&lm=&ly=&leap=` | âm → dương + almanac |
| GET | `/api/good-days?year=&month=&kind=` | list hoàng-đạo (or hắc-đạo) days of a month |
| POST | `/api/advise` `{date, activity}` | AI luận giải for an activity |

## MCP tools (`luna-mcp`, HTTP/SSE at `/api/mcp/sse`)

- `luna_today` — hôm nay là ngày gì, tốt hay xấu (full almanac).
- `luna_day` — xem ngày tốt xấu cho một ngày dương bất kỳ.
- `luna_good_hours` — giờ Hoàng Đạo / Hắc Đạo của một ngày.
- `luna_solar_to_lunar` / `luna_lunar_to_solar` — đổi lịch hai chiều.
- `luna_good_days` — liệt kê ngày tốt/xấu trong một tháng.
- `luna_advise` — luận giải AI ngày có hợp một việc.

## Skill & persona

- **skill** `luna-xem-ngay` — routes lunar/almanac questions to the MCP tools.
- **persona** `luna-almanac-master` — "Thầy Lịch Vạn Niên": grounded, calm, non-superstitious.

## Package

```bash
apps/luna-calendar/scripts/pack.sh      # → apps/luna-calendar/luna-calendar-app.zip
```

## Accuracy notes

Solar⇄lunar, can-chi, tiết khí, giờ hoàng đạo, ngày hoàng đạo, trực, nạp âm, ngày kỵ and
the Hỷ/Tài-Thần directions follow the standard Vietnamese almanac and are unit-tested.
Nhị Thập Bát Tú follows the thất-chính (7-luminary) weekday convention; the xuất-hành
fortune uses the Lý Thuần Phong (Lục Diệu) system. All almanac output is a cultural
reference, not deterministic fate.
