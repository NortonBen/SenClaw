/// Tag helpers for Space notes.
///
/// Notes carry an explicit tag list, but users also write `#hashtags` straight
/// into the body (see the screenshot-capture flow and Keep habits). These
/// helpers pull those out and keep the whole set tidy so filtering is reliable.
library;

/// Matches an inline `#hashtag`. Unicode-aware so Vietnamese tags work
/// (`#học-lập-trình`, `#7-ngày`, `#lộ-trình`). The `(?:^|\s)` boundary means it
/// never grabs a URL fragment (`page.html#frag`) or a Markdown heading
/// (`## Title` has a space after the hashes, so nothing is captured).
final RegExp _bodyHashtag = RegExp(
  r'(?:^|\s)#([\p{L}\p{N}][\p{L}\p{N}_-]*)',
  unicode: true,
  multiLine: true,
);

/// Extract inline `#hashtags` from a note body, normalised (lower-cased,
/// deduped, order-preserving).
List<String> extractBodyTags(String body) =>
    normaliseTags(_bodyHashtag.allMatches(body).map((m) => m.group(1)!));

/// Clean a raw tag list: strip leading `#`, trim, lower-case, drop empties,
/// dedupe — while preserving first-seen order.
List<String> normaliseTags(Iterable<String> tags) {
  final out = <String>[];
  final seen = <String>{};
  for (final raw in tags) {
    final t =
        raw.trim().toLowerCase().replaceAll(RegExp(r'^#+'), '').trim();
    if (t.isEmpty || !seen.add(t)) continue;
    out.add(t);
  }
  return out;
}
