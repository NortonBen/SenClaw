// DOM extraction entry points. Thin wrappers around DomTreeBuilder.
//
// The legacy `buildSnapshot()` shape is preserved as a compatibility flag.
// New consumers should call `getBrowserState()` for the rich payload.

import {
  buildDomTree,
  renderBrowserState,
  type DomTreeResult,
  type ViewportInfo,
} from './DomTreeBuilder';
import { drawHighlights, clearHighlights } from './HighlightOverlay';
import { getElementByIndex } from './SelectorMap';

export interface ExtractedElement {
  index: number;
  tag: string;
  role: string;
  text: string;
  attributes: Record<string, string>;
  bbox: { x: number; y: number; width: number; height: number };
  enabled: boolean;
  selected: boolean;
  is_new: boolean;
  viewport_status: 'in' | 'above' | 'below';
  frame_path?: string;
  /** True when the element is an independently scrollable sub-container. */
  scrollable?: boolean;
  /** Remaining scroll distance (px) per side; present only when `scrollable`. */
  scroll_data?: { top: number; bottom: number; left: number; right: number };
}

export interface BrowserState {
  url: string;
  title: string;
  elements: ExtractedElement[];
  viewport: ViewportInfo;
  /** Pre-rendered tab-indented compact view for the LLM. */
  formatted: {
    header: string;
    content: string;
    footer: string;
  };
  /** Total interactive elements found before any cap. */
  total_interactive: number;
  /** Whether the result was capped at maxInteractive. */
  capped: boolean;
  text_content_summary: string;
}

export interface GetSnapshotOptions {
  viewport_expansion?: number;
  max_interactive?: number;
  walk_iframes?: boolean;
  walk_shadow?: boolean;
  highlight?: boolean;
}

export function getBrowserState(opts: GetSnapshotOptions = {}): BrowserState {
  const tree = buildDomTree({
    viewportExpansion: opts.viewport_expansion ?? 0,
    maxInteractive: opts.max_interactive ?? 300,
    walkIframes: opts.walk_iframes ?? true,
    walkShadow: opts.walk_shadow ?? true,
  });

  if (opts.highlight) {
    const targets: { index: number; el: HTMLElement }[] = [];
    for (const n of tree.interactive) {
      if (n.viewportStatus !== 'in') continue;
      const el = getElementByIndex(n.highlightIndex!);
      if (el) targets.push({ index: n.highlightIndex!, el });
    }
    drawHighlights(targets);
  } else {
    clearHighlights();
  }

  return toBrowserState(tree);
}

function toBrowserState(tree: DomTreeResult): BrowserState {
  const elements: ExtractedElement[] = tree.interactive.map((n) => ({
    index: n.highlightIndex!,
    tag: n.tag,
    role: n.role,
    text: n.text,
    attributes: n.attributes,
    bbox: n.bbox,
    enabled: n.enabled,
    selected: n.selected,
    is_new: !!n.isNew,
    viewport_status: n.viewportStatus,
    frame_path: n.framePath,
    scrollable: n.scrollable,
    scroll_data: n.scrollData,
  }));

  const formatted = renderBrowserState(tree);
  const summary = (document.body?.innerText ?? '').slice(0, 2000);

  return {
    url: tree.url,
    title: tree.title,
    elements,
    viewport: tree.viewport,
    formatted,
    total_interactive: tree.interactive.length,
    capped: tree.interactive.length >= 300,
    text_content_summary: summary,
  };
}

/** Legacy shape kept for the existing browser_server.rs snapshot path. */
export function buildSnapshot(): {
  url: string;
  title: string;
  elements: ExtractedElement[];
  text_content_summary: string;
} {
  const state = getBrowserState();
  return {
    url: state.url,
    title: state.title,
    elements: state.elements,
    text_content_summary: state.text_content_summary,
  };
}

export function extractText(selector?: string): { text: string; url: string } {
  const container = selector ? document.querySelector(selector) : document.body;
  return {
    url: location.href,
    text: container?.textContent?.trim() ?? '',
  };
}

export function extractLinks(
  selector?: string,
): { links: { text: string; url: string }[]; source_url: string } {
  const container = selector ? document.querySelector(selector) : document.body;
  const links = Array.from((container ?? document).querySelectorAll('a[href]'))
    .map((a) => ({
      text: (a.textContent ?? '').trim().slice(0, 200),
      url: a.getAttribute('href') ?? '',
    }))
    .filter((l) => l.url && !l.url.startsWith('#') && !l.url.startsWith('javascript:'));
  return { links, source_url: location.href };
}

export function extractTable(
  selector?: string,
): { data: Record<string, string>[]; source_url: string } {
  const table = selector
    ? (document.querySelector(selector) as HTMLTableElement | null)
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
