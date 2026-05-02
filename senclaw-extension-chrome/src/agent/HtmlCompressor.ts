// Client-side HTML compression for LLM analysis.
// Mirrors the Rust implementation at src/browser/html_compressor.rs

const NOISE_TAGS = new Set([
  'script', 'style', 'noscript', 'iframe', 'svg', 'canvas',
  'object', 'embed', 'applet', 'audio', 'video', 'source', 'track',
]);

const INTERACTIVE_TAGS = new Set([
  'a', 'button', 'input', 'select', 'textarea', 'option', 'details', 'summary', 'label',
]);

const SEMANTIC_TAGS = new Set([
  'nav', 'header', 'footer', 'main', 'article', 'section',
  'aside', 'figure', 'figcaption', 'dialog', 'fieldset',
  'p', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'li', 'dt', 'dd', 'td', 'th',
  'pre', 'code', 'blockquote', 'strong', 'em',
]);

export interface CompressedNode {
  tag: string;
  index?: number;
  text: string;
  attrs: Record<string, string>;
  interactive: boolean;
}

export interface CompressedResult {
  root: CompressedNode | null;
  interactiveElements: CompressedNode[];
  textContent: string;
  stats: { originalSize: number; compressedSize: number; ratio: number };
}

/**
 * Compress an HTML string into a lightweight semantic representation.
 * Uses DOM-based extraction for real pages, plus regex-based fallback for raw HTML.
 */
export function compressHtml(html: string): CompressedResult {
  const originalSize = html.length;

  // For real pages, use the DOM directly
  const interactiveElements = extractInteractiveFromDom();
  const textContent = (document.body?.innerText ?? document.body?.textContent ?? '').trim();

  const compressedSize = textContent.length + JSON.stringify(interactiveElements).length;

  return {
    root: null,
    interactiveElements,
    textContent: textContent.slice(0, 5000),
    stats: {
      originalSize,
      compressedSize,
      ratio: originalSize > 0 ? compressedSize / originalSize : 1,
    },
  };
}

function extractInteractiveFromDom(): CompressedNode[] {
  const elements: CompressedNode[] = [];
  const selector = [
    'a', 'button', 'input', 'select', 'textarea',
    '[role="button"]', '[role="link"]', '[role="textbox"]',
    '[role="combobox"]', '[role="checkbox"]', '[role="radio"]',
    '[tabindex]:not([tabindex="-1"])', '[contenteditable="true"]',
  ].join(',');

  document.querySelectorAll(selector).forEach((el, i) => {
    if (i >= 500) return;
    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;

    const tag = el.tagName.toLowerCase();
    const attrs: Record<string, string> = {};
    for (const key of ['href', 'placeholder', 'type', 'name', 'aria-label', 'role', 'value']) {
      const val = el.getAttribute(key);
      if (val) attrs[key] = val;
    }

    const text = (el.textContent?.trim().slice(0, 200)
      || el.getAttribute('aria-label')
      || (el as HTMLInputElement).placeholder
      || '');

    elements.push({
      tag,
      index: i,
      text,
      attrs,
      interactive: true,
    });
  });

  return elements;
}

/**
 * Compress raw HTML string (for offline use, not in content script).
 */
export function compressRawHtml(html: string): CompressedResult {
  const originalSize = html.length;

  // Remove scripts, styles, comments
  let cleaned = html
    .replace(/<script[^>]*>[\s\S]*?<\/script>/gi, '')
    .replace(/<style[^>]*>[\s\S]*?<\/style>/gi, '')
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<noscript[^>]*>[\s\S]*?<\/noscript>/gi, '');

  // Extract text from remaining tags
  cleaned = cleaned.replace(/<[^>]+>/g, ' ');
  cleaned = cleaned.replace(/\s+/g, ' ').trim();

  // Extract links
  const linkRegex = /<a[^>]+href=["']([^"']+)["'][^>]*>([^<]*)<\/a>/gi;
  const interactiveElements: CompressedNode[] = [];
  let match;
  while ((match = linkRegex.exec(html)) !== null) {
    interactiveElements.push({
      tag: 'a',
      index: interactiveElements.length,
      text: match[2].trim().slice(0, 200),
      attrs: { href: match[1] },
      interactive: true,
    });
  }

  return {
    root: null,
    interactiveElements,
    textContent: cleaned.slice(0, 5000),
    stats: {
      originalSize,
      compressedSize: cleaned.length,
      ratio: originalSize > 0 ? cleaned.length / originalSize : 1,
    },
  };
}
