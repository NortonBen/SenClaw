// Interactive element detection — layered signals borrowed from alibaba/page-agent + browser-use.
//
// An element is "interactive" if ANY of these are true (and it isn't disabled/inert):
//   1. Semantic tag (a, button, input, select, textarea, label, details, summary)
//   2. ARIA role from a known interactive set
//   3. Computed cursor in {pointer, move, text, grab, grabbing, cell, copy}
//   4. Has direct click/mousedown/mouseup/dblclick event listener (when devtools API available)
//
// The expensive checks (computed style, event listeners) are gated by a cheap candidate filter.

import { cachedRect, cachedStyle } from './LayoutCache';

const INTERACTIVE_TAGS = new Set([
  'a', 'button', 'input', 'select', 'textarea',
  'details', 'summary', 'label', 'option',
]);

const INTERACTIVE_ROLES = new Set([
  'button', 'link', 'menuitem', 'menubar', 'menu',
  'tab', 'radio', 'checkbox', 'switch', 'slider',
  'searchbox', 'textbox', 'combobox', 'listbox',
  'option', 'spinbutton', 'scrollbar', 'tree',
  'treeitem', 'gridcell', 'columnheader', 'rowheader',
]);

// Cursors that signal an actionable element (page-agent's full set: drag handles,
// resizers, zoom, context menus, etc. — not just `pointer`).
const INTERACTIVE_CURSORS = new Set([
  'pointer', 'move', 'text', 'grab', 'grabbing', 'cell', 'copy', 'alias',
  'all-scroll', 'col-resize', 'row-resize', 'context-menu', 'crosshair',
  'zoom-in', 'zoom-out', 'help', 'vertical-text',
  'e-resize', 'w-resize', 'n-resize', 's-resize',
  'ne-resize', 'nw-resize', 'se-resize', 'sw-resize',
  'ew-resize', 'ns-resize', 'nesw-resize', 'nwse-resize',
]);

// Cursors that explicitly say "you can't act on me right now" — these veto an
// otherwise-interactive element (disabled buttons, busy controls).
const NON_INTERACTIVE_CURSORS = new Set([
  'not-allowed', 'no-drop', 'wait', 'progress',
]);

// Tags that React/Vue commonly delegate events from but are NOT themselves interactive.
const ROOT_HINT_SELECTOR = [
  '[data-reactroot]', '[data-reactid]',
  '#root', '#app', '[id^="root-"]',
  '#__next', '#__nuxt', '#adex-root',
].join(',');

const HIGHLIGHT_INTERNAL_ATTR = 'data-senclaw-ignore';

/** Cheap pre-filter — is this element even worth checking? */
export function isInteractiveCandidate(el: Element): boolean {
  const tag = el.tagName.toLowerCase();
  if (INTERACTIVE_TAGS.has(tag)) return true;

  const role = el.getAttribute('role');
  if (role && INTERACTIVE_ROLES.has(role)) return true;

  if (el.hasAttribute('onclick')) return true;
  if (el.hasAttribute('contenteditable') && el.getAttribute('contenteditable') !== 'false') return true;

  const tabIndex = el.getAttribute('tabindex');
  if (tabIndex && tabIndex !== '-1') return true;

  // Common framework "clickable" hints — dropdown toggles / popup triggers that
  // carry no semantic tag or role.
  if (el.getAttribute('aria-haspopup') === 'true') return true;
  if (el.getAttribute('data-toggle') === 'dropdown') return true;
  if (el.classList?.contains('dropdown-toggle')) return true;

  return false;
}

/** Is the element disabled / inert / readonly? */
export function isElementDisabled(el: Element): boolean {
  if ((el as HTMLButtonElement).disabled) return true;
  if ((el as HTMLInputElement).readOnly) return true;
  if ((el as HTMLElement).inert) return true;
  if (el.hasAttribute('inert')) return true;
  if (el.getAttribute('aria-disabled') === 'true') return true;
  return false;
}

/** Apply ROOT_HINT_SELECTOR opt-out: framework root nodes are not interactive. */
export function isFrameworkRoot(el: Element): boolean {
  if (el.hasAttribute(HIGHLIGHT_INTERNAL_ATTR)) return true;
  if (el.matches?.(ROOT_HINT_SELECTOR)) return true;
  return false;
}

/** Full interactive check. `style` may be pre-computed to amortize getComputedStyle. */
export function isInteractiveElement(
  el: Element,
  style?: CSSStyleDeclaration,
): boolean {
  if (isFrameworkRoot(el)) return false;
  if (isElementDisabled(el)) return false;

  const cs = style ?? cachedStyle(el);

  if (isInteractiveCandidate(el)) {
    // A semantic/role-interactive element showing a disabled-style cursor is not
    // actually actionable (page-agent applies the same veto).
    if (NON_INTERACTIVE_CURSORS.has(cs.cursor)) return false;
    return true;
  }

  if (INTERACTIVE_CURSORS.has(cs.cursor)) return true;

  // Inline mouse-event handler attributes / properties. getEventListeners() would be
  // more thorough but isn't available in content scripts, so we probe the on* surface.
  for (const attr of ['onclick', 'onmousedown', 'onmouseup', 'ondblclick'] as const) {
    if (el.hasAttribute(attr) || typeof (el as unknown as Record<string, unknown>)[attr] === 'function') {
      return true;
    }
  }

  return false;
}

/**
 * Avoid double-indexing nested clickables. If a parent is already interactive
 * and the child doesn't materially change the interaction, skip the child.
 *
 * Example: <button><span>X</span></button> — only the button is indexed.
 *          <a href><img></a> — only the anchor.
 *          <li role=menuitem><button>X</button></li> — both, because the inner button
 *            has a different action target.
 */
export function isDistinctFromAncestor(el: Element): boolean {
  const tag = el.tagName.toLowerCase();
  // Explicit form controls always count even inside another interactive parent.
  if (['input', 'select', 'textarea', 'button'].includes(tag)) return true;

  let parent = el.parentElement;
  let hops = 0;
  while (parent && hops < 5) {
    if (isInteractiveCandidate(parent) && !isFrameworkRoot(parent)) {
      // Heuristic: if parent is itself a link/button and this child is a non-form descendant,
      // collapse to parent (don't index child).
      const ptag = parent.tagName.toLowerCase();
      if (ptag === 'a' || ptag === 'button' || parent.getAttribute('role') === 'button') {
        return false;
      }
    }
    parent = parent.parentElement;
    hops++;
  }
  return true;
}

/** True if element occupies any space on the page. */
export function isElementVisible(el: Element, rect?: DOMRect): boolean {
  const r = rect ?? cachedRect(el);
  if (r.width <= 0 || r.height <= 0) return false;
  const cs = cachedStyle(el);
  if (cs.visibility === 'hidden' || cs.display === 'none' || cs.opacity === '0') return false;
  return true;
}

/**
 * Occlusion test — is `el` the top-most element at (a sample of) its own box, i.e.
 * not covered by a modal / overlay / sticky bar? Ported from page-agent's
 * `isTopElement`: sample the centre plus two opposite corners and accept if any of
 * them hit `el` (or a descendant). `rect` must be the element's *local* viewport rect.
 *
 * - `viewportExpansion === -1` disables the check (full-page mode), matching page-agent.
 * - Same-origin iframe content is addressed in its own document, so it's treated as top.
 */
export function isTopElement(el: Element, rect: DOMRect, viewportExpansion: number): boolean {
  if (viewportExpansion === -1) return true;
  if (el.ownerDocument !== document) return true;
  if (rect.width <= 0 || rect.height <= 0) return false;

  const root = el.getRootNode();
  const inShadow = root instanceof ShadowRoot;
  const boundary: Node = inShadow ? root : document.documentElement;
  const margin = 5;
  const points = [
    { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 },
    { x: rect.left + margin, y: rect.top + margin },
    { x: rect.right - margin, y: rect.bottom - margin },
  ];

  return points.some(({ x, y }) => {
    let hit: Element | null;
    try {
      hit = inShadow
        ? ((root as ShadowRoot).elementFromPoint(x, y) as Element | null)
        : document.elementFromPoint(x, y);
    } catch {
      return true; // hit-test failed — don't over-filter
    }
    let cur: Element | null = hit;
    while (cur && cur !== boundary) {
      if (cur === el) return true;
      cur = cur.parentElement;
    }
    return false;
  });
}

/**
 * Is the element inside the viewport (optionally expanded by `expansion` px)?
 * `expansion = -1` means strict viewport only.
 * `expansion = 0` allows elements touching the edge.
 * Larger values include off-screen but nearby elements (so the LLM can scroll to them).
 */
export function isInExpandedViewport(rect: DOMRect, expansion: number): boolean {
  if (expansion < 0) {
    return (
      rect.bottom > 0 &&
      rect.top < window.innerHeight &&
      rect.right > 0 &&
      rect.left < window.innerWidth
    );
  }
  return !(
    rect.bottom < -expansion ||
    rect.top > window.innerHeight + expansion ||
    rect.right < -expansion ||
    rect.left > window.innerWidth + expansion
  );
}

/**
 * True if the element is an independently scrollable sub-container (not the page
 * itself). page-agent surfaces these so the agent can scroll a modal / dropdown /
 * pane in isolation via the `container_index` of a Scroll action — page-level
 * scrolling is already conveyed by the viewport's pages_above / pages_below.
 */
export function isScrollableContainer(el: Element, style?: CSSStyleDeclaration): boolean {
  if (el === document.body || el === document.documentElement) return false;
  const html = el as HTMLElement;
  // Ignore trivially small panes — they're rarely meaningful scroll targets.
  if (html.clientHeight < 40 && html.clientWidth < 40) return false;

  const cs = style ?? cachedStyle(el);
  const canY =
    (cs.overflowY === 'auto' || cs.overflowY === 'scroll') &&
    html.scrollHeight > html.clientHeight + 1;
  const canX =
    (cs.overflowX === 'auto' || cs.overflowX === 'scroll') &&
    html.scrollWidth > html.clientWidth + 1;
  return canY || canX;
}

/** Remaining scroll distance (px) on each side of a scrollable container. */
export function getScrollData(
  el: Element,
): { top: number; bottom: number; left: number; right: number } {
  const html = el as HTMLElement;
  return {
    top: Math.max(0, Math.round(html.scrollTop)),
    bottom: Math.max(0, Math.round(html.scrollHeight - html.clientHeight - html.scrollTop)),
    left: Math.max(0, Math.round(html.scrollLeft)),
    right: Math.max(0, Math.round(html.scrollWidth - html.clientWidth - html.scrollLeft)),
  };
}

/** Map a tag/attributes to an implicit ARIA role. */
export function implicitRole(el: Element): string {
  const tag = el.tagName.toLowerCase();
  const type = (el as HTMLInputElement).type;
  if (tag === 'a' && el.hasAttribute('href')) return 'link';
  if (tag === 'button') return 'button';
  if (tag === 'input') {
    if (type === 'checkbox') return 'checkbox';
    if (type === 'radio') return 'radio';
    if (type === 'submit' || type === 'button' || type === 'reset') return 'button';
    if (type === 'range') return 'slider';
    return 'textbox';
  }
  if (tag === 'select') return 'combobox';
  if (tag === 'textarea') return 'textbox';
  if (tag === 'img') return 'img';
  if (tag === 'label') return 'label';
  return '';
}
