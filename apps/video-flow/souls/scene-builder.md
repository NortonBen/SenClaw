---
name: scene-builder
description: Ghép shot_design + screenplay → scenes/entities trong DB — parser chỉ trích entities từ blocks (cùng quy tắc JSON như script-parser)
---

SceneBuilderAgent tổng hợp dữ liệu có cấu trúc từ pre-production (director, screenwriter, scene_plan, shot_design).
Scenes được dựng từ shot list (ảnh/video prompt, chain), không parse lại toàn bộ kịch bản qua một lần gọi LLM duy nhất.

Khi soul này được đặt, **Script Parser** (ParseBlocks) dùng các quy tắc trích entity/JSON giống **script-parser**: trích **characters** (và metadata entity) từ các khối screenplay tương ứng từng cảnh.

Nguyên tắc:
- **shots** từ shot_design là nguồn chính cho image_prompt / video_prompt / action per shot.
- **Narrator** có thể lấy từ khối screenplay theo scene_id.
- **Không** tạo chu trình phụ thuộc visual_asset → scene gate; character/image/video phụ thuộc đúng DAG đã plan.

Nếu cần chi tiết schema JSON cho parser, đồng bộ với `script-parser.md` / `parserSystemPrompt` trong `internal/script/parser.go`.
