---
name: widget-pack
description: Insert Widget Pack app widgets into the chat box — countdown, progress bar, data table, plus iframe variants of chart/image/video — via emit_widget kind "app"
version: 1.0.0
when-to-use: When the user wants to display a countdown to a moment, a progress/percent-complete bar, or a compact data table right inside the chat
triggers:
  - đếm ngược
  - dem nguoc
  - countdown
  - tiến độ
  - tien do
  - progress
  - phần trăm hoàn thành
  - percent complete
  - bảng dữ liệu
  - bang du lieu
  - data table
allowed-tools:
  - emit_widget
  - widget_list
---

# Widget Pack — custom widgets in the chat box

The `widget-pack` app provides iframe widgets inserted straight into the chat
via `emit_widget` with `kind: "app"` (do NOT pass `data` — pass `widget` +
`params`). Call `widget_list` if you need the latest catalog/params.

## Widgets and how to call them

**Countdown** — deadlines, events:
```
emit_widget { "kind": "app", "widget": "widget-pack.countdown",
  "params": { "to": "2026-12-31", "label": "Tết Dương lịch" } }
```
`to` accepts `YYYY-MM-DD` or an ISO datetime (`2026-12-31T09:00`).

**Progress bar** — completion %, goals:
```
emit_widget { "kind": "app", "widget": "widget-pack.progress",
  "params": { "value": 68, "max": 100, "label": "Notebook done" } }
```

**Data table** — compact lists with columns:
```
emit_widget { "kind": "app", "widget": "widget-pack.table",
  "params": { "title": "Menu", "cols": "Item,Price",
              "rows": "[[\"Coffee\",25000],[\"Milk coffee\",30000]]" } }
```
`rows` is a JSON STRING (array-of-arrays or array of objects).

## Chart / Image / Video

In chat, prefer the builtin `emit_widget` kinds (`chart` with `rows`, `image`,
`video`) — they render natively and are lighter than an iframe. The Widget
Pack iframe variants (`widget-pack.chart` / `.image` / `.video`) are meant for
the Dashboard or when a fixed frame is wanted; see `widget_list` for params.

## Notes

- Messaging channels (Telegram/Zalo/…) only receive a one-line text fallback —
  the full widget shows on the SenClaw Web/Desktop UI.
- Params travel as a query string; compute values yourself and fill them in —
  never write temp files/scripts to prepare widget data.
