/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viDashboardUsage = {
  // ── Dashboard: header + hero ───────────────────────────────────────────
  'Dashboard': 'Tổng quan',
  'Overview of your SenClaw agents and activity':
      'Tổng quan về agent và hoạt động SenClaw của bạn',
  'Voice chat': 'Trò chuyện thoại',
  'Good night': 'Khuya rồi',
  'Good morning': 'Chào buổi sáng',
  'Good afternoon': 'Chào buổi chiều',
  'Good evening': 'Chào buổi tối',
  '{n} agent working right now': '{n} agent đang làm việc',
  '{n} agents working right now': '{n} agent đang làm việc',
  'Your agents are standing by': 'Các agent đang chờ sẵn',
  'New chat': 'Trò chuyện mới',
  'New note': 'Ghi chú mới',
  'Open wiki': 'Mở Wiki',
  'Online': 'Trực tuyến',
  'Offline': 'Ngoại tuyến',

  // ── Dashboard: unread alert ────────────────────────────────────────────
  '{n} unread notification': '{n} thông báo chưa đọc',
  '{n} unread notifications': '{n} thông báo chưa đọc',
  'Mark all read': 'Đánh dấu đã đọc hết',
  '{n} unread chat message': '{n} tin nhắn chưa đọc',
  '{n} unread chat messages': '{n} tin nhắn chưa đọc',
  'Across your conversations': 'Trong các cuộc trò chuyện của bạn',
  '+{n} more': '+{n} nữa',

  // ── Dashboard: stat cards ──────────────────────────────────────────────
  'Active agents': 'Agent hoạt động',
  'Total chats': 'Tổng cuộc trò chuyện',
  'Wiki documents': 'Tài liệu Wiki',
  'Knowledge nodes': 'Nút tri thức',
  'Skills': 'Skill',
  'MCP servers': 'MCP server',

  // ── Dashboard: pinned apps + widgets ───────────────────────────────────
  'Pinned apps': 'App đã ghim',
  'No pinned apps — right-click an app to pin it.':
      'Chưa ghim app nào — bấm chuột phải vào app để ghim.',
  'Widgets': 'Widget',
  'No widgets yet — press "Add" to place one.':
      'Chưa có widget — bấm "Thêm" để thêm.',
  'Add widget': 'Thêm widget',
  'No widgets available.\n'
      'Install an app that ships widgets to get started.':
      'Không có widget khả dụng.\nCài app có kèm widget để bắt đầu.',
  'Small': 'Nhỏ',
  'Medium': 'Vừa',
  'Large': 'Lớn',

  // ── Dashboard: events + schedules ──────────────────────────────────────
  'Upcoming events': 'Sự kiện sắp tới',
  'No upcoming events.': 'Không có sự kiện sắp tới.',
  'Open calendar': 'Mở lịch',
  'New event': 'Sự kiện mới',
  '{date} · All day': '{date} · Cả ngày',
  'Upcoming schedules': 'Lịch chạy sắp tới',
  'No schedules — create one in Space › Schedules.':
      'Chưa có lịch nào — tạo trong Space › Schedules.',
  'Manage schedules': 'Quản lý lịch',
  'no upcoming run': 'chưa có lần chạy kế tiếp',
  'due': 'đến hạn',
  'now': 'bây giờ',
  'in {n}m': 'sau {n} phút',
  'in {n}h': 'sau {n} giờ',
  'in {n}d': 'sau {n} ngày',

  // ── Dashboard: recent chats ────────────────────────────────────────────
  'Recent chats': 'Trò chuyện gần đây',
  'No chats yet — start one from the Chat tab.':
      'Chưa có cuộc trò chuyện — bắt đầu từ tab Trò chuyện.',
  '{n}m': '{n} phút',
  '{n}h': '{n} giờ',
  '{n}d': '{n} ngày',

  // ── Dashboard: live activity ───────────────────────────────────────────
  'Live activity': 'Hoạt động trực tiếp',
  'All agents idle.': 'Mọi agent đang rảnh.',
  '{n} dispatch run in progress': '{n} lượt dispatch đang chạy',
  '{n} dispatch runs in progress': '{n} lượt dispatch đang chạy',
  'running': 'đang chạy',
  'thinking': 'đang nghĩ',
  'executing': 'đang thực thi',
  'processing': 'đang xử lý',
  'needs approval': 'chờ phê duyệt',
  'needs input': 'chờ nhập',
  'Open agent console': 'Mở bảng điều khiển agent',

  // ── Usage: header + stat cards ─────────────────────────────────────────
  'Token Usage': 'Mức dùng token',
  'Token in/out and estimated cost — agents, '
      'Space Apps, cognitive, embeddings':
      'Token vào/ra và chi phí ước tính — agent, Space App, cognitive, '
          'embeddings',
  'Tokens in (today)': 'Token vào (hôm nay)',
  'Tokens out (today)': 'Token ra (hôm nay)',
  'Est. cost (today)': 'Chi phí ước tính (hôm nay)',
  'Cache-read (today)': 'Cache-read (hôm nay)',
  '+{n} tokens unpriced': '+{n} token chưa có giá',

  // ── Usage: daily chart + breakdowns ────────────────────────────────────
  'Tokens per day — 30 days': 'Token mỗi ngày — 30 ngày',
  'Tokens in': 'Token vào',
  'Tokens out': 'Token ra',
  'No data yet — numbers appear after the first LLM calls.':
      'Chưa có dữ liệu — số liệu xuất hiện sau các lần gọi LLM đầu tiên.',
  'No data yet': 'Chưa có dữ liệu',
  'By model — 7 days': 'Theo model — 7 ngày',
  'By Space App — 7 days': 'Theo Space App — 7 ngày',
  'Calls': 'Lượt gọi',
  'In': 'Vào',
  'Out': 'Ra',
  'Cost': 'Chi phí',

  // ── Usage: model pricing editor ────────────────────────────────────────
  'Model pricing (USD / 1M tokens)': 'Bảng giá model (USD / 1M token)',
  'Exact id match first, then prefix. A model with '
      'no price is reported as "unpriced", never billed as \$0.':
      'Khớp id chính xác trước, rồi theo prefix. Model không có giá được báo '
          'là "chưa có giá", không bao giờ tính \$0.',
  'Add model': 'Thêm model',
  'No pricing rows yet': 'Chưa có dòng giá nào',
  'Model (prefix match)': 'Model (khớp prefix)',
  'Cache R': 'Cache đọc',
  'Cache W': 'Cache ghi',
  'Delete model pricing': 'Xoá giá model',
  'Drop the price list for "{model}"? Tokens for this model are '
      'then counted as "unpriced" (not \$0).':
      'Bỏ bảng giá cho "{model}"? Token của model này sẽ được tính là '
          '"chưa có giá" (không phải \$0).',
  'Delete failed: {e}': 'Xoá thất bại: {e}',
  'Add model pricing': 'Thêm giá model',
  'Edit model pricing': 'Sửa giá model',
  'Model id (prefix match)': 'Model id (khớp prefix)',
  'e.g. gpt-5.2': 'vd: gpt-5.2',
  'In \$/1M': 'Vào \$/1M',
  'Out \$/1M': 'Ra \$/1M',
  'Cache read \$/1M (optional)': 'Cache đọc \$/1M (tuỳ chọn)',
  'Cache write \$/1M (optional)': 'Cache ghi \$/1M (tuỳ chọn)',
  'Model id plus In and Out prices (numbers) are required.':
      'Cần model id cùng giá Vào và Ra (dạng số).',
  'Save failed: {e}': 'Lưu thất bại: {e}',
};
