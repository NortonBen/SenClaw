/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
const Map<String, String> viSettingsScreen = {
  // Appearance
  'System follows your OS appearance setting and switches automatically.':
      'Hệ thống bám theo cài đặt giao diện của hệ điều hành và tự chuyển đổi.',
  'Applies everywhere immediately. System follows your OS language (Vietnamese → Tiếng Việt, otherwise English).':
      'Áp dụng ngay trên toàn ứng dụng. Hệ thống bám theo ngôn ngữ hệ điều hành (tiếng Việt → Tiếng Việt, còn lại → English).',

  // General → Network access (daemon bind host)
  'Network access': 'Truy cập mạng',
  'Who can reach this daemon. Private keeps it on this machine; '
          'Public lets phones and other computers on your network use it.':
      'Ai được kết nối tới daemon này. Riêng tư thì chỉ máy này dùng được; '
          'Công khai thì điện thoại và máy khác trong mạng cũng dùng được.',
  // ('Private' → 'Riêng tư' is already keyed below, under Telegram chat types.)
  'Public': 'Công khai',
  'Anyone on your network can reach SenClaw. The daemon '
          'requires the API token from every non-local device — it is '
          'in ~/.senclaw/api_token on this machine.':
      'Mọi thiết bị trong mạng đều tới được SenClaw. Daemon sẽ bắt buộc '
          'token API với mọi thiết bị không phải máy này — token nằm ở '
          '~/.senclaw/api_token trên máy này.',
  'The running daemon still uses the previous setting. Restart it to apply.':
      'Daemon đang chạy vẫn dùng thiết lập cũ. Khởi động lại để áp dụng.',
  'This daemon was started outside the app, so it keeps '
          'its own setting until it is restarted here.':
      'Daemon này được khởi động từ bên ngoài ứng dụng nên vẫn giữ thiết lập '
          'riêng của nó cho tới khi được khởi động lại ở đây.',
  'Restart daemon': 'Khởi động lại daemon',
  'Daemon restarted with the new setting.':
      'Đã khởi động lại daemon với thiết lập mới.',

  // Sidebar sections
  'Channels': 'Kênh',
  'Profiles': 'Hồ sơ',
  'Tool Rules': 'Luật công cụ',
  'LLM Models': 'Model LLM',
  'Provider Sign-in': 'Đăng nhập nhà cung cấp',
  'Local Models': 'Model cục bộ',
  'Embedding': 'Embedding',
  'Knowledge': 'Tri thức',
  'Speech-to-Text': 'Giọng nói thành chữ',
  'Text-to-Speech': 'Chữ thành giọng nói',
  'OCR': 'OCR',
  'Updates': 'Cập nhật',
  'Speech-to-Text (Whisper)': 'Giọng nói thành chữ (Whisper)',

  // Updates
  'Development build': 'Bản build dev',
  'Version {v}': 'Phiên bản {v}',
  'Installed at {path}': 'Cài tại {path}',
  'The web console is served by the daemon and updates with it. '
          'Update the daemon on the host machine: senclaw update':
      'Bảng điều khiển web do daemon phục vụ và cập nhật cùng daemon. '
          'Hãy cập nhật daemon trên máy chủ: senclaw update',
  'This build has no release version, so it cannot be updated in place. '
          'Rebuild from source, or install a release with: senclaw install desktop':
      'Bản build này không có số phiên bản phát hành nên không thể cập nhật tại chỗ. '
          'Hãy build lại từ mã nguồn, hoặc cài bản phát hành bằng: senclaw install desktop',
  'Check for updates automatically': 'Tự động kiểm tra cập nhật',
  'Once a day, in the background. Nothing installs without your say-so.':
      'Mỗi ngày một lần, chạy nền. Không tự cài gì khi bạn chưa đồng ý.',
  'At every start and once a day after that, in the background. A new version '
          'pops up a notice; nothing installs without your say-so.':
      'Mỗi lần khởi động và sau đó mỗi ngày một lần, chạy nền. Có bản mới sẽ '
          'hiện thông báo; không tự cài gì khi bạn chưa đồng ý.',
  'You asked not to be notified about {v}.':
      'Bạn đã chọn không nhận thông báo về {v}.',
  'Reminders about {v} are paused until {when}.':
      'Tạm ngưng nhắc về {v} — sẽ nhắc lại {when}.',
  'Notify me again': 'Thông báo lại',
  'later': 'sau',
  'the next check': 'lần kiểm tra tới',
  'in {n}m': 'sau {n} phút',
  'in {n}h': 'sau {n} giờ',
  'in {n}d': 'sau {n} ngày',
  "What's new in {v}": 'Có gì mới ở {v}',
  'You are on the latest version.': 'Bạn đang dùng phiên bản mới nhất.',
  'Version {v} is available.': 'Đã có phiên bản {v}.',
  'Downloading {v}…': 'Đang tải {v}…',
  'Version {v} is ready to install.': 'Phiên bản {v} đã sẵn sàng để cài.',
  'Installing — SenClaw will restart…': 'Đang cài — SenClaw sẽ khởi động lại…',
  'Something went wrong.': 'Đã có lỗi xảy ra.',
  'Not checked yet.': 'Chưa kiểm tra lần nào.',
  'Last checked {ago}.': 'Kiểm tra lần cuối {ago}.',
  '{n}m ago': '{n} phút trước',
  '{n}h ago': '{n} giờ trước',
  '{n}d ago': '{n} ngày trước',
  'Install & Restart': 'Cài & khởi động lại',
  'Check now': 'Kiểm tra ngay',
  'Install update?': 'Cài bản cập nhật?',
  'SenClaw will quit, install the update, and reopen. '
          'Running agents and background tasks will be stopped.':
      'SenClaw sẽ thoát, cài bản cập nhật rồi mở lại. '
          'Các agent đang chạy và tác vụ nền sẽ bị dừng.',

  // General — connection
  'Connection': 'Kết nối',
  'API access token — only needed when the daemon is exposed beyond '
          'localhost (SENCLAW_UI_BIND_HOST=0.0.0.0). The daemon machine keeps '
          'it in ~/.senclaw/api_token.':
      'Token truy cập API — chỉ cần khi daemon được mở ra ngoài localhost '
          '(SENCLAW_UI_BIND_HOST=0.0.0.0). Máy chạy daemon giữ token trong '
          '~/.senclaw/api_token.',
  'Empty for the local daemon': 'Để trống nếu dùng daemon cục bộ',
  'API token saved — applies to new requests.':
      'Đã lưu token API — áp dụng cho các yêu cầu mới.',

  // General — permissions & behavior
  'Permissions': 'Quyền',
  'Skip all-agent permissions': 'Bỏ qua hỏi quyền cho mọi agent',
  'Auto-accept tool calls for every agent.':
      'Tự động chấp nhận lệnh gọi công cụ cho mọi agent.',
  'Skip main-agent permissions': 'Bỏ qua hỏi quyền cho agent chính',
  'Auto-accept tool calls for the main agent only.':
      'Chỉ tự động chấp nhận lệnh gọi công cụ cho agent chính.',
  'Agent behavior': 'Hành vi agent',
  'After-process hook': 'Hook sau xử lý',
  'Run the post-processing step after each turn.':
      'Chạy bước hậu xử lý sau mỗi lượt.',
  'Pre-cognitive recall': 'Gợi nhớ trước khi xử lý',
  'Inject relevant memories before processing.':
      'Chèn các bộ nhớ liên quan trước khi xử lý.',
  'Memory recall': 'Gợi nhớ bộ nhớ',
  'Consolidate dropped history into memory files and '
          'inject relevant saved memories into each request.':
      'Cô đọng phần lịch sử bị cắt vào các tệp bộ nhớ và chèn những bộ nhớ '
          'liên quan đã lưu vào mỗi yêu cầu.',
  'Pre-trigger skill': 'Skill kích hoạt trước',
  'Evaluate trigger skills before the main turn.':
      'Đánh giá các skill kích hoạt trước lượt chính.',
  'Autonomous tasks': 'Tác vụ tự động',
  'Auto-run Kanban tasks (dispatcher)': 'Tự chạy tác vụ Kanban (dispatcher)',
  'Automatically assign a worker agent to each task in a '
          'Kanban board\'s Ready column, run it, and complete or block '
          'it. Agents act unattended — leave OFF unless you want that.':
      'Tự gán một agent thợ cho mỗi tác vụ ở cột Ready của bảng Kanban, chạy '
          'rồi hoàn thành hoặc chặn tác vụ đó. Agent hành động không giám sát — '
          'hãy để TẮT trừ khi bạn thực sự muốn vậy.',

  // General — screen capture
  'Screen capture': 'Chụp màn hình',
  'Capture shortcut': 'Phím tắt chụp',
  'Press this anywhere to grab a region — the same selector as '
          'macOS Cmd+Shift+4. Needs at least one modifier.':
      'Bấm ở bất cứ đâu để chọn vùng chụp — cùng bộ chọn với Cmd+Shift+4 của '
          'macOS. Cần ít nhất một phím bổ trợ.',
  'Press the shortcut…': 'Bấm tổ hợp…',
  'Reset to default (⌃ ⇧ 4)': 'Về mặc định (⌃ ⇧ 4)',

  // Channels
  'Add channel': 'Thêm kênh',
  'No channels connected.': 'Chưa có kênh nào được kết nối.',
  'Edit channel': 'Sửa kênh',
  'Rename or reconfigure this channel': 'Đổi tên hoặc cấu hình lại kênh này',
  'Connect a messaging platform to your agent':
      'Kết nối một nền tảng nhắn tin với agent của bạn',
  'Connector': 'Bộ kết nối',
  'PLATFORM': 'NỀN TẢNG',
  'NAME': 'TÊN',
  'My Telegram bot': 'Bot Telegram của tôi',
  'BOT TOKEN': 'BOT TOKEN',
  'Leave empty to use the .env default bot':
      'Để trống để dùng bot mặc định trong .env',
  'CHAT TYPE': 'LOẠI CHAT',
  'Group': 'Nhóm',
  'Private': 'Riêng tư',
  'HUB URL': 'HUB URL',
  'Registers with the hub, then shows a QR code for the '
          'Senclaw mobile app to scan.':
      'Đăng ký với hub, sau đó hiện mã QR để app Senclaw trên điện thoại quét.',
  'Show pairing QR': 'Hiện mã QR ghép nối',
  'APP ID': 'APP ID',
  'APP SECRET': 'APP SECRET',
  'Sandbox': 'Sandbox',
  'Use the QQ sandbox environment': 'Dùng môi trường sandbox của QQ',
  'Require @mention to trigger': 'Phải @nhắc tên mới trả lời',
  'Only reply when the bot is explicitly mentioned':
      'Chỉ trả lời khi bot được nhắc tên rõ ràng',
  'Registering…': 'Đang đăng ký…',
  'Register & Get QR': 'Đăng ký & lấy QR',
  'Pairing failed: {e}': 'Ghép nối thất bại: {e}',
  'Scan to connect': 'Quét để kết nối',
  'Open the Senclaw mobile app and scan this code to pair.':
      'Mở app Senclaw trên điện thoại và quét mã này để ghép nối.',
  'Pairing link copied': 'Đã sao chép liên kết ghép nối',
  'Copy pairing link': 'Sao chép liên kết ghép nối',

  // Profiles (agents)
  'New profile': 'Hồ sơ mới',
  'No agent profiles.': 'Chưa có hồ sơ agent nào.',
  'folder: {folder}': 'thư mục: {folder}',
  '{n} channel': '{n} kênh',
  '{n} channels': '{n} kênh',
  'Edit agent · {folder}': 'Sửa agent · {folder}',
  'My assistant': 'Trợ lý của tôi',
  'Global default': 'Mặc định toàn cục',
  '{id} (current)': '{id} (hiện tại)',
  'BOUND CHANNELS': 'KÊNH ĐÃ LIÊN KẾT',
  'No channels — add one in the Channels tab.':
      'Chưa có kênh nào — thêm ở mục Kênh.',
  'MEMORY.md (agent long-term memory)…':
      'MEMORY.md (bộ nhớ dài hạn của agent)…',
  'Core prompt (SOUL.md)…': 'Prompt cốt lõi (SOUL.md)…',
  'Already bound to another profile': 'Đã liên kết với hồ sơ khác',

  // Tool rules
  'Dangerously accept all': 'Chấp nhận tất cả (nguy hiểm)',
  'Auto-accept every tool call without prompting.':
      'Tự động chấp nhận mọi lệnh gọi công cụ mà không hỏi.',
  'Auto-accept rules': 'Luật tự động chấp nhận',
  'Add rule': 'Thêm luật',
  'No rules. Tool calls follow per-agent defaults.':
      'Chưa có luật nào. Lệnh gọi công cụ theo mặc định của từng agent.',
  'Add tool rule': 'Thêm luật công cụ',
  'ACTION': 'HÀNH ĐỘNG',
  'Auto accept': 'Tự chấp nhận',
  'Accept + remember': 'Chấp nhận + ghi nhớ',
  'Always ask': 'Luôn hỏi',
  'Auto deny': 'Tự từ chối',
  'MATCH': 'ĐIỀU KIỆN KHỚP',
  'Bash glob': 'Bash glob',
  'Bash regex': 'Bash regex',
  'Tool name': 'Tên công cụ',
  'Skill name': 'Tên skill',
  'MCP glob': 'MCP glob',
  'MCP server': 'MCP server',
  'Tool category': 'Nhóm công cụ',
  'All tools': 'Mọi công cụ',
  'PATTERN': 'MẪU',
  'TOOL NAME': 'TÊN CÔNG CỤ',
  'SKILL NAME': 'TÊN SKILL',
  'MCP SERVER': 'MCP SERVER',
  'TOOL (optional — blank = all)': 'CÔNG CỤ (tuỳ chọn — để trống = tất cả)',
  'CATEGORY': 'NHÓM',
  'DESCRIPTION (optional)': 'MÔ TẢ (tuỳ chọn)',
  'Why this rule exists': 'Lý do có luật này',

  // LLM models
  'Extended thinking': 'Suy nghĩ mở rộng',
  'Let the model reason before replying':
      'Cho model suy luận trước khi trả lời',
  'Add endpoint': 'Thêm endpoint',
  'Main': 'Chính',
  'Cognitive': 'Tri thức',
  'Quick': 'Nhanh',
  'Set role': 'Đặt vai trò',
  'Set as Main': 'Đặt làm model chính',
  'Set as Cognitive': 'Đặt làm model tri thức',
  'Set as Quick': 'Đặt làm model nhanh',
  'Set as…': 'Đặt làm…',
  'Delete endpoint?': 'Xoá endpoint?',
  '"{label}" will be removed. Chats '
          'using it fall back to the active '
          'default model.':
      '"{label}" sẽ bị gỡ. Các cuộc trò chuyện đang dùng nó sẽ quay về model '
          'mặc định đang bật.',
  'Edit LLM endpoint': 'Sửa endpoint LLM',
  'Add LLM endpoint': 'Thêm endpoint LLM',
  'Provider': 'Nhà cung cấp',
  'Custom LLM endpoint': 'Endpoint LLM tuỳ chỉnh',
  'Base URL': 'Base URL',
  'API key': 'API key',
  'Stored key — edit to replace': 'Key đã lưu — sửa để thay thế',
  'Your Anthropic API key': 'API key Anthropic của bạn',
  'Your OpenAI API key': 'API key OpenAI của bạn',
  'Your Moonshot API key': 'API key Moonshot của bạn',
  'Your MiniMax API key': 'API key MiniMax của bạn',
  'Your DeepSeek API key': 'API key DeepSeek của bạn',
  'Your Zhipu API key': 'API key Zhipu của bạn',
  'Your OpenRouter API key': 'API key OpenRouter của bạn',
  'Your Alibaba Cloud API key': 'API key Alibaba Cloud của bạn',
  'Your API key': 'API key của bạn',
  'API type (compatibility)': 'Loại API (tương thích)',
  'OpenAI-compatible': 'Tương thích OpenAI',
  'Anthropic-compatible': 'Tương thích Anthropic',
  'Model name': 'Tên model',
  'Fetch': 'Lấy danh sách',
  'Available models': 'Model khả dụng',
  'Vision (image input)': 'Vision (nhận ảnh)',
  'Auto (infer from model name)': 'Tự động (suy ra từ tên model)',
  'Supported': 'Có hỗ trợ',
  'Not supported': 'Không hỗ trợ',
  'Test': 'Kiểm tra',
  '✓ Loaded {n} model(s)': '✓ Đã nạp {n} model',
  'No models': 'Không có model nào',
  '✓ Connection OK': '✓ Kết nối OK',
  '✗ Model name is required': '✗ Bắt buộc nhập tên model',

  // Local models
  'Platform: {platform} — local MLX inference only runs '
          'on macOS (Apple Silicon).':
      'Nền tảng: {platform} — suy luận MLX cục bộ chỉ chạy trên macOS '
          '(Apple Silicon).',
  'Downloading': 'Đang tải về',
  'Downloading {pct}%': 'Đang tải về {pct}%',
  'Use as LLM': 'Dùng làm LLM',
  'Load': 'Nạp',
  'Unload': 'Gỡ khỏi bộ nhớ',
  'Already in LLM Models: {label}': 'Đã có trong Model LLM: {label}',
  'Added as LLM profile and set active: {label}':
      'Đã thêm làm hồ sơ LLM và đặt đang dùng: {label}',
  'Added as LLM profile: {label}': 'Đã thêm làm hồ sơ LLM: {label}',
  'Failed to add as LLM: {e}': 'Thêm làm LLM thất bại: {e}',
  'Removed {label}': 'Đã gỡ {label}',
  'Delete failed: {e}': 'Xoá thất bại: {e}',

  // Local inference settings
  'Inference settings': 'Cài đặt suy luận',
  'Inference backend': 'Backend suy luận',
  'Engine for Load / Use as LLM. MLX is Apple-Silicon-only & fastest.':
      'Engine cho Nạp / Dùng làm LLM. MLX chỉ chạy trên Apple Silicon và nhanh nhất.',
  'Auto': 'Tự động',
  'MLX native (~60–100 tok/s)': 'MLX gốc (~60–100 tok/s)',
  'Candle (~12 tok/s)': 'Candle (~12 tok/s)',
  'Idle unload (secs)': 'Gỡ khi rảnh (giây)',
  '0 = never; ≥60 to free RAM after inactivity. Default 60.':
      '0 = không bao giờ; ≥60 để giải phóng RAM sau khi không dùng. Mặc định 60.',
  'KV TurboQuant bits': 'Số bit KV TurboQuant',
  'Quantize KV cache to save RAM on long generation.':
      'Lượng tử hoá KV cache để tiết kiệm RAM khi sinh văn bản dài.',
  'Auto (4-bit for 4-bit models)': 'Tự động (4-bit cho model 4-bit)',
  'TQ4 — 4-bit total': 'TQ4 — tổng 4-bit',
  'TQ3 — 3-bit total': 'TQ3 — tổng 3-bit',
  'Off — FP16': 'Tắt — FP16',
  'MLX packed KV (Metal)': 'KV nén MLX (Metal)',
  'MLX-native GPU KV quantization. Reload the model after changing.':
      'Lượng tử hoá KV trên GPU theo chuẩn MLX. Hãy nạp lại model sau khi đổi.',
  '4-bit packed': 'Nén 4-bit',
  '8-bit packed': 'Nén 8-bit',
  'TQ activate after (tokens)': 'Bật TQ sau (token)',
  'Cached tokens before TurboQuant kicks in. Default 16384.':
      'Số token đã cache trước khi TurboQuant bật. Mặc định 16384.',
  'Max prompt tokens': 'Token prompt tối đa',
  'Hard cap on prompt length (512–262144). Default 128000.':
      'Giới hạn cứng độ dài prompt (512–262144). Mặc định 128000.',
  'Max new tokens': 'Token sinh mới tối đa',
  'Max tokens generated per request (1–8192). Default 8192.':
      'Số token sinh tối đa mỗi yêu cầu (1–8192). Mặc định 8192.',
  'Max KV tokens': 'Token KV tối đa',
  'KV-cache sliding window (128–262144). Default 16384.':
      'Cửa sổ trượt của KV-cache (128–262144). Mặc định 16384.',
  'Temperature (MLX)': 'Temperature (MLX)',
  '0 = greedy. Empty = server default (Gemma ≈0.65).':
      '0 = greedy. Để trống = mặc định của server (Gemma ≈0.65).',
  'Repetition penalty (MLX)': 'Repetition penalty (MLX)',
  '1 = off. Empty = server default (Gemma ≈1.15).':
      '1 = tắt. Để trống = mặc định của server (Gemma ≈1.15).',
  'Thinking mode (Qwen3)': 'Chế độ suy nghĩ (Qwen3)',
  'Chain-of-thought before answering. Off is faster.':
      'Suy luận từng bước trước khi trả lời. Tắt sẽ nhanh hơn.',
  'Release cache after session (MLX)': 'Giải phóng cache sau phiên (MLX)',
  'Drop per-session KV/prefix cache when a chat ends. Weights stay.':
      'Bỏ KV/prefix cache của phiên khi kết thúc trò chuyện. Trọng số vẫn giữ.',
  'Inference settings saved': 'Đã lưu cài đặt suy luận',
  'Unloaded all models': 'Đã gỡ mọi model khỏi bộ nhớ',
  'Unload all now': 'Gỡ tất cả ngay',

  // Hugging Face add-model card
  'Add model from Hugging Face': 'Thêm model từ Hugging Face',
  'org/repo or URL (e.g. facebook/mms-tts-vie)':
      'org/repo hoặc URL (vd: facebook/mms-tts-vie)',
  'Check': 'Kiểm tra',
  'Check failed: {e}': 'Kiểm tra thất bại: {e}',
  'Download failed: {e}': 'Tải về thất bại: {e}',
  'Download started — progress shows in the list below':
      'Đã bắt đầu tải — tiến trình hiện ở danh sách bên dưới',
  'Try anyway': 'Cứ thử',

  // Embedding
  'None (disabled)': 'Không (tắt)',
  'Local (on-device)': 'Cục bộ (trên máy)',
  'Model path (optional)': 'Đường dẫn model (tuỳ chọn)',
  'Dimensions (optional)': 'Số chiều (tuỳ chọn)',
  'Embedding config saved': 'Đã lưu cấu hình embedding',
  'Save failed: {e}': 'Lưu thất bại: {e}',
  'Local models': 'Model cục bộ',
  'Installed': 'Đã cài',
  'Downloading model…': 'Đang tải model…',

  // Knowledge (cognitive)
  'Knowledge (Cognitive)': 'Tri thức (Cognitive)',
  'Enable cognitive layer': 'Bật lớp tri thức',
  'Graph + Hebbian recall across sessions.':
      'Đồ thị + gợi nhớ Hebbian xuyên suốt các phiên.',
  'Auto-reflect on every user message': 'Tự chiêm nghiệm mỗi tin nhắn người dùng',
  'Cognify each incoming message automatically.':
      'Tự động trích tri thức từ mỗi tin nhắn đến.',
  'Extraction': 'Trích xuất',
  'Max concurrent extractions': 'Số lượt trích xuất song song tối đa',
  'Semaphore size for in-flight cognify calls. Keep low on '
          'local models.':
      'Kích thước semaphore cho các lượt cognify đang chạy. Nên để thấp khi '
          'dùng model cục bộ.',
  'Max LLM output chars': 'Số ký tự output LLM tối đa',
  'Hard cap on cognify-LLM output; streams abort past this.':
      'Giới hạn cứng output của LLM cognify; vượt mức này sẽ ngắt luồng.',
  'Reflection': 'Chiêm nghiệm',
  'Min chars': 'Số ký tự tối thiểu',
  'Skip reflection for messages shorter than this.':
      'Bỏ qua chiêm nghiệm với tin nhắn ngắn hơn mức này.',
  'Max chars': 'Số ký tự tối đa',
  'Window size: buffered turns flush to one extraction '
          'call when they reach this length.':
      'Kích thước cửa sổ: các lượt đang gom sẽ dồn thành một lượt trích xuất '
          'khi đạt độ dài này.',
  'Cooldown (ms)': 'Thời gian chờ (ms)',
  'Minimum gap between window flushes per agent.':
      'Khoảng cách tối thiểu giữa hai lần dồn cửa sổ của mỗi agent.',
  'Window idle (ms)': 'Cửa sổ rảnh (ms)',
  'Flush the conversation window after this much chat '
          'silence. 0 = flush per message.':
      'Dồn cửa sổ hội thoại sau khoảng lặng này. 0 = dồn theo từng tin nhắn.',
  'Maintenance': 'Bảo trì',
  'Sweep interval (hours)': 'Chu kỳ quét dọn (giờ)',
  'How often the background decay/prune sweep runs.':
      'Tần suất chạy đợt quét suy giảm/cắt tỉa chạy nền.',
  'Cognitive config saved': 'Đã lưu cấu hình tri thức',
  'Maintenance started': 'Đã bắt đầu bảo trì',
  'Run maintenance': 'Chạy bảo trì',

  // Space Apps
  'Register Space App': 'Đăng ký Space App',
  'Manifest URL': 'URL manifest',
  'Register': 'Đăng ký',
  'Space App registered': 'Đã đăng ký Space App',
  'Register failed: {e}': 'Đăng ký thất bại: {e}',
  'File picker error: {e}': 'Lỗi hộp thoại chọn tệp: {e}',
  'Space App installed': 'Đã cài Space App',
  'Install failed: {e}': 'Cài đặt thất bại: {e}',
  '{n} app has an update': '{n} app có bản mới',
  '{n} apps have updates': '{n} app có bản mới',
  'All apps are up to date': 'Mọi app đã ở phiên bản mới nhất',
  'Updating…': 'Đang cập nhật…',
  'Updated {id} → {v}': 'Đã cập nhật {id} → {v}',
  '{id} is already up to date': '{id} đã ở bản mới nhất',
  'Update failed: {e}': 'Cập nhật thất bại: {e}',
  'Install, register, and remove embedded Space Apps.':
      'Cài, đăng ký và gỡ các Space App nhúng.',
  'Install ZIP': 'Cài từ ZIP',
  'Register URL': 'Đăng ký bằng URL',
  'Check updates': 'Kiểm tra cập nhật',
  'No Space Apps installed': 'Chưa cài Space App nào',
  'Uninstall': 'Gỡ cài đặt',
  'Restart': 'Khởi động lại',
  'Restarting…': 'Đang khởi động lại…',
  'Restarted': 'Đã khởi động lại',
  'Restart failed: {e}': 'Khởi động lại thất bại: {e}',
  'Integration': 'Tích hợp',
  '(imported disabled — enable in Plugins → Alias)':
      '(nhập ở trạng thái tắt — bật trong Plugins → Alias)',
  'No MCP declared': 'Không khai báo MCP',
  'auto': 'tự động',
  'Copy all logs': 'Sao chép toàn bộ nhật ký',
  'Refresh logs': 'Làm mới nhật ký',
  '(no logs)': '(chưa có nhật ký)',
  'Log copied': 'Đã sao chép nhật ký',

  // Media models (whisper / tts / ocr)
  'Active model': 'Model đang dùng',
  '(default)': '(mặc định)',
  'Voice': 'Giọng đọc',
  'Speed': 'Tốc độ',
  'Test voice': 'Nghe thử giọng',
  'Test (pick image)': 'Thử (chọn ảnh)',
  'Stop & transcribe': 'Dừng & chuyển thành chữ',
  'Record & transcribe': 'Ghi âm & chuyển thành chữ',
  'Transcription': 'Kết quả nhận dạng',
  '(no speech recognized)': '(không nhận ra lời nói nào)',
  'Transcribe failed: {e}': 'Nhận dạng giọng nói thất bại: {e}',
  'Microphone permission denied': 'Không có quyền truy cập micro',
  'OCR result — {name}': 'Kết quả OCR — {name}',
  '(no text recognized)': '(không nhận ra chữ nào)',
  'OCR failed: {e}': 'OCR thất bại: {e}',
  'Fallback voice used: {voice}': 'Đã dùng giọng dự phòng: {voice}',
  'Spoke via {backend}': 'Đọc qua {backend}',
  'Test failed: {e}': 'Kiểm tra thất bại: {e}',
  'Speed must be 0.25–4.0': 'Tốc độ phải trong khoảng 0.25–4.0',
  'Saved': 'Đã lưu',
};
