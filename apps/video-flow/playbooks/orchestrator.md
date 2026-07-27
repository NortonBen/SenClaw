---
name: orchestrator
description: LLM lập kế hoạch DAG — chọn agent, thứ tự phụ thuộc, prompt từng bước khi tạo pipeline
triggers:
  - lập kế hoạch dag
  - orchestrator
  - tự lập pipeline

---

# orchestrator — Orchestrator (DAG planner)

Skill **tham chiếu / catalog**: không phải lệnh shell như `pipeline`. Trên runtime, **`OrchestratorAgent`** được gọi khi client tạo pipeline (`POST /api/pipeline`): một lần gọi LLM (có retry tinh chỉnh nếu JSON không hợp lệ) để sinh **một đồ thị tác vụ không chu trình (DAG)**.

## Vai trò

- Đầu vào: mục tiêu dự án + ngữ cảnh (tóm tắt project, chế độ pipeline).
- Đầu ra: JSON **`{ "tasks": [ ... ] }`** — mỗi phần tử có `label`, `agent_type`, `prompt`, `depends_on`, `timeout_seconds`.
- Chỉ được dùng các **`agent_type`** mà backend liệt kê cho planner tại thời điểm đó: agent DAG built-in + **skill agent** đăng ký trong DB (nếu có).

## Nhóm agent (thứ tự gợi ý trong system prompt)

| Giai đoạn | Ý nghĩa |
|-----------|---------|
| **Pre-production** | `director` → `screenwriter` → `scene_plan` → `shot_design` → `visual_asset` — từ ý tưởng đến kịch bản, bối cảnh, shot list, DNA nhân vật. |
| **Production** | `scene_builder` hoặc `script_parser` → `character` → `image` → `video` → `audio` → `concat` — ưu tiên `scene_builder` khi đã có shot_design. |
| **QA / continuity** | `critic`, `director_frame` — tùy kịch bản. |
| **Post** | **`media_download` trước `concat`** — tải URL về local rồi mới ghép file local. |

## Quy tắc bắt buộc

- **`depends_on`**: mỗi giá trị phải là **`label`** của một task khác trong cùng plan; không được tạo chu trình.
- **`prompt`**: hướng dẫn cụ thể cho worker agent — kết quả các bước trước được gộp theo label trong working context (upstream JSON).
- Đầu ra cho LLM: **chỉ JSON**, không bọc markdown.

## Phân biệt nhanh

| | Orchestrator (skill này) | `pipeline` |
|--|--------------------------|----------------|
| **Cơ chế** | LLM trong API pipeline | Curl/poll/script với API & worker |
| **Khi nào** | Lúc **lập DAG** khi tạo pipeline | **Chạy stage** (ảnh, video, upscale, TTS…) sau khi đã có project |
