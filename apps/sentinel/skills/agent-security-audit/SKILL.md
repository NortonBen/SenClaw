---
name: agent-security-audit
description: Điều tra bảo mật hoạt động của AI agent trong SenClaw qua lịch sử lịch và lịch sử hoạt động. Dùng khi người dùng hỏi hệ thống có bị xâm nhập không, có hoạt động bất thường không, agent đã chạy lệnh gì, lịch nào đáng ngờ, có prompt injection hay rò rỉ dữ liệu không, hoặc muốn kiểm tra tư thế bảo mật tổng thể.
triggers:
  - điều tra bảo mật
  - kiểm tra bảo mật
  - audit bảo mật
  - an ninh agent
  - bảo mật ai agent
  - agent có bị chiếm quyền
  - có ai xâm nhập
  - hoạt động bất thường
  - lịch sử hoạt động agent
  - lịch sử lịch
  - kiểm tra lịch đáng ngờ
  - agent tự đặt lịch
  - prompt injection
  - tool poisoning
  - rò rỉ dữ liệu
  - exfiltration
  - ai đã chạy lệnh gì
  - tool nào đã chạy
  - phê duyệt bị bỏ qua
  - human in the loop
  - sentinel
  - security audit
  - agent forensics
  - incident investigation
---

# Điều tra bảo mật AI Agent bằng Sentinel

Sentinel là lớp **phát hiện** (detective) cho chính SenClaw. Nó đọc dấu vết hoạt
động của agent ở chế độ chỉ-đọc, chạy luật phát hiện tất định, và giúp con người
điều tra. Nó **không** chặn hành động — việc chặn là của permission gate trong lõi.

## Nguyên tắc trả lời

1. **Phân biệt bằng chứng với suy đoán.** Nhiều luật dựa trên tương quan thời
   gian, không phải nhân quả. Luôn nói rõ điều gì đã xác định và điều gì chỉ là
   giả thuyết.
2. **Không kết luận "đã bị tấn công" khi chưa đủ chứng cứ.** Phần lớn phát hiện
   có một giải thích vô hại. Hãy nêu cả hai khả năng.
3. **Không bao giờ đề xuất tự sửa cấu hình.** Sentinel cố ý không có tool ghi.
   Nếu cần tắt một luật auto-accept hay tạm dừng một lịch, hướng dẫn người dùng
   tự làm trong giao diện SenClaw.
4. Nếu người dùng hỏi chung chung ("có gì bất thường không"), bắt đầu bằng
   `sen_dashboard` rồi mới đi sâu.

## Quy trình chuẩn

### Bước 1 — nắm tư thế hiện tại

```
mcp__sentinel-mcp__sen_dashboard
```

Đọc khối `posture`. Bốn câu hỏi quan trọng nhất:
- `hitl_disabled` — phê duyệt của con người có đang bị tắt không?
- `wildcard_autoaccept_rules` — có luật nào cho qua nguyên một server rủi ro?
- `apps_exposed_on_lan` — có Space App nào nghe được từ máy khác trong mạng?
- `shell_schedules` — có lịch nào chạy shell tuỳ ý?

Nếu kho chưa có dữ liệu mới, chạy `sen_ingest` rồi `sen_scan` trước.

### Bước 2 — đọc hàng đợi phát hiện

```
mcp__sentinel-mcp__sen_findings   {"status": "open", "limit": 30}
```

Xếp sẵn theo điểm. Với phát hiện đáng chú ý:

```
mcp__sentinel-mcp__sen_finding_detail   {"id": <id>}
```

Trả về cả mô tả luật và toàn bộ sự kiện chứng cứ. Đọc chứng cứ trước khi kết luận.

### Bước 3 — điều tra bằng pivot

Đây là bước phân biệt "báo động" với "hiểu chuyện gì đã xảy ra". Từ một sự kiện:

```
mcp__sentinel-mcp__sen_pivot   {"id": <event_id>, "mode": "preceding"}
```

- `preceding` — chuyện gì xảy ra **ngay trước**. Dùng để tìm nguồn injection: nếu
  agent làm việc lạ, cái gì đã đi vào ngữ cảnh của nó trước đó?
- `actor` — mọi việc cùng một phiên/lịch trong cửa sổ thời gian.
- `tool` — mọi lần dùng cùng tool đó, xem có thành mẫu lặp không.
- `schedule` — toàn bộ lần chạy của một lịch.

### Bước 4 — kiểm tra lịch (khi nghi ngờ cắm chốt)

Các luật `SEN-PERSIST-*` nhắm đúng vào việc này:

| Mã | Ý nghĩa |
|---|---|
| `SEN-PERSIST-01` | Lịch tạo qua MCP chứ không qua giao diện — đường agent dùng được |
| `SEN-PERSIST-02` | Lịch chế độ `script` chạy `bash -c` không qua bất kỳ kiểm tra nào |
| `SEN-PERSIST-03` | Lệnh của lịch chứa `curl`/`base64 -d`/`crontab`… |
| `SEN-PERSIST-04` | Lịch đã bị xoá nhưng còn nhật ký chạy |
| `SEN-PERSIST-05` | Lịch được tạo ngay sau một dấu hiệu injection |
| `SEN-PERSIST-07` | Lịch `isolated` báo thành công nhưng thực tế chưa từng chạy |

`SEN-PERSIST-01` là **suy đoán** — lịch tạo bằng CLI cũng rơi vào nhóm này. Cách
kiểm chứng: hỏi người dùng có chủ động đặt lịch đó không.

### Bước 5 — khôi phục đối số tool khi cần

Bảng `tool_executions` của daemon chỉ lưu **kết quả**, không lưu tham số. Muốn
biết agent đã đọc file nào, gọi URL nào:

```
mcp__sentinel-mcp__sen_tool_args   {"date": "2026-07-31"}
```

Lấy từ `~/.senclaw/llm_logs`, đã lọc bí mật. Chỉ giữ 30 ngày gần nhất.

### Bước 6 — gom thành vụ việc

Khi nhiều phát hiện thật ra là một sự việc:

```
mcp__sentinel-mcp__sen_case_open   {"title": "...", "finding_ids": [1, 2, 3]}
mcp__sentinel-mcp__sen_case_hypothesis   {"id": <case_id>}
mcp__sentinel-mcp__sen_case_report   {"id": <case_id>}
```

`sen_case_hypothesis` luôn kèm một giả thuyết vô hại để đối chứng — đọc nó nghiêm
túc, đừng bỏ qua.

## Kiểm tra tính toàn vẹn của chính chứng cứ

```
mcp__sentinel-mcp__sen_verify_chain
```

Kho sự kiện có chuỗi băm. Nếu gãy, một bản ghi quá khứ đã bị sửa hoặc xoá — và
mọi kết luận từ điểm đó trở đi đều không còn đáng tin. Nên chạy trước khi viết
báo cáo.

## Giảm nhiễu đúng cách

Nếu một luật kêu vì lý do chính đáng của môi trường này (ví dụ máy vốn chạy tác
vụ đêm nên `SEN-ANOM-01` luôn kêu):

```
mcp__sentinel-mcp__sen_suppress   {"rule_id": "SEN-ANOM-01", "reason": "máy chạy tác vụ nền ban đêm theo thiết kế", "until": "2027-01-01T00:00:00Z"}
```

Lý do là **bắt buộc**. Không bao giờ tắt hẳn một luật chỉ vì nó ồn — hãy thu hẹp
bằng suppression có hạn dùng, để sáu tháng sau còn biết vì sao.

## Điều Sentinel KHÔNG thấy được

Nói rõ những giới hạn này khi báo cáo, đừng tạo cảm giác an toàn giả:

- Đối số tool không có trong DB (chỉ có trong `llm_logs`, giữ 30 ngày).
- Lệnh shell của lịch `script` không được ghi vào nhật ký chạy.
- Lần auto-approve không để lại bản ghi nào — chỉ đo được gián tiếp qua
  `SEN-CTRL-02`.
- Hành động chi tiết trong trình duyệt không được ghi.
- Việc xoá lịch không được ghi lại; chỉ suy ra từ nhật ký mồ côi.
- Sentinel chạy theo mẻ (mỗi phút), nên luôn đến **sau** sự việc.
