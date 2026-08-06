/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
///
/// Area: the right dock (Console / Workbench / Files / Apps / Terminal tabs)
/// and the Cowork multi-agent team screen. Keys already covered by
/// `vi/common.dart` (Add, Cancel, Close, Save, Delete, Remove, Refresh,
/// Reload, Copied, Back, Files, Done) are intentionally absent.
const Map<String, String> viDockCowork = {
  // ── Right dock: tab bar ────────────────────────────────────────────────
  'Console': 'Điều khiển',
  'Workbench': 'Bàn làm việc',
  'Apps': 'Ứng dụng',
  'Terminal': 'Terminal',
  'Shrink': 'Thu nhỏ',
  'Expand (70%)': 'Mở rộng (70%)',

  // ── Console tab (sub-agent dispatch + agent todos) ─────────────────────
  'No sub-agent activity': 'Chưa có hoạt động của agent phụ',
  'Dispatch': 'Điều phối',
  'Remove this DAG card?': 'Gỡ thẻ DAG này?',
  'Remove task "{label}"?': 'Gỡ tác vụ "{label}"?',
  'ACTIVITY': 'HOẠT ĐỘNG',

  // ── Workbench tab (artifacts) ──────────────────────────────────────────
  'No artifacts yet': 'Chưa có artifact nào',
  'Close artifact': 'Đóng artifact',
  'Artifact': 'Artifact',

  // ── Files tab (session workspace browser) ──────────────────────────────
  'No workspace for this chat': 'Trò chuyện này chưa có thư mục làm việc',
  'Open folder': 'Mở thư mục',
  'Failed to read: {e}': 'Không đọc được: {e}',
  '(empty file)': '(tệp trống)',
  'Truncated to 512 KB': 'Đã cắt còn 512 KB',

  // ── Apps tab (Space App launcher inside the dock) ──────────────────────
  'All apps': 'Tất cả ứng dụng',
  'Close app': 'Đóng ứng dụng',
  'No apps installed': 'Chưa cài ứng dụng nào',

  // ── Cowork: team list ──────────────────────────────────────────────────
  'Multi-agent teams': 'Nhóm nhiều agent',
  'New team': 'Nhóm mới',
  'No teams yet': 'Chưa có nhóm nào',
  'New team from template': 'Tạo nhóm từ mẫu',
  '{n} member': '{n} thành viên',
  '{n} members': '{n} thành viên',
  'manager · {folder}': 'quản lý · {folder}',

  // ── Cowork: team detail header + members strip ─────────────────────────
  'Team': 'Nhóm',
  'Workspace files': 'Tệp trong thư mục làm việc',
  'Refresh tasks': 'Tải lại tác vụ',
  'Add member': 'Thêm thành viên',
  'Profile folder (slug)': 'Thư mục hồ sơ (slug)',

  // ── Cowork: member editor ──────────────────────────────────────────────
  'Edit {folder}': 'Sửa {folder}',
  'Role': 'Vai trò',
  'Responsibilities': 'Trách nhiệm',
  'Handoff rules': 'Quy tắc bàn giao',
  'Acceptance criteria': 'Tiêu chí nghiệm thu',
  'Output format': 'Định dạng kết quả',
  'SLA': 'SLA',
  'Limits': 'Giới hạn',

  // ── Cowork: trigger-rules editor ───────────────────────────────────────
  'Triggers': 'Điều kiện kích hoạt',
  'No triggers': 'Chưa có điều kiện kích hoạt',
  '💬 Message received': '💬 Nhận tin nhắn',
  '@ On mention': '@ Khi được nhắc tên',
  '📋 Task assigned': '📋 Được giao tác vụ',
  '🔄 Task status changed': '🔄 Tác vụ đổi trạng thái',
  '⏰ Cron schedule': '⏰ Lịch cron',
  'From (sender, optional)': 'Từ (người gửi, tuỳ chọn)',
  'Message type (optional)': 'Loại tin nhắn (tuỳ chọn)',
  'Status (optional)': 'Trạng thái (tuỳ chọn)',
  'Assignee (optional)': 'Người phụ trách (tuỳ chọn)',
  'To status (optional)': 'Sang trạng thái (tuỳ chọn)',
  'Cron expression (e.g. 0 9 * * 1)': 'Biểu thức cron (ví dụ 0 9 * * 1)',

  // ── Cowork: Kanban board (column labels come from kCoworkColumns) ──────
  'To do': 'Cần làm',
  'In progress': 'Đang làm',
  'Review': 'Đang xem lại',
  'Blocked': 'Bị chặn',
  'Task actions': 'Thao tác tác vụ',
  'Move to {label}': 'Chuyển sang {label}',
  'Result': 'Kết quả',
  '{n} chars': '{n} ký tự',

  // ── Cowork: task detail sheet ──────────────────────────────────────────
  'Copy result': 'Sao chép kết quả',
  'DESCRIPTION': 'MÔ TẢ',
  'RESULT': 'KẾT QUẢ',
  'No result yet': 'Chưa có kết quả',

  // ── Cowork: workspace browser dialog ───────────────────────────────────
  'Workspace': 'Thư mục làm việc',
  'Workspace / {path}': 'Thư mục làm việc / {path}',
  'Empty folder': 'Thư mục trống',
};
