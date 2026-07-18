import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/features/space/note_tags.dart';

void main() {
  group('extractBodyTags', () {
    test('pulls inline hashtags, lower-cased & deduped', () {
      expect(
        extractBodyTags('Learning #Rust and #rust today #todo'),
        ['rust', 'todo'],
      );
    });

    test('handles Vietnamese diacritics and hyphenated tags', () {
      expect(
        extractBodyTags('lộ trình #học-lập-trình trong #7-ngày #lộ-trình'),
        ['học-lập-trình', '7-ngày', 'lộ-trình'],
      );
    });

    test('ignores Markdown headings (space after #)', () {
      expect(extractBodyTags('# Heading\n## Sub\ntext'), isEmpty);
    });

    test('ignores URL fragments and mid-word hashes', () {
      expect(extractBodyTags('see https://x.com/a#frag or foo#bar'), isEmpty);
    });

    test('matches a hashtag at the very start of the body', () {
      expect(extractBodyTags('#screenshot at top'), ['screenshot']);
    });
  });

  group('normaliseTags', () {
    test('trims, strips leading #, lower-cases, dedupes, keeps order', () {
      expect(
        normaliseTags(['  Work ', '#idea', 'work', 'IDEA', '', '##bug']),
        ['work', 'idea', 'bug'],
      );
    });
  });
}
