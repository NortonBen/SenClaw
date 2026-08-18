/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viPluginsScreen = {
  // ── Section rail ──────────────────────────────────────────────────────────
  'Plugins': 'Plugin',
  'Skills': 'Skill',
  'Subagents': 'Subagent',
  'MCP servers': 'MCP server',
  'Hooks': 'Hook',
  'Schedules': 'Tác vụ hẹn giờ',
  'Knowledge': 'Tri thức',

  // ── Code tab ──────────────────────────────────────────────────────────────
  'Code executor': 'Trình chạy code',
  'Sandboxed JS/TS via senclaw-js, plus host-shell Bash':
      'JS/TS chạy trong sandbox qua senclaw-js, kèm Bash trên shell máy chủ',
  'LANGUAGES': 'NGÔN NGỮ',
  'LIVE': 'ĐANG CHẠY',
  'HOST': 'MÁY CHỦ',
  'PLANNED': 'DỰ KIẾN',
  'JS & TS run sandboxed; Bash runs on the host (not isolated); the rest are planned.':
      'JS & TS chạy trong sandbox; Bash chạy trên máy chủ (không cách ly); phần còn lại còn trong kế hoạch.',
  'MCP TOOLS · senclaw-js': 'CÔNG CỤ MCP · senclaw-js',
  'Run a JavaScript snippet; returns the value, console output, and any error':
      'Chạy một đoạn JavaScript; trả về giá trị, output console và lỗi nếu có',
  'Run TypeScript — transpiled to JS (types stripped), then run in the sandbox':
      'Chạy TypeScript — biên dịch sang JS (bỏ kiểu), rồi chạy trong sandbox',
  'Read a .js / .mjs file from disk and run it in the same sandbox':
      'Đọc tệp .js / .mjs từ ổ đĩa và chạy trong cùng sandbox',
  'TRY IT': 'DÙNG THỬ',
  'ARTIFACTS': 'ARTIFACT',

  // Feature cards.
  'JavaScript Sandbox (QuickJS)': 'Sandbox JavaScript (QuickJS)',
  'Agents run JavaScript via the senclaw-js MCP server — standard ECMAScript intrinsics plus a captured console, with no filesystem, network, or process access.':
      'Agent chạy JavaScript qua MCP server senclaw-js — các hàm ECMAScript tiêu chuẩn cùng console được ghi lại, không truy cập tệp, mạng hay tiến trình.',
  'Sandboxed Execution (JS / TS)': 'Chạy trong sandbox (JS / TS)',
  'JS/TS runs are bounded by a wall-clock timeout (default 5s, max 60s) and a memory cap (default 128 MiB, max 1 GiB). Infinite loops and over-allocation are killed — zero risk to the host.':
      'Mỗi lần chạy JS/TS bị giới hạn thời gian thực (mặc định 5s, tối đa 60s) và bộ nhớ (mặc định 128 MiB, tối đa 1 GiB). Vòng lặp vô hạn hay cấp phát quá mức đều bị dừng — không rủi ro cho máy chủ.',
  'TypeScript support': 'Hỗ trợ TypeScript',
  'TypeScript snippets are transpiled to JS (types stripped, no type-checking) and run in the same sandbox — interfaces, generics, enums, and casts all work.':
      'Đoạn TypeScript được biên dịch sang JS (bỏ kiểu, không kiểm kiểu) và chạy trong cùng sandbox — interface, generic, enum và ép kiểu đều dùng được.',
  'Bash (brush sandbox)': 'Bash (sandbox brush)',
  'Bash runs in brush — a pure-Rust shell — with no env, empty PATH (external programs blocked), a temp working dir, and a kill-enforced timeout (killable child process). In-process isolation, not an OS jail.':
      'Bash chạy trong brush — shell thuần Rust — không biến môi trường, PATH rỗng (chặn chương trình ngoài), thư mục làm việc tạm và timeout cưỡng chế (tiến trình con có thể bị kill). Cách ly trong tiến trình, không phải sandbox cấp hệ điều hành.',
  'More language runtimes': 'Thêm runtime ngôn ngữ',
  'Python, Go, and Rust are on the roadmap, reusing the same isolation + resource-limit model.':
      'Python, Go và Rust nằm trong kế hoạch, dùng lại cùng mô hình cách ly + giới hạn tài nguyên.',
  'Integrated Debugging': 'Gỡ lỗi tích hợp',
  'Set breakpoints, inspect variables, and step through execution. Supports stack-trace visualization and memory profiling.':
      'Đặt breakpoint, xem biến và chạy từng bước. Hỗ trợ hiển thị stack trace và đo bộ nhớ.',
  'Artifact Publishing': 'Xuất bản artifact',
  'Package and publish code outputs as reusable artifacts — share scripts, notebooks, and utilities across your agent network.':
      'Đóng gói và xuất bản kết quả code thành artifact tái dùng — chia sẻ script, notebook và tiện ích trong mạng lưới agent của bạn.',

  // ── JS REPL ───────────────────────────────────────────────────────────────
  'Interactive REPL': 'REPL tương tác',
  'Bash runs in the brush sandbox (pure-Rust): no env, empty PATH (external programs blocked), temp dir, kill-enforced timeout. In-process isolation, not an OS jail.':
      'Bash chạy trong sandbox brush (thuần Rust): không biến môi trường, PATH rỗng (chặn chương trình ngoài), thư mục tạm, timeout cưỡng chế. Cách ly trong tiến trình, không phải sandbox cấp hệ điều hành.',
  'Runs in the senclaw-js sandbox · 5s / 128 MiB limits':
      'Chạy trong sandbox senclaw-js · giới hạn 5s / 128 MiB',
  'Save as artifact': 'Lưu thành artifact',
  'Saved "{name}"': 'Đã lưu "{name}"',
  'Save failed: {e}': 'Lưu thất bại: {e}',
  'RESULT': 'KẾT QUẢ',
  'TIMED OUT': 'QUÁ THỜI GIAN',
  'ERROR': 'LỖI',
  'unknown error': 'lỗi không rõ',
  'No artifacts yet — write code above and tap “Save”.':
      'Chưa có artifact nào — viết code ở trên rồi bấm “Lưu”.',
  'Load into editor': 'Nạp vào trình soạn',

  // ── Subagents ─────────────────────────────────────────────────────────────
  'Search subagents…': 'Tìm subagent…',
  'New subagent': 'Subagent mới',
  'No subagents': 'Không có subagent',
  'No subagents match "{q}"': 'Không có subagent khớp "{q}"',
  'max {n}': 'tối đa {n}',
  '{n} tools': '{n} công cụ',
  'off': 'tắt',
  'No description': 'Không có mô tả',
  'Name and content are required': 'Bắt buộc nhập tên và nội dung',
  'Persona file (Markdown + frontmatter)': 'Tệp persona (Markdown + frontmatter)',

  // ── Skills ────────────────────────────────────────────────────────────────
  'Search skills…': 'Tìm skill…',
  'Create skill': 'Tạo skill',
  'Install from ClawHub': 'Cài từ ClawHub',
  'No skills': 'Không có skill',
  'No skills match "{q}"': 'Không có skill khớp "{q}"',
  'Uninstall': 'Gỡ cài đặt',
  'Bundled': 'Đi kèm',
  'Global': 'Toàn cục',
  'Other': 'Khác',
  'Name (slug)': 'Tên (slug)',
  'e.g. my-skill': 'vd. my-skill',
  'Letters, digits, - and _ only': 'Chỉ chữ, số, - và _',
  'Invalid slug': 'Slug không hợp lệ',
  'When should the agent use this skill?': 'Khi nào agent nên dùng skill này?',
  'Instructions (markdown)': 'Hướng dẫn (markdown)',
  'Leave empty to scaffold a starter template…':
      'Để trống để tạo khung mẫu ban đầu…',

  // ── MCP servers ───────────────────────────────────────────────────────────
  'Search servers/tools…': 'Tìm server/công cụ…',
  'Add server': 'Thêm server',
  'No MCP servers': 'Không có MCP server',
  'No servers match "{q}"': 'Không có server khớp "{q}"',
  'built-in': 'tích hợp sẵn',
  'Revoke auto-accept': 'Thu hồi tự động chấp nhận',
  'Auto-accept all': 'Tự động chấp nhận tất cả',
  'Auto-accept this tool': 'Tự động chấp nhận công cụ này',
  'Connect': 'Kết nối',
  'Disconnect': 'Ngắt kết nối',
  'Test': 'Kiểm tra',
  'Edit MCP server': 'Sửa MCP server',
  'Add MCP server': 'Thêm MCP server',
  'Server name': 'Tên server',
  'e.g. filesystem-server': 'vd. filesystem-server',
  'Transport': 'Giao thức',
  'Scope': 'Phạm vi',
  'User': 'Người dùng',
  'Project': 'Dự án',
  'Command': 'Lệnh',
  'Arguments (space-separated)': 'Tham số (cách nhau bởi dấu cách)',
  'Environment (KEY=VALUE per line)': 'Biến môi trường (mỗi dòng KEY=VALUE)',
  'Headers (Name: Value per line)': 'Header (mỗi dòng Name: Value)',

  // ── Alias ─────────────────────────────────────────────────────────────────
  'Add alias': 'Thêm alias',
  'Edit alias': 'Sửa alias',
  'Rename or override MCP tools': 'Đổi tên hoặc ghi đè công cụ MCP',
  '• New name: an alias that doesn\'t exist yet — the target tool '
      'shows up under the new name (the original name still resolves).\n'
      '• Override: an alias equal to an existing tool name — every call '
      'to that name executes the target tool instead.\n'
      '• Space-App aliases (mcp.toolAliases in senclaw-manifest.json) are '
      'imported disabled — enable them here before they take effect.':
      '• Tên mới: alias chưa tồn tại — công cụ đích sẽ xuất hiện dưới tên mới '
          '(tên gốc vẫn dùng được).\n'
          '• Ghi đè: alias trùng tên một công cụ đã có — mọi lời gọi tới tên đó '
          'sẽ chạy công cụ đích thay thế.\n'
          '• Alias của Space App (mcp.toolAliases trong senclaw-manifest.json) '
          'được nhập ở trạng thái tắt — bật tại đây thì mới có hiệu lực.',
  'No aliases yet — add one to rename or override a tool':
      'Chưa có alias nào — thêm một alias để đổi tên hoặc ghi đè công cụ',
  'Delete alias "{alias}"?': 'Xoá alias "{alias}"?',
  'App-declared aliases are re-imported (disabled) the next time the app starts.':
      'Alias do app khai báo sẽ được nhập lại (ở trạng thái tắt) trong lần khởi động app kế tiếp.',
  'override': 'ghi đè',
  'new name': 'tên mới',
  'user': 'người dùng',
  'Maps the name agents call to the tool that actually runs. '
      'Use an existing tool name as the alias to override that tool.':
      'Ánh xạ tên mà agent gọi tới công cụ thực sự chạy. Dùng tên một công cụ '
          'đã có làm alias để ghi đè công cụ đó.',
  'Alias (name agents call)': 'Alias (tên agent gọi)',
  'Target tool (actually executes)': 'Công cụ đích (thực sự chạy)',
  'Overrides an existing tool — every call to this name will run the target instead.':
      'Ghi đè một công cụ đã có — mọi lời gọi tới tên này sẽ chạy công cụ đích.',
  'New name — the target tool will show up under this name.':
      'Tên mới — công cụ đích sẽ xuất hiện dưới tên này.',
  'Tool exists on a connected MCP server.':
      'Công cụ có trên một MCP server đang kết nối.',
  'Not found on any connected MCP server — check the name or start '
      'its server. You can still save.':
      'Không tìm thấy trên MCP server nào đang kết nối — kiểm tra lại tên hoặc '
          'khởi động server đó. Bạn vẫn có thể lưu.',
  'Optional — shown instead of the target\'s description on rename':
      'Tuỳ chọn — hiển thị thay cho mô tả của công cụ đích khi đổi tên',

  // ── Marketplace ───────────────────────────────────────────────────────────
  'Search skills, subagents, MCP servers…': 'Tìm skill, subagent, MCP server…',
  'Sources ({n})': 'Nguồn ({n})',
  'Add source': 'Thêm nguồn',
  'No marketplace sources — add one to browse':
      'Chưa có nguồn Marketplace — thêm một nguồn để duyệt',
  'Catalog is empty': 'Danh mục trống',
  'A hub catalog only lists plugins. Apps, skills and workflows are installed '
          'by name from the same hub.':
      'Danh mục hub chỉ liệt kê plugin. App, skill và workflow được cài theo '
          'tên từ chính hub đó.',
  'Install by name, e.g. senclaw/clock':
      'Cài theo tên, ví dụ senclaw/clock',
  'Install': 'Cài',
  'Nothing matches your filter': 'Không có gì khớp bộ lọc',
  'Sync all sources': 'Đồng bộ tất cả nguồn',
  'Previous page': 'Trang trước',
  'Next page': 'Trang sau',
  '{from}–{to} of {total}   ·   page {page}/{pages}':
      '{from}–{to} trên {total}   ·   trang {page}/{pages}',
  'Author': 'Tác giả',
  'All authors': 'Tất cả tác giả',
  'License': 'Giấy phép',
  'Repository': 'Kho mã',
  'Marketplace sources': 'Nguồn Marketplace',
  'No sources yet': 'Chưa có nguồn nào',
  'Add marketplace source': 'Thêm nguồn Marketplace',
  'Hub store (marketplace.json)': 'Kho Hub (marketplace.json)',
  'Git repository': 'Kho Git',
  'Local directory': 'Thư mục cục bộ',
  'Directory path': 'Đường dẫn thư mục',
  'Git URL': 'URL Git',
  'Hub URL': 'URL Hub',
  'by {author}': 'bởi {author}',
  '{n} skills': '{n} skill',
  '{n} subagents': '{n} subagent',
  'hooks': 'hook',
  'not installed': 'chưa cài',
  'Installed': 'Đã cài',
  'No item details in the catalog': 'Danh mục không có chi tiết mục nào',
  'enabled': 'đang bật',
  'sync error': 'lỗi đồng bộ',
  'Synced: {at}': 'Đồng bộ: {at}',
  'Sync': 'Đồng bộ',
  'Enable all plugins': 'Bật tất cả plugin',
  'Disable all plugins': 'Tắt tất cả plugin',
  'Remove source': 'Xoá nguồn',

  // ── Hooks ─────────────────────────────────────────────────────────────────
  'Add hook': 'Thêm hook',
  'Edit hook': 'Sửa hook',
  'No hooks configured': 'Chưa cấu hình hook nào',
  'Event': 'Sự kiện',
  'Matcher': 'Điều kiện khớp',
  'e.g. Bash — empty = all tools': 'vd. Bash — để trống = mọi công cụ',
  'e.g. Review this tool call for security issues':
      'vd. Rà soát lời gọi công cụ này xem có vấn đề bảo mật không',
  'ADVANCED': 'NÂNG CAO',
  'Fired when user submits a prompt': 'Kích hoạt khi người dùng gửi một prompt',
  'Before any tool is called': 'Trước khi gọi bất kỳ công cụ nào',
  'After any tool completes': 'Sau khi một công cụ chạy xong',
  'When permission is requested': 'Khi có yêu cầu cấp quyền',
  'When the agent stops': 'Khi agent dừng',
  'When a session begins': 'Khi một phiên bắt đầu',
  'When a session ends': 'Khi một phiên kết thúc',
  'Before context compaction': 'Trước khi nén ngữ cảnh',
  'After context compaction': 'Sau khi nén ngữ cảnh',
  'On agent error': 'Khi agent gặp lỗi',

  // ── ClawHub install dialog ────────────────────────────────────────────────
  'Search skills on clawhub.ai…': 'Tìm skill trên clawhub.ai…',
  'Type a keyword to search ClawHub': 'Nhập từ khoá để tìm trên ClawHub',
  'Installed {slug}': 'Đã cài {slug}',
  'Install failed: {e}': 'Cài đặt thất bại: {e}',

  // ── Kits (kits_panel.dart / kit_params_form.dart) ─────────────────────────
  'This daemon does not serve /api/kits yet — rebuild and restart the daemon.':
      'Daemon này chưa phục vụ /api/kits — cần build lại và khởi động daemon mới.',
  'Skill': 'Skill',
  'Hook': 'Hook',
  'Scheduled job': 'Lịch chạy',
  'Space App': 'Space App',
  'created': 'đã tạo',
  'The kit created this item.': 'Kit tạo mới mục này.',
  'already there': 'đã có sẵn',
  'That name already existed — the daemon left the original alone and did not overwrite it. Removing the kit will not touch it.':
      'Tên này đã tồn tại — daemon giữ nguyên bản cũ, không ghi đè. Gỡ kit cũng không đụng tới nó.',
  'The daemon parses this but does not install it — that subsystem has its own consent flow.':
      'Daemon đọc được nhưng không tự cài loại này — nó có luồng đồng ý riêng.',
  'This item failed; the remaining items were still processed.':
      'Mục này lỗi; các mục còn lại vẫn được xử lý tiếp.',
  'removed': 'đã gỡ',
  'Deleted from disk / database.': 'Đã xoá khỏi ổ đĩa / cơ sở dữ liệu.',
  'already gone': 'không còn',
  'You had already deleted it by hand. Not an error.':
      'Bạn đã tự xoá trước đó. Không phải lỗi.',
  'Invalid JSON: {e}': 'JSON không hợp lệ: {e}',
  'Kit installed': 'Đã cài kit',
  'Kit installed with failures — see the report below':
      'Kit cài xong nhưng có mục lỗi — xem báo cáo bên dưới',
  'Removed kit "{name}"': 'Đã gỡ kit "{name}"',
  'Removed with failures — the receipt was kept':
      'Gỡ xong nhưng có mục lỗi — sổ biên nhận được giữ lại',
  'Remove failed: {e}': 'Không gỡ được: {e}',
  'the picker returned no file contents':
      'hộp thoại chọn tệp không trả về nội dung',
  'Cannot read file: {e}': 'Không đọc được tệp: {e}',
  'A kit is one JSON manifest that installs a whole setup in a single call: personas, skills, workflows, hooks and scheduled jobs. The daemon keeps the never-overwrite rule (a name that is taken is skipped) and a receipt, so removing a kit deletes only what it created.':
      'Một kit là một manifest JSON cài trọn bộ trong một lần gọi: persona, skill, workflow, hook và lịch chạy. Daemon giữ luật không ghi đè (tên đã có thì bỏ qua) và một sổ biên nhận, nên gỡ kit chỉ xoá đúng thứ nó tạo ra.',
  'Installed kits': 'Kit đã cài',
  'No kits installed — paste a manifest below to start.':
      'Chưa cài kit nào — dán một manifest bên dưới để bắt đầu.',
  'Install a kit from a manifest': 'Cài kit từ manifest',
  'Choose .json file…': 'Chọn tệp .json…',
  'Install kit': 'Cài kit',
  'Still missing: {names}': 'Còn thiếu: {names}',
  'Items whose name is taken are left untouched.':
      'Mục trùng tên sẽ được giữ nguyên, không ghi đè.',
  'Installed with:': 'Cài với:',
  'nothing created (every item already existed)':
      'không tạo gì (mọi mục đều đã có sẵn)',
  'What it created': 'Đã tạo những gì',
  'Remove kit "{name}"?': 'Gỡ kit "{name}"?',
  'Only the items this kit created itself are deleted; anything you made yourself is left alone.':
      'Chỉ những mục kit này tự tạo mới bị xoá; thứ bạn tự tạo được giữ nguyên.',
  'Nothing in this manifest can be installed.':
      'Manifest này không có mục nào cài được.',
  'Manifest v1: only agents and jobs are read — the other lists are ignored.':
      'Manifest v1: chỉ đọc agents và jobs — các danh sách khác bị bỏ qua.',
  '{n} MCP server / Space App entries will NOT be installed — they have their own consent flow (MCP Servers / Marketplace).':
      '{n} mục MCP server / Space App sẽ KHÔNG được cài — chúng có luồng đồng ý riêng (MCP Servers / Marketplace).',
  'Removal result for "{id}"': 'Kết quả gỡ "{id}"',
  'Install result for "{id}" (v{v})': 'Kết quả cài "{id}" (v{v})',
  'Install parameters': 'Tham số cài đặt',
  'This kit asks for {n} value(s) before installing. They are substituted into {{param.<key>}} in the manifest.':
      'Kit này hỏi {n} thông tin trước khi cài. Giá trị được thay vào chỗ {{param.<key>}} trong manifest.',
  'required': 'bắt buộc',
  'secret': 'bí mật',
  'Choose a folder for "{name}"': 'Chọn thư mục cho "{name}"',
  'Kits': 'Kit',

  // ── Kits: hộp thoại cài + marketplace ─────────────────────────────────────
  'Install result': 'Kết quả cài',
  'Marketplace': 'Marketplace',
  'installed': 'đã cài',
  'Reinstall': 'Cài lại',
  'Preview': 'Xem trước',
  'Close': 'Đóng',
  'Choose .zip or .json file…': 'Chọn tệp .zip hoặc .json…',
  'or paste a manifest': 'hoặc dán manifest',
  '.zip bundle': 'gói .zip',
  'JSON manifest': 'manifest JSON',
  'daemon does not install': 'daemon không cài',
  'Skills in the bundle': 'Skill trong gói',
  'Workflows in the bundle': 'Workflow trong gói',
  'The kit needs you to fill in': 'Kit cần bạn điền',
  '.zip — the whole kit: kit.json declares it, with skills/, workflows/ and apps/ carrying the actual files. .json — the manifest only, which cannot carry an app.':
      '.zip — kit đầy đủ: kit.json khai báo, kèm thư mục skills/, workflows/, apps/ chứa tệp cài đặt thật. .json — chỉ manifest, không mang theo được app.',
  'Choose a .zip / .json file, or paste a manifest, to see what the kit contains':
      'Chọn tệp .zip / .json, hoặc dán manifest, để xem kit chứa gì',
  'Install again, ignoring the security warning':
      'Cài lại, bỏ qua cảnh báo bảo mật',
  'This kit is already installed (v{v})': 'Kit này đã cài (v{v})',
  'Installing again will not overwrite: anything with a matching name is left as it is and reported as “already there”.':
      'Cài lại sẽ không ghi đè: mục nào trùng tên sẽ được giữ nguyên và báo là “đã có sẵn”.',
  'This kit installs {n} Space App(s) on your machine':
      'Kit này cài {n} Space App vào máy',
  'Each app goes through the pre-install security scan; a blocked app is reported on its own and the rest of the kit still installs.':
      'Mỗi app đều qua bước quét bảo mật trước khi cài; app bị chặn sẽ báo lỗi riêng và phần còn lại của kit vẫn cài bình thường.',
  'The catalog entry declares no file to download.':
      'Mục trong catalog không khai báo tệp để tải.',
  'No kits installed — use “Install kit” to add one.':
      'Chưa cài kit nào — bấm “Cài kit” để thêm.',
  'No marketplace source offers a kit yet. Kits are declared in the kits[] array of a source’s marketplace.json — add a source on the Marketplace page.':
      'Chưa có marketplace source nào chào kit. Kit được khai báo trong mảng kits[] của marketplace.json ở source — thêm source tại trang Marketplace.',
  'A kit installs a whole setup in one go: personas, skills, workflows, hooks and scheduled jobs. A .json carries only the declaration; a .zip also carries the real files of its skills and workflows, and even a Space App. The daemon keeps the never-overwrite rule (a name that is taken is skipped) and a receipt, so removing a kit deletes only what it created.':
      'Một kit cài trọn bộ trong một lần: persona, skill, workflow, hook và lịch chạy. Dạng .json chỉ mang được phần khai báo; dạng .zip mang thêm tệp thật của skill/workflow và cả Space App. Daemon giữ luật không ghi đè (trùng tên thì bỏ qua) và một sổ biên nhận, nên gỡ kit chỉ xoá đúng thứ nó đã tạo.',

  // ── Kits: danh sách từng mục trong hộp thoại ──────────────────────────────
  'Will install': 'Sẽ cài',
  'The kit declares nothing to install.': 'Kit không khai báo mục nào để cài.',
  'installed paused': 'cài ở trạng thái tạm dừng',
  'matches': 'khớp',
  'if': 'nếu',
  'can block the agent loop': 'có thể chặn vòng lặp agent',
  'the daemon does not install this — use the MCP servers page':
      'daemon không cài — cài qua trang MCP servers',
  'file from the .zip bundle': 'tệp từ gói .zip',
};
