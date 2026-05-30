// Per-build memoization of layout reads — mirrors page-agent's getCached* helpers.
//
// Building the DOM tree touches each element's geometry and computed style several
// times (visibility, viewport, interactivity, scrollable and occlusion checks).
// Without caching, every one of those reads can force a synchronous reflow, which
// is the dominant cost on large pages. The DOM is not mutated during a single
// buildDomTree() pass, so cached values stay valid for that pass; resetLayoutCache()
// clears them at the start of the next one.

let rectCache = new WeakMap<Element, DOMRect>();
let styleCache = new WeakMap<Element, CSSStyleDeclaration>();
let clientRectsCache = new WeakMap<Element, DOMRectList>();

export function resetLayoutCache(): void {
  rectCache = new WeakMap();
  styleCache = new WeakMap();
  clientRectsCache = new WeakMap();
}

export function cachedRect(el: Element): DOMRect {
  let r = rectCache.get(el);
  if (!r) {
    r = el.getBoundingClientRect();
    rectCache.set(el, r);
  }
  return r;
}

/** Computed style, resolved through the element's own window so iframe content is correct. */
export function cachedStyle(el: Element): CSSStyleDeclaration {
  let s = styleCache.get(el);
  if (!s) {
    const view = el.ownerDocument?.defaultView ?? window;
    s = view.getComputedStyle(el);
    styleCache.set(el, s);
  }
  return s;
}

export function cachedClientRects(el: Element): DOMRectList {
  let r = clientRectsCache.get(el);
  if (!r) {
    r = el.getClientRects();
    clientRectsCache.set(el, r);
  }
  return r;
}
