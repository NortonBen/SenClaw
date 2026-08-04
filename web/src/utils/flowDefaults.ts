// Cached client-side view of the daemon's default-flow settings + widget
// catalog (Plugins → Widget). Link-click handlers must stay synchronous
// (popup blockers), so callers warm the cache with prefetch* and read the
// sync snapshot; before the first fetch resolves, behavior falls back to
// today's default (plain new-tab anchor).

import type { FlowDefaults, WidgetCatalogEntry } from '../types';

const FALLBACK: FlowDefaults = {
  openLink: 'system-browser',
  media: 'inline-widget',
  search: 'browser',
  searchEngine: 'google',
  note: 'space-notes',
  disabledWidgets: [],
};

let defaultsCache: FlowDefaults | null = null;
let defaultsInflight: Promise<FlowDefaults> | null = null;

export function prefetchFlowDefaults(): Promise<FlowDefaults> {
  if (defaultsCache) return Promise.resolve(defaultsCache);
  if (!defaultsInflight) {
    defaultsInflight = fetch('/api/defaults')
      .then((r) => (r.ok ? r.json() : FALLBACK))
      .then((d: FlowDefaults) => {
        // An old daemon answers unknown /api routes with the SPA index page —
        // guard on the shape, not just the status code.
        defaultsCache = d && typeof d.openLink === 'string' ? d : FALLBACK;
        return defaultsCache;
      })
      .catch(() => {
        defaultsCache = FALLBACK;
        return FALLBACK;
      });
  }
  return defaultsInflight;
}

/** Sync snapshot; null until the first prefetch resolves. */
export function flowDefaultsSync(): FlowDefaults | null {
  return defaultsCache;
}

/** Drop the cache (after a PUT /api/defaults from the settings panel). */
export function invalidateFlowDefaults(): void {
  defaultsCache = null;
  defaultsInflight = null;
}

/**
 * Click handler for chat links. Only the `mini-browser` default changes web
 * behavior — in a plain browser tab, "system-browser" IS the new tab, and the
 * desktop app intercepts externals natively. Returns true when handled.
 */
export function handleLinkPerDefaults(e: { preventDefault(): void }, href?: string): boolean {
  if (!href || !/^https?:\/\//i.test(href)) return false;
  const d = flowDefaultsSync();
  if (!d || d.openLink !== 'mini-browser') return false;
  e.preventDefault();
  // Same internal-route navigation the calendar `link` field uses.
  window.location.assign(`/space/app/mini-browser?url=${encodeURIComponent(href)}`);
  return true;
}

let catalogCache: WidgetCatalogEntry[] | null = null;
let catalogInflight: Promise<WidgetCatalogEntry[]> | null = null;

/** Widget catalog (for resolving fence-emitted `app` widgets with no entry). */
export function getWidgetCatalog(): Promise<WidgetCatalogEntry[]> {
  if (catalogCache) return Promise.resolve(catalogCache);
  if (!catalogInflight) {
    catalogInflight = fetch('/api/widgets')
      .then((r) => (r.ok ? r.json() : { widgets: [] }))
      .then((d: { widgets?: WidgetCatalogEntry[] }) => {
        catalogCache = Array.isArray(d?.widgets) ? d.widgets : [];
        return catalogCache;
      })
      .catch(() => {
        catalogCache = [];
        return [];
      });
  }
  return catalogInflight;
}

export function invalidateWidgetCatalog(): void {
  catalogCache = null;
  catalogInflight = null;
}
