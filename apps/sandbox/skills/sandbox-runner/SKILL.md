---
name: sandbox-runner
description: >-
  Chạy lệnh shell và mã nguồn của người dùng trong môi trường cách ly khỏi máy
  thật, qua app Sandbox (Space App). Dùng khi người dùng muốn chạy thử một đoạn
  Python/JavaScript/Bash, tính toán bằng code, kiểm tra một lệnh có chạy đúng
  không, chạy code lạ hoặc code tải từ đâu đó mà chưa tin, cần cài thư viện rồi
  chạy, hoặc nói thẳng là muốn "chạy trong sandbox / máy ảo / docker". Ví dụ:
  "chạy thử đoạn python này", "tính hộ tôi bằng code", "đoạn script này có an
  toàn không, chạy thử xem", "cài pandas rồi chạy file này", "chạy lệnh này
  nhưng đừng động vào máy tôi".
---

# Sandbox Runner

Bạn điều khiển app **Sandbox** qua MCP server `sandbox-mcp`. Tên tool đầy đủ
dạng `mcp__sandbox-mcp__sbx_*`.

## Chọn công cụ: một lần hay nhiều bước

Đây là quyết định đầu tiên và hầu hết yêu cầu rơi vào vế trái.

| Tình huống | Dùng |
|---|---|
| Chạy một đoạn mã, xem kết quả, xong | `sbx_run` — tạo sandbox tạm, chạy, tự xoá |
| Nhiều bước nối nhau, cần giữ file / gói đã cài | `sbx_create` rồi `sbx_run_in` / `sbx_exec` |

Đừng `sbx_create` cho một phép tính. Sandbox tạo ra mà không xoá sẽ đọng lại.

## Trình tự chuẩn

1. **`sbx_run`** với `language` + `code`. Xong. Không cần gọi gì trước đó — nếu
   máy không chạy được, lỗi trả về sẽ nói rõ lý do và cách sửa.
2. Nếu lỗi nói backend không dùng được → gọi `sbx_capabilities` để đọc chi tiết,
   rồi báo người dùng đúng việc họ cần làm (thường là mở Docker Desktop).
3. Việc nhiều bước: `sbx_create` → `sbx_file_write` (đưa dữ liệu vào) →
   `sbx_run_in` / `sbx_exec` → `sbx_delete` khi xong.

## Bảng tool

| Tool | Dùng để |
|---|---|
| `mcp__sandbox-mcp__sbx_capabilities` | Máy này chạy được kiểu cách ly nào |
| `mcp__sandbox-mcp__sbx_run` | **Mặc định** — chạy một đoạn mã rồi dọn sạch |
| `mcp__sandbox-mcp__sbx_create` | Tạo sandbox tồn tại lâu |
| `mcp__sandbox-mcp__sbx_list` | Liệt kê sandbox đang có |
| `mcp__sandbox-mcp__sbx_exec` | Chạy lệnh shell trong sandbox đã có |
| `mcp__sandbox-mcp__sbx_run_in` | Chạy đoạn mã trong sandbox đã có |
| `mcp__sandbox-mcp__sbx_install` | Cài gói pip / npm / apt |
| `mcp__sandbox-mcp__sbx_file_write` | Đưa dữ liệu vào sandbox |
| `mcp__sandbox-mcp__sbx_file_read` | Đọc file kết quả |
| `mcp__sandbox-mcp__sbx_files` | Liệt kê file |
| `mcp__sandbox-mcp__sbx_update` | Bật/tắt mạng, đổi CPU/RAM |
| `mcp__sandbox-mcp__sbx_delete` | Xoá sandbox |
| `mcp__sandbox-mcp__sbx_runs` | Lịch sử các lần chạy |
| `mcp__sandbox-mcp__sbx_stats` | CPU/RAM đang dùng + danh sách tiến trình |
| `mcp__sandbox-mcp__sbx_kill` | Dừng một tiến trình, hoặc dừng tất cả |
| `mcp__sandbox-mcp__sbx_mount` | Gắn thư mục thật trên máy vào sandbox |
| `mcp__sandbox-mcp__sbx_unmount` | Gỡ thư mục đã gắn |
| `mcp__sandbox-mcp__sbx_fs_mode` | Đổi mức cách ly ĐỌC đĩa của một sandbox |
| `mcp__sandbox-mcp__sbx_settings` | Xem/đổi cài đặt mặc định của app |
| `mcp__sandbox-mcp__sbx_trace` | Bật/tắt theo dõi hoạt động (cho kiểm thử) |
| `mcp__sandbox-mcp__sbx_events` | Xem file/tiến trình/mạng đã ghi nhận được |

## Theo dõi hoạt động (kiểm thử)

Mặc định TẮT. Bật bằng `sbx_trace` rồi **chạy lại mã**, sau đó `sbx_events`.

Ghi nhận được: đọc/ghi file, khởi tạo tiến trình (kèm argv), kết nối mạng (kèm
địa chỉ), tra cứu tên miền (kèm hostname). Lọc bằng `kind` = `file` | `proc` |
`net`, hoặc `runId` để chỉ xem một lần chạy.

Hoạt động bằng hook trong tiến trình: Python qua `sys.addaudithook`, Node qua
`--require`; ngôn ngữ khác thì so sánh thư mục sandbox trước/sau nên vẫn thấy
được file bị ghi. Hook lan sang cả tiến trình con.

**Đây là công cụ quan sát cho kiểm thử, KHÔNG phải bằng chứng an ninh.** Hook
chạy bên trong sandbox và nhật ký nằm trong thư mục sandbox — mã cố tình lẩn
tránh thì né được. Đừng bao giờ nói với người dùng "nhật ký sạch nên đoạn mã này
an toàn". Thứ thật sự chặn được mã độc là bản thân sandbox (cách ly đọc, ghi,
mạng), do nhân hệ điều hành cưỡng chế.

Dùng nó để trả lời "đoạn mã này *thực sự* đụng vào những gì" khi kiểm thử, và để
chỉ ra hành vi đáng ngờ — ví dụ một script cài đặt lại đi đọc `~/.aws` hay gọi
ra một tên miền lạ.

## Ba mức cách ly ĐỌC đĩa

Ghi thì luôn bị nhốt trong thư mục sandbox. Đọc thì có ba mức, đổi bằng
`sbx_fs_mode` hoặc đặt lúc `sbx_create` qua `fsMode`:

| Mức | Sandbox đọc được gì |
|---|---|
| `strict` (**mặc định**) | Thư mục sandbox + thư mục đã gắn + thư viện hệ thống |
| `allowlist` | Như `strict`, cộng các thư mục khai sẵn trong cài đặt app |
| `open` | Cả đĩa, trừ `~/.ssh`, `~/.aws`, Keychain, dữ liệu SenClaw |

**Đừng vội hạ xuống `open`.** Mặc định `strict` nghĩa là mã không đọc được dữ
liệu người dùng — đó là điểm bán hàng của app. Khi đoạn mã cần một thư mục cụ
thể, cách đúng là `sbx_mount` đúng thư mục đó (chỉ đọc nếu được), **không** phải
mở toang cả đĩa. Chỉ dùng `open` khi người dùng bảo thẳng như vậy.

Backend `docker` không dùng thiết lập này — container vốn đã cách ly toàn bộ.

`sbx_settings` đổi mặc định cho sandbox **tạo mới**; sandbox đang có giữ nguyên.
Lưu ý `allowlist` trong `sbx_settings` **thay thế** cả danh sách chứ không thêm
vào — đọc danh sách hiện tại trước rồi gửi lại đầy đủ.

## Theo dõi và dừng

`sbx_stats` trả về CPU tổng, RAM tổng và từng tiến trình (pid, %CPU, RAM, thời
gian chạy, lệnh). Dùng khi người dùng hỏi "còn chạy không", "sao máy chậm", hoặc
trước khi quyết định dừng cái gì.

`sbx_kill` không có `pid` = dừng tất cả. Có `pid` = dừng đúng tiến trình đó (lấy
pid từ `sbx_stats`). Chỉ dừng được tiến trình của chính sandbox — pid lạ sẽ bị
từ chối, nên đừng thử dùng nó để dừng thứ gì khác trên máy.

Với backend docker, "dừng tất cả" là khởi động lại container. File trong sandbox
và gói đã cài vẫn còn.

## Gắn thư mục từ máy thật

`sbx_mount` cho mã trong sandbox đọc/ghi thẳng vào một thư mục có thật trên máy.
Đây là **lỗ hổng có chủ ý** trên hàng rào sandbox, nên:

- **Mặc định nên đặt `readOnly: true`.** Chỉ mở ghi khi công việc thật sự cần
  ghi ra, và nói cho người dùng biết.
- **Mã nguồn chưa tin được thì đừng gắn gì cả**, hoặc chỉ gắn đúng thư mục dữ
  liệu cần thiết ở chế độ chỉ đọc.
- Thư mục nhà, thư mục hệ thống và nơi chứa khoá bí mật sẽ bị từ chối — đó là
  chủ ý, đừng tìm đường lách bằng thư mục con của chúng.
- Mã trong sandbox thấy thư mục tại `<tên target>` ngay trong thư mục gốc
  sandbox, ví dụ gắn `target: "du-lieu"` thì đọc bằng `open("du-lieu/a.csv")`.
- `sbx_unmount` chỉ gỡ liên kết, **không** xoá dữ liệu thật.

## Hai backend, khác nhau ở đâu

| | `direct` (chạy trực tiếp) | `docker` |
|---|---|---|
| Cần gì | Không cần gì | Docker daemon đang chạy |
| Khởi động | Tức thì | Vài giây, lần đầu phải tải image |
| Chặn ghi ra ngoài sandbox | Có (Seatbelt / bubblewrap) | Có |
| Chặn đọc ~/.ssh, ~/.aws, Keychain | Có | Có |
| Chặn đọc phần còn lại của đĩa | Có, ở mức `strict`/`allowlist` (mặc định) | Có |
| Windows | Không | Có |

**Bỏ trống `backend` là đúng trong hầu hết trường hợp** — app tự chọn theo
khả năng thật của máy.

Chọn `docker` khi mã nguồn thật sự đáng ngờ (người dùng dán từ trên mạng, từ
email, từ một repo lạ) và bạn muốn ranh giới chắc nhất — nhưng `direct` ở mức
`strict` cũng đã chặn đọc dữ liệu người dùng rồi.

## Quy tắc bắt buộc

- **Mạng mặc định TẮT.** Chỉ bật (`network: true`) khi công việc cần mạng và
  **nói cho người dùng biết bạn đang bật**. Đoạn mã lạ + mạng bật = dữ liệu có
  thể bị gửi đi.
- **Cài gói thì phải có mạng.** `sbx_install` sẽ từ chối nếu sandbox đang tắt
  mạng; bật bằng `sbx_update` trước, và nói lý do.
- **Đọc `isolation` trong kết quả trả về.** Nó cho biết mức cách ly nào ĐÃ THỰC
  SỰ được áp dụng cho lần chạy đó: `seatbelt`, `bubblewrap`, `container` hay
  `degraded`.
- **`degraded` nghĩa là KHÔNG có rào chắn nào.** Máy thiếu công cụ cách ly. Phải
  báo người dùng trước khi chạy tiếp, đừng chạy rồi mới nói.
- **Gắn thư mục thì mặc định chỉ đọc.** Xem mục "Gắn thư mục từ máy thật".
- **Mã báo "không tìm thấy file" mà file có thật** thường là do `strict` chặn
  đọc, không phải sai đường dẫn. Gắn thư mục đó vào, đừng đổi sang `open`.
- **`purge: true` xoá sạch file, không khôi phục được.** Chỉ dùng khi người dùng
  bảo xoá hẳn, hoặc với sandbox tạm do chính bạn tạo.

## Đọc kết quả

Mỗi lần chạy trả về:

- `ok` — chạy xong và mã thoát bằng 0
- `exitCode` — `null` nghĩa là bị giết (quá giờ), không phải thành công
- `timedOut` — quá hạn; **kết quả in ra trước đó có thể đã mất**, đừng kết luận
  là mã sai, hãy nói là quá giờ và hỏi có tăng `timeoutMs` không
- `truncated` — output quá dài đã bị cắt; số liệu cuối có thể thiếu
- `isolation` — mức cách ly thật sự
- `stdout` / `stderr`

Trình bày cho người dùng: kết quả trước, mức cách ly sau, một dòng là đủ. Đừng
dán lại toàn bộ mã họ vừa đưa.

## Khi Docker không chạy

Đây là tình huống hay gặp nhất. `sbx_capabilities` trả về `docker.detail` với
lý do đo được — thường là "Docker CLI có nhưng daemon chưa trả lời".

Việc cần làm: nói người dùng mở Docker Desktop, rồi gọi lại
`sbx_capabilities` với `refresh: true`. **Đừng** kết luận là máy không chạy được
sandbox — backend `direct` gần như luôn dùng được trên macOS và Linux.
