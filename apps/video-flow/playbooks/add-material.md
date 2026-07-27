---
name: add-material
description: Chuẩn hóa phong cách Material cho project và pipeline ảnh
triggers:
  - phong cách hình ảnh
  - material
  - đổi style
  - style ảnh
  - photorealistic
  - ghibli

---

# add-material — Thiết kế tính năng Material cho Project

Mục tiêu của tính năng Material là chuẩn hóa phong cách hình ảnh theo từng project, giúp ảnh nhân vật/bối cảnh và ảnh scene giữ được một ngôn ngữ thị giác thống nhất trong toàn pipeline.

---

## 1) Mục tiêu sản phẩm

- Cho phép người dùng chọn một material ngay khi tạo project.
- Material ảnh hưởng xuyên suốt:
  - Prompt tạo ảnh entity (character/location/object)
  - Prompt scene image (thông qua `scene_prefix`)
- Hỗ trợ 2 loại material:
  - Built-in (mặc định hệ thống, không xóa)
  - Custom (do người dùng tạo, có thể xóa nếu chưa dùng)
- Đảm bảo backward-compatible với project cũ chưa có material.

---

## 2) Phạm vi chức năng

### In scope
- Danh sách material: `GET /api/materials`
- Tạo material custom: `POST /api/materials`
- Xóa material custom: `DELETE /api/materials/{id}`
- Bắt buộc trường `material` khi tạo project mới.
- Material được áp dụng khi:
  - Sinh prompt entity reference
  - Tạo scene prompt ban đầu

### Out of scope (phase sau)
- Đổi material hàng loạt cho project đã có scene/image/video hoàn chỉnh.
- Auto-regenerate toàn bộ asset khi đổi material.
- Versioning material nâng cao.

---

## 3) Data model đề xuất

### `materials`
- `id` (PK, text, lowercase snake_case)
- `name` (text, unique)
- `style_instruction` (text, required)
- `negative_prompt` (text, nullable)
- `scene_prefix` (text, nullable)
- `lighting` (text, nullable, default fallback ở service)
- `is_builtin` (bool, default false)
- `created_at`, `updated_at`

### `projects`
- Bổ sung cột `material` (text, required cho dữ liệu mới)
- FK mềm theo `materials.id` (app-level validation), tránh lock migration cứng khi seed thay đổi.

---

## 4) Luồng nghiệp vụ

### A. Tạo project
1. Frontend load materials trước khi submit form.
2. Người dùng chọn 1 material.
3. Backend validate `material` tồn tại.
4. Lưu `projects.material`.

### B. Sinh ảnh entity
Service prompt builder ghép:
1. Prompt nội dung entity
2. `style_instruction`
3. `lighting`
4. `negative_prompt` (nếu có)

### C. Tạo scene
- Khi tạo scene mới, backend prepend `scene_prefix` vào `scene.prompt`.
- Người dùng vẫn nhập phần action/story chính, nhưng không phải tự thêm style.

---

## 5) API contract

### `GET /api/materials`
Trả về danh sách material built-in + custom:

```json
[
  {
    "id": "3d_pixar",
    "name": "3D Pixar",
    "style_instruction": "...",
    "negative_prompt": "...",
    "scene_prefix": "...",
    "lighting": "...",
    "is_builtin": true
  }
]
```

### `POST /api/materials`
Request:

```json
{
  "id": "watercolor_soft",
  "name": "Watercolor Soft",
  "style_instruction": "...",
  "negative_prompt": "...",
  "scene_prefix": "...",
  "lighting": "..."
}
```

Validation:
- `id` bắt buộc, regex: `^[a-z0-9_]+$`
- Không trùng built-in hoặc custom đã có
- `style_instruction` bắt buộc, min length hợp lý (ví dụ >= 20)

### `DELETE /api/materials/{id}`
- Chỉ cho xóa `is_builtin = false`
- Nếu đang có project dùng material đó: trả `409 Conflict`

### `POST /api/projects`
- `material` là required.
- Nếu client cũ chưa gửi `material`, backend fallback `realistic` trong giai đoạn chuyển tiếp (có warning log), sau khi frontend rollout xong thì bật strict mode.

---

## 6) Thiết kế frontend

### Create Project page
- Thêm dropdown/select `Material`.
- Mặc định chọn `realistic`.
- Hiển thị preview ngắn: `name` + mô tả style 1 dòng.
- Disable submit nếu load materials thất bại hoặc chưa chọn.

### Settings/Skills UX
- Có thể bổ sung trang quản lý custom material:
  - Tạo mới
  - Xóa
  - Cảnh báo nếu material đang được dùng

---

## 7) Migration & rollout

1. Tạo bảng `materials` + seed built-in.
2. Bổ sung cột `projects.material`:
   - Giai đoạn 1: nullable + backfill `realistic` cho dữ liệu cũ
   - Giai đoạn 2: set NOT NULL
3. Deploy backend trước (support fallback cho client cũ).
4. Deploy frontend bắt buộc chọn material.
5. Sau khi ổn định, tắt fallback và enforce strict validation.

---

## 8) Test plan

### Backend
- Unit test validation cho create/delete material.
- Unit test prompt builder có apply đầy đủ material fields.
- Integration test create project với material hợp lệ/không hợp lệ.
- Integration test delete material đang được project sử dụng (`409`).

### Frontend
- Render danh sách material thành công/thất bại.
- Submit create project có gửi `material`.
- Chặn submit khi không có material.

---

## 9) Rủi ro & cách giảm thiểu

- Prompt bị "quá style", lấn át nội dung scene  
  → Giới hạn độ dài `scene_prefix`, hướng dẫn best practice.
- User xóa custom material đang dùng  
  → Chặn xóa và trả lỗi rõ ràng.
- Mismatch giữa built-in seed và enum hardcode frontend  
  → Frontend chỉ đọc từ API, không hardcode danh sách.

---

## 10) Definition of Done

- Có thể tạo project với material hợp lệ từ UI.
- Prompt entity/scene thực tế có áp dụng material.
- Project cũ hoạt động bình thường sau migration.
- Có test backend/frontend cho các case chính.
- Tài liệu skill phản ánh đúng flow triển khai và vận hành.
