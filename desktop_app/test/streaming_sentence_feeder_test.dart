import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/features/chat/audio_service.dart';

void main() {
  test('emits each sentence as soon as its boundary streams in', () {
    final f = StreamingSentenceFeeder();
    expect(f.update('Xin '), isEmpty);
    expect(f.update('Xin chào bạn'), isEmpty);
    expect(f.update('Xin chào bạn. Hôm nay'), ['Xin chào bạn.']);
    expect(f.update('Xin chào bạn. Hôm nay trời đẹp! Chúng'),
        ['Hôm nay trời đẹp!']);
    expect(f.flush(), ['Chúng']);
  });

  test('several boundaries in one delta emit several chunks', () {
    final f = StreamingSentenceFeeder();
    expect(f.update('Một. Hai! Ba? Bốn'), ['Một.', 'Hai!', 'Ba?']);
    expect(f.flush(), ['Bốn']);
  });

  test('decimal dot does not cut', () {
    final f = StreamingSentenceFeeder();
    expect(f.update('Phiên bản 1.5 rất ổn. Tiếp'), ['Phiên bản 1.5 rất ổn.']);
  });

  test('early cut after minWordsEarly words when the pipeline is idle', () {
    final f = StreamingSentenceFeeder(minWordsEarly: 5);
    // 5 whole words + one still-streaming word, no boundary yet.
    expect(f.update('một hai ba bốn năm sá', pipelineIdle: true),
        ['một hai ba bốn năm']);
    // The partial word stays pending and joins the next chunk.
    expect(f.update('một hai ba bốn năm sáu bảy.', pipelineIdle: true),
        ['sáu bảy.']);
  });

  test('no early cut while the pipeline is busy', () {
    final f = StreamingSentenceFeeder(minWordsEarly: 5);
    expect(f.update('một hai ba bốn năm sáu bảy tám'), isEmpty);
    expect(f.flush(), ['một hai ba bốn năm sáu bảy tám']);
  });

  test('a brand-new stream (not an extension) resets consumption', () {
    final f = StreamingSentenceFeeder();
    expect(f.update('Câu đầu tiên. '), ['Câu đầu tiên.']);
    // New stream starts over with different content.
    expect(f.update('Trả lời khác hẳn. '), ['Trả lời khác hẳn.']);
  });

  test('flush is empty when everything was already emitted', () {
    final f = StreamingSentenceFeeder();
    expect(f.update('Xong rồi.'), ['Xong rồi.']);
    expect(f.flush(), isEmpty);
    expect(f.flush(), isEmpty);
  });

  test('newline acts as a boundary', () {
    final f = StreamingSentenceFeeder();
    expect(f.update('Dòng một\nDòng hai'), ['Dòng một']);
    expect(f.flush(), ['Dòng hai']);
  });
}
