/**
 * Strip leading model "reasoning" / chain-of-thought wrappers from assistant text
 * so the UI can show them in a collapsible (Gemini-style) instead of raw tags.
 *
 * Supports common model wrappers: Qwen `think`, DeepSeek `redacted_reasoning`, `redacted_thinking`, and a few mixed open/close pairs.
 */
function leadingReasoningRe(open: string, close?: string): RegExp {
  const c = close ?? open;
  return new RegExp(`^(\\s*)<${open}\\b[^>]*>([\\s\\S]*?)<\\/${c}>`, 'i');
}

const LEADING_REASONING_RES: RegExp[] = [
  leadingReasoningRe('think'),
  leadingReasoningRe('redacted_' + 'reasoning'),
  leadingReasoningRe('redacted_' + 'thinking'),
  leadingReasoningRe('think', 'redacted_' + 'reasoning'),
  leadingReasoningRe('redacted_' + 'reasoning', 'think'),
];

export function extractLeadingReasoningBlocks(full: string): { reasoning: string; body: string } {
  const parts: string[] = [];
  let rest = full;
  for (;;) {
    const head = rest.trimStart();
    let matched = false;
    for (const re of LEADING_REASONING_RES) {
      const m = head.match(re);
      if (m && m.index === 0) {
        const inner = (m[2] ?? '').trim();
        if (inner.length > 0) {
          parts.push(inner);
        }
        rest = head.slice(m[0].length);
        matched = true;
        break;
      }
    }
    if (!matched) {
      break;
    }
  }
  return {
    reasoning: parts.join('\n\n'),
    body: rest.trimStart(),
  };
}
