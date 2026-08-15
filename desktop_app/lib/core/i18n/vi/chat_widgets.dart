/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viChatWidgets = {
  // widget_card.dart — inline widget chrome (series names & payload stay data)
  'Unknown widget: "{kind}"': 'Widget không xác định: "{kind}"',
  'No chart data': 'Không có dữ liệu biểu đồ',
  'Image unavailable': 'Ảnh không khả dụng',
  'Failed to load image': 'Không tải được ảnh',
  'Video needs a playable http(s) URL (not a file path).':
      'Video cần một URL http(s) phát được (không dùng đường dẫn file).',
  'Audio needs a playable http(s) URL (not a file path).':
      'Audio cần một URL http(s) phát được (không dùng đường dẫn file).',
  'Open externally': 'Mở ngoài',
  'Play': 'Phát',
  'Widget {id} is unavailable — open the app in Apps to view it.':
      'Widget {id} không khả dụng — mở app trong mục Apps để xem.',
  'Click to load widget': 'Bấm để tải widget',
  // Fallback heading for an uncaptioned video card. Unchanged in Vietnamese —
  // the key exists so other locales have a hook.
  'Video': 'Video',

  // message_widgets.dart — bubbles, message actions, timestamps
  'now': 'bây giờ',
  '{n}m': '{n} phút',
  // Token count under a bubble. "tok" is the same abbreviation in Vietnamese.
  '{n} tok': '{n} tok',
  'Save note': 'Lưu ghi chú',
  'Saved': 'Đã lưu',
  'Play (TTS)': 'Phát (TTS)',
  'Speaking…': 'Đang đọc…',
  'TTS failed': 'Lỗi TTS',
  'think': 'suy nghĩ',

  // message_widgets.dart — permission card
  'Permission required': 'Cần cấp quyền',
  'Resolved: {key}': 'Đã xử lý: {key}',
  'answered': 'đã trả lời',

  // message_widgets.dart — tool group verbs
  'Read a file': 'Đọc tệp',
  'Created a file': 'Tạo tệp',
  'Edited a file': 'Sửa tệp',
  'Ran a command': 'Chạy lệnh',
  'Searched files': 'Tìm tệp',
  'Searched content': 'Tìm nội dung',
  'Fetched a URL': 'Tải một URL',
  'Searched the web': 'Tìm trên web',
  'Discovered a tool': 'Tra cứu công cụ',
  'Invoked a skill': 'Gọi skill',
  'Spawned a subagent': 'Tạo agent con',
  'Browser action': 'Thao tác trình duyệt',
  'Memory lookup': 'Tra cứu bộ nhớ',
  'Wiki action': 'Thao tác Wiki',
  'Used a tool': 'Dùng công cụ',

  // message_widgets.dart — diff view + dispatch card
  'New file': 'Tệp mới',
  '{size} bytes': '{size} byte',
  '… {n} more lines': '… còn {n} dòng',
  'Dispatch': 'Điều phối',

  // form_card.dart — FormUI chrome (field labels/options come from the agent)
  'Form': 'Biểu mẫu',
  'Submit': 'Gửi',
  'Submitted': 'Đã gửi',
  '{n} required field left': 'Còn {n} trường bắt buộc',
  '{n} required fields left': 'Còn {n} trường bắt buộc',
  'Skip': 'Bỏ qua',
  'Select…': 'Chọn…',
  'Pick a date…': 'Chọn ngày…',
  'Add row': 'Thêm hàng',
  '(unsupported field: {type})': '(trường không hỗ trợ: {type})',

  // question_card.dart — AskUserQuestion chrome (questions/options are data)
  'Question': 'Câu hỏi',
  'Answered': 'Đã trả lời',
  'Other': 'Khác',
  'Your answer…': 'Câu trả lời của bạn…',

  // plan_exit_dialog.dart — the two button labels are the daemon's defaults
  // (plan_provider.dart); a custom label from the daemon passes through.
  'Plan ready for review': 'Kế hoạch sẵn sàng để duyệt',
  '_No plan content._': '_Không có nội dung kế hoạch._',
  'Approve plan and start editing': 'Duyệt kế hoạch và bắt đầu chỉnh sửa',
  'Clear context and start fresh': 'Xoá ngữ cảnh và bắt đầu lại',

  // Tool parameter rendering (ToolParams)
  'Yes': 'Có',
  'No': 'Không',
};
