# Chính Sách Quyền Riêng Tư (Privacy Policy)

Chào mừng bạn đến với **Kaen Vocabulary Helper**. Tiện ích này được thiết kế theo hướng local-first: dữ liệu từ vựng của bạn được lưu vào app **Kaen** chạy ngay trên máy của bạn (mặc định `http://localhost:4500`), không có tài khoản, không đăng nhập, không máy chủ từ xa của chúng tôi.

## 1. Thu Thập và Sử Dụng Dữ Liệu

### Thông tin cá nhân

Tiện ích **không** thu thập bất kỳ thông tin nhận dạng cá nhân nào (tên, email, mật khẩu, token). Không có chức năng đăng nhập.

### Dữ liệu duyệt web

Tiện ích chỉ đọc từ/cụm từ bạn chủ động chọn (double-click, bôi đen, context menu) để tra cứu. Chúng tôi **không** theo dõi lịch sử duyệt web hay lưu trữ nội dung các trang bạn truy cập.

### Dữ liệu từ vựng

- Khi tra từ, từ đó được gửi tới các dịch vụ từ điển bên thứ ba: Cambridge Dictionary, Google Translate, Free Dictionary API (dictionaryapi.dev), và dictionary nội bộ của app Kaen trên máy bạn.
- Khi lưu từ, dữ liệu (từ, IPA, định nghĩa, ví dụ, nghĩa dịch) chỉ được gửi tới app Kaen chạy **local trên máy của bạn** — không rời khỏi máy bạn.
- Kết quả tra cứu được cache trong bộ nhớ của trình duyệt (chrome.storage.local) trong 7 ngày.

## 2. Giải Thích Về Các Quyền (Permissions)

- **storage**: lưu cài đặt (URL app Kaen, ngôn ngữ dịch, lesson đã chọn) và cache kết quả tra từ.
- **activeTab**: đọc từ đang được bôi đen trên tab hiện tại khi bạn mở popup.
- **contextMenus**: thêm tùy chọn tra từ vào menu chuột phải.
- **host_permissions**:
  - `http://localhost:4500/*`, `http://127.0.0.1:4500/*` — giao tiếp với app Kaen local.
  - `dictionary.cambridge.org`, `translate.googleapis.com`, `api.dictionaryapi.dev` — nguồn tra từ.

## 3. Chia Sẻ Dữ Liệu

Chúng tôi không bán, không chia sẻ dữ liệu của bạn với bất kỳ bên nào. Các truy vấn tra từ gửi tới dịch vụ bên thứ ba tuân theo chính sách riêng tư của các dịch vụ đó.

## 4. Liên Hệ

Mọi thắc mắc về chính sách này, vui lòng mở issue trong repository của dự án SenClaw.
