/// Vietnamese strings for this area. English string = key. Filled by the
/// localization sweep; keep entries sorted roughly by screen order.
///
/// Area: Knowledge (cognitive graph + data list), Wiki, Diagnostics.
/// Keys already covered by `common.dart` (Cancel, Save, Delete, Edit, Create,
/// OK, Close, Add, Reload, Refresh, Download, History, Logs, No results) are
/// intentionally absent — this map is merged after `viCommon`, so repeating a
/// key here would override it everywhere in the app.
const Map<String, String> viCognitiveWiki = {
  // ── Knowledge — header ──────────────────────────────────────────────────
  'User info aggregated from chats · extended with '
      'uploaded documents · Recall researches it for detailed, '
      'grounded answers':
      'Thông tin về bạn được tổng hợp từ các cuộc trò chuyện · mở rộng bằng '
          'tài liệu bạn tải lên · Recall tra cứu để trả lời chi tiết, có dẫn '
          'chứng',
  'Knowledge': 'Tri thức',
  'nodes': 'nút',
  'edges': 'cạnh',
  'Graph': 'Đồ thị',
  'Data': 'Dữ liệu',
  '🌐 All knowledge': '🌐 Toàn bộ tri thức',
  'Add knowledge': 'Thêm tri thức',

  // ── Knowledge — maintenance menu ────────────────────────────────────────
  'Maintenance': 'Bảo trì',
  'Maintain': 'Bảo trì',
  'Re-extract pending': 'Trích xuất lại phần còn chờ',
  'Cleanup': 'Dọn dẹp',
  'Decay log': 'Nhật ký suy giảm',
  'Maintenance run': 'Đã chạy bảo trì',
  'Cleanup done': 'Đã dọn dẹp',
  'Backfill started': 'Đã bắt đầu bù dữ liệu',
  'removed {n} node(s)': 'đã xoá {n} nút',
  '{n} chunk(s) queued': 'đã xếp hàng {n} đoạn',
  'No runs yet': 'Chưa có lần chạy nào',
  'scanned {scanned} · pruned {pruned} · promoted {promoted}':
      'quét {scanned} · tỉa {pruned} · nâng {promoted}',

  // ── Knowledge — data list & node detail ─────────────────────────────────
  'Search knowledge…': 'Tìm trong tri thức…',
  'No nodes': 'Chưa có nút nào',
  'Select a node': 'Chọn một nút',
  'Re-extract': 'Trích xuất lại',
  'Forget': 'Quên',
  '(no content)': '(không có nội dung)',

  // ── Knowledge — add / upload dialog ─────────────────────────────────────
  'What to remember…': 'Cần nhớ điều gì…',
  'tags, comma, separated': 'thẻ, ngăn, bằng dấu phẩy',
  'or': 'hoặc',
  'Upload a file': 'Tải lên một tệp',
  '{name}: added to knowledge': '{name}: đã thêm vào tri thức',
  'Upload failed: {err}': 'Tải lên thất bại: {err}',

  // ── Knowledge — Recall dialog ───────────────────────────────────────────
  'Recall — grounded answer': 'Recall — câu trả lời có dẫn chứng',
  'Ask a question…': 'Đặt một câu hỏi…',
  'Hybrid (vec+FTS)': 'Kết hợp (vec+FTS)',
  'Keyword (FTS)': 'Từ khoá (FTS)',
  'Ask': 'Hỏi',
  'SOURCES ({n})': 'NGUỒN ({n})',

  // ── Knowledge — graph explorer ──────────────────────────────────────────
  'Graph error: {err}': 'Lỗi đồ thị: {err}',
  'Not enough connected nodes to graph': 'Không đủ nút liên kết để vẽ đồ thị',
  'No graph data yet': 'Chưa có dữ liệu đồ thị',
  '{n} nodes': '{n} nút',
  '{n} edges': '{n} cạnh',
  'Search nodes…': 'Tìm nút…',
  'Chunks': 'Đoạn',
  'Graph truncated': 'Đồ thị đã bị cắt bớt',
  'Drag node = move · Scroll = zoom · Tap node = focus':
      'Kéo nút = di chuyển · Cuộn = thu phóng · Chạm nút = chọn',
  'Open in data view': 'Mở trong chế độ Dữ liệu',
  '{n} connections': '{n} liên kết',
  'No connections': 'Không có liên kết',
  '+{n} more': '+{n} nữa',

  // ── Wiki — tree context menu ────────────────────────────────────────────
  'New file…': 'Tệp mới…',
  'New folder…': 'Thư mục mới…',
  'Upload file…': 'Tải tệp lên…',
  'Delete folder': 'Xoá thư mục',
  'New file': 'Tệp mới',
  'New folder': 'Thư mục mới',
  'name': 'tên',
  'Delete folder?': 'Xoá thư mục?',
  'Delete file?': 'Xoá tệp?',
  'Only empty folders can be removed.': 'Chỉ xoá được thư mục rỗng.',
  'This cannot be undone.': 'Không thể hoàn tác.',
  'Delete failed: {err}': 'Xoá thất bại: {err}',
  'Save {name}': 'Lưu {name}',
  'Saved {name}': 'Đã lưu {name}',

  // ── Wiki — sidebar ──────────────────────────────────────────────────────
  'New page': 'Trang mới',
  'Upload file': 'Tải tệp lên',
  'Folder path': 'Đường dẫn thư mục',
  '{n} pages': '{n} trang',
  '{n} folders': '{n} thư mục',
  '{n} tags': '{n} thẻ',
  'Search wiki…': 'Tìm trong Wiki…',
  'Select a document': 'Chọn một tài liệu',

  // ── Wiki — page view & dialogs ──────────────────────────────────────────
  'Markdown (with frontmatter)…': 'Markdown (kèm frontmatter)…',
  'New wiki page': 'Trang Wiki mới',
  'Path': 'Đường dẫn',
  'Content (Markdown)': 'Nội dung (Markdown)',
  'Path is required': 'Cần nhập đường dẫn',
  'History · {path}': 'Lịch sử · {path}',
  'No history': 'Chưa có lịch sử',

  // ── Diagnostics ─────────────────────────────────────────────────────────
  'Diagnostics': 'Chẩn đoán',
  'Daemon supervision & ports': 'Giám sát daemon và cổng',
  'Restart daemon': 'Khởi động lại daemon',
  'Running (supervised)': 'Đang chạy (có giám sát)',
  'Running (adopted)': 'Đang chạy (tiếp quản)',
  'External (web)': 'Bên ngoài (web)',
  'Starting…': 'Đang khởi động…',
  'Crashed': 'Đã sập',
  'Idle': 'Nghỉ',
  'since {t}': 'từ {t}',
  'free': 'trống',
  'Copy all': 'Sao chép tất cả',
  'Copied {n} log lines': 'Đã sao chép {n} dòng nhật ký',
  'No logs yet': 'Chưa có nhật ký',
};
