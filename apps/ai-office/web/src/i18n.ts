/* Lightweight i18n for the AI Office UI.
 *
 * Design: the *source string is the key*. Vietnamese is the canonical text
 * written inline in the components; `t("…")` returns it unchanged when the
 * language is Vietnamese, or the English override from EN when the language
 * is English (falling back to the Vietnamese if a key is missing).
 *
 * The current language lives at module scope; App sets it once per render via
 * setLang(lang) so every t() below in the same render tree sees it. Changing
 * the language is a state update in App, which re-renders the whole tree. */

export type Lang = 'vi' | 'en'

let current: Lang = 'vi'

export function setLang(l: Lang) {
  current = l
}
export function getLang(): Lang {
  return current
}

/** Vietnamese → English. Any string not present falls back to Vietnamese. */
const EN: Record<string, string> = {}

/** Translate a canonical (Vietnamese) UI string to the active language. */
export function tr(vi: string): string {
  if (current === 'vi') return vi
  return EN[vi] ?? vi
}

/** Merge additional translations (used to assemble the dictionary in one
 *  place while the strings themselves live across many components). */
export function addTranslations(pairs: Record<string, string>) {
  Object.assign(EN, pairs)
}

/* ------------------------------------------------------------------------ */
/* The dictionary. Keys are the exact Vietnamese strings passed to t().      */
/* ------------------------------------------------------------------------ */
addTranslations({
  // ---- header / chrome ----
  'Giao diện: Auto theo hệ thống / Sáng / Tối': 'Theme: Auto (system) / Light / Dark',
  '◐ Auto': '◐ Auto',
  '☀ Sáng': '☀ Light',
  '🌙 Tối': '🌙 Dark',
  'BẬT': 'ON',
  'TẮT': 'OFF',
  'Kế toán': 'Accounting',
  'Nhân sự': 'Staff',
  'Lịch sử': 'History',
  'Nhiệm vụ mới': 'New task',
  'Cài đặt': 'Settings',
  'công ty một người — v1.0': 'one-person company — v1.0',
  'Gõ nhiệm vụ vào ô bên dưới rồi Enter. Trưởng phòng sẽ phân công agent làm việc & bàn giao, cuối cùng nộp báo cáo tổng hợp cho Sếp.':
    'Type a task in the box below and press Enter. The manager assigns agents to work and hand off, then submits a final consolidated report to the Boss.',
  'Nhiệm vụ': 'Task',
  // ---- teams / scene ----
  'ĐỘI': 'TEAM',
  'agent trực ca': 'agents on shift',
  'đang làm': 'working',
  'Quản lý đội nhóm': 'Manage teams',
  'Đội': 'Team',
  'Mô phỏng văn phòng': 'Office simulation',
  'SẾP (BẠN)': 'BOSS (YOU)',
  'VĂN PHÒNG': 'OFFICE',
  '· Mô phỏng — kéo để xoay · lăn chuột phóng to · giữ Shift kéo để dời':
    '· Simulation — drag to rotate · scroll to zoom · hold Shift and drag to pan',
  'Xoay nhanh 45°': 'Rotate 45°',
  'Phóng to': 'Zoom in',
  'Thu nhỏ': 'Zoom out',
  'Về mặc định': 'Reset view',
  'xong': 'done',
  'đi bàn giao': 'handing off',
  'Hàng đợi': 'Queue',
  // ---- composer / task ----
  'Phòng đang bận — gõ hoặc nói để xếp vào hàng đợi…':
    'Office is busy — type or speak to add to the queue…',
  'Giao nhiệm vụ cho phòng (gõ hoặc bấm 🎤 để nói)…':
    'Assign a task to the office (type or tap 🎤 to speak)…',
  'Giao việc': 'Assign',
  'Phòng đang bận — nhiệm vụ này sẽ xếp vào hàng đợi (hiện có':
    'Office is busy — this task will be queued (currently',
  'chờ) và tự chạy khi xong việc trước.':
    'waiting) and will run automatically once earlier work finishes.',
  'Trưởng phòng sẽ nhận và phân công cho cả phòng ngay.':
    'The manager will take it and assign the whole office right away.',
  'Ví dụ: nghiên cứu 5 xu hướng nội thất 2026 và đề xuất bộ sưu tập ra mắt':
    'e.g. research 5 furniture trends for 2026 and propose a launch collection',
  'Xếp hàng đợi': 'Add to queue',
  // ---- staff panel ----
  'Cho': 'Let',
  'nghỉ việc? Bàn làm việc sẽ bị thu hồi.': 'go? Their desk will be reclaimed.',
  'Giải thể đội': 'Disband team',
  'Toàn bộ nhân sự của đội sẽ bị xoá.': 'All members of the team will be removed.',
  'Đội nhóm & nhân sự': 'Teams & staff',
  'Đóng': 'Close',
  'Tên đội mới, ví dụ: Chăm sóc khách hàng': 'New team name, e.g. Customer Support',
  'Tạo đội': 'Create team',
  'Tuyển nhân sự vào đội': 'Hire staff into team',
  'Tên': 'Name',
  'Vai trò': 'Role',
  'Loại': 'Type',
  'Chế độ': 'Mode',
  'Trưởng phòng': 'Manager',
  'Chuyên môn': 'Specialist',
  'Kiểm định': 'QA',
  'tự nhận việc': 'auto-assign',
  'tăng cường': 'on call',
  'trực ca': 'on shift',
  'tạm nghỉ': 'off duty',
  'Chi tiết': 'Details',
  'Sửa': 'Edit',
  'Tạm dừng': 'Pause',
  'Kích hoạt': 'Activate',
  'Xoá': 'Delete',
  // ---- staff dialog ----
  'Sửa hồ sơ': 'Edit profile',
  'Tuyển nhân sự mới': 'Hire new staff',
  'Tên hiển thị': 'Display name',
  'VD: THIẾT KẾ': 'e.g. DESIGN',
  'VD: Thiết kế & hình ảnh': 'e.g. Design & visuals',
  'Nhiệm vụ cố định': 'Fixed duties',
  'Mô tả nhiệm vụ mà nhân sự này luôn đảm nhận trong quy trình…':
    'Describe the duties this member always handles in the workflow…',
  'không đổi được': 'cannot be changed',
  'Chuyên môn (worker)': 'Specialist (worker)',
  '— đã có': '— already exists',
  'Nhận việc': 'Assignment',
  'Tự nhận nhiệm vụ — luôn có phần việc trong mọi kế hoạch. Bỏ chọn = tăng cường (Trưởng phòng chỉ giao khi cần chuyên môn này).':
    'Auto-assign — always has a part in every plan. Uncheck = on call (the manager only assigns when this expertise is needed).',
  'Skill / sub-agent nắm giữ': 'Skills / sub-agents held',
  'Bỏ chọn': 'Remove',
  '🔍 Tìm skill / sub-agent…': '🔍 Search skill / sub-agent…',
  'Đang tải danh mục…': 'Loading catalog…',
  'Không lấy được danh mục từ daemon — kiểm tra SenClaw daemon.':
    "Couldn't load the catalog from the daemon — check the SenClaw daemon.",
  'Không có mục nào khớp': 'No items match',
  'Lưu hồ sơ': 'Save profile',
  'Tuyển vào phòng': 'Hire into office',
  // ---- staff detail ----
  'Tạm nghỉ — không tham gia nhiệm vụ': 'Off duty — not participating in tasks',
  'Tự nhận nhiệm vụ — luôn có phần việc': 'Auto-assign — always has a part',
  'Tăng cường — chỉ được giao khi cần chuyên môn':
    'On call — only assigned when the expertise is needed',
  'Trực ca': 'On shift',
  'Skill / sub-agent': 'Skill / sub-agent',
  'Trạng thái': 'Status',
  'Trí nhớ riêng': 'Private memory',
  'không đọc được': "couldn't read",
  'đang đếm…': 'counting…',
  'ký ức trong space': 'memories in space',
  'Xem chi tiết trong Knowledge (desktop app) — chọn space này ở bộ lọc.':
    'See details in Knowledge (desktop app) — pick this space in the filter.',
  // ---- new task dialog ----
  // ---- history ----
  'Lịch sử nhiệm vụ': 'Task history',
  'danh sách': 'list',
  'chưa có báo cáo': 'no report yet',
  'Chưa có nhiệm vụ nào — giao việc đầu tiên cho phòng đi Sếp!':
    'No tasks yet — give the office its first assignment, Boss!',
  'Trước': 'Prev',
  'Trang': 'Page',
  'nhiệm vụ': 'tasks',
  'Sau': 'Next',
  // ---- ledger ----
  'Tổng nhiệm vụ': 'Total tasks',
  'Đã hoàn thành': 'Completed',
  'Lượt gọi LLM': 'LLM calls',
  'Token đã dùng (ước tính)': 'Tokens used (estimated)',
  'vào': 'in',
  'ra': 'out',
  'Model gần nhất': 'Latest model',
  'Lương nhân sự': 'Staff salary',
  '0 ₫ (agent không nhận lương 😜)': "$0 (agents don't get paid 😜)",
  'Đang tải…': 'Loading…',
  // ---- settings ----
  'Ngôn ngữ': 'Language',
  'Workspace folder': 'Workspace folder',
  '~/Documents/ai-office hoặc đường dẫn tuyệt đối': '~/Documents/ai-office or an absolute path',
  'Chọn…': 'Browse…',
  'Lưu': 'Save',
  'Kho tài liệu chung của phòng: Sếp bỏ tệp tham khảo vào đây (mở bằng Finder), nhân sự sẽ đọc khi làm việc và ghi kết quả vào':
    'The office\'s shared document store: drop reference files here (open in Finder), staff read them while working and write results to',
  'Để trống rồi Lưu = quay về thư mục mặc định.':
    'Leave empty and Save = return to the default folder.',
  'Hiện có': 'Currently',
  'tệp': 'files',
  'mặc định': 'default',
  'đã lưu': 'saved',
  'Góc nhìn văn phòng': 'Office viewpoint',
  'Xoay tự do 360° quanh tâm sàn — kéo chuột trái/phải ngay trên khung mô phỏng, hoặc chỉnh bằng thanh trượt / nút góc ở đây.':
    'Rotate freely 360° around the floor center — drag left/right on the simulation, or adjust with the slider / angle buttons here.',
  'Chức năng phòng': 'Office features',
  'Vận hành': 'Operation',
  'Mỗi agent xử lý thật phần việc của mình qua LLM của SenClaw daemon.':
    "Each agent actually handles its own work via the SenClaw daemon's LLM.",
  'Hiện không kết nối được daemon LLM.': "Can't connect to the LLM daemon right now.",
  'Daemon LLM sẵn sàng.': 'LLM daemon is ready.',
  'MCP cho agent ngoài': 'MCP for external agents',
  'Server': 'Server',
  'agent SenClaw có thể giao việc bằng': 'SenClaw agents can assign tasks via',
  'và lấy kết quả bằng': 'and fetch results via',
  'Worker dùng công cụ (MCP / search)': 'Workers use tools (MCP / search)',
  'Nhân sự có gán skill/sub-agent sẽ chạy như agent thật: gọi được web-search, browser, MCP.':
    'Staff with assigned skills/sub-agents run like real agents: they can call web-search, browser, MCP.',
  'Trí nhớ riêng mỗi nhân sự': 'Private memory per staff member',
  'Nhớ lại & lưu ký ức vào knowledge space riêng qua mỗi nhiệm vụ.':
    'Recall and save memories to a private knowledge space across each task.',
  'Lưu báo cáo vào wiki': 'Save reports to wiki',
  'Báo cáo tổng hợp tự lưu vào kho wiki của daemon.':
    "Consolidated reports are saved automatically to the daemon's wiki store.",
  'Đọc / ghi workspace': 'Read / write workspace',
  'Đọc tài liệu Sếp bỏ vào workspace và ghi kết quả ra file.':
    'Read documents the Boss puts in the workspace and write results to files.',
  'Tự viết tiếp khi bị cắt': 'Auto-continue when cut off',
  'Nếu LLM cắt giữa chừng, tự yêu cầu viết tiếp cho trọn.':
    'If the LLM is cut off mid-way, automatically request a continuation to finish.',
  // ---- folder picker ----
  'Chọn workspace folder': 'Choose workspace folder',
  'lên thư mục cha': 'up to parent folder',
  'không có thư mục con': 'no subfolders',
  'Chọn thư mục này': 'Choose this folder',
  // ---- voice ----
  'Không truy cập được micro': "Can't access microphone",
  'Không nghe rõ, thử lại': "Didn't catch that, try again",
  'Dừng & chuyển thành chữ': 'Stop & transcribe',
  'Giao việc bằng giọng nói': 'Assign a task by voice',
  'Dừng': 'Stop',
  'Đọc to bằng giọng nói': 'Read aloud',
  // ---- feed ----
  'SẾP': 'BOSS',
  'HỆ THỐNG': 'SYSTEM',
  'BÁO CÁO': 'REPORT',
  'Đọc': 'Read',
  // ---- view tabs ----
  'ĐIỀU HÀNH': 'HQ DESK',
  'BẢNG VIỆC': 'TASK BOARD',
  'NHÂN SỰ': 'STAFF',
  'LỊCH SỬ': 'HISTORY',
  'SƠ ĐỒ TỔ CHỨC': 'ORG CHART',
  'đội nhóm & biên chế nhân sự AI': 'teams & AI staffing',
  'LỊCH SỬ NHIỆM VỤ': 'TASK HISTORY',
  'sổ ghi mọi việc đã qua tay văn phòng': 'a record of every task the office handled',
  // ---- dashboard điều hành ----
  'BÀN LÀM VIỆC CỦA SẾP': "BOSS'S DESK",
  'ĐỘ BÁM HƯỚNG': 'ON-COURSE',
  'Chưa có việc đang mở trên bảng': 'No open tasks on the board',
  'việc đang bám mục tiêu': 'tasks tied to a goal',
  'MỤC TIÊU QUÝ': 'QUARTER GOALS',
  'mục tiêu': 'goals',
  'Tiến độ trung bình các mục tiêu': 'Average progress across goals',
  'CHỜ SẾP DUYỆT': 'AWAITING BOSS',
  'việc': 'tasks',
  'Bấm để nghiệm thu trên Bảng việc': 'Click to review on the Task board',
  'Bàn Sếp đang sạch': "Boss's desk is clear",
  'NHỊP ĐIỀU HÀNH': 'OPERATING RHYTHM',
  'ngày liên tiếp': 'day streak',
  'Hôm nay đã họp sáng': 'Morning briefing done today',
  'Hôm nay chưa họp sáng': 'No morning briefing yet today',
  'CHI PHÍ AI (THÁNG)': 'AI SPEND (MONTH)',
  'việc đang mở trên bảng': 'open tasks on the board',
  'HỌP SÁNG VỚI GIÁM ĐỐC VẬN HÀNH': 'MORNING BRIEFING WITH THE COO',
  'HỌP TỐI — TỔNG KẾT NGÀY': 'EVENING WRAP-UP',
  'Giám đốc vận hành đang chuẩn bị biên bản…': 'The COO is preparing the minutes…',
  'Chưa có biên bản họp nào.': 'No meeting minutes yet.',
  'Bấm ☀ HỌP SÁNG để Giám đốc vận hành điểm tình hình công ty và đề xuất 3 ưu tiên hôm nay.':
    "Press ☀ MORNING BRIEFING for the COO to review the company and propose today's top 3 priorities.",
  'BIÊN BẢN HỌP SÁNG': 'MORNING BRIEFING MINUTES',
  'BIÊN BẢN HỌP TỐI': 'EVENING WRAP-UP MINUTES',
  'GIÁM ĐỐC VẬN HÀNH': 'COO',
  'Mở Bảng việc': 'Open the Task board',
  // ---- mục tiêu quý ----
  'mọi việc trên bảng đều truy được về đây': 'every task on the board traces back here',
  'Thêm mục tiêu': 'Add goal',
  'Chưa có mục tiêu quý — đặt mục tiêu để đo ĐỘ BÁM HƯỚNG của bảng việc.':
    'No quarter goals yet — set goals to measure how ON-COURSE the board is.',
  'việc đang mở': 'open tasks',
  'chưa có kết quả then chốt — bấm Sửa để thêm': 'no key results yet — press Edit to add',
  'Lưu trữ': 'Archive',
  'Xoá mục tiêu': 'Delete goal',
  'Việc đang gắn sẽ thành lạc hướng.': 'Tasks linked to it become off-course.',
  'Sửa mục tiêu quý': 'Edit quarter goal',
  'Thêm mục tiêu quý': 'Add quarter goal',
  'Mục tiêu': 'Goal',
  'VD: Đạt 30 triệu doanh thu/tháng từ khoá học': 'e.g. Reach $1,200/month revenue from the course',
  'Quý': 'Quarter',
  'Kết quả then chốt': 'Key results',
  'Mỗi dòng một kết quả đo được, VD:\n50 học viên mới trong quý\nChuỗi email 5 thư chạy tự động':
    'One measurable result per line, e.g.:\n50 new students this quarter\n5-email sales sequence running',
  'Lưu mục tiêu': 'Save goal',
  'Đặt mục tiêu': 'Set goal',
  // ---- bảng việc ----
  'AI làm, Sếp duyệt': 'AI works, Boss approves',
  'GIAO VIỆC MỚI': 'ASSIGN NEW TASK',
  'HỘP VIỆC': 'INBOX',
  'ĐANG LÀM': 'DOING',
  'HOÀN TẤT': 'DONE',
  '(trống)': '(empty)',
  'lỗi': 'error',
  'hàng đợi': 'queued',
  'đang chạy': 'running',
  'làm lại': 'redo',
  'lạc hướng': 'off-course',
  'Nội dung việc': 'Task',
  'Ví dụ: khảo sát 5 kênh TikTok cùng ngách để rút công thức video':
    'e.g. survey 5 TikTok channels in the same niche to extract a video formula',
  'Đội xử lý': 'Team',
  '— không gắn (lạc hướng) —': '— no goal (off-course) —',
  'Chạy': 'Run',
  'Chạy ngay (bỏ chọn = để trong Hộp việc, chạy sau)':
    'Run now (uncheck = keep in the Inbox, run later)',
  'Đưa vào Hộp việc': 'Add to Inbox',
  'CHI TIẾT VIỆC': 'TASK DETAILS',
  'Chờ Sếp duyệt': 'Awaiting Boss approval',
  'Hoàn tất — Sếp đã duyệt': 'Done — approved by the Boss',
  'Ghi chú Sếp': "Boss's note",
  'Việc đang chạy — xem cả đội làm việc trực tiếp ở tab VĂN PHÒNG.':
    'Task is running — watch the team live in the OFFICE tab.',
  'Phân công': 'Assignments',
  'Báo cáo tổng hợp': 'Consolidated report',
  'Chạy lại': 'Run again',
  'Chạy việc này': 'Run this task',
  'DUYỆT — nghiệm thu': 'APPROVE',
  'TRẢ LẠI': 'RETURN',
  'Xoá việc này khỏi bảng? Nhật ký của nó cũng bị xoá.':
    'Remove this task from the board? Its log is deleted too.',
  'Sếp muốn sửa gì? VD: phần định giá thiếu đối thủ X, viết lại ngắn hơn…':
    'What should change? e.g. pricing misses competitor X, make it shorter…',
  'Thôi': 'Cancel',
  'Trả lại — đội làm lại theo ghi chú': 'Return — the team redoes it per the note',
  'Hộp việc — chưa chạy': 'Inbox — not started',
  'Đang xếp hàng đợi': 'Queued',
  'Trưởng nhóm đang phân công': 'Team lead is assigning',
  'Cả đội đang làm': 'Team is working',
  'Kiểm định đang soát': 'QA is reviewing',
  'AI đã xong': 'AI finished',
  'Lỗi — cần Sếp xử lý': 'Error — needs the Boss',
  // ---- default seed content (team / agent names, roles, duties). Only the
  //      shipped defaults are here; anything a user renames falls back to
  //      their own text automatically. ----
  'NGHIÊN CỨU THỊ TRƯỜNG': 'MARKET RESEARCH',
  'PHÁT TRIỂN ỨNG DỤNG': 'APP DEVELOPMENT',
  'DỮ LIỆU & THỐNG KÊ': 'DATA & STATISTICS',
  'Đội nghiên cứu thị trường, đối thủ, hành vi khách hàng và cơ hội kinh doanh.':
    'Team researching the market, competitors, customer behavior and business opportunities.',
  'Đội phát triển sản phẩm/ứng dụng: thiết kế, lập trình, kiểm thử.':
    'Team building the product/app: design, engineering, testing.',
  'Đội tìm kiếm, tổng hợp và thống kê dữ liệu để ra quyết định.':
    'Team searching, aggregating and analyzing data to drive decisions.',
  'TRƯỞNG NHÓM': 'TEAM LEAD',
  'NGHIÊN CỨU': 'RESEARCH',
  'PHÂN TÍCH': 'ANALYSIS',
  'KIỂM ĐỊNH': 'QA',
  'THIẾT KẾ': 'DESIGN',
  'LẬP TRÌNH': 'ENGINEERING',
  'KIỂM THỬ': 'TESTING',
  'THU THẬP DL': 'DATA INTAKE',
  'THỐNG KÊ': 'STATISTICS',
  'KIỂM ĐỊNH DL': 'DATA QA',
  'Điều phối & tổng hợp': 'Coordination & synthesis',
  'Thu thập & phân tích thông tin': 'Gather & analyze information',
  'Số liệu, logic, đánh giá': 'Data, logic, evaluation',
  'Giám sát chất lượng & rủi ro': 'Quality & risk oversight',
  'Thiết kế & trải nghiệm': 'Design & experience',
  'Phát triển tính năng': 'Feature development',
  'Kiểm thử & chất lượng': 'Testing & quality',
  'Tìm kiếm & thu thập dữ liệu': 'Search & collect data',
  'Thống kê & trực quan hoá': 'Statistics & visualization',
  'Giám sát chất lượng dữ liệu': 'Data quality oversight',
  'Nhận nhiệm vụ từ Sếp, phân công cho đội và nộp báo cáo tổng hợp.':
    'Take tasks from the Boss, assign the team and submit a consolidated report.',
  'Phân tích đề bài, thu thập dữ kiện thị trường làm đầu vào cho đội.':
    'Analyze the brief and gather market facts as input for the team.',
  'Rà soát logic, bổ sung số liệu và hoàn thiện kết quả nghiên cứu.':
    'Review logic, add figures and finalize the research results.',
  'Soát lỗi, chỉ ra rủi ro trước khi bàn giao Trưởng nhóm.':
    'Check for errors and flag risks before handing off to the team lead.',
  'Nhận yêu cầu từ Sếp, chia việc cho đội và tổng hợp kết quả bàn giao.':
    'Take requests from the Boss, split work across the team and consolidate the handoff.',
  'Phác thảo giao diện, luồng người dùng và trải nghiệm sản phẩm.':
    'Sketch the interface, user flows and product experience.',
  'Triển khai tính năng, mô tả kỹ thuật và giải pháp khả thi.':
    'Implement features, technical specs and feasible solutions.',
  'Soát lỗi, rủi ro kỹ thuật và xác nhận chất lượng trước khi bàn giao.':
    'Check errors, technical risks and confirm quality before handoff.',
  'Nhận nhiệm vụ dữ liệu từ Sếp, phân công và tổng hợp báo cáo.':
    'Take data tasks from the Boss, assign work and consolidate the report.',
  'Tìm nguồn, thu thập và làm sạch dữ liệu cho đội.':
    'Find sources, collect and clean data for the team.',
  'Phân tích thống kê, rút ra xu hướng và trực quan hoá số liệu.':
    'Run statistical analysis, extract trends and visualize the data.',
  'Xác minh độ chính xác, chỉ ra sai lệch trước khi bàn giao.':
    'Verify accuracy and flag deviations before handoff.',
})
