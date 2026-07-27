---
name: refresh-urls
description: Lấy lại link ảnh/video khi scene đã COMPLETED mà không xem được, rồi tải hẳn về máy
triggers:
  - không xem được video
  - không xem được ảnh
  - mất hình
  - mất link
  - link hết hạn
  - thumbnail trống
  - lấy lại link
  - refresh url
  - broken image
---

# Lấy lại link media

Dùng khi scene báo `COMPLETED` nhưng không xem được ảnh/clip, hoặc project cũ
bỗng mất hình.

## Vì sao xảy ra

Google Flow **không còn trả URL video** ở API sinh — chỉ trả media id. Link chỉ
tồn tại trong dữ liệu trang Flow, và link ảnh là URL ký ngắn hạn nên vài giờ sau
sẽ hết hạn.

App xử lý bằng hai bước: nhờ extension mở trang project Flow trong tab nền để
bắt link, rồi **tải hẳn file về máy** (`/api/media/<id>/file`) để lần sau không
phụ thuộc Google nữa.

## Cách làm

```bash
PID="<project_id>"
BASE="http://127.0.0.1:4460"   # đổi nếu SenClaw cấp cổng khác

# 1) Bắt link từ trang Flow + tải về (cần extension đang kết nối)
curl -sS -X POST "$BASE/api/media/fetch-urls" \
  -H 'content-type: application/json' -d "{\"project_id\":\"$PID\"}"
# → {"downloaded":N,"failed":0,"scenes_still_without_url":0}

# 2) Quét thêm: tải mọi asset còn nằm trên URL remote về local
curl -sS -X POST "$BASE/api/media/localize" \
  -H 'content-type: application/json' -d "{\"project_id\":\"$PID\"}"
```

Bằng MCP: `mcp__video-flow-mcp__vf_fetch_video_urls` rồi
`mcp__video-flow-mcp__vf_media_localize`.

## Kiểm tra lại

```bash
curl -s "$BASE/api/scenes?video_id=<VID>" | python3 -c '
import json,sys
for s in json.load(sys.stdin):
    print(s["display_order"], s.get("vertical_video_status"), s.get("vertical_video_url"))'
```

URL bắt đầu bằng `/api/media/` là đã nằm trên máy — an toàn. Còn `https://…`
nghĩa là vẫn phụ thuộc link Google và sẽ hết hạn.

## Khi vẫn không được

- `vf_status` xem extension đã kết nối chưa — không có extension thì không lấy
  được link.
- Project chưa từng sinh ảnh/video thì app chưa biết Flow project id; sinh một
  ảnh tham chiếu trước.
- Còn kẹt thì xem log: `grep 'SUCCESSFUL but no URL' <app_dir>/.senclaw/runtime.log`
