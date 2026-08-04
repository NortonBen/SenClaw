---
name: ipscout-investigate
description: >-
  Điều tra một địa chỉ IP hoặc máy chủ qua app Điều Tra IP: IP đó của ai (ASN,
  tổ chức, dải CIDR, liên hệ abuse), ở đâu (địa lý kèm độ tin), traffic đi qua
  đâu (CDN/cloud đứng trước), có nằm trong danh sách chặn thư rác không; và quét
  cổng, nhận dạng ứng dụng + phiên bản trên từng cổng mở, đọc chứng thư TLS,
  đoán hệ điều hành. Dùng khi người dùng hỏi "IP này của ai", "IP ở đâu", "server
  tôi mở cổng nào", "cổng đó chạy gì", "máy chủ dùng hệ điều hành gì", "có phải
  sau Cloudflare không", hoặc muốn theo dõi bề mặt phơi ra Internet theo thời gian.
triggers:
  - điều tra ip
  - tra ip
  - ip này của ai
  - ip ở đâu
  - kiểm tra ip
  - quét cổng
  - port nào đang mở
  - cổng mở
  - server chạy hệ điều hành gì
  - server dùng gì
  - asn
  - whois
  - rdap
  - ip có bị blacklist không
  - máy chủ có sau cloudflare không
  - dịch vụ nào đang chạy
---

# ipscout-investigate

Dùng MCP server `ipscout-mcp` của app **Điều Tra IP** (cổng 4710).

## App **không** kiểm sở hữu — bạn phải kiểm

Cả hai lớp đều chạy được ngay: `ip_profile` (đọc RDAP/DNS/GeoIP/DNSBL, **không**
chạm mục tiêu) và `ip_scan_ports` (TCP connect thật, **có** chạm mục tiêu). App
tin người dùng chủ SenClaw có quyền với mục tiêu họ khai, nên không có bước xác
minh sở hữu.

**Trách nhiệm này rơi vào agent.** Trước khi gọi `ip_scan_ports`:

- Nếu người dùng đưa **máy chủ của bên thứ ba** (một trang web ngẫu nhiên, một
  IP lạ trong log không phải của họ) → **từ chối và giải thích**, đừng quét. Đưa
  họ dùng `ip_profile` (thụ động, chạy với IP bất kỳ) nếu chỉ cần biết IP đó của
  ai / ở đâu.
- Nếu không rõ mục tiêu là của ai → **hỏi trước khi quét**, đừng đoán.
- Quét hệ thống của người khác là hành vi bị cấm theo Điều 7.5 Luật An ninh mạng
  116/2025. Với hạ tầng của chính người dùng thì Điều 15.2.a lại đặt **nghĩa vụ**
  phải tự kiểm tra — đó là chỗ đứng hợp pháp.

App **không** khai thác lỗ hổng, **không** dò mật khẩu, **không** quét SYN/stealth,
**không** quét dải hàng loạt. Đó là ranh giới thiết kế, không phải phần chưa làm.
Đừng hứa với người dùng những việc đó.

Chốt kỹ thuật duy nhất còn tự chạy: app **tự chặn** các điểm cuối metadata cloud
(`169.254.169.254`, `168.63.129.16`, `100.100.100.200`, `fd00:ec2::254`,
`fd20:ce::254`) — không có ca dùng hợp lệ nào cho việc quét chúng.

## Quy trình

1. `ip_target_list` — xem đã có mục tiêu nào, id bao nhiêu. **Luôn gọi trước**,
   đừng đoán id.
2. Chưa có thì `ip_target_add` (nhận IP trần, tên miền, hoặc URL đầy đủ).
3. `ip_profile` — lập hồ sơ.
4. `ip_scan_ports` — sau khi đã xác nhận mục tiêu thuộc quyền người dùng.

## Ba cái bẫy phải nói ra, không được im lặng bỏ qua

### 1. IP sau CDN không phải máy chủ của người dùng

Khi `ip_profile` trả `network.fronted = true`, **mọi kết luận sau đó nói về CDN,
không nói về hạ tầng người dùng**: cổng mở là cổng của Cloudflare, banner là của
Cloudflare, hệ điều hành là của Cloudflare.

Phải nói thẳng điều này. Không nói thì người dùng đọc bản báo cáo và tưởng đang
xem máy chủ của mình — rồi đi vá nhầm máy. Nếu họ muốn biết hạ tầng thật thì
phải điều tra từ bên trong, tra từ ngoài vào không ra được.

### 2. Thành phố trong kết quả địa lý thường **không** dùng được

Luôn đọc `geo.confidence` trước khi trích con số. GeoIP là suy luận từ đăng ký,
không phải đo đạc:

- `confidence.country` = `cao` → nói được quốc gia.
- `confidence.city` = `trung bình` hoặc thấp hơn → nói là "khoảng", đừng khẳng định.
- `confidence.city` = `không dùng được` → **đừng nhắc tên thành phố nữa.** IP
  anycast được quảng bá ở nhiều châu lục; con số đó chỉ mô tả một PoP.
- `confidence.sources_agree = false` → hai CSDL độc lập không khớp. Nói ra, đừng
  chọn bừa một cái.

### 3. "Tra không được" khác "không có"

`reputation.unknown_count > 0` nghĩa là có danh sách chặn **chưa tra được**, không
phải IP sạch. Nguyên nhân phổ biến nhất: máy đang dùng resolver công cộng
(8.8.8.8 / 1.1.1.1) và Spamhaus từ chối các resolver đó. Tương tự với
`ptr.lookup_ok = false`.

Đừng bao giờ rút gọn thành "IP sạch" khi thật ra chưa hỏi được.

## Đọc kết quả đoán hệ điều hành

`ip_scan_ports` trả `os` với `confidence` (0–97) và **danh sách bằng chứng**.
Đây **không phải** vân tay ngăn xếp TCP/IP kiểu `nmap -O` — cách đó cần raw
socket và gửi gói dị dạng, app cố tình không làm.

- Luôn kèm phần trăm khi nói, đừng phát biểu như sự thật đã kiểm chứng.
- `conflicts` không rỗng → bằng chứng chỉ về nhiều hệ khác nhau, thường là có
  proxy/load balancer đứng trước. Nói ra thay vì chọn cái điểm cao hơn.
- `os = null` → **không kết luận được, và đó là chuyện bình thường**. Máy chủ
  làm cứng đúng cách sẽ gỡ nhãn phân phối khỏi banner và giấu header `Server`.
  Đừng biến "không có bằng chứng" thành "chắc là Linux".
- Trước khi kết luận "máy chủ này ổn", đọc `os.not_covered` và `ip_capabilities`
  → `never_does`: chúng liệt kê thẳng những gì công cụ không thấy được.

## Theo dõi theo thời gian

Mỗi lần chạy là một **ảnh chụp** độc lập. Dùng `ip_runs` lấy id rồi `ip_diff` để
trả lời "từ lần trước tới giờ có gì đổi":

- `opened` — cổng vừa mở thêm. Đáng chú ý nhất.
- `closed` — cổng đã đóng.
- `changed` — **đổi phiên bản dịch vụ**. Nghĩa là ai đó vừa cập nhật — hoặc vừa
  cài đè lên máy chủ. Danh sách phẳng không bao giờ chỉ ra được điều này.
- `ip_changed` — mục tiêu đã trỏ về máy khác; mọi so sánh cổng bên dưới là giữa
  hai máy khác nhau, phải nói rõ.

`ip_diff` chỉ so hai lần **quét cổng**, không so hồ sơ với quét cổng.

## Trả lời "app tra được gì" — dùng ip_capabilities

`ip_capabilities` trả danh mục đầy đủ: từng lớp làm gì, giới hạn cứng, và
`never_does`. Gọi nó khi người dùng hỏi app làm được gì, hỏi "có kiểm X không",
hoặc **trước khi kết luận "không có vấn đề"**.

## Luôn lấy số từ tool

Kết quả đổi sau mỗi lần điều tra. Gọi `ip_findings` / `ip_dashboard` thay vì nhớ
lại từ lượt trước.
