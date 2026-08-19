/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
///
/// Area: the Plugins sub-panels — Sandbox, Cowork, Widget. Policy identifiers
/// (execShell / runPython / runNode / schedulerScript), fs modes
/// (strict / allowlist / open), mount paths and MCP tool names stay literal;
/// only their human-facing descriptions are translated.
const Map<String, String> viPluginsMisc = {
  // ── sandbox_panel.dart — header + capabilities ────────────────────────────
  'Run commands and code isolated from the real machine — sessions, CPU/RAM monitoring, and the exec/python/node/script enforcement switches.':
      'Chạy lệnh và mã nguồn cách ly khỏi máy thật — luồng, giám sát CPU/RAM, cơ chế cưỡng chế exec/python/node/script.',
  'This daemon does not serve /api/sandbox yet — rebuild and restart the daemon.':
      'Daemon chưa hỗ trợ /api/sandbox — cần build lại và khởi động daemon mới.',
  'Available isolation': 'Cách ly khả dụng',
  'ready': 'sẵn sàng',
  'no': 'không',

  // ── sandbox_panel.dart — enforcement switches ─────────────────────────────
  'Security enforcement — run through the sandbox':
      'Cơ chế bảo mật — chạy trên sandbox',
  'Exec (agent Bash tool)': 'Exec (tool Bash của agent)',
  "Agent shell commands run inside the OS sandbox and can only write to the chat's working directory. Note: build caches outside the workspace (npm/cargo…) will be blocked from writing.":
      'Lệnh shell của agent chạy trong OS sandbox, chỉ được ghi vào thư mục làm việc của chat. Lưu ý: build cache ngoài workspace (npm/cargo…) sẽ bị chặn ghi.',
  'Network': 'Mạng',
  'Disk read': 'Đọc đĩa',
  'Local ports': 'Cổng local',
  "Local ports the agent's shell may call (e.g. the dev server it is working on). Empty = none: loopback is where SenClaw's own API and every Space App live, and none of them ask for credentials.":
      'Cổng local mà shell của agent được gọi (ví dụ dev server đang làm). Để trống = không cổng nào: loopback là chỗ API của chính SenClaw và mọi Space App chạy, đều không hỏi mật khẩu.',
  'Allow real Python (REPL + sbx tools). Always sandboxed; switching off refuses to run.':
      'Cho phép chạy Python thật (REPL + tool sbx). Luôn trong sandbox; tắt là từ chối chạy.',
  'Allow real Node.js. Always sandboxed; switching off refuses to run.':
      'Cho phép chạy Node.js thật. Luôn trong sandbox; tắt là từ chối chạy.',
  'Network for Python/Node': 'Mạng cho Python/Node',
  'Off by default — enable when a snippet needs network access.':
      'Mặc định tắt — bật khi snippet cần gọi mạng.',
  'Scheduled scripts (scheduler)': 'Script hẹn giờ (scheduler)',
  'script / script-agent task commands run in a throwaway sandbox.':
      'Lệnh của task chế độ script/script-agent chạy trong sandbox dùng-một-lần.',

  // ── sandbox_panel.dart — sandbox list, runs, defaults ─────────────────────
  'Managed sandboxes ({n})': 'Luồng đang quản lý ({n})',
  'No sandboxes yet — the agent creates one when it runs code, or create one with sbx_create.':
      'Chưa có sandbox nào — agent sẽ tự tạo khi chạy code, hoặc tạo qua tool sbx_create.',
  'net: on': 'mạng: on',
  'net: off': 'mạng: off',
  'Stop all processes': 'Dừng mọi tiến trình',
  'Delete (keep files)': 'Xoá (giữ file)',
  'Recent runs': 'Lịch sử chạy gần nhất',
  'No runs yet.': 'Chưa có lần chạy nào.',
  'Defaults for new sandboxes': 'Mặc định cho sandbox mới',
  'Default disk read': 'Đọc đĩa mặc định',
  'Default network': 'Mạng mặc định',
  'Allowlist — extra folders the sandbox may READ in allowlist mode (writes stay blocked)':
      'Allowlist — thư mục sandbox được ĐỌC thêm ở chế độ allowlist (ghi vẫn bị chặn)',
  'No folders yet.': 'Chưa có thư mục nào.',
  'Choose folder…': 'Chọn thư mục…',
  'Choose a folder the sandbox may read': 'Chọn thư mục cho sandbox đọc',
  'or type an absolute path: /Users/you/data':
      'hoặc gõ đường dẫn tuyệt đối: /Users/ban/du-lieu',
  'Add path': 'Thêm đường dẫn',
  'Remove from allowlist': 'Bỏ khỏi allowlist',
  'An absolute path is required (starts with / or C:\\)':
      'Cần đường dẫn tuyệt đối (bắt đầu bằng / hoặc C:\\)',
  'Already in the allowlist': 'Đã có trong allowlist',

  // ── sandbox_panel.dart — snackbars & dialogs ──────────────────────────────
  'Enforcement saved': 'Đã lưu cơ chế sandbox',
  'Defaults saved': 'Đã lưu cài đặt mặc định',
  // Shared with widgets_panel.dart.
  'Save failed: {e}': 'Lưu thất bại: {e}',
  'Stopped all processes in "{name}"': 'Đã dừng mọi tiến trình của "{name}"',
  'Stop failed: {e}': 'Không dừng được: {e}',
  'Delete sandbox?': 'Xoá sandbox?',
  'Delete with all files?': 'Xoá kèm toàn bộ file?',
  'Removes "{name}" from the list. Files on disk are kept.':
      'Xoá "{name}" khỏi danh sách. File trên đĩa được giữ lại.',
  'Deletes "{name}" AND every file in its working directory. This cannot be undone.':
      'Xoá "{name}" VÀ toàn bộ file trong thư mục làm việc của nó. Không khôi phục được.',
  'Delete files': 'Xoá sạch',
  'Deleted': 'Đã xoá',
  'Delete failed: {e}': 'Xoá thất bại: {e}',
  'Measuring…': 'Đang đo…',
  'No processes running.': 'Không có tiến trình nào đang chạy.',
  'Stop this process': 'Dừng tiến trình này',
  'Stopped process {pid}': 'Đã dừng tiến trình {pid}',
  'Delete with files': 'Xoá kèm file',

  // ── cowork_panel.dart — header & tabs ─────────────────────────────────────
  'Manage team templates & multi-agent teams':
      'Quản lý mẫu team và các team đa agent',
  'Templates': 'Mẫu',
  'Teams ({n})': 'Team ({n})',
  'Personas': 'Persona',

  // ── cowork_panel.dart — template cards ────────────────────────────────────
  'Built-in blueprints + your custom templates. Use one to spin up a team.':
      'Mẫu có sẵn + mẫu bạn tự tạo. Chọn một mẫu để dựng team.',
  'New template': 'Mẫu mới',
  'built-in': 'tích hợp sẵn',
  'No description': 'Không có mô tả',
  '{n} member': '{n} thành viên',
  '{n} members': '{n} thành viên',
  'Use': 'Dùng',
  'Clone to edit': 'Nhân bản để sửa',
  'Team created from {name}': 'Đã tạo team từ {name}',

  // ── cowork_panel.dart — team cards ────────────────────────────────────────
  'No teams yet — create one from a template':
      'Chưa có team nào — tạo một team từ mẫu',
  'Save as template': 'Lưu thành mẫu',
  'Saved as template': 'Đã lưu thành mẫu',

  // ── cowork_panel.dart — template editor ───────────────────────────────────
  'Edit template': 'Sửa mẫu',
  'Icon': 'Biểu tượng',
  'Manager folder': 'Thư mục manager',
  'Manager role': 'Vai trò manager',
  'Members': 'Thành viên',
  'Add member': 'Thêm thành viên',
  'folder (persona)': 'thư mục (persona)',
  'role': 'vai trò',
  'responsibilities': 'trách nhiệm',
  'triggers JSON e.g. [{"type":"task_assigned"}]':
      'JSON triggers, ví dụ [{"type":"task_assigned"}]',
  'Auto-create tasks on each user message':
      'Tự tạo tác vụ cho mỗi tin nhắn người dùng',
  'Name and manager folder are required': 'Cần nhập tên và thư mục manager',

  // ── cowork_panel.dart — team settings & personas ──────────────────────────
  '{name} · settings': '{name} · cài đặt',
  'Manager preamble': 'Lời dẫn cho manager',
  'Extra system instructions prepended for the manager':
      'Chỉ dẫn hệ thống thêm, chèn trước phần của manager',
  'No personas': 'Chưa có persona',
  'Persona markdown…': 'Markdown của persona…',

  // ── widgets_panel.dart — catalog ──────────────────────────────────────────
  'This daemon does not serve /api/widgets yet — rebuild and restart the daemon.':
      'Daemon chưa hỗ trợ /api/widgets — cần build lại và khởi động daemon mới.',
  'Widgets render in the chat pane (emit_widget) and on the Dashboard. Space Apps declare them in senclaw-manifest.json → widgets[]; plugins in widgets/widgets.json.':
      'Widget hiển thị trong ô chat (emit_widget) và trên Dashboard. Space App khai báo trong senclaw-manifest.json → widgets[]; plugin trong widgets/widgets.json.',
  'Widget catalog': 'Danh mục widget',
  'No widgets yet.': 'Chưa có widget nào.',

  // ── widgets_panel.dart — default flows ────────────────────────────────────
  'Default flows': 'Luồng mặc định',
  'Open link': 'Mở link',
  'System browser': 'Trình duyệt hệ thống',
  'New tab (web UI)': 'Tab mới (web UI)',
  'Mini Browser (inside SenClaw)': 'Mini Browser (trong SenClaw)',
  'install the mini-browser app to open inside SenClaw':
      'cài app mini-browser để mở trong SenClaw',
  'Play inline in chat (widget)': 'Phát ngay trong chat (widget)',
  'App Search (federated)': 'App Search (tìm liên hợp)',
  'install the search app for federated search':
      'cài app search để dùng federated search',
  'Note': 'Ghi chú',
  'These defaults go into the agent system prompt ("User defaults") and drive what a link tap does. Messaging channels always get a text summary instead of the widget.':
      'Các mặc định này được đưa vào system prompt của agent ("User defaults") và điều khiển hành vi click link. Kênh nhắn tin luôn nhận bản tóm tắt text thay cho widget.',

  // ── space_app_sandbox_dialog.dart — sandbox riêng cho từng Space App ───────
  'Sandbox settings': 'Cấu hình sandbox',
  'Sandbox — {name}': 'Sandbox — {name}',
  'This machine cannot confine a Space App (isolation: {kind}). The app keeps running, unconfined — the switch is stored but not enforced.':
      'Máy này không cách ly được Space App (cơ chế: {kind}). App vẫn chạy nhưng không bị giới hạn — công tắc chỉ được lưu, chưa cưỡng chế.',
  'Run this app inside the sandbox': 'Chạy app này trong sandbox',
  'The app may only write its own folder and its own data folder. Everything below applies while this is on.':
      'App chỉ được ghi vào thư mục của chính nó và thư mục dữ liệu của nó. Các mục dưới đây chỉ có hiệu lực khi bật.',
  'Folders': 'Thư mục',
  'May read': 'Được đọc',
  'Everything except credentials (default)': 'Mọi thứ trừ thư mục chứa khoá (mặc định)',
  'Only its own + granted folders': 'Chỉ thư mục của nó + thư mục được cấp',
  'Always granted, read and write:': 'Luôn được cấp, đọc và ghi:',
  'read-only': 'chỉ đọc',
  'read+write': 'đọc+ghi',
  'Add folder': 'Thêm thư mục',
  'Choose a folder this app may use': 'Chọn thư mục cho app này dùng',
  'If the app stores data somewhere else, add that folder here — otherwise it fails to write and says so in its log.':
      'Nếu app lưu dữ liệu ở nơi khác, thêm thư mục đó vào đây — không thì app sẽ ghi lỗi và báo trong log của nó.',
  'Everything (like an app outside the sandbox)': 'Toàn bộ (như app chạy ngoài sandbox)',
  'Only these sites': 'Chỉ các trang này',
  'No network at all': 'Không có mạng',
  'On this platform the site list is not enforced — only the folder rules are.':
      'Trên nền tảng này danh sách trang KHÔNG được cưỡng chế — chỉ luật thư mục là thật.',
  'No site listed yet — the app can reach nothing.':
      'Chưa khai trang nào — app không ra được đâu cả.',
  'Add site': 'Thêm trang',
  "Enforced by SenClaw's allowlist proxy on loopback: the app gets no direct way out, so a request to anything not listed fails. Traffic stays end-to-end encrypted — only the destination is checked.":
      'Cưỡng chế bằng proxy allowlist của SenClaw trên loopback: app không có đường ra trực tiếp, nên mọi yêu cầu tới trang không khai đều thất bại. Lưu lượng vẫn mã hoá đầu-cuối — chỉ đích đến bị kiểm tra.',
  'Proxy live on 127.0.0.1:{port} — {ok} allowed, {no} refused':
      'Proxy đang chạy ở 127.0.0.1:{port} — {ok} cho qua, {no} bị chặn',
  'The app wanted:': 'App đang cần:',
  'This machine': 'Máy này',
  "May call SenClaw's own API on 127.0.0.1:{port}":
      'Được gọi API của chính SenClaw ở 127.0.0.1:{port}',
  "Required by the AI bridge, which is what most apps use for anything intelligent. It is also SenClaw's unauthenticated local API — uncheck it for an app that does not need AI. Every other local service stays closed.":
      'Cần cho AI bridge — thứ hầu hết app dùng để làm phần thông minh. Đây cũng là API local không xác thực của SenClaw — bỏ tick nếu app không cần AI. Mọi dịch vụ local khác vẫn đóng.',
  'Other local ports': 'Cổng local khác',
  'Save & restart app': 'Lưu & khởi động lại app',
  'Saved — app restarted with the new sandbox':
      'Đã lưu — app đã khởi động lại với sandbox mới',
  'Saved. The site list applies immediately; other changes need a restart.':
      'Đã lưu. Danh sách trang có hiệu lực ngay; các thay đổi khác cần khởi động lại app.',
  'Saved. Restart the app for it to take effect.':
      'Đã lưu. Khởi động lại app để có hiệu lực.',

  // ── space_app_runtime_panel.dart — theo dõi tiến trình của một Space App ──
  'Process monitor': 'Theo dõi tiến trình',
  'Cannot read the state: {e}': 'Không đọc được trạng thái: {e}',
  'Reading the state…': 'Đang đọc trạng thái…',
  'running': 'đang chạy',
  'running but not answering': 'chạy nhưng không trả lời',
  'not running': 'không chạy',
  'port {p}': 'cổng {p}',
  'up {t}': 'đã chạy {t}',
  '{n} launches': '{n} lần khởi chạy',
  'How many times the daemon has launched this app since it started. A number that keeps climbing on its own means the app keeps dying.':
      'Số lần daemon khởi chạy app kể từ lúc daemon bật. Con số tự tăng đều nghĩa là app đang chết đi sống lại.',
  'This app has been launched many times — it is most likely dying and being restarted. The log below says why.':
      'App đã được khởi chạy nhiều lần — nhiều khả năng nó chết rồi được bật lại. Log bên dưới nói lý do.',
  'Restart': 'Khởi động lại',
  'Open': 'Mở',
  'Open folder': 'Mở thư mục',
  '{n} processes': '{n} tiến trình',
  'allowlist proxy 127.0.0.1:{port} — {ok} allowed, {no} refused':
      'proxy allowlist 127.0.0.1:{port} — {ok} cho qua, {no} bị chặn',
  'No sockets': 'Không có socket nào',
  'The app is not running': 'App không chạy',
  'Folder': 'Thư mục',
  'Command': 'Lệnh chạy',
  'Environment': 'Biến môi trường',
  'Log file': 'File log',
  'Copy': 'Sao chép',

  // ── sandbox_panel.dart — card Space Apps trong màn Sandbox ────────────────
  'Space Apps — per-app sandbox': 'Space Apps — sandbox từng app',
  'This machine cannot confine a served app (isolation: {kind}) — the switches are stored but not enforced.':
      'Máy này không cách ly được app đang phục vụ (cơ chế: {kind}) — các công tắc được lưu nhưng không cưỡng chế.',
  'No server Space App installed': 'Chưa có Space App nào chạy server',
  'off': 'tắt',
  'enabled': 'đã bật',
  'restart needed': 'cần khởi động lại',
  'everything': 'toàn bộ',
  'only some sites': 'chỉ vài trang',
  'no network': 'không có mạng',
  '{n} refused': '{n} bị chặn',
  'App restarted': 'Đã khởi động lại app',
  'Restart failed: {e}': 'Khởi động lại thất bại: {e}',

  // ── phân trang + dialog theo dõi mở từ card Sandbox ───────────────────────
  'Previous page': 'Trang trước',
  'Next page': 'Trang sau',
  'Process monitor — {name}': 'Theo dõi tiến trình — {name}',

  // ── sắp xếp danh sách app trong card Sandbox ──────────────────────────────
  'Sort by': 'Sắp xếp',
  'Status': 'Trạng thái',
  'Name': 'Tên',
  'Launches': 'Số lần khởi chạy',
  'Ascending': 'Tăng dần',
  'Descending': 'Giảm dần',

  // ── app đang chạy nhưng daemon không khởi chạy (adopted) ───────────────────
  'adopted': 'ngoài daemon',
  'unknown': 'không rõ',
  'running (adopted)': 'đang chạy (ngoài daemon)',
  "This process is running but was NOT launched by the current daemon — it was already alive on the app's port, usually left over from a daemon restart. Whether the sandbox confines it is therefore unknown; restart the app if you need to be sure.":
      'Tiến trình này đang chạy nhưng KHÔNG do daemon hiện tại khởi chạy — nó đã sống sẵn trên cổng của app, thường là còn sót sau khi daemon khởi động lại. Vì vậy không biết nó có bị sandbox nhốt hay không; khởi động lại app nếu cần chắc chắn.',

  // ── patterns_panel.dart — Zen Patterns ────────────────────────────────────
  "Cancel": "Huỷ",
  "Confirm": "Xác nhận",
  "Sources": "Nguồn",
  "Reload": "Tải lại",
  "Add git source": "Thêm nguồn git",
  "last sync failed": "lần đồng bộ trước lỗi",
  "Sync": "Đồng bộ",
  "Remove source": "Gỡ nguồn",
  "All sources": "Mọi nguồn",
  "Search name or description": "Tìm tên hoặc mô tả",
  "New pattern": "Pattern mới",
  "No patterns yet — add a git source (Fabric, for example) or write one":
      "Chưa có pattern nào — thêm nguồn git (ví dụ Fabric) hoặc tự viết một cái",
  "Nothing matches the filter": "Không khớp bộ lọc",
  "Delete pattern": "Xoá pattern",
  "Git source — to change it, save your own copy under the same name; it takes priority":
      "Nguồn git — sửa bằng cách lưu bản riêng cùng tên vào nguồn của bạn",
  "Paste the text to transform (article, transcript, log, notes…)":
      "Dán văn bản cần xử lý (bài báo, transcript, log, ghi chú…)",
  "Strategy (optional)": "Strategy (tuỳ chọn)",
  "No strategy": "Không strategy",
  "Language: follow the input": "Ngôn ngữ: theo input",
  "Language: Vietnamese": "Ngôn ngữ: tiếng Việt",
  "Language: let the pattern decide": "Ngôn ngữ: pattern tự quyết",
  "Run": "Chạy",
  "Assemble the prompt only — costs no model call":
      "Chỉ ghép prompt để xem trước — không tốn lượt gọi model",
  "Preview prompt": "Xem prompt",
  "the pattern keeps the placeholder rather than deleting it":
      "pattern giữ nguyên chỗ trống thay vì xoá đi",
  "A repository URL is required": "Cần URL repo",
  "Add a pattern source from git": "Thêm nguồn pattern từ git",
  "Pin a tag, do not track a branch. A pattern is placed in the system-prompt position — following a moving branch lets an upstream commit silently rewrite instructions the agent will obey.":
      "Nên ghim tag, đừng để nhánh. Pattern được đặt vào vị trí system prompt — theo một nhánh đang chạy nghĩa là một commit phía trên có thể lặng lẽ viết lại chỉ thị mà agent sẽ tuân theo.",
  "Source id (blank = after the repo name)":
      "Id nguồn (bỏ trống = theo tên repo)",
  "Folder holding the patterns": "Thư mục chứa pattern",
  "Strategies folder (optional)": "Thư mục strategies (tuỳ chọn)",
  "Add and download": "Thêm và tải về",
  "A name and a system prompt are required":
      "Cần tên và system prompt",
  "Letters, digits, - and _ . Diacritics fold to a plain slug.":
      "Chữ, số, - và _ . Tên có dấu sẽ được bỏ dấu thành slug.",
  "Fabric convention: # IDENTITY and PURPOSE → # STEPS → # OUTPUT INSTRUCTIONS → # INPUT. Use {{input}} to place the text mid-prompt; without it the text becomes the user message.":
      "Quy ước Fabric: # IDENTITY and PURPOSE → # STEPS → # OUTPUT INSTRUCTIONS → # INPUT. Dùng {{input}} để chèn văn bản vào giữa prompt; không có thì văn bản thành user message.",
  "Save": "Lưu",
  "A named prompt for one text transform: text in, text out, one model call, no tools. Agents reach them through pattern_run.":
      "Prompt đặt tên sẵn cho một phép biến đổi văn bản: chữ vào → chữ ra, một lượt model, không tool. Agent gọi chúng qua pattern_run.",
  "shadows {n} other source(s)":
      "đè lên {n} nguồn khác",
  "Also in: {others} — the copy in \"{winner}\" is the one used":
      "Cũng có trong: {others} — bản ở \"{winner}\" được dùng",
  "Remove \"{id}\" and delete its {n} pattern(s)?":
      "Gỡ \"{id}\" và xoá {n} pattern của nó?",
  "Delete \"{name}\"?":
      "Xoá \"{name}\"?",
  "{n} pattern(s) from \"{id}\"":
      "{n} pattern từ \"{id}\"",
  "Downloaded {n} pattern(s)":
      "Đã tải {n} pattern",
  "rendered only, no model call":
      "chỉ render, chưa gọi model",
  "Unfilled variables: {vars} — the pattern keeps the placeholder rather than deleting it":
      "Biến chưa điền: {vars} — pattern giữ nguyên chỗ trống thay vì xoá đi",
  "bundled":
      "đi kèm",
  "Installed":
      "Đã cài",
  "Install now":
      "Cài ngay",
  "Download":
      "Tải về",
  "Get started":
      "Bắt đầu",
  "From a .zip file":
      "Từ tệp .zip",
  "offline":
      "ngoại tuyến",
  "Choose a .zip":
      "Chọn tệp .zip",
  "Import a .zip of pattern folders":
      "Import .zip các thư mục pattern",
  "A zip whose sub-folders each hold a system.md. A GitHub download works too — the wrapping folder is stripped.":
      "Một zip mà mỗi thư mục con là một pattern có system.md. Zip tải từ GitHub cũng được — thư mục bọc ngoài được bỏ tự động.",
  "This source tracks a moving branch: an upstream commit can silently rewrite instructions the agent will obey.":
      "Nguồn này bám một nhánh đang chạy: một commit phía trên có thể lặng lẽ viết lại chỉ thị mà agent sẽ tuân theo.",
  "Installed {n} pattern(s) from \"{name}\"":
      "Đã cài {n} pattern từ \"{name}\"",
  "Imported {n}/{found} pattern(s)":
      "Đã import {n}/{found} pattern",
  "Hide this source": "Ẩn nguồn này",
  "Show again": "Hiện lại",
};
