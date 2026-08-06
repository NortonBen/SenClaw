/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viWorkflow = {
  // ── Plugins → Workflow: template manager header ───────────────────────────
  'Workflow templates': 'Mẫu workflow',
  'multi-step routine definitions (agent + script)':
      'định nghĩa quy trình nhiều bước (agent + script)',
  'Run history': 'Lịch sử chạy',
  'Draft with agent': 'Nhờ agent soạn',
  'New workflow': 'Workflow mới',
  'Execution settings (LLM parallel, retries)':
      'Cài đặt thực thi (LLM song song, số lần thử lại)',
  'Cannot load workflows: {e}': 'Không tải được danh sách workflow: {e}',
  'No workflows yet — click "New workflow" or "Import" to add one.\nDefinitions live in ~/senclaw/workflows/*.md':
      'Chưa có workflow nào — bấm "Workflow mới" hoặc "Nhập" để thêm.\nCác định nghĩa nằm ở ~/senclaw/workflows/*.md',
  '{n} step': '{n} bước',
  'Tune guidance': 'Chỉnh hướng dẫn',

  // ── Editor dialog (create / edit / import paste) ──────────────────────────
  'Edit: {name}': 'Sửa: {name}',
  'Markdown with YAML frontmatter…': 'Markdown kèm YAML frontmatter…',
  'Import workflow (paste .md content)': 'Nhập workflow (dán nội dung .md)',
  'Cannot load definition: {e}': 'Không tải được định nghĩa: {e}',
  'Created workflow "{name}"': 'Đã tạo workflow "{name}"',
  'Saved "{name}"': 'Đã lưu "{name}"',
  'Save failed: {e}': 'Lưu thất bại: {e}',

  // ── Execution settings dialog ────────────────────────────────────────────
  'Execution settings': 'Cài đặt thực thi',
  'Parallel LLM requests (1–16)': 'Số yêu cầu LLM chạy song song (1–16)',
  'Many providers allow only 1 request at a time. Agent steps beyond the budget WAIT as pending — their timeout only starts when they run.':
      'Nhiều nhà cung cấp chỉ cho chạy 1 yêu cầu mỗi lần. Các bước agent vượt hạn mức sẽ CHỜ ở trạng thái chờ xử lý — thời gian tối đa chỉ bắt đầu tính khi bước đó thật sự chạy.',
  'Retries when no result (0–5)': 'Số lần thử lại khi không có kết quả (0–5)',
  'Agent steps that hit a session error or return empty text are retried this many times before failing.':
      'Bước agent gặp lỗi phiên hoặc trả về nội dung rỗng sẽ được thử lại bấy nhiêu lần trước khi coi là thất bại.',
  'Applied live — queued steps pick it up immediately.':
      'Áp dụng ngay — các bước đang xếp hàng nhận giá trị mới lập tức.',
  'Cannot load settings: {e}': 'Không tải được cài đặt: {e}',
  'Settings saved: {p} parallel, {r} retries':
      'Đã lưu cài đặt: {p} song song, {r} lần thử lại',

  // ── "Draft with agent" dialog ────────────────────────────────────────────
  '✨ Draft with agent': '✨ Nhờ agent soạn',
  'Describe the routine — the agent picks matching personas, builds the steps + guidance, and returns a draft for review. Takes ~30–120s.':
      'Mô tả quy trình — agent sẽ chọn persona phù hợp, dựng các bước + hướng dẫn, rồi trả về bản nháp để bạn xem lại. Mất khoảng 30–120 giây.',
  'e.g. Weekly: research a topic from 3 angles in parallel, fetch pricing with a script, then summarize into one report.':
      'vd: Hằng tuần: nghiên cứu một chủ đề từ 3 góc nhìn song song, lấy bảng giá bằng script, rồi tóm tắt thành một báo cáo.',
  'Drafting…': 'Đang soạn…',
  'Draft': 'Soạn nháp',
  'Draft failed: {e}': 'Soạn nháp thất bại: {e}',

  // ── Tune-guidance dialog ─────────────────────────────────────────────────
  'Tune guidance: {name}': 'Chỉnh hướng dẫn: {name}',
  'Guidance is the RULES layer (persona = identity, prompt = task). Editing here never touches the DAG structure; empty = remove.':
      'Hướng dẫn là lớp LUẬT (persona = danh tính, prompt = nhiệm vụ). Sửa ở đây không đụng tới cấu trúc DAG; để trống = xoá.',
  'Workflow guidance (applies to all agent steps)':
      'Hướng dẫn cho cả workflow (áp dụng cho mọi bước agent)',
  'Workspace (cwd of every step, persists across runs)':
      'Thư mục làm việc (cwd của mọi bước, giữ lại qua các lần chạy)',
  'empty = default per-workflow directory':
      'để trống = thư mục mặc định riêng của workflow',
  'timeout (s)': 'thời gian tối đa (giây)',
  'Rules for this step: output format, scope, tone…':
      'Luật cho bước này: định dạng kết quả, phạm vi, giọng văn…',
  'Saved guidance for "{name}"': 'Đã lưu hướng dẫn cho "{name}"',

  // ── Import / export ──────────────────────────────────────────────────────
  'Imported workflow "{name}"': 'Đã nhập workflow "{name}"',
  'Imported workflow "{name}" (overwritten)':
      'Đã nhập workflow "{name}" (ghi đè)',
  'Import failed: {e}': 'Nhập thất bại: {e}',
  'Workflow already exists': 'Workflow đã tồn tại',
  'Overwrite the existing definition?': 'Ghi đè định nghĩa hiện có?',
  'Overwrite': 'Ghi đè',
  'Export workflow': 'Xuất workflow',
  'Exported to {path}': 'Đã xuất ra {path}',
  'Export: {name}': 'Xuất: {name}',
  'Copy to clipboard': 'Sao chép vào clipboard',
  'Export failed: {e}': 'Xuất thất bại: {e}',

  // ── Delete a template ────────────────────────────────────────────────────
  'Delete workflow "{name}"?': 'Xoá workflow "{name}"?',
  'Run history and the workspace directory are kept.':
      'Lịch sử chạy và thư mục làm việc vẫn được giữ lại.',
  'Deleted "{name}"': 'Đã xoá "{name}"',
  'Delete failed: {e}': 'Xoá thất bại: {e}',

  // ── Template detail dialog ───────────────────────────────────────────────
  'Workflow: {name}': 'Workflow: {name}',
  'Workspace: {path}': 'Thư mục làm việc: {path}',
  'Inputs': 'Tham số đầu vào',
  '(required)': '(bắt buộc)',
  'default: {v}': 'mặc định: {v}',
  'Steps ({n})': 'Các bước ({n})',
  '← waits for: {steps}': '← chờ: {steps}',

  // ── Run-inputs dialog ────────────────────────────────────────────────────
  'Run: {name}': 'Chạy: {name}',
  'This workflow takes no inputs.': 'Workflow này không cần tham số đầu vào.',
  'Missing required input(s): {names}': 'Thiếu tham số bắt buộc: {names}',
  'Started: {id}': 'Đã bắt đầu: {id}',
  'Run failed: {e}': 'Chạy thất bại: {e}',

  // ── Run monitor: list, grouping, sorting ─────────────────────────────────
  'Workflow runs': 'Các lần chạy workflow',
  'Group & sort': 'Nhóm & sắp xếp',
  'Group by': 'Nhóm theo',
  'Workflow': 'Workflow',
  'No grouping': 'Không nhóm',
  'Sort by': 'Sắp xếp theo',
  'Recent activity': 'Hoạt động gần đây',
  'Created': 'Ngày tạo',
  'Name A–Z': 'Tên A–Z',
  'Cannot load runs: {e}': 'Không tải được các lần chạy: {e}',
  'No runs yet': 'Chưa có lần chạy nào',
  'Show more ({n})': 'Xem thêm ({n})',
  'Select a run to see its details': 'Chọn một lần chạy để xem chi tiết',
  'Cancel run': 'Huỷ lần chạy',

  // Date-bucket group headers (Today / Yesterday come from common.dart)
  'Past 7 days': '7 ngày qua',
  'Past 30 days': '30 ngày qua',
  'Older': 'Cũ hơn',

  // Run + step status values from the daemon, translated at the display site
  'running': 'đang chạy',
  'done': 'xong',
  'partial-failed': 'lỗi một phần',
  'cancelled': 'đã huỷ',
  'interrupted': 'gián đoạn',
  'pending': 'chờ xử lý',
  'failed': 'thất bại',
  'skipped': 'bỏ qua',

  // ── Rename / delete a run ────────────────────────────────────────────────
  'Rename run': 'Đổi tên lần chạy',
  'Empty resets back to the run id.': 'Để trống sẽ quay về mã lần chạy.',
  'Rename failed: {e}': 'Đổi tên thất bại: {e}',
  'Delete run "{title}"?': 'Xoá lần chạy "{title}"?',
  'Only the history record is removed — workspace files are kept.':
      'Chỉ bản ghi lịch sử bị xoá — tệp trong thư mục làm việc vẫn được giữ.',
  'Cancel requested: {id}': 'Đã yêu cầu huỷ: {id}',
  'Cancel failed: {e}': 'Huỷ thất bại: {e}',
  'Definition "{name}" no longer exists': 'Định nghĩa "{name}" không còn tồn tại',

  // ── Run detail ───────────────────────────────────────────────────────────
  'Download full result (.md)': 'Tải toàn bộ kết quả (.md)',
  'Save full result to wiki': 'Lưu toàn bộ kết quả vào wiki',
  'Cancel the run before deleting': 'Huỷ lần chạy trước khi xoá',
  'Delete run': 'Xoá lần chạy',
  'Re-run': 'Chạy lại',
  'Save markdown': 'Lưu tệp markdown',
  'Saved to {path}': 'Đã lưu vào {path}',
  'Copied to clipboard': 'Đã sao chép vào clipboard',
  'Saved to wiki: {path}': 'Đã lưu vào wiki: {path}',
  'Wiki save failed: {e}': 'Lưu vào wiki thất bại: {e}',
  'Trigger': 'Kích hoạt',
  'Started': 'Bắt đầu',
  'Finished': 'Kết thúc',
  'Download step result (.md)': 'Tải kết quả của bước (.md)',
  'Save step result to wiki': 'Lưu kết quả của bước vào wiki',
  'Result ({n} chars)': 'Kết quả ({n} ký tự)',

  // ── Chat sidebar section + workflow session pane ─────────────────────────
  'WORKFLOWS': 'WORKFLOW',
  'More {n} workflows →': 'Thêm {n} workflow →',
  '{status} · {done}/{total} steps': '{status} · {done}/{total} bước',
  'Run "{id}" not found (history may have rotated)':
      'Không tìm thấy lần chạy "{id}" (lịch sử có thể đã bị xoay vòng)',
  'workflow session': 'phiên workflow',
  'Open in run monitor': 'Mở trong màn hình theo dõi',
  'Activity ({n}) — click to expand': 'Hoạt động ({n}) — bấm để mở rộng',
  'ACTIVITY': 'HOẠT ĐỘNG',
  'Waiting for the agent…': 'Đang chờ agent…',
  'No activity recorded': 'Chưa ghi nhận hoạt động nào',
  'Thinking… ({n} chars)': 'Đang suy nghĩ… ({n} ký tự)',
  'Writing… ({n} chars)': 'Đang viết… ({n} ký tự)',

  // ── New-session Workflow tab (quick start) ───────────────────────────────
  'Run a saved workflow': 'Chạy một workflow đã lưu',
  'No workflows yet — create one below': 'Chưa có workflow nào — tạo mới bên dưới',
  'Pick a workflow…': 'Chọn một workflow…',
  'Starting…': 'Đang khởi động…',
  'Run workflow': 'Chạy workflow',
  'or': 'hoặc',
  'Create a new workflow with the AI agent': 'Tạo workflow mới bằng agent AI',
  'Describe the routine… e.g. Weekly: research a topic from 3 angles in parallel, fetch pricing with a script, then summarize into one report.':
      'Mô tả quy trình… vd: Hằng tuần: nghiên cứu một chủ đề từ 3 góc nhìn song song, lấy bảng giá bằng script, rồi tóm tắt thành một báo cáo.',
  'Agent is drafting (30–120s)…': 'Agent đang soạn (30–120 giây)…',
  'Create workflow': 'Tạo workflow',
  'The draft opens in an editor for review — Save to keep it, Cancel to discard.':
      'Bản nháp sẽ mở trong trình soạn thảo để bạn xem lại — Lưu để giữ, Huỷ để bỏ.',
  'Describe the routine first': 'Hãy mô tả quy trình trước đã',
  'Review draft — edit if needed, then Save':
      'Xem lại bản nháp — sửa nếu cần rồi bấm Lưu',
  'The content is validated (DAG, personas, cycles…) on save. Cancel discards the draft.':
      'Nội dung sẽ được kiểm tra (DAG, persona, vòng lặp…) khi lưu. Bấm Huỷ để bỏ bản nháp.',
  'Save workflow': 'Lưu workflow',
  'Saved "{name}" — fill the inputs and press Run':
      'Đã lưu "{name}" — điền tham số rồi bấm Chạy',
};
