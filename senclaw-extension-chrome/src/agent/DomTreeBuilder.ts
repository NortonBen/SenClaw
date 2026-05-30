// Flat DOM tree builder — port of alibaba/page-agent's `buildDomTree`.
//
// Walks the document recursively (including same-origin iframes + shadow DOM),
// assigning every node a unique numeric id. Interactive elements get an extra
// `highlightIndex` (1-based, monotonic).
//
// The result is intentionally small: live element refs are stored in a separate
// SelectorMap (not serialized) so the tree can be JSON-encoded for transport
// without losing the live reference for later action dispatch.

import {
  isElementVisible,
  isInExpandedViewport,
  isInteractiveElement,
  isDistinctFromAncestor,
  isScrollableContainer,
  getScrollData,
  implicitRole,
} from './InteractiveDetector';
import { putInSelectorMap, resetSelectorMap, markUrl } from './SelectorMap';

export interface DomNode {
  id: string;
  /** Element node only (we drop pure-text leaves except as element.text). */
  tag: string;
  /** Computed ARIA role (or implicit). Empty string if none. */
  role: string;
  /** Text content up to next clickable descendant, capped. */
  text: string;
  attributes: Record<string, string>;
  bbox: { x: number; y: number; width: number; height: number };
  /** Set on every interactive node; absent for plain structural nodes. */
  highlightIndex?: number;
  /** Whether this interactive element is newly seen at the current URL. */
  isNew?: boolean;
  enabled: boolean;
  selected: boolean;
  /**
   * Where the element sits relative to the viewport.
   * 'in' = currently visible, 'above' = scroll up to see, 'below' = scroll down.
   */
  viewportStatus: 'in' | 'above' | 'below';
  /** Optional frame path (e.g. "0/1" = top.frames[0].frames[1]) for nested same-origin frames. */
  framePath?: string;
  /**
   * Set when this element is an independently scrollable sub-container. The agent
   * can scroll it in isolation by passing its highlightIndex as `container_index`.
   */
  scrollable?: boolean;
  /** Remaining scroll distance (px) per side, present only when `scrollable`. */
  scrollData?: { top: number; bottom: number; left: number; right: number };
  /** Child node ids (flat-map design — children are pointers, not nested objects). */
  children?: string[];
}

export interface ViewportInfo {
  width: number;
  height: number;
  scroll_x: number;
  scroll_y: number;
  document_width: number;
  document_height: number;
  pages_above: number;
  pages_below: number;
}

export interface DomTreeResult {
  /** Map of id -> node (flat). */
  nodes: Record<string, DomNode>;
  /** Top-level ids in document order. */
  rootIds: string[];
  /** Just the interactive ones, in encounter order, for the LLM. */
  interactive: DomNode[];
  viewport: ViewportInfo;
  url: string;
  title: string;
}

export interface BuildOptions {
  /** Pixels beyond the viewport to still include. -1 = strict viewport only. */
  viewportExpansion: number;
  /** Skip nodes hidden via CSS (visibility/display). Default true. */
  skipHidden: boolean;
  /** Maximum interactive elements (hard cap to bound token cost). */
  maxInteractive: number;
  /** Whether to walk same-origin iframes. */
  walkIframes: boolean;
  /** Whether to walk shadow DOM. */
  walkShadow: boolean;
}

export const DEFAULT_BUILD_OPTIONS: BuildOptions = {
  viewportExpansion: 0,
  skipHidden: true,
  maxInteractive: 300,
  walkIframes: true,
  walkShadow: true,
};

/** Attributes worth keeping for the LLM. Per page-agent's compact set. */
const KEEP_ATTRS = new Set([
  'title', 'type', 'checked', 'name', 'role', 'value', 'placeholder',
  'alt', 'aria-label', 'aria-expanded', 'aria-checked', 'aria-selected',
  'aria-disabled', 'data-state', 'id', 'for', 'target', 'aria-haspopup',
  'aria-controls', 'aria-owns', 'contenteditable', 'href',
]);

const CAP_ATTR = 60;
const CAP_TEXT = 200;
/** Tighter cap applied only to the rendered LLM view (storage keeps CAP_ATTR). */
const CAP_ATTR_OUT = 30;

function cap(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + '…' : s;
}

/**
 * Render a node's attributes for the LLM, dropping redundancy the way page-agent's
 * `flatTreeToString` does: no `role` duplicating the tag, no aria-label/placeholder/
 * title that merely repeats the visible text, and no two attributes sharing the same
 * (longer) value. Values are capped tight to keep the snapshot cheap.
 */
function compactAttrs(node: DomNode): string {
  const attrs: Record<string, string> = {};
  for (const [k, v] of Object.entries(node.attributes)) {
    if (v) attrs[k] = v;
  }

  // role that just echoes the tag carries no signal.
  if (attrs.role && attrs.role === node.tag) delete attrs.role;

  // Attributes that merely repeat the element's own text are noise.
  const text = node.text?.toLowerCase().trim();
  if (text) {
    for (const a of ['aria-label', 'placeholder', 'title']) {
      if (attrs[a] && attrs[a].toLowerCase().trim() === text) delete attrs[a];
    }
  }

  // Collapse duplicate values (only worth it for longer strings).
  const seen = new Set<string>();
  for (const k of Object.keys(attrs)) {
    const v = attrs[k];
    if (v.length > 5) {
      if (seen.has(v)) {
        delete attrs[k];
        continue;
      }
      seen.add(v);
    }
  }

  return Object.entries(attrs)
    .map(([k, v]) => `${k}=${JSON.stringify(cap(v, CAP_ATTR_OUT))}`)
    .join(' ');
}

/** Compact scroll affordance hint, e.g. `scroll=↓1200px` — only sides with room left. */
function scrollHint(node: DomNode): string {
  if (!node.scrollable || !node.scrollData) return '';
  const d = node.scrollData;
  const parts: string[] = [];
  if (d.top > 0) parts.push(`↑${d.top}px`);
  if (d.bottom > 0) parts.push(`↓${d.bottom}px`);
  if (d.left > 0) parts.push(`←${d.left}px`);
  if (d.right > 0) parts.push(`→${d.right}px`);
  return parts.length ? ` scroll=${parts.join(',')}` : ' scroll=here';
}

function getAttrs(el: Element): Record<string, string> {
  const out: Record<string, string> = {};
  for (const a of Array.from(el.attributes)) {
    if (KEEP_ATTRS.has(a.name)) {
      out[a.name] = cap(a.value, CAP_ATTR);
    }
  }
  return out;
}

/**
 * Get text content for an element, but stop descending at any other
 * interactive descendant (so each clickable owns its own label cleanly).
 */
function getTextStopAtClickable(el: Element): string {
  const parts: string[] = [];
  const walk = (n: Node) => {
    if (n.nodeType === Node.TEXT_NODE) {
      const t = n.textContent?.trim();
      if (t) parts.push(t);
      return;
    }
    if (n.nodeType !== Node.ELEMENT_NODE) return;
    const e = n as Element;
    if (e !== el && isInteractiveElement(e)) return;
    for (const c of Array.from(n.childNodes)) walk(c);
  };
  for (const c of Array.from(el.childNodes)) walk(c);
  return cap(parts.join(' ').replace(/\s+/g, ' ').trim(), CAP_TEXT);
}

function classifyViewportStatus(rect: DOMRect): 'in' | 'above' | 'below' {
  const vh = window.innerHeight;
  if (rect.bottom < 0) return 'above';
  if (rect.top > vh) return 'below';
  return 'in';
}

function readViewportInfo(): ViewportInfo {
  const docW = Math.max(
    document.documentElement.scrollWidth,
    document.body?.scrollWidth ?? 0,
  );
  const docH = Math.max(
    document.documentElement.scrollHeight,
    document.body?.scrollHeight ?? 0,
  );
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const sx = window.scrollX;
  const sy = window.scrollY;
  return {
    width: vw,
    height: vh,
    scroll_x: sx,
    scroll_y: sy,
    document_width: docW,
    document_height: docH,
    pages_above: vh > 0 ? sy / vh : 0,
    pages_below: vh > 0 ? Math.max(0, (docH - vh - sy) / vh) : 0,
  };
}

interface WalkState {
  nodes: Record<string, DomNode>;
  rootIds: string[];
  interactive: DomNode[];
  nextId: number;
  nextHighlight: number;
  opts: BuildOptions;
}

function walkElement(
  el: Element,
  state: WalkState,
  framePath: string | undefined,
  frameOffsetX: number,
  frameOffsetY: number,
): string | null {
  if (state.interactive.length >= state.opts.maxInteractive) return null;

  // Skip own overlay / framework-tagged nodes
  if (el.hasAttribute?.('data-senclaw-ignore')) return null;

  // SCRIPT/STYLE/NOSCRIPT etc — no value to LLM
  const tag = el.tagName.toLowerCase();
  if (['script', 'style', 'noscript', 'svg', 'canvas', 'meta', 'link', 'head'].includes(tag)) {
    return null;
  }

  let rect: DOMRect;
  try {
    rect = el.getBoundingClientRect();
  } catch {
    return null;
  }
  // Apply iframe offset so coordinates are relative to top frame's viewport
  if (frameOffsetX || frameOffsetY) {
    rect = new DOMRect(
      rect.x + frameOffsetX,
      rect.y + frameOffsetY,
      rect.width,
      rect.height,
    );
  }

  const visible = isElementVisible(el, rect);
  const inExpandedViewport = isInExpandedViewport(rect, state.opts.viewportExpansion);

  // Pre-fetch computed style once
  let style: CSSStyleDeclaration | undefined;
  if (visible) {
    try {
      style = window.getComputedStyle(el);
    } catch {
      /* cross-origin issue, skip */
    }
  }

  const interactive =
    visible &&
    inExpandedViewport &&
    isInteractiveElement(el, style) &&
    isDistinctFromAncestor(el);

  // A scrollable sub-container that isn't itself clickable still gets indexed so
  // the agent can target it with a Scroll(container_index) action.
  const scrollable =
    visible && inExpandedViewport && isScrollableContainer(el, style);

  const id = String(state.nextId++);
  const node: DomNode = {
    id,
    tag,
    role: el.getAttribute('role') || implicitRole(el),
    text: '',
    attributes: getAttrs(el),
    bbox: { x: rect.x, y: rect.y, width: rect.width, height: rect.height },
    enabled: !(el as HTMLButtonElement).disabled,
    selected:
      (el as HTMLInputElement).checked ||
      el.getAttribute('aria-selected') === 'true' ||
      el.getAttribute('aria-checked') === 'true',
    viewportStatus: classifyViewportStatus(rect),
    framePath,
    children: [],
  };

  if (interactive || scrollable) {
    const highlightIndex = state.nextHighlight++;
    node.highlightIndex = highlightIndex;
    if (interactive) node.text = getTextStopAtClickable(el);
    if (scrollable) {
      node.scrollable = true;
      node.scrollData = getScrollData(el);
    }
    const { isNew } = putInSelectorMap(highlightIndex, el as HTMLElement);
    if (isNew) node.isNew = true;
    state.interactive.push(node);
  }

  // Descend into shadow DOM first (encapsulated content)
  if (state.opts.walkShadow && (el as any).shadowRoot) {
    for (const c of Array.from((el as any).shadowRoot.childNodes)) {
      if ((c as Node).nodeType === Node.ELEMENT_NODE) {
        const childId = walkElement(
          c as Element,
          state,
          framePath,
          frameOffsetX,
          frameOffsetY,
        );
        if (childId) node.children!.push(childId);
      }
    }
  }

  // Same-origin iframe
  if (state.opts.walkIframes && tag === 'iframe') {
    try {
      const iframe = el as HTMLIFrameElement;
      const doc = iframe.contentDocument || iframe.contentWindow?.document;
      if (doc && doc.body) {
        const ox = frameOffsetX + rect.x;
        const oy = frameOffsetY + rect.y;
        const nextFramePath = framePath ? `${framePath}/` : '';
        // walk body of the frame; build a unique path id we can use later
        const subId = walkElement(
          doc.body,
          state,
          `${nextFramePath}${id}`,
          ox,
          oy,
        );
        if (subId) node.children!.push(subId);
      }
    } catch {
      /* cross-origin — give up */
    }
  }

  // Normal children
  for (const c of Array.from(el.children)) {
    const childId = walkElement(c, state, framePath, frameOffsetX, frameOffsetY);
    if (childId) node.children!.push(childId);
  }

  if (!node.children!.length) delete node.children;

  state.nodes[id] = node;
  return id;
}

export function buildDomTree(opts: Partial<BuildOptions> = {}): DomTreeResult {
  const options: BuildOptions = { ...DEFAULT_BUILD_OPTIONS, ...opts };

  // Reset selectorMap on URL change so isNew detection makes sense.
  resetSelectorMap();
  markUrl(location.href);

  const state: WalkState = {
    nodes: {},
    rootIds: [],
    interactive: [],
    nextId: 0,
    nextHighlight: 0,
    opts: options,
  };

  if (document.body) {
    const rootId = walkElement(document.body, state, undefined, 0, 0);
    if (rootId) state.rootIds.push(rootId);
  }

  return {
    nodes: state.nodes,
    rootIds: state.rootIds,
    interactive: state.interactive,
    viewport: readViewportInfo(),
    url: location.href,
    title: document.title,
  };
}

/**
 * Render the interactive set as the compact tab-indented format the LLM consumes:
 *
 *   [0]<button aria-label='Submit'>Submit</button>
 *   *[1]<input placeholder='Email'></input>
 *
 * Returns `{header, content, footer}` so the gateway can budget tokens.
 */
export function renderBrowserState(tree: DomTreeResult): {
  header: string;
  content: string;
  footer: string;
} {
  const headerLines: string[] = [];
  headerLines.push(`URL: ${tree.url}`);
  headerLines.push(`Title: ${tree.title}`);
  headerLines.push(
    `Viewport: ${Math.round(tree.viewport.width)}x${Math.round(tree.viewport.height)} (${tree.viewport.pages_above.toFixed(1)} pages above, ${tree.viewport.pages_below.toFixed(1)} pages below)`,
  );
  if (tree.viewport.pages_above <= 0.01) {
    headerLines.push('[Start of page]');
  } else {
    headerLines.push(
      `[${Math.round(tree.viewport.pages_above * tree.viewport.height)}px above viewport]`,
    );
  }

  const lines: string[] = [];
  for (const node of tree.interactive) {
    if (node.viewportStatus !== 'in') continue;
    const attrs = compactAttrs(node);
    const prefix = node.isNew ? '*' : '';
    const attrPart = attrs ? ` ${attrs}` : '';
    const text = node.text ? node.text : '';
    lines.push(
      `${prefix}[${node.highlightIndex}]<${node.tag}${attrPart}${scrollHint(node)}>${text}</${node.tag}>`,
    );
  }

  const footerLines: string[] = [];
  if (tree.viewport.pages_below <= 0.01) {
    footerLines.push('[End of page]');
  } else {
    footerLines.push(
      `[${Math.round(tree.viewport.pages_below * tree.viewport.height)}px below viewport]`,
    );
  }

  return {
    header: headerLines.join('\n'),
    content: lines.join('\n'),
    footer: footerLines.join('\n'),
  };
}
