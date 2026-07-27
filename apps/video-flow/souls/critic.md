---
name: critic
description: Kiểm tra đầu vào TRƯỚC khi render (deterministic, không gọi LLM) — chặn pipeline nếu thiếu thứ sẽ làm hỏng ảnh/clip
---

# Pre-flight check

Agent này **không gọi LLM**, nên sửa file này không đổi hành vi của nó. Nó đọc
thẳng scene + entity trong DB và trả lời một câu hỏi: *đầu vào đã đủ tốt để bỏ
tiền render chưa?*

Trước đây đây là "video critic" — một model chỉ đọc được chữ nhưng bị yêu cầu
phát hiện "mặt chảy nhựa", "frequency crawl" trong một video **nó chưa từng
nhận** (prompt chỉ có `{"video_id": "…"}`), và không ai đọc kết quả. Nó lại chạy
sau `concat`, tức sau khi đã trả tiền cho toàn bộ clip. Vừa vô nghĩa vừa quá muộn.

Vị trí mới: **giữa ảnh tham chiếu và khâu render**. Sai sót bị bắt trong vài giây
thay vì sau 9 clip.

## LỖI — dừng pipeline, không render

| Mã | Ý nghĩa |
|---|---|
| `no_image_prompt` | Cảnh không có `image_prompt` lẫn `prompt` — ảnh khung hình sẽ vô nghĩa |
| `no_video_prompt` | Cảnh không có `video_prompt` — clip thiếu chỉ dẫn máy quay |
| `entity_without_reference` | Cảnh tham chiếu entity chưa có ảnh ref — mất nhất quán nhân vật, đúng thứ pipeline sinh ra để giữ |

## CẢNH BÁO — vẫn render, chỉ nhắc

| Mã | Ý nghĩa |
|---|---|
| `no_timing` | `video_prompt` thiếu mốc `0-3s: …` — Veo3 kém ổn định hơn |
| `no_entities` | Cảnh không tham chiếu entity nào |
| `unknown_entity` | Tên trong `character_names` không khớp entity nào |
| `no_continuity_bridge` | Cảnh từ thứ 2 trở đi chưa có cầu nối liên tục |

## Vì sao không dùng LLM

Mọi điều kiện trên đều kiểm tra chính xác được từ DB. Hỏi model những thứ một
câu `SELECT` trả lời được thì vừa chậm, vừa tốn, vừa có thể sai. Muốn chấm chất
lượng hình ảnh thật thì cần model xem được video — chưa có thì đừng giả vờ có.
