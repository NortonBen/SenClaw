/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viSettingsMisc = {
  // Provider Sign-in — page, headings, risk banner
  'Provider Sign-in': 'Đăng nhập nhà cung cấp',
  "Subscription sign-in is against the vendors' terms of service":
      'Đăng nhập bằng tài khoản thuê bao vi phạm điều khoản dịch vụ của nhà '
          'cung cấp',
  "Subscription credentials are licensed for each vendor's own clients. "
          'Using them from SenClaw can get the account suspended, and the '
          'vendors detect it. SenClaw identifies itself honestly rather than '
          'imitating the vendor client, so a provider that blocks third-party '
          'access returns a clear error instead of failing silently. For '
          'anything you depend on, use an API key.':
      'Thông tin đăng nhập thuê bao chỉ được cấp phép cho ứng dụng của chính '
          'nhà cung cấp. Dùng chúng từ SenClaw có thể khiến tài khoản bị đình '
          'chỉ, và nhà cung cấp phát hiện được điều đó. SenClaw tự xưng danh '
          'trung thực thay vì giả dạng ứng dụng của nhà cung cấp, nên nhà '
          'cung cấp nào chặn truy cập bên thứ ba sẽ trả về lỗi rõ ràng thay '
          'vì âm thầm thất bại. Với những gì bạn cần dùng ổn định, hãy dùng '
          'API key.',
  'Subscription accounts': 'Tài khoản thuê bao',
  'Could not load providers: {e}': 'Không tải được danh sách nhà cung cấp: {e}',
  'Free-tier providers': 'Nhà cung cấp gói miễn phí',
  'Ready-made endpoints with a free allowance. Each needs its own API key '
          'unless marked otherwise.':
      'Các endpoint dựng sẵn kèm hạn mức miễn phí. Mỗi nhà cung cấp cần API '
          'key riêng, trừ khi được ghi chú khác.',
  'Could not load the catalog: {e}': 'Không tải được danh mục: {e}',

  // Provider cards
  'Device code': 'Mã thiết bị',
  'Browser redirect': 'Chuyển hướng trình duyệt',
  'Expired': 'Đã hết hạn',
  'No expiry': 'Không hết hạn',
  '{t} left': 'còn {t}',
  'Use as model': 'Dùng làm model',
  'Refresh token': 'Làm mới token',
  'No refresh token — reconnect by hand':
      'Không có refresh token — hãy kết nối lại thủ công',
  'Disconnect': 'Ngắt kết nối',
  'Connect': 'Kết nối',
  'Add another': 'Thêm tài khoản khác',
  'Needs port 1455 free.': 'Cần cổng 1455 còn trống.',

  // Sign-in flow
  'Finish the sign-in in your browser.':
      'Hoàn tất đăng nhập trong trình duyệt của bạn.',
  'Connected {label}': 'Đã kết nối {label}',
  'Sign-in failed': 'Đăng nhập thất bại',
  'Connect {name}': 'Kết nối {name}',
  'Enter this code at {url}': 'Nhập mã này tại {url}',
  'Copy code': 'Sao chép mã',
  'Code copied': 'Đã sao chép mã',
  'Open page': 'Mở trang',
  'Waiting for approval — this closes on its own.':
      'Đang chờ phê duyệt — hộp thoại sẽ tự đóng.',
  'Token refreshed': 'Đã làm mới token',

  // Disconnect dialog
  'Disconnect {label}?': 'Ngắt kết nối {label}?',
  // Distinct from the connection-status 'Disconnected' in common.dart: this is
  // the toast after the user deliberately removes an account.
  'Account disconnected': 'Đã ngắt kết nối tài khoản',
  'SenClaw forgets the stored tokens. Any model bound to this account stops '
          'working until you connect again.':
      'SenClaw sẽ quên các token đã lưu. Mọi model gắn với tài khoản này sẽ '
          'ngừng hoạt động cho đến khi bạn kết nối lại.',

  // Bind-as-model dialog
  'Use {label} as a model': 'Dùng {label} làm model',
  'Creates a model entry backed by this account. No token is written into '
          'config.json — only a reference to the account.':
      'Tạo một mục model dựa trên tài khoản này. Không có token nào được ghi '
          'vào config.json — chỉ lưu tham chiếu đến tài khoản.',
  'unavailable': 'không khả dụng',
  'Test this model': 'Kiểm tra model này',
  'Model id': 'ID model',
  // Kept in English: the term used verbatim by every provider's own docs.
  'API key': 'API key',
  'Test all {n}': 'Kiểm tra cả {n}',
  'Add model': 'Thêm model',
  'Added "{label}"': 'Đã thêm "{label}"',

  // Free-tier presets
  'Add {name}': 'Thêm {name}',
  '{field} is required': 'Cần nhập {field}',
  'Added {name}': 'Đã thêm {name}',
  'No key': 'Không cần key',
  'needs {field}': 'cần {field}',
  'Get key': 'Lấy key',
};
