/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viBackground = {
  // ── Background screen: header ──────────────────────────────────────────────
  'Background': 'Chạy nền',
  'Tasks SenClaw runs by itself — no chat, no reply':
      'Các tác vụ SenClaw tự chạy — không chat, không trả lời',
  'Quick task': 'Tác vụ nhanh',
  'New task': 'Tác vụ mới',
  'Show core upkeep jobs (cognitive decay, maintenance, …)':
      'Hiện các job nền của hệ thống (suy giảm tri thức, bảo trì, …)',

  // Stats row
  'Runs': 'Lần chạy',
  '{n} in flight': '{n} đang chạy',
  '{n} skipped': '{n} bỏ qua',
  'Avg': 'Trung bình',
  'Tokens': 'Token',

  // Attention band
  'Needs attention ({n})': 'Cần chú ý ({n})',
  'auto-paused': 'tự tạm dừng',
  '{n}× failed': 'thất bại {n} lần',

  // Status filter chips + list counter
  'Paused': 'Tạm dừng',
  'Completed': 'Hoàn tất',
  'Cancelled': 'Đã huỷ',
  '{n} task': '{n} tác vụ',
  'No tasks with status "{s}".': 'Không có tác vụ nào ở trạng thái "{s}".',

  // Status / enum values shown as pills or detail values (lowercase = raw
  // daemon values; English mode shows them verbatim)
  'active': 'hoạt động',
  'paused': 'tạm dừng',
  'failed': 'thất bại',
  'completed': 'hoàn tất',
  'cancelled': 'đã huỷ',
  'running': 'đang chạy',
  'success': 'thành công',
  'error': 'lỗi',
  'timeout': 'hết thời gian',
  'skipped': 'bỏ qua',
  'schedule': 'theo lịch',
  'manual': 'thủ công',
  'install': 'khi cài đặt',
  'catch_up': 'chạy bù',
  'static': 'tĩnh',
  'template': 'mẫu',
  'generator': 'sinh tự động',
  'fresh': 'fresh (chạy mới mỗi lần)',
  'thread (remembers prior runs)': 'thread (nhớ các lần chạy trước)',
  'skip': 'bỏ qua lần mới',
  'queue': 'xếp hàng chờ',
  'cancel_previous': 'huỷ lần đang chạy',

  // Task row actions
  'Run now': 'Chạy ngay',
  'Owned by {app} — uninstall the app to remove':
      'Thuộc app "{app}" — gỡ cài đặt app để xoá',
  'Core upkeep — pause it instead':
      'Job nền của hệ thống — hãy tạm dừng thay vì xoá',
  'Cancel run': 'Huỷ lần chạy',

  // Delete confirm
  'Delete "{title}"?': 'Xoá "{title}"?',
  'The task stops firing and is removed. Its run history is kept.':
      'Tác vụ sẽ ngừng kích hoạt và bị gỡ bỏ. Lịch sử chạy vẫn được giữ lại.',

  // Detail pane
  'Task no longer exists': 'Tác vụ không còn tồn tại',
  'Declared by the "{app}" app. Its configuration lives in the app manifest — an edit here would be reverted on reinstall. You can still pause it or run it now.':
      'Do app "{app}" khai báo. Cấu hình nằm trong manifest của app — sửa ở đây sẽ bị hoàn tác khi cài lại. Bạn vẫn có thể tạm dừng hoặc chạy ngay.',
  'Core upkeep. Its body is Rust, not a prompt. You can pause it or run it now.':
      'Job nền của hệ thống. Phần thân là mã Rust, không phải prompt. Bạn có thể tạm dừng hoặc chạy ngay.',
  'Trigger': 'Kích hoạt',
  'Next run': 'Lần chạy tiếp theo',
  'Last run': 'Lần chạy gần nhất',
  'Prompt kind': 'Loại prompt',
  'Prompt': 'Prompt',
  'Context URL': 'URL ngữ cảnh',
  'Persona': 'Persona',
  'Native job': 'Job native',
  'Continuity': 'Liên tục',
  'On overlap': 'Khi chạy trùng',
  'Tools': 'Công cụ',
  'Consecutive failures': 'Thất bại liên tiếp',
  'Run history': 'Lịch sử chạy',
  'Has not run yet.': 'Chưa chạy lần nào.',

  // Empty state
  'No background tasks': 'Chưa có tác vụ nền',
  'Background tasks run on a schedule with nobody watching — periodic upkeep, unattended follow-up, an app\'s standing duties. Unlike a calendar schedule, they never reply to you; their output lands here.':
      'Tác vụ nền chạy theo lịch mà không ai theo dõi — dọn dẹp định kỳ, việc tự lo không cần trông, nhiệm vụ thường trực của app. Khác với lịch hẹn, chúng không bao giờ trả lời bạn; kết quả nằm ở đây.',

  // Trigger prose + relative times (background_models.dart, via L10n.global)
  'You': 'Bạn',
  'Manual only': 'Chỉ chạy thủ công',
  'Once, on install': 'Một lần, khi cài đặt',
  'Once at {t}': 'Một lần lúc {t}',
  'Every {t}': 'Mỗi {t}',
  '{n}s': '{n} giây',
  '{n}m': '{n} phút',
  '{n}h': '{n} giờ',
  '{n}d': '{n} ngày',
  'Every minute': 'Mỗi phút',
  'Hourly at :{m}': 'Hằng giờ vào phút {m}',
  'Daily': 'Hằng ngày',
  'Daily at {t}': 'Hằng ngày lúc {t}',
  'Weekly on {day}': 'Hằng tuần vào {day}',
  'Weekly on {day} at {t}': 'Hằng tuần vào {day} lúc {t}',
  'Weekdays': 'Ngày trong tuần',
  'Weekdays at {t}': 'Ngày trong tuần lúc {t}',
  'Monthly on day {d}': 'Hằng tháng vào ngày {d}',
  'Monthly on day {d} at {t}': 'Hằng tháng vào ngày {d} lúc {t}',
  'Sunday': 'Chủ nhật',
  'Monday': 'Thứ Hai',
  'Tuesday': 'Thứ Ba',
  'Wednesday': 'Thứ Tư',
  'Thursday': 'Thứ Năm',
  'Friday': 'Thứ Sáu',
  'Saturday': 'Thứ Bảy',
  'in {t}': 'sau {t}',
  '{t} ago': '{t} trước',
  'Not scheduled': 'Không có lịch',
  'Due now': 'Đến hạn chạy',
  'Next {rel}': 'Lần tới {rel}',

  // Task editor
  'Edit background task': 'Sửa tác vụ nền',
  'New background task': 'Tác vụ nền mới',
  'Daily knowledge cleanup': 'Dọn dẹp tri thức hằng ngày',
  'Optional — what this is for': 'Tuỳ chọn — tác vụ này để làm gì',
  'Nobody is on the other end: the prompt must be self-contained, say what "done" looks like, and say what to do when there is nothing to do.':
      'Không có ai ở đầu bên kia: prompt phải tự đủ ngữ cảnh, nêu rõ thế nào là "xong", và nói rõ phải làm gì khi không có việc gì.',
  'Review the knowledge base for contradictions…':
      'Rà soát tri thức tìm mâu thuẫn…',
  'Prompt source': 'Nguồn prompt',
  'Static': 'Tĩnh',
  'Template': 'Mẫu',
  'Generated': 'Sinh tự động',
  'Fetched before each run; its JSON fills {{placeholders}}. An empty response skips the run — so a task with nothing to do costs no tokens.':
      'Được tải trước mỗi lần chạy; JSON trả về điền vào các {{placeholder}}. Phản hồi rỗng thì bỏ qua lần chạy — tác vụ không có việc sẽ không tốn token.',
  'The prompt above is an instruction; the model writes the real prompt from it each run. Doubles token cost and can invent its own task — prefer Template when the data can be fetched.':
      'Prompt ở trên là chỉ dẫn; mỗi lần chạy model tự viết prompt thật từ đó. Tốn gấp đôi token và có thể tự bịa ra việc — ưu tiên dùng Mẫu khi dữ liệu tải về được.',
  'Hourly': 'Hằng giờ',
  'Weekly': 'Hằng tuần',
  'Monthly': 'Hằng tháng',
  'Every N minutes': 'Mỗi N phút',
  'Advanced (cron)': 'Nâng cao (cron)',
  'Once, at a time': 'Một lần, hẹn thời điểm',
  'Optional — e.g. sale-closer': 'Tuỳ chọn — vd: sale-closer',
  'Comma-separated. Empty = the persona\'s own list':
      'Cách nhau bằng dấu phẩy. Để trống = dùng danh sách của persona',
  'Memory across runs': 'Bộ nhớ giữa các lần chạy',
  'Fresh': 'Không nhớ',
  'Remembers': 'Ghi nhớ',
  'Recent run summaries are injected. Use this for anything touching people — otherwise it contacts the same person twice.':
      'Tóm tắt các lần chạy gần đây được đưa vào. Dùng cho việc đụng tới con người — nếu không nó sẽ liên hệ cùng một người hai lần.',
  'Each run starts clean.': 'Mỗi lần chạy bắt đầu hoàn toàn mới.',
  'If the previous run is still going': 'Nếu lần chạy trước vẫn đang chạy',
  'Skip': 'Bỏ qua',
  'Wait': 'Chờ',
  'Cancel it': 'Huỷ lần trước',
  'Catch up after downtime': 'Chạy bù sau thời gian tắt máy',
  'Run once for a window missed while the daemon was off. Off = the gap is dropped.':
      'Chạy bù một lần cho khung giờ bị lỡ khi daemon tắt. Tắt = bỏ qua khoảng bị lỡ.',
  '🔔 Notify only': '🔔 Chỉ thông báo',
  'Pushes an OS notification with the Prompt text, does NOT run an agent. Use for reminders/alerts — fast, reliable, no tokens.':
      'Đẩy thông báo OS với nội dung ở ô Prompt, KHÔNG chạy agent. Dùng cho nhắc/thông báo — nhanh, chắc chắn, không tốn token.',
  'This task can act outside this machine, so it will be created paused. Review it and press play to start it.':
      'Tác vụ này có thể tác động ra ngoài máy, nên sẽ được tạo ở trạng thái tạm dừng. Hãy xem lại rồi bấm nút chạy để bắt đầu.',
  'Create (paused)': 'Tạo (tạm dừng)',
  'Only runs when you press "Run now".': 'Chỉ chạy khi bạn bấm "Chạy ngay".',
  'Cron expression': 'Biểu thức cron',
  '5-field form, evaluated in your local timezone.':
      'Dạng 5 trường, tính theo múi giờ máy bạn.',
  'When (RFC3339)': 'Thời điểm (RFC3339)',
  'Every': 'Mỗi',
  'minutes': 'phút',
  'At {t}': 'Lúc {t}',
  'At minute': 'Vào phút',
  'Day {n}': 'Ngày {n}',

  // Session dialog
  'Background session': 'Phiên chạy nền',
  'Started': 'Bắt đầu',
  'Duration': 'Thời lượng',
  'Turns': 'Lượt',
  '{i} in / {o} out': '{i} vào / {o} ra',
  'Prompt sent': 'Prompt đã gửi',
  'after template/generator resolution': 'sau khi xử lý template/generator',
  'Why it skipped': 'Lý do bỏ qua',
  'Result': 'Kết quả',
  'Transcript ({n})': 'Bản ghi ({n})',
  'Nothing ran — the task skipped this window.':
      'Không chạy gì — tác vụ đã bỏ qua khung giờ này.',
  'No activity recorded.': 'Chưa ghi nhận hoạt động nào.',

  // Quick dialog
  'Describe the task in one line — AI fills in the schedule and prompt.':
      'Mô tả tác vụ bằng một câu — AI sẽ tự điền lịch chạy và nội dung.',
  'e.g. every morning at 9, review the knowledge base and clean up contradictions':
      'vd: mỗi sáng 9h rà soát tri thức và dọn mâu thuẫn',
  'Analyzing…': 'Đang phân tích…',
  'Analyze with AI': 'AI phân tích',
  'Re-analyze': 'Phân tích lại',
  'Creating…': 'Đang tạo…',
  'Create task': 'Tạo tác vụ',
  'AI suggestion': 'AI đề xuất',
  'Schedule': 'Lịch chạy',
  'Kind': 'Kiểu',
  '🔔 Notification (no agent run)': '🔔 Thông báo (không chạy agent)',
  'Memory': 'Bộ nhớ',
  'remembers prior runs': 'nhớ các lần chạy trước',
  'Content': 'Nội dung',
};
