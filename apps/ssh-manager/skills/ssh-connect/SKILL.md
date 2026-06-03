---
name: ssh-connect
description: Hướng dẫn kết nối SSH và thực hiện lệnh trên server (SSH Connection Guide)
triggers:
  - "kết nối ssh"
  - "connect ssh"
  - "thực hiện lệnh"
  - "vào server"
---

# Hướng dẫn kết nối SSH (SSH Connection Guide)

Khi người dùng yêu cầu "kết nối SSH", "connect to ssh", hoặc thực hiện lệnh trên một server cụ thể, hãy làm theo các bước sau:

1. **Tìm Host ID**:
   - Sử dụng tool `ssh_list_hosts` để lấy danh sách các server đã được lưu.
   - Tìm server có tên (name) hoặc địa chỉ (host/IP) khớp với yêu cầu của người dùng.
   - Lấy `id` (host_id) của server đó.

2. **Bắt đầu kết nối (Start Connection)**:
   - Sử dụng tool `ssh_start_connect` và truyền vào tham số `host_id`.
   - Tool sẽ khởi tạo kết nối SSH và trả về một `connection_id` duy nhất. 
   - **Lưu ý:** Giao diện ứng dụng sẽ tự động mở một tab Terminal mới để người dùng có thể nhìn thấy tiến trình kết nối và các lệnh bạn sắp thực thi!

3. **Thực thi lệnh (Execute Commands)**:
   - Sử dụng tool `ssh_execute_command` với `connection_id` vừa nhận được ở Bước 2 để thực hiện các lệnh shell.
   - Bạn có thể gọi `ssh_execute_command` nhiều lần với cùng một `connection_id` nếu cần chạy nhiều lệnh.
   - Kết quả trả về và lệnh bạn chạy sẽ hiển thị trực tiếp trên tab Terminal của người dùng, giúp họ theo dõi tiến độ một cách trực quan.

4. **Đóng kết nối (Close Connection)**:
   - Khi hoàn thành tất cả các thao tác trên server, sử dụng tool `ssh_close_connect` với `connection_id` đó để đóng kết nối và giải phóng tài nguyên.
