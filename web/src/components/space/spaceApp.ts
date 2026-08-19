/**
 * Shape of one installed Space App as the Apps screen renders it, plus the
 * two helpers the screen and its install dialog both need.
 *
 * Split out of `AppsGallery` when the install flows moved into their own
 * dialog: both files turn a `/api/space/apps` row into this shape, and a
 * second copy of `normalizeApp` would drift the moment the manifest grows a
 * field.
 */

export interface SpaceApp {
  id: string;
  name: string;
  description?: string;
  icon?: string;
  integration: { type: 'iframe' | 'esm'; url: string; launcher?: boolean };
  enabled: boolean;
  manifest?: any;
}

export interface SpaceAppRow {
  id: string;
  manifest: any;
  enabled: boolean;
}

export function normalizeApp(row: SpaceAppRow): SpaceApp {
  return {
    id: row.id,
    name: row.manifest?.name ?? row.id,
    description: row.manifest?.description,
    icon: row.manifest?.icon,
    integration: row.manifest?.integration ?? { type: 'iframe', url: row.manifest?.url ?? '#' },
    enabled: row.enabled,
    manifest: row.manifest,
  };
}

/**
 * Lowercase and strip Vietnamese diacritics so "kho" finds "Quản lý Kho" and
 * "du doan" finds "Siêu Dự Đoán". `đ` survives NFD decomposition — it is a
 * distinct letter, not `d` plus a combining mark — so it is replaced by hand,
 * the same rule the daemon's own search folding uses.
 */
export function fold(s: string): string {
  return s
    .toLowerCase()
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/đ/g, 'd');
}

/** Does this app match a launcher search query? An empty query matches everything. */
export function appMatches(
  app: { id?: string; name?: string; description?: string },
  query: string,
): boolean {
  const q = fold(query.trim());
  if (!q) return true;
  return fold(`${app.name ?? ''} ${app.description ?? ''} ${app.id ?? ''}`).includes(q);
}
