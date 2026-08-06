/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viKanban = {
  // ── Board list ──────────────────────────────────────────────────────────
  'Task board — agents work the Ready column':
      'Bảng tác vụ — agent nhận việc ở cột Ready',
  'AI board': 'Bảng AI',
  'New board': 'Bảng mới',
  'No boards yet': 'Chưa có bảng nào',
  'Create a board, or let AI plan one from a goal.':
      'Tạo một bảng, hoặc để AI lên kế hoạch từ mục tiêu.',
  'Delete board': 'Xoá bảng',
  'Delete board?': 'Xoá bảng?',
  'Delete “{title}” and all of its cards?':
      'Xoá “{title}” và toàn bộ thẻ trong đó?',
  '{c} columns · {k} cards': '{c} cột · {k} thẻ',

  // ── Board view ──────────────────────────────────────────────────────────
  'Board': 'Bảng',
  'Drag cards between columns · Ready is picked up by workers':
      'Kéo thẻ giữa các cột · Thẻ ở cột Ready sẽ được worker nhận',
  'Workspace: {dir}': 'Thư mục làm việc: {dir}',
  'Back to boards': 'Về danh sách bảng',
  'Worker lanes': 'Làn worker',
  'All workers': 'Tất cả worker',
  'AI Task': 'Tác vụ AI',
  'Add column': 'Thêm cột',
  'Hide activity': 'Ẩn hoạt động',
  'Show running tasks': 'Xem tác vụ đang chạy',

  // ── Activity drawer ─────────────────────────────────────────────────────
  'Activity': 'Hoạt động',
  'Running now ({n})': 'Đang chạy ({n})',
  'No tasks in progress': 'Không có tác vụ nào đang chạy',
  'Recent worker feed': 'Hoạt động worker gần đây',
  'No activity yet': 'Chưa có hoạt động nào',

  // ── Columns & cards ─────────────────────────────────────────────────────
  'Column': 'Cột',
  'Delete column': 'Xoá cột',
  'Unassigned': 'Chưa giao',
  'Add card': 'Thêm thẻ',
  'Card title…': 'Tiêu đề thẻ…',
  'New column': 'Cột mới',
  // Column roles (the "Type" dropdown). 'Ready'/'Done'/'Blocked' live in
  // common.dart / other areas.
  'Custom': 'Tuỳ chỉnh',
  'Triage': 'Phân loại',
  'Todo': 'Cần làm',
  'In Progress': 'Đang làm',
  // Card priority — daemon enum values, translated only where displayed.
  'low': 'thấp',
  'medium': 'trung bình',
  'high': 'cao',
  'urgent': 'khẩn',

  // ── AI task dialog ──────────────────────────────────────────────────────
  'Describe the request — AI will break it down into tasks and add them to the board to run.':
      'Mô tả yêu cầu — AI sẽ chia nhỏ thành các tác vụ và thêm vào bảng để chạy.',
  'Request': 'Yêu cầu',
  'e.g. Write a Q3 market analysis report…':
      'vd. Viết báo cáo phân tích thị trường quý 3…',
  'Generate': 'Tạo',
  'AI breaking the task down…': 'AI đang chia nhỏ tác vụ…',
  'Tasks added to the board': 'Đã thêm tác vụ vào bảng',
  'AI failed: {e}': 'AI lỗi: {e}',

  // ── Card detail dialog ──────────────────────────────────────────────────
  'Complete — summary (optional)': 'Hoàn thành — tóm tắt (tuỳ chọn)',
  'Complete': 'Hoàn thành',
  'Unblock': 'Bỏ chặn',
  'Block — reason': 'Chặn — lý do',
  'Block': 'Chặn',
  'Priority': 'Ưu tiên',
  'Labels (a, b)': 'Nhãn (a, b)',
  'Assignee (worker profile)': 'Người nhận (hồ sơ worker)',
  '— default profile —': '— hồ sơ mặc định —',
  'Break down (AI)': 'Chia nhỏ (AI)',
  'Dependencies': 'Phụ thuộc',
  '⛔ blocked by: {title}': '⛔ bị chặn bởi: {title}',
  '→ blocks: {title}': '→ chặn: {title}',
  'Comments ({n})': 'Bình luận ({n})',
  'Add a note…': 'Thêm ghi chú…',
  // Comment kinds — daemon enum values shown as a badge.
  'complete': 'hoàn thành',
  'block': 'chặn',
  'unblock': 'bỏ chặn',
  'system': 'hệ thống',

  // ── New-board / AI-board dialogs ────────────────────────────────────────
  'Workspace folder (optional)': 'Thư mục làm việc (tuỳ chọn)',
  '~/work/… (worker outputs land here)':
      '~/work/… (kết quả của worker lưu ở đây)',
  'Choose the board workspace folder': 'Chọn thư mục làm việc cho bảng',
  'Browse…': 'Duyệt…',
  'Columns template': 'Mẫu cột',
  'AI generates columns': 'AI tự tạo cột',
  '{name} (custom)': '{name} (tuỳ chỉnh)',
  'e.g. Q3 product launch': 'vd. Ra mắt sản phẩm quý 3',
  'AI board from a goal': 'Bảng AI từ mục tiêu',
  'Goal': 'Mục tiêu',
  'e.g. Plan a customer workshop in 6 weeks':
      'vd. Lên kế hoạch workshop khách hàng trong 6 tuần',
  'Generating board with AI…': 'Đang tạo bảng bằng AI…',

  // ── Templates panel (Plugins → Kanban) ──────────────────────────────────
  'Kanban templates': 'Mẫu Kanban',
  'Reusable column workflows for new boards': 'Bộ cột dùng lại cho bảng mới',
  'New template': 'Mẫu mới',
  'Edit template': 'Sửa mẫu',
  'builtin': 'có sẵn',
  'custom': 'tuỳ chỉnh',
  'Export (copy JSON)': 'Xuất (chép JSON)',
  'Duplicate as custom': 'Nhân bản thành mẫu tuỳ chỉnh',
  'Copied "{name}" template JSON to clipboard':
      'Đã chép JSON của mẫu "{name}" vào bộ nhớ tạm',
  'Import template': 'Nhập mẫu',
  'Paste exported template JSON…': 'Dán JSON mẫu đã xuất…',
  'Template needs a name and at least one column':
      'Mẫu cần có tên và ít nhất một cột',
  'Imported "{name}"': 'Đã nhập "{name}"',
  'Import failed: {e}': 'Nhập thất bại: {e}',
  'Delete "{name}"?': 'Xoá "{name}"?',
  'This custom template will be removed. Boards already created from it are not affected.':
      'Mẫu tuỳ chỉnh này sẽ bị gỡ. Các bảng đã tạo từ mẫu không bị ảnh hưởng.',
  'A name and at least one column are required':
      'Cần có tên và ít nhất một cột',
  'Save failed: {e}': 'Lưu thất bại: {e}',
  'e.g. Marketing sprint': 'vd. Sprint marketing',
  'COLUMNS': 'CÁC CỘT',

  // Builtin template names/descriptions — authored English from the daemon
  // (src/kanban/templates.rs), translated at the display site.
  'Standard (Hermes)': 'Chuẩn (Hermes)',
  "Triage → Todo → Ready → In Progress → Blocked → Done. The autonomous dispatcher's native workflow.":
      'Triage → Todo → Ready → In Progress → Blocked → Done. Quy trình gốc của bộ điều phối tự động.',
  'Advanced (review + WIP)': 'Nâng cao (review + WIP)',
  'Adds a Backlog and a human Review gate, with WIP limits on the flow stages.':
      'Thêm cột Backlog và chốt Review của người, kèm giới hạn WIP cho các giai đoạn.',
  'Simple (classic)': 'Đơn giản (cổ điển)',
  'To Do → In Progress → Done. No dispatcher automation (no Ready column).':
      'To Do → In Progress → Done. Không tự động điều phối (không có cột Ready).',
};
