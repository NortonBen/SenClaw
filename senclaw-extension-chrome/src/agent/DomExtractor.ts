// DOM extraction: flattened accessibility tree with interactive element indexing.
import type { SnapshotElement, PageSnapshot } from '../types/protocol';

const INTERACTIVE_SELECTOR = [
  'a', 'button', 'input', 'select', 'textarea',
  '[role="button"]', '[role="link"]', '[role="textbox"]',
  '[role="combobox"]', '[role="checkbox"]', '[role="radio"]',
  '[tabindex]:not([tabindex="-1"])', '[contenteditable="true"]',
  'details', 'summary', '[onclick]',
].join(',');

const KEEP_ATTRS = [
  'href', 'src', 'alt', 'title', 'placeholder', 'type',
  'name', 'id', 'value', 'role', 'aria-label', 'aria-expanded',
  'aria-selected', 'aria-checked', 'checked', 'disabled',
  'selected', 'readonly', 'required', 'maxlength',
];

function implicitRole(el: Element): string {
  const tag = el.tagName.toLowerCase();
  const type = (el as HTMLInputElement).type;
  if (tag === 'a' && el.hasAttribute('href')) return 'link';
  if (tag === 'button') return 'button';
  if (tag === 'input') {
    if (type === 'checkbox') return 'checkbox';
    if (type === 'radio') return 'radio';
    if (type === 'submit' || type === 'button') return 'button';
    return 'textbox';
  }
  if (tag === 'select') return 'combobox';
  if (tag === 'textarea') return 'textbox';
  if (tag === 'img') return 'img';
  return '';
}

function getAttributes(el: Element): Record<string, string> {
  const attrs: Record<string, string> = {};
  for (const key of KEEP_ATTRS) {
    const val = el.getAttribute(key);
    if (val) attrs[key] = val;
  }
  return attrs;
}

export function buildSnapshot(depth?: number): PageSnapshot {
  const elements: SnapshotElement[] = [];
  const interactive = document.querySelectorAll(INTERACTIVE_SELECTOR);

  interactive.forEach((el, i) => {
    if (i >= 500) return; // cap at 500 elements

    const rect = el.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return; // skip invisible

    const text = (el.textContent?.trim().slice(0, 200)
      || el.getAttribute('aria-label')
      || (el as HTMLInputElement).placeholder
      || (el as HTMLInputElement).value
      || '');

    elements.push({
      index: i,
      tag: el.tagName.toLowerCase(),
      role: el.getAttribute('role') || implicitRole(el),
      text,
      attributes: getAttributes(el),
      bbox: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
      enabled: !(el as HTMLButtonElement).disabled,
      selected: (el as HTMLInputElement).checked
        || el.getAttribute('aria-selected') === 'true',
    });
  });

  return {
    url: location.href,
    title: document.title,
    elements,
    text_content_summary: (document.body?.innerText ?? '').slice(0, 1000),
  };
}

export function extractText(selector?: string): { text: string; url: string } {
  const container = selector ? document.querySelector(selector) : document.body;
  return {
    url: location.href,
    text: container?.textContent?.trim() ?? '',
  };
}

export function extractLinks(selector?: string): { links: { text: string; url: string }[]; source_url: string } {
  const container = selector ? document.querySelector(selector) : document.body;
  const links = Array.from((container ?? document).querySelectorAll('a[href]'))
    .map((a) => ({
      text: (a.textContent ?? '').trim().slice(0, 200),
      url: a.getAttribute('href') ?? '',
    }))
    .filter((l) => l.url && !l.url.startsWith('#') && !l.url.startsWith('javascript:'));
  return { links, source_url: location.href };
}

export function extractTable(selector?: string): { data: Record<string, string>[]; source_url: string } {
  const table = selector
    ? document.querySelector(selector) as HTMLTableElement
    : document.querySelector('table');

  if (!table) return { data: [], source_url: location.href };

  const headers: string[] = [];
  table.querySelectorAll('thead th, thead td, tr:first-child th, tr:first-child td').forEach((th) => {
    headers.push((th.textContent ?? '').trim());
  });

  const rows: Record<string, string>[] = [];
  table.querySelectorAll('tbody tr, tr:not(:first-child)').forEach((tr) => {
    const row: Record<string, string> = {};
    tr.querySelectorAll('td, th').forEach((td, i) => {
      const key = headers[i] ?? `col_${i}`;
      row[key] = (td.textContent ?? '').trim();
    });
    if (Object.keys(row).length > 0) rows.push(row);
  });

  return { data: rows, source_url: location.href };
}
