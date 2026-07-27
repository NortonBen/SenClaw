---
name: agent-status
description: Xem trạng thái thực thi DAG và từng task pipeline
triggers:
  - trạng thái pipeline
  - pipeline chạy tới đâu
  - task nào lỗi
  - agent status
  - xem tiến độ

---

# agent-status — Xem Trạng Thái DAG Agent Execution

Hiển thị trạng thái chi tiết của một pipeline đang chạy, bao gồm từng agent task.

## Mô Tả

Xem real-time status của pipeline DAG: task nào đang chạy, đã xong, bị lỗi, kết quả từng bước.

## Xem Status Pipeline

```bash
# Xem toàn bộ pipeline
curl http://127.0.0.1:4460/api/pipeline/{pipeline_id} | python3 -m json.tool

# Output mẫu:
{
  "ID": "uuid-...",
  "ProjectID": "uuid-...",
  "Status": "active",
  "Tasks": [
    {
      "Label": "parse_script",
      "AgentType": "script_parser",
      "Status": "done",
      "Result": "{\"scene_count\": 5, \"character_count\": 2}"
    },
    {
      "Label": "gen_characters",
      "AgentType": "character",
      "Status": "active",
      "StartedAt": "2026-04-23T10:00:05Z"
    },
    {
      "Label": "gen_images",
      "AgentType": "image",
      "Status": "registered"
    }
  ]
}
```

## Xem Agents Hiện Có

```bash
curl http://127.0.0.1:4460/api/agents
```

Output:
```json
[
  {"type": "orchestrator", "description": "Plans the video production DAG"},
  {"type": "script_parser", "description": "Parses screenplay → structured scenes"},
  {"type": "character", "description": "Generates character reference images"},
  {"type": "image", "description": "Generates scene still images"},
  {"type": "video", "description": "Generates Veo3 video clips"},
  {"type": "audio", "description": "Generates TTS narration audio"},
  {"type": "concat", "description": "Concatenates video clips into final output"}
]
```

## WebSocket Realtime Events

```javascript
// Events từ /ws/dashboard:
{"type": "agent:state", "data": {
  "task_id": "...",
  "label": "gen_images",
  "agent_type": "image",
  "status": "active|done|error",
  "pipeline_id": "...",
  "summary": "Submitted 5 image gen requests"
}}

{"type": "pipeline:updated", "data": {
  "pipeline_id": "...",
  "status": "active",
  "completed_tasks": 3,
  "total_tasks": 6
}}
```

## Xem Requests Queue (Worker)

```bash
# Pending requests (đang chờ extension xử lý)
curl http://127.0.0.1:4460/api/requests/pending

# Batch status cho project
curl "http://127.0.0.1:4460/api/requests/batch-status?project_id={pid}"
# Output: {"PENDING": 3, "PROCESSING": 1, "COMPLETED": 12}
```

## Task Status Meanings

| Status | Ý nghĩa |
|--------|---------|
| `registered` | Đang chờ dependencies hoàn thành |
| `active` | Đang chạy (agent đang execute) |
| `done` | Hoàn thành thành công |
| `error` | Bị lỗi (không block downstream) |
| `timeout` | Quá thời gian timeout |
