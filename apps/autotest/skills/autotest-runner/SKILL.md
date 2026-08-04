---
name: autotest-runner
description: >-
  Tự động hoá kiểm thử qua app Tự Động Kiểm Thử: tạo bộ kiểm thử (suite) và test case
  3 loại — API HTTP, script/CLI, web UI (qua Mini Browser) — với assertion chi tiết,
  biến môi trường {{var}} và trích biến nối chuỗi giữa các case; chạy test và đọc kết
  quả từng assertion, xem lịch sử, phát hiện test flaky, đặt lịch chạy định kỳ, nhờ AI
  sinh test case từ mô tả/OpenAPI/curl và chẩn đoán lần chạy fail. Dùng khi người dùng
  nói về kiểm thử, chạy test, test case, smoke/regression test, test fail hay pass rate.
triggers:
  - kiểm thử
  - tự động hoá kiểm thử
  - chạy test
  - test case
  - bộ kiểm thử
  - test suite
  - test api
  - test tự động
  - kết quả test
  - test fail
  - test flaky
  - smoke test
  - regression test
  - viết test
  - sinh test case
  - automation test
  - run tests
  - test report
  - pass rate
---

# autotest-runner

Dùng MCP server `autotest-mcp` của app **Tự Động Kiểm Thử** (port 4640). App chỉ chạy
test do NGƯỜI DÙNG tự định nghĩa trên máy họ — dữ liệu 100% local (SQLite).

## Mô hình dữ liệu

- **Suite** → chứa các **test case** chạy theo thứ tự `position`. Mỗi case có `kind`:
  - `http` — gọi API: `config = {method, url, headers{}, body}` (body là chuỗi hoặc
    object JSON — object tự gửi kèm `Content-Type: application/json`).
  - `script` — lệnh shell: `config = {command, cwd, env{}}`.
  - `web` — điều khiển app **Mini Browser** (port 4360, PHẢI đang chạy):
    `config = {steps: [{action:"navigate",url}|{action:"act",instruction}|{action:"wait",ms}]}`.
    `act` nhận mô tả tiếng tự nhiên và tự kiểm chứng qua Mini Browser.
- **Environment** — bộ biến `{{var}}` (base_url, token…). Suite trỏ env mặc định qua
  `env_id`; mỗi lần chạy có thể override.
- **Assertions** (mảng trong case):
  - http: `{type:"status",op,value}` · `{type:"json",path:"data.x",op,value}` (op:
    `eq|ne|lt|lte|gt|gte|contains|exists|not_exists`) · `{type:"body_contains",value}` ·
    `{type:"header",name,value}` · `{type:"duration_max_ms",value}`
  - script: `{type:"exit_code",value}` · `{type:"stdout_contains",value}` ·
    `{type:"stdout_matches",value:"regex"}` · `{type:"stderr_contains",value}`
  - web: `{type:"text_contains",value}` · `{type:"text_not_contains",value}` ·
    `{type:"url_contains",value}`
- **Extract** — trích biến từ kết quả cho các case SAU trong cùng lần chạy (login lấy
  token → case sau dùng `{{token}}`): `{var,from:"json",path}` · `{var,from:"header",name}`
  · `{var,from:"regex",pattern}` (regex chạy trên body/stdout/text trang, lấy group 1).

## Chọn công cụ

- **`mcp__autotest-mcp__autotest_report`** — LUÔN gọi trước khi trả lời câu hỏi tổng
  quan ("test dạo này thế nào", "có test nào hay hỏng không"): xu hướng pass 30 run,
  test flaky kèm chuỗi kết quả, case fail nhiều nhất 30 ngày.
- **`mcp__autotest-mcp__autotest_suite_get`** — LUÔN gọi trước khi sửa case: trả về
  config/assertions/extract hiện tại. `autotest_case_update` THAY THẾ nguyên khối
  trường được truyền.
- **`mcp__autotest-mcp__autotest_run_suite` / `autotest_run_case`** — chạy và ĐỢI kết
  quả chi tiết từng assertion (desc/pass/actual/expected) + log. Suite dài chạy vài
  phút; case bị cap 10 phút. Chạy một case để debug nhanh, cả suite để kiểm tra đủ.
- **`mcp__autotest-mcp__autotest_run_get`** — soi lại một lần chạy cũ khi cần biết
  chính xác assertion nào lệch, giá trị thực tế là gì.
- **`mcp__autotest-mcp__autotest_env_set`** — tạo/sửa environment TRƯỚC khi viết case,
  để case dùng `{{base_url}}` thay vì hard-code URL.
- **`mcp__autotest-mcp__autotest_ai_generate`** — sinh test case từ mô tả/OpenAPI/curl.
  `apply:false` để xem trước không ghi. AI được gợi ý biến environment sẵn có.
- **`mcp__autotest-mcp__autotest_ai_diagnose`** — chẩn đoán run fail: phân biệt lỗi
  sản phẩm vs lỗi test, đề xuất bước sửa.
- **`mcp__autotest-mcp__autotest_schedule_set`** — lịch chạy định kỳ theo phút
  (`interval_min ≥ 1`); scheduler của app tự chạy nền, không cần cron ngoài.

## Nguyên tắc

- **Số liệu lấy từ tool.** Pass rate, flaky, số lần fail… đều do app tính — không đếm
  tay từ trí nhớ.
- **Test web cần Mini Browser.** Case `web` lỗi "không gọi được Mini Browser" nghĩa là
  app Mini Browser (port 4360) chưa chạy — bảo người dùng mở app đó trước, đừng đoán
  lỗi khác.
- **Suite fail ≠ sản phẩm hỏng.** Dùng `autotest_ai_diagnose` hoặc đọc log/assertion để
  phân biệt lỗi test (URL/biến/assertion sai, môi trường chưa bật) với lỗi sản phẩm
  thật trước khi kết luận.
- **Không xoá bừa.** `autotest_suite_delete` mất cả lịch sử run — ưu tiên archive
  (`autotest_suite_update status="archived"`).
