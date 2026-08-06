/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
///
/// Area: New Chat page, hands-free voice overlay, the interactive reminder
/// dialog, the notifications bell and the chat-side providers.
const Map<String, String> viChatMisc = {
  // ── New Chat page: kind selector + greeting ─────────────────────────────
  '💬 Chat': '💬 Trò chuyện',
  '⌨️ Code': '⌨️ Code',
  '👥 Cowork': '👥 Cowork',
  '🕐 Schedule': '🕐 Lịch',
  '🔁 Workflow': '🔁 Workflow',
  'How can I help today?': 'Hôm nay tôi giúp gì được cho bạn?',
  'Chat with {agent}': 'Trò chuyện với {agent}',
  'No workspace needed — just a conversation.':
      'Không cần thư mục làm việc — chỉ trò chuyện thôi.',
  'What should we build in {folder}?': 'Chúng ta xây gì trong {folder}?',
  'Pick a workspace folder to start': 'Chọn thư mục làm việc để bắt đầu',
  'Choose your project root below.': 'Chọn thư mục gốc dự án bên dưới.',
  'Start a Cowork team': 'Lập nhóm Cowork',
  'Pick a template, then describe the goal.':
      'Chọn một mẫu, rồi mô tả mục tiêu.',
  'Create a schedule': 'Tạo lịch',
  'Describe the task and when it should run.':
      'Mô tả tác vụ và thời điểm chạy.',
  'Run a workflow': 'Chạy một workflow',
  'Pick a saved routine, or describe a new one and let the agent build it.':
      'Chọn một workflow đã lưu, hoặc mô tả cái mới để agent dựng giúp bạn.',

  // ── New Chat page: composer + toolbar ───────────────────────────────────
  'Ask anything, or describe a task…   / # skill · @ file':
      'Hỏi bất cứ điều gì, hoặc mô tả một tác vụ…   / # skill · @ tệp',
  'Attach images': 'Đính kèm ảnh',
  'Dictate (Whisper)': 'Đọc chính tả (Whisper)',
  'Stop recording': 'Dừng ghi âm',
  'image {n}': 'ảnh {n}',
  'Profile': 'Hồ sơ',
  'Active default': 'Mặc định đang dùng',
  'Pick a team template': 'Chọn mẫu nhóm',
  'Agent — full tool access': 'Agent — toàn quyền dùng công cụ',
  'Plan — research then propose': 'Plan — tìm hiểu rồi đề xuất',
  'DAG — multi-agent dispatch': 'DAG — điều phối nhiều agent',
  'Start (Enter)': 'Bắt đầu (Enter)',
  'Daily': 'Hằng ngày',
  'Weekly': 'Hằng tuần',
  'Monthly': 'Hằng tháng',
  'Once': 'Một lần',

  // ── New Chat page: suggestion chips ─────────────────────────────────────
  'Summarize my unread messages': 'Tóm tắt tin nhắn chưa đọc của tôi',
  'Plan a project roadmap': 'Lập lộ trình cho một dự án',
  'Research a topic and cite sources': 'Tìm hiểu một chủ đề và trích nguồn',
  'Help me debug an error': 'Giúp tôi gỡ một lỗi',

  // ── Project picker ──────────────────────────────────────────────────────
  'Search projects': 'Tìm dự án',
  'ADD NEW PROJECT': 'THÊM DỰ ÁN MỚI',
  'Start from scratch': 'Bắt đầu từ đầu',
  'Create a new folder': 'Tạo thư mục mới',
  'Use an existing folder': 'Dùng thư mục có sẵn',
  "Don't work in a project": 'Không làm trong dự án nào',
  'Start a new project folder': 'Tạo thư mục dự án mới',
  'Folder path': 'Đường dẫn thư mục',
  'Create failed: {e}': 'Tạo thất bại: {e}',

  // ── New Chat page: results ──────────────────────────────────────────────
  'New chat with {agent}': 'Trò chuyện mới với {agent}',
  'Schedule created': 'Đã tạo lịch',
  'Failed to create team': 'Không tạo được nhóm',
  'Failed: {e}': 'Thất bại: {e}',

  // ── Voice: shared mic/transcription messages ────────────────────────────
  'Microphone permission denied': 'Không có quyền truy cập micro',
  'Transcription failed: {e}': 'Nhận dạng giọng nói thất bại: {e}',
  'Could not start recording: {e}': 'Không thể bắt đầu ghi âm: {e}',

  // ── Hands-free voice overlay ────────────────────────────────────────────
  'Voice assistant': 'Trợ lý thoại',
  'Voice chat · {title}': 'Trò chuyện thoại · {title}',
  'Listening…': 'Đang nghe…',
  'Transcribing…': 'Đang nhận dạng…',
  'Thinking…': 'Đang suy nghĩ…',
  'Speaking…': 'Đang đọc…',
  'Tap to talk': 'Nhấn để nói',
  'Paused': 'Tạm dừng',
  'Speak to start. The assistant will answer out loud.':
      'Hãy nói để bắt đầu. Trợ lý sẽ trả lời bằng giọng nói.',
  'You': 'Bạn',
  'Assistant': 'Trợ lý',
  'End': 'Kết thúc',

  // ── Interactive reminder dialog ─────────────────────────────────────────
  'Reminders': 'Nhắc nhở',
  'Late': 'Trễ',
  'Calendar reminder': 'Nhắc việc từ lịch',
  'SenClaw is working…': 'SenClaw đang xử lý…',
  'Type or talk to reschedule, delete, or ask SenClaw for something else 👋':
      'Nhắn hoặc nói để dời lịch, xoá, hay nhờ SenClaw việc khác 👋',
  'Open {app}': 'Mở {app}',
  'content': 'nội dung',
  'Snooze 10 minutes': 'Nhắc lại sau 10 phút',
  'Move to tonight 20:00': 'Dời sang tối nay 20:00',
  'Delete reminder': 'Xoá nhắc nhở',
  'Message SenClaw…': 'Nhắn cho SenClaw…',
  'Stop & send': 'Dừng & gửi',
  'Talk (voice)': 'Nói (giọng nói)',

  // ── Notifications bell ──────────────────────────────────────────────────
  'Notifications': 'Thông báo',
  'No notifications': 'Không có thông báo',
  'Clear all': 'Xoá tất cả',
  'Notification': 'Thông báo',
  'Reminder': 'Nhắc việc',
  'Upcoming': 'Sắp tới',
  'Scheduled': 'Đã lên lịch',

  // ── Plan providers ──────────────────────────────────────────────────────
  'Approve plan and start editing': 'Duyệt kế hoạch và bắt đầu chỉnh sửa',
  'Clear context and start fresh': 'Xoá ngữ cảnh và bắt đầu lại',
  'Untitled plan': 'Kế hoạch chưa đặt tên',
};
