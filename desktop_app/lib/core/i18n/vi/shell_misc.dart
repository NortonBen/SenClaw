/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
///
/// Area: the app frame (nav rail, connection dot, version label), the boot
/// splash / daemon-crash screen, the updater, OS notifications, the screenshot
/// capture flow and the shared widgets (schedule editor, note body, section
/// scaffold, embedded web fallback).
const Map<String, String> viShellMisc = {
  // ── Nav rail (lib/app/nav.dart labels, wrapped in shell.dart) ──────────
  // "Kanban" / "Wiki" are product names and stay as-is.
  'Dashboard': 'Tổng quan',
  'Chat': 'Trò chuyện',
  'Apps': 'Ứng dụng',
  'Calendar': 'Lịch',
  'Plugins': 'Plugin',
  'Background': 'Chạy nền',
  'Usage': 'Mức dùng',

  // ── Shell footer: update badge + connection dot ────────────────────────
  'SenClaw {v} is available.': 'Đã có SenClaw {v}.',
  'Update available': 'Có bản cập nhật',

  // ── Startup "update available" popup (update_announcer.dart) ───────────
  'A new version of SenClaw is available': 'Đã có phiên bản SenClaw mới',
  'Version {v} — you are running {current}.':
      'Phiên bản {v} — bạn đang dùng {current}.',
  'View update': 'Xem cập nhật',
  'Remind me later': 'Nhắc lại sau',
  'Skip this version': 'Bỏ qua bản này',

  'Offline': 'Ngoại tuyến',
  'Daemon: {status} · open Diagnostics': 'Daemon: {status} · mở Chẩn đoán',

  // ── Startup gate (boot splash / daemon crash) ──────────────────────────
  'Starting SenClaw daemon…': 'Đang khởi động daemon SenClaw…',
  'Connecting to the SenClaw daemon…': 'Đang kết nối tới daemon SenClaw…',
  'Daemon started': 'Daemon đã chạy',
  'Cannot reach the SenClaw daemon': 'Không kết nối được daemon SenClaw',
  'Free the port and retry': 'Giải phóng cổng rồi thử lại',

  // ── Updater (update_service.dart / update_provider.dart) ───────────────
  'This is a dev build — updates are disabled.':
      'Đây là bản dev — cập nhật bị tắt.',
  'Updates are disabled in a dev build.': 'Bản dev không hỗ trợ cập nhật.',
  'Could not reach the update server.':
      'Không kết nối được máy chủ cập nhật.',
  'Release {v} has no bundle for this platform ({target}).':
      'Bản phát hành {v} không có gói cho nền tảng này ({target}).',
  'Version {from} is too old to update directly to {to} — reinstall from the '
          'SenClaw website.':
      'Phiên bản {from} quá cũ để cập nhật thẳng lên {to} — hãy cài lại từ '
          'trang SenClaw.',
  'Cannot write to {dir} — the app was installed by another user. Update from '
          'a terminal instead: senclaw update':
      'Không ghi được vào {dir} — ứng dụng do người dùng khác cài. Hãy cập '
          'nhật từ terminal: senclaw update',
  'Download failed (HTTP {code}).': 'Tải về thất bại (HTTP {code}).',
  'Download cancelled.': 'Đã huỷ tải về.',
  'Download failed: {e}': 'Tải về thất bại: {e}',
  'Cannot find the senclaw binary to run the update with.':
      'Không tìm thấy binary senclaw để chạy cập nhật.',
  'Could not start the updater: {e}':
      'Không khởi động được trình cập nhật: {e}',

  // ── OS notifications (system_notifier.dart) ────────────────────────────
  'Reminder': 'Nhắc việc',
  'Calendar reminder': 'Nhắc việc từ lịch',
  'Upcoming': 'Sắp tới',
  'Scheduled activity': 'Hoạt động theo lịch',

  // ── Screenshot capture: permission + error cards ───────────────────────
  'Screen Recording permission required': 'Cần quyền ghi màn hình',
  'Enable SenClaw in System Settings → Privacy & Security → Screen '
          'Recording.':
      'Bật SenClaw trong System Settings → Privacy & Security → Screen '
          'Recording.',
  'Already enabled but still asked? macOS only picks this permission up after '
          'a restart. Quit and reopen SenClaw.':
      'Đã bật rồi mà vẫn hỏi? macOS chỉ nhận quyền này sau khi khởi động lại. '
          'Thoát và mở lại SenClaw.',
  'Open Settings': 'Mở Settings',
  'Quit to reopen': 'Thoát để mở lại',
  'Screenshot failed': 'Chụp màn hình thất bại',
  'Capture is not supported on this platform':
      'Nền tảng này chưa hỗ trợ chụp màn hình',

  // ── Screenshot capture: review dialog ──────────────────────────────────
  'Save screenshot to a note': 'Lưu ảnh vào ghi chú',
  'Fill in the details': 'Điền thông tin',
  'Reading the image…': 'Đang đọc ảnh…',
  'AI fill': 'AI tự điền',
  'Note title': 'Tiêu đề ghi chú',
  'More notes (optional)': 'Ghi chú thêm (tuỳ chọn)',
  'Save note': 'Lưu ghi chú',
  'Enter a title first.': 'Nhập tiêu đề đã.',
  '_Captured at {t}._': '_Chụp lúc {t}._',
  'Save failed: {e}': 'Lưu thất bại: {e}',

  // ── Capture hotkey (Settings shows the registration error) ─────────────
  'This shortcut is already taken by the system or another app. Pick a '
          'different one.':
      'Tổ hợp này đã bị hệ thống hoặc ứng dụng khác chiếm. Chọn tổ hợp khác.',
  'Could not register the shortcut: {e}':
      'Không đăng ký được phím tắt: {e}',

  // ── Schedule editor dialog ─────────────────────────────────────────────
  'New schedule': 'Tác vụ hẹn giờ mới',
  'Edit schedule': 'Sửa lịch',
  'Failed to save schedule: {e}': 'Lưu lịch thất bại: {e}',
  'Describe the task to run on schedule…': 'Mô tả tác vụ sẽ chạy theo lịch…',
  'Frequency': 'Tần suất',
  'Daily': 'Hằng ngày',
  'Weekdays': 'Ngày trong tuần',
  'Weekly': 'Hằng tuần',
  'Monthly': 'Hằng tháng',
  'Once': 'Một lần',
  'Once (auto-delete)': 'Một lần (tự xoá)',
  'Advanced (cron)': 'Nâng cao (cron)',
  'Run date': 'Ngày chạy',
  'Next occurrence of the time (today / tomorrow)':
      'Lần kế tiếp của giờ này (hôm nay / ngày mai)',
  'Clear date': 'Xoá ngày',
  'Weekday': 'Thứ',
  'Mon': 'T2',
  'Tue': 'T3',
  'Wed': 'T4',
  'Thu': 'T5',
  'Fri': 'T6',
  'Sat': 'T7',
  'Sun': 'CN',
  'Day of month': 'Ngày trong tháng',
  'Cron expression': 'Biểu thức cron',
  'Agent mode': 'Chế độ agent',
  'Plan': 'Kế hoạch',
  'Profile (agent)': 'Hồ sơ (agent)',
  'Active default': 'Mặc định đang dùng',
  'Paused': 'Tạm dừng',
  'Cancelled': 'Đã huỷ',

  // ── Note body checklist (Keep-style) ───────────────────────────────────
  '{n} completed': '{n} mục đã xong',
  'List item': 'Mục danh sách',

  // ── Embedded web fallback card ─────────────────────────────────────────
  'Web content': 'Nội dung web',
  'Open in browser': 'Mở trong trình duyệt',

  // ── Section scaffold placeholder ───────────────────────────────────────
  '{feature} — migration {phase}': '{feature} — chuyển đổi {phase}',
  'Scaffolded. Implementation tracked in the migration plan.':
      'Đã dựng khung. Phần triển khai được theo dõi trong kế hoạch chuyển đổi.',
};
