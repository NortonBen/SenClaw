---
name: secscan-audit
description: >-
  Quét và đánh giá bảo mật website/máy chủ của chính người dùng qua app Quét Bảo
  Mật: security header, cờ cookie, lộ thông tin, tư thế DNS/email (SPF/DMARC/
  CAA/DNSSEC), chấm điểm A+..F, so sánh giữa các lần quét. Dùng khi người dùng
  hỏi site của họ có an toàn không, muốn kiểm tra lỗ hổng, security header, SSL,
  SPF/DMARC, hoặc muốn theo dõi tình trạng bảo mật theo thời gian.
triggers:
  - quét bảo mật
  - kiểm tra bảo mật
  - lỗ hổng
  - an toàn website
  - audit server
  - security header
  - kiểm tra ssl
  - kiểm tra dns
  - spf dmarc
  - site của tôi có an toàn không
  - scan bảo mật
  - đánh giá bảo mật
---

# secscan-audit

Dùng MCP server `secscan-mcp` của app **Quét Bảo Mật** (cổng 4690).

## Ranh giới — nói rõ với người dùng ngay từ đầu

App này **chỉ quét hạ tầng người dùng sở hữu**, và **chỉ đánh giá tư thế**:
nó không khai thác lỗ hổng, không brute-force, không DoS. Nếu người dùng đưa
một tên miền không phải của họ, **từ chối và giải thích**, đừng quét.

Quét hệ thống của người khác là hành vi bị cấm theo Điều 7.5 Luật An ninh mạng
116/2025. Ngược lại Điều 15.2.a đặt **nghĩa vụ** chủ hệ thống phải tự kiểm tra —
đó là chỗ đứng hợp pháp của việc này.

## Quy trình

1. `mcp__secscan-mcp__sec_asset_list` — xem đã có tài sản nào, id bao nhiêu,
   đã xác minh chưa. **Luôn gọi trước**, đừng đoán id.
2. Chưa có thì `sec_asset_add` (kind `website` | `host` | `domain`).
3. `sec_scan_web` — quét thụ động. **Không cần xác minh sở hữu** cho bước này:
   nó chỉ làm đúng những gì một trình duyệt bình thường làm.
4. Đọc kết quả bằng `sec_findings`. **Luôn lấy số từ tool**, đừng nhớ lại từ
   lượt trước — kết quả đổi sau mỗi lần quét.

## Quét chủ động (L2) — sec_scan_active

Dò tệp lộ ra ngoài (`.git/`, `.env`, `backup.sql`, kết xuất CSDL), liệt kê thư mục, và
cấu hình CORS. **Không cần xác minh sở hữu**: SenClaw là AI cá nhân, người dùng tự chịu
trách nhiệm target họ thêm.

Bắt được `package.json`/`composer.json` lộ ra ngoài thì L2 **đối chiếu OSV/KEV/EPSS luôn**
cho danh sách gói. KEV nâng thẳng mức lên nghiêm trọng bất kể CVSS — nghĩa là đang bị khai
thác thật, phải vá trước mọi thứ khác.

Nếu tài sản nằm trong dải nội bộ (127.0.0.1, 192.168, 10.x), phải xác minh bằng phương
thức `local` một lần trước — không phải cổng chặn, mà là RÀO SSRF để scanner không tự
biến thành công cụ tấn công. Không xác minh thì tool trả finding `active:unreachable` chỉ
đường sang đó.

Vẫn không khai thác, không brute-force. Nhịp cố ý thấp (~4 req/s, trần 40 yêu cầu) nên
chạy được trên production, nhưng **nếu `truncated` là `true` thì kết quả BÁN PHẦN** — phải
nói với người dùng, đừng trình bày như đã phủ hết.

Thấy `active:soft-404` nghĩa là máy chủ trả 200 cho mọi đường dẫn, nên các phát hiện tệp lộ
ở lần quét đó có độ tin cậy thấp hơn — nên nói rõ thay vì khẳng định chắc nịch.

## Quét máy chủ (L3) — sec_scan_host

Kiểm cấu hình SSHD, tường lửa, quyền tệp, gói OS quá hạn, và **đối chiếu OSV/KEV/EPSS
cho toàn bộ gói OS bắt được** — chỗ CVE trả lãi lớn nhất, một máy Debian điển hình có
hàng trăm gói. Chỉ đọc, có test cưỡng chế mọi lệnh đều không có động từ ghi.

Tài sản phải có `ssh_ref` trỏ tới id máy bên app `ssh-manager`. **secscan không bao giờ
giữ mật khẩu hay khoá riêng** — hai app tách nhau để một app bảo mật không lặp lại lỗi
"tự lưu credential plaintext" của app khác. Cần biến môi trường `SECSCAN_SSH_MANAGER_URL`
vì ssh-manager dùng cổng động; đoán cổng là sai.

## Xác minh sở hữu (tuỳ chọn) — sec_asset_verify_*

Không còn là điều kiện để quét, nhưng vẫn có ích cho hai tình huống:
- Đích nội bộ (127.0.0.1, LAN): dùng phương thức `local` để mở rào SSRF của scanner.
- Đánh dấu "tôi thật sự sở hữu cái này" cho báo cáo và kiểm toán về sau.

Độ mạnh giảm dần: `dns-txt` > `dns-cname` > `well-known` > `meta`. Dùng `dns-cname` khi
apex đã bị CNAME-flatten (Cloudflare, Netlify).

## Trả lời "quét những gì" — dùng sec_rules

`mcp__secscan-mcp__sec_rules` trả danh mục đầy đủ: mỗi phép kiểm kèm mức nặng tối đa,
**lý do** đặt mức đó, và cờ `implemented`. Gọi nó khi người dùng hỏi app kiểm gì, hỏi
"có kiểm X không", hoặc **trước khi kết luận "không có vấn đề"** — trường `not_covered`
liệt kê thẳng những loại lỗ hổng công cụ tự động không thấy được.

Đừng liệt kê lại danh mục từ trí nhớ: mục nào đã cài, mục nào chưa, đổi theo phiên bản.

## Tổng hợp nhanh — dùng sec_dashboard

`mcp__secscan-mcp__sec_dashboard` trả xu hướng điểm qua các lần quét, phân bố theo mức
và theo nhóm, 5 mục nặng nhất còn mở (KEV lên đầu), số mục tái phát và số đã chấp nhận
rủi ro. Dùng cái này khi người dùng hỏi "tình hình thế nào" — đừng đọc từng phát hiện rồi
tự cộng.

`regressed > 0` đáng nói riêng: đó là thứ đã từng được vá nhưng quay lại, thường vì bản vá
bị ghi đè khi triển khai hoặc chỉ sửa ở một máy chủ trong cụm.

## Thêm luật riêng — sec_rule_add

Luật tự thêm **chạy thật** trong mỗi lần quét, không phải ghi chú. Dạng khai báo: so khớp
trên header HTTP, thuộc tính cookie, hoặc bản ghi TXT. `id` phải bắt đầu bằng `custom:`.

Dùng khi người dùng có quy định nội bộ mà scanner mặc định không biết — ví dụ "mọi phản hồi
phải có `X-Request-Id`", "trang riêng tư phải `Cache-Control: no-store`".

Luật hỏng (biểu thức sai, mức lạ, id giả dạng luật dựng sẵn) **bị từ chối ngay lúc thêm** kèm
lý do — đọc lỗi rồi sửa, đừng thử lại y nguyên.

## Nhập bộ luật từ nguồn khác — sec_rule_import

**Mặc định chỉ xem trước.** Gọi lần đầu không kèm `apply` để lấy danh sách luật hợp lệ và
luật bị loại kèm lý do, **trình cho người dùng xem**, chỉ khi họ đồng ý mới gọi lại với
`apply: true`.

Đây không phải thủ tục hình thức: bộ luật từ nguồn ngoài đổi cách chấm điểm của mọi lần quét
sau đó. Nguồn URL đi qua cùng bộ chặn SSRF như đích quét và chỉ nhận `https`.

## Chỉnh luật dựng sẵn — sec_rule_override

Đổi mức hoặc tắt hẳn một luật dựng sẵn. Khớp theo **tiền tố**: `hdr:csp` phủ cả họ luật CSP.
`enabled: false` thì phát hiện bị **loại hẳn** khỏi kết quả, không phải hạ xuống info.

Luôn ghi `note` nêu lý do. Sáu tháng sau, một luật bị tắt mà không có lý do trông y hệt một
lỗ hổng bị bỏ quên.

## TLS — đọc cho đúng mức

`tls:cert:expired` là **NGHIÊM TRỌNG**, không phải "cao": trình duyệt đang chặn người dùng
ngay lúc đó, không phải rủi ro tương lai. Báo cho người dùng như một sự cố đang diễn ra.

`tls:cert:incomplete-chain` hay bị coi nhẹ vì "trên máy tôi vẫn vào được" — đúng, vì trình
duyệt máy tính tự bù bản trung gian qua AIA. Client di động và dòng lệnh thì không, nên lỗi
chỉ hiện với một phần người dùng. Nói rõ điều đó.

Nếu thấy `http:unreachable` **cùng với** một phát hiện TLS, đừng báo hai vấn đề: TLS mới là
nguyên nhân, HTTP hỏng chỉ là hệ quả.

## Đọc kết quả cho đúng

**Điểm và hạng dùng để so với chính mình theo thời gian**, không phải để khoe.
Sau lần quét thứ hai trở đi, `sec_diff` trả lời "từ lần trước tới giờ đổi gì"
tốt hơn hẳn việc đọc lại cả danh sách.

Mức độ đã được hiệu chỉnh theo hành vi trình duyệt thật — **đừng tự nâng lên**:

- `info` **không phải lỗi**. Thiếu `Referrer-Policy` là info vì mặc định của
  trình duyệt đã an toàn; thiếu `Permissions-Policy` là info vì Firefox và
  Safari không hỗ trợ header đó.
- "Không kiểm tra được DKIM" nghĩa là **không kiểm được**, không phải "không có
  DKIM". Selector không liệt kê được qua DNS.
- "Không truy vấn được ..." là lỗi tra cứu, **không phải kết luận thiếu bản ghi**.

## Nguyên tắc

- **Số liệu lấy từ tool, không lấy từ trí nhớ.**
- Khi báo cáo, **nói rõ app không phủ cái gì** — lấy đúng danh sách từ `sec_rules`
  (`not_covered`), đừng tự nhớ. Một báo cáo ngụ ý "sạch = an toàn" còn nguy hiểm hơn
  không có báo cáo.
- Ưu tiên vá theo thứ tự app trả về, đừng sắp lại theo cảm tính.
- `sec_asset_remove` **không hoàn tác được** — xoá cả lịch sử quét. Hỏi lại trước.
