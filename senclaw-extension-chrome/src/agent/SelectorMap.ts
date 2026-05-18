// SelectorMap: stable lookup from highlight index → live HTMLElement.
//
// The map lives on `window` of the content script (which Chrome isolates per
// frame and per extension), so it persists between GetSnapshot and the next
// Click/Type action even though our DaemonMessage handlers are stateless.
//
// We also track which elements have been seen at the current URL so the LLM
// can be told via `isNew: true` that an element is newly appearing. Persisted
// across snapshots via a WeakMap (auto-garbage-collected when the element
// detaches).

const KEY = '__senclawSelectorMap__';
const STATE: {
  map: Map<number, WeakRef<HTMLElement>>;
  seen: WeakMap<HTMLElement, string>; // element -> URL where first seen
  currentUrl: string;
} = (window as any)[KEY] || {
  map: new Map(),
  seen: new WeakMap(),
  currentUrl: '',
};
(window as any)[KEY] = STATE;

export function resetSelectorMap(): void {
  STATE.map.clear();
}

export function markUrl(url: string): void {
  if (STATE.currentUrl !== url) {
    // Different URL — old elements aren't useful for the "new" detector now.
    // (We don't clear `seen` because it's a WeakMap — old refs naturally drop.)
    STATE.currentUrl = url;
  }
}

/**
 * Register `el` under `index` for later lookup. Returns whether this element
 * is being seen for the first time at the current URL.
 */
export function putInSelectorMap(
  index: number,
  el: HTMLElement,
): { isNew: boolean } {
  STATE.map.set(index, new WeakRef(el));
  const prevUrl = STATE.seen.get(el);
  if (prevUrl === STATE.currentUrl) return { isNew: false };
  STATE.seen.set(el, STATE.currentUrl);
  return { isNew: prevUrl == null };
}

/** Resolve an index back to its element, or null if it's been collected. */
export function getElementByIndex(index: number): HTMLElement | null {
  const ref = STATE.map.get(index);
  if (!ref) return null;
  const el = ref.deref();
  if (!el) {
    STATE.map.delete(index);
    return null;
  }
  // Element might have detached from the DOM since snapshot
  if (!el.isConnected) return null;
  return el;
}

/** Clear all tracked state — used on page navigation. */
export function clearSeenElements(): void {
  STATE.seen = new WeakMap();
  STATE.map.clear();
  STATE.currentUrl = '';
}
