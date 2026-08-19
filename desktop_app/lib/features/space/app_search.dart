// Khớp từ khoá cho danh sách app — dùng chung giữa lưới launcher
// (space_screen.dart) và tab cửa hàng của hộp thoại cài app.
//
// Parity với web `web/src/components/space/spaceApp.ts`, nhưng KHÔNG cùng cách
// làm: Dart không có `String.normalize`, nên không tách được dấu bằng NFD như
// bên JS. Ở đây phải tra bảng ký tự tiếng Việt tường minh.

/// Nguyên âm có dấu → nguyên âm gốc. `đ` nằm trong bảng vì nó là một chữ cái
/// riêng, không phải `d` cộng dấu.
const Map<String, String> _base = {
  'àáạảãâầấậẩẫăằắặẳẵ': 'a',
  'èéẹẻẽêềếệểễ': 'e',
  'ìíịỉĩ': 'i',
  'òóọỏõôồốộổỗơờớợởỡ': 'o',
  'ùúụủũưừứựửữ': 'u',
  'ỳýỵỷỹ': 'y',
  'đ': 'd',
};

/// Bảng phẳng ký tự → chữ cái gốc, dựng một lần.
final Map<int, String> _foldTable = {
  for (final e in _base.entries)
    for (final ch in e.key.runes) ch: e.value,
};

/// Bỏ dấu và hạ chữ thường để "kho" tìm ra "Quản lý Kho", "du doan" tìm ra
/// "Siêu Dự Đoán".
String foldSearch(String s) {
  final lower = s.toLowerCase();
  final out = StringBuffer();
  for (final r in lower.runes) {
    out.write(_foldTable[r] ?? String.fromCharCode(r));
  }
  return out.toString();
}

/// Các trường này có khớp từ khoá không. Từ khoá rỗng thì khớp tất cả.
bool searchMatches(Iterable<String> fields, String query) {
  final q = foldSearch(query.trim());
  if (q.isEmpty) return true;
  return foldSearch(fields.join(' ')).contains(q);
}
