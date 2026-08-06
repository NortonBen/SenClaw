/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viChatMain = {
  // ── Conversation header ─────────────────────────────────────────────
  // Original hard-coded Vietnamese literal, preserved verbatim.
  'Voice chat': 'Trò chuyện thoại',
  'Chat info': 'Thông tin đoạn chat',
  'Team tasks': 'Việc của nhóm',
  'Open Cowork board': 'Mở bảng Cowork',
  'Schedule': 'Lịch chạy',
  'Plan history': 'Lịch sử kế hoạch',
  'Toggle console / workbench': 'Bật/tắt bảng điều khiển',
  'No messages yet. Say hello 👋': 'Chưa có tin nhắn nào. Nói xin chào nhé 👋',
  'Context: {use} / {max} tokens · {remaining} left':
      'Ngữ cảnh: {use} / {max} token · còn {remaining}',

  // ── Mode toggle (Agent / Plan / Dag) ────────────────────────────────
  'Agent': 'Agent',
  'Plan': 'Kế hoạch',
  'Dag': 'Dag',

  // ── Composer ─────────────────────────────────────────────────────────
  'Default model': 'Model mặc định',
  'image {n}': 'ảnh {n}',
  'Message the agent…   / # skill · @ file':
      'Nhắn cho agent…   / # skill · @ tệp',
  'Send (Enter)': 'Gửi (Enter)',
  'Attach images': 'Đính kèm ảnh',
  'Stop & transcribe': 'Dừng & chuyển thành chữ',
  'Voice input': 'Nhập bằng giọng nói',
  'Transcription failed: {e}': 'Nhận dạng giọng nói thất bại: {e}',
  'Microphone permission denied': 'Không có quyền truy cập micro',

  // ── Plan history dialog ─────────────────────────────────────────────
  'No plans yet': 'Chưa có kế hoạch nào',
  'Select a plan': 'Chọn một kế hoạch',

  // ── Cowork tasks dialog ──────────────────────────────────────────────
  'In progress': 'Đang làm',
  'Review': 'Đang xem lại',
  'To do': 'Cần làm',
  'Blocked': 'Bị chặn',
  'Edit task': 'Sửa việc',
  'Content (prompt to run)': 'Nội dung (prompt để chạy)',
  'New task': 'Tác vụ mới',
  'No tasks yet': 'Chưa có việc nào',
  '(untitled)': '(chưa đặt tên)',

  // ── Chat info dialog ─────────────────────────────────────────────────
  'Memory saved': 'Đã lưu bộ nhớ',
  'Save failed': 'Lưu thất bại',
  'Active default': 'Mặc định đang dùng',
  'CONTEXT LENGTH': 'ĐỘ DÀI NGỮ CẢNH',
  '{use} / {max} tokens ({pct}%) · {remaining} left{promptPart}':
      '{use} / {max} token ({pct}%) · còn {remaining}{promptPart}',
  ' · prompt {p}': ' · prompt {p}',
  'No usage reported yet (send a message).':
      'Chưa có dữ liệu sử dụng (hãy gửi một tin nhắn).',
  'Compacting context…': 'Đang nén ngữ cảnh…',
  'Compact context': 'Nén ngữ cảnh',
  'MEMORY CONTEXT (MEMORY.md)': 'BỘ NHỚ NGỮ CẢNH (MEMORY.md)',
  'No agent folder bound to this chat.':
      'Đoạn chat này chưa gắn với thư mục agent nào.',
  'No memory yet — type to add notes the agent should remember…':
      'Chưa có bộ nhớ — nhập ghi chú mà agent nên nhớ…',
  'DANGER ZONE': 'VÙNG NGUY HIỂM',
  'Clear all messages': 'Xoá tất cả tin nhắn',
  'Stops the agent and permanently deletes every message, tool log, and chat event of this session.':
      'Dừng agent và xoá vĩnh viễn mọi tin nhắn, nhật ký công cụ và sự kiện chat của phiên này.',
  'Clear all messages?': 'Xoá tất cả tin nhắn?',
  'This stops the agent and permanently deletes the entire chat history of this session. This cannot be undone.':
      'Thao tác này dừng agent và xoá vĩnh viễn toàn bộ lịch sử chat của phiên này. Không thể hoàn tác.',
  'Chat history deleted': 'Đã xoá lịch sử chat',

  // ── Kind badge (Cowork is a brand name and stays untranslated) ──────
  'Code': 'Code',

  // ── Schedule info dialog ─────────────────────────────────────────────
  'Queued to run now': 'Đã xếp hàng để chạy ngay',
  'Run failed': 'Chạy thất bại',
  'Run now': 'Chạy ngay',
  'PROMPT': 'PROMPT',
  'Next run': 'Lần chạy tiếp theo',
  'Last run': 'Lần chạy gần nhất',
  'HISTORY': 'LỊCH SỬ',
  'No runs yet': 'Chưa có lần chạy nào',

  // ── Session list (sidebar) ───────────────────────────────────────────
  'now': 'bây giờ',
  '{n}m': '{n} phút',
  '{n}h': '{n} giờ',
  '{n}d': '{n} ngày',
  '(unknown)': '(không rõ)',
  'Sessions': 'Phiên',
  'Older': 'Cũ hơn',
  'Previous 7 days': '7 ngày trước',
  'Previous 30 days': '30 ngày trước',
  'Projects': 'Dự án',
  'Pinned': 'Đã ghim',
  'New Session': 'Phiên mới',
  'Reload chats': 'Tải lại danh sách trò chuyện',
  'No chats yet.\nClick + New Session above.':
      'Chưa có cuộc trò chuyện nào.\nBấm + Phiên mới ở trên.',
  'Group & sort': 'Nhóm & sắp xếp',
  'Group by': 'Nhóm theo',
  'Sort by': 'Sắp xếp theo',
  'Project': 'Dự án',
  'Recent activity': 'Hoạt động gần đây',
  'Created': 'Ngày tạo',
  'Copy ID': 'Sao chép ID',

  // ── Mini chat window ─────────────────────────────────────────────────
  'New chat': 'Trò chuyện mới',
  'Switch session': 'Chuyển phiên',
  'Open full window': 'Mở cửa sổ đầy đủ',
};
