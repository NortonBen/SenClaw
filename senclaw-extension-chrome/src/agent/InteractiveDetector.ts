// Interactive element detection — layered signals borrowed from alibaba/page-agent + browser-use.
//
// An element is "interactive" if ANY of these are true (and it isn't disabled/inert):
//   1. Semantic tag (a, button, input, select, textarea, label, details, summary)
//   2. ARIA role from a known interactive set
//   3. Computed cursor in {pointer, move, text, grab, grabbing, cell, copy}
//   4. Has direct click/mousedown/mouseup/dblclick event listener (when devtools API available)
//
// The expensive checks (computed style, event listeners) are gated by a cheap candidate filter.

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

const INTERACTIVE_CURSORS = new Set([
  'pointer', 'move', 'text', 'grab', 'grabbing', 'cell', 'copy',
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

  return false;
}

/** Is the element disabled / inert / readonly? */
export function isElementDisabled(el: Element): boolean {
  if ((el as HTMLButtonElement).disabled) return true;
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

  if (isInteractiveCandidate(el)) return true;

  const cs = style ?? window.getComputedStyle(el);
  if (INTERACTIVE_CURSORS.has(cs.cursor)) return true;

  // Inline event listener probe (only works in some contexts; cheap when not present)
  // chrome.devtools.getEventListeners would be more thorough but isn't available in content scripts.
  if ((el as any).onclick != null) return true;

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
  const r = rect ?? el.getBoundingClientRect();
  if (r.width <= 0 || r.height <= 0) return false;
  const cs = window.getComputedStyle(el);
  if (cs.visibility === 'hidden' || cs.display === 'none' || cs.opacity === '0') return false;
  return true;
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
