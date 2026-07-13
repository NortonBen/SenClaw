import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/features/chat/audio_service.dart';

void main() {
  test('splits on sentence enders', () {
    final parts =
        splitSentences('Câu một. Câu hai dài hơn một chút! Câu ba?');
    expect(parts, ['Câu một.', 'Câu hai dài hơn một chút!', 'Câu ba?']);
  });

  test('merges tiny fragments like list numbers', () {
    final parts = splitSentences('1. Mở ứng dụng. 2. Chọn cài đặt.');
    expect(parts.length, 2);
    expect(parts.first, contains('Mở ứng dụng'));
    expect(parts.first, startsWith('1.'));
  });

  test('long run-on sentence is cut at spaces under maxChars', () {
    final long = List.filled(60, 'từ').join(' '); // no enders, > 120 chars
    final parts = splitSentences(long, maxChars: 60);
    expect(parts.length, greaterThan(1));
    for (final p in parts) {
      expect(p.length, lessThanOrEqualTo(60));
      expect(p.trim(), p);
    }
  });

  test('empty and whitespace-only input yields nothing', () {
    expect(splitSentences(''), isEmpty);
    expect(splitSentences('   \n '), isEmpty);
  });

  test('single short text stays one chunk', () {
    expect(splitSentences('Xin chào'), ['Xin chào']);
  });

  test('decimal and version dots do not split', () {
    final parts = splitSentences(
        'Tải trung bình 0.08 trong 1 phút. Kernel 6.6.56-v8 chạy ổn.');
    expect(parts, [
      'Tải trung bình 0.08 trong 1 phút.',
      'Kernel 6.6.56-v8 chạy ổn.',
    ]);
  });

  test('stripMarkdownForSpeech keeps content, drops syntax', () {
    const md = '''**1. Thông tin OS & Kernel:**
`Linux server-lab 6.6.56-v8+`

---

- Tổng RAM: ~1.85 GB (1846 MB)
- Xem [tài liệu](https://example.com/docs) để biết thêm.

## Kết luận
> Ổ cứng *khá đầy* với 95% dung lượng.''';
    final t = stripMarkdownForSpeech(md);
    expect(t, isNot(contains('*')));
    expect(t, isNot(contains('`')));
    expect(t, isNot(contains('#')));
    expect(t, isNot(contains('~')));
    expect(t, isNot(contains('](')));
    expect(t, isNot(contains('\n- ')));
    expect(t, contains('1. Thông tin OS & Kernel:'));
    expect(t, contains('Linux server-lab 6.6.56-v8+'));
    expect(t, contains('khoảng 1.85 GB'));
    expect(t, contains('Xem tài liệu để biết thêm.'));
    expect(t, contains('Kết luận'));
    expect(t, contains('Ổ cứng khá đầy với 95% dung lượng.'));
  });

  test('stripMarkdownForSpeech drops table plumbing, keeps cells', () {
    const md = '| Cột A | Cột B |\n|---|---|\n| giá trị 1 | giá trị 2 |';
    final t = stripMarkdownForSpeech(md);
    expect(t, isNot(contains('|')));
    expect(t, isNot(contains('---')));
    expect(t, contains('Cột A'));
    expect(t, contains('giá trị 2'));
  });

  test('markdown symbol-only fragments are filtered as unspeakable', () {
    expect(hasSpeakableContent('---'), isFalse);
    expect(hasSpeakableContent('```'), isFalse);
    expect(hasSpeakableContent('**'), isFalse);
    expect(hasSpeakableContent('- Tổng RAM: ~1.85 GB'), isTrue);
    expect(hasSpeakableContent('0.08'), isTrue);
  });
}
