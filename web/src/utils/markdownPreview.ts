/** Rút gọn markdown để hiển thị preview (vài đoạn / giới hạn ký tự). */
export function truncateToPreview(
  text: string,
  maxParagraphs = 3,
  maxChars = 800,
): { preview: string; isTruncated: boolean } {
  const trimmed = text.trim();
  if (!trimmed) return { preview: '', isTruncated: false };

  const parts = trimmed.split(/\n\n+/);
  let preview =
    parts.length > maxParagraphs
      ? parts.slice(0, maxParagraphs).join('\n\n')
      : trimmed;

  if (preview.length > maxChars) {
    preview = `${preview.slice(0, maxChars).trimEnd()}…`;
  }

  const isTruncated = preview.length < trimmed.length || parts.length > maxParagraphs;
  return { preview, isTruncated };
}
