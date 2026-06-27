// Small shared helpers for the code/graph views.

export function langFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  switch (ext) {
    case 'rs': return 'rust';
    case 'py':
    case 'pyi': return 'python';
    case 'ts': return 'typescript';
    case 'tsx': return 'tsx';
    case 'js':
    case 'jsx':
    case 'mjs':
    case 'cjs': return 'javascript';
    case 'go': return 'go';
    default: return 'text';
  }
}

/** A stable accent colour per symbol kind (shared by cards, tags, and the graph). */
export function kindColor(kind: string): string {
  switch (kind) {
    case 'function':
    case 'method': return '#2563eb';
    case 'class':
    case 'struct': return '#7c3aed';
    case 'trait':
    case 'interface': return '#0891b2';
    case 'enum': return '#ca8a04';
    case 'type': return '#16a34a';
    case 'const':
    case 'macro': return '#ea580c';
    case 'module':
    case 'impl': return '#64748b';
    default: return '#64748b';
  }
}

export function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + '…' : s;
}

/** A directory-stable accent colour for a file path (for graph node colouring). */
const DIR_PALETTE = ['#2563eb', '#0891b2', '#7c3aed', '#db2777', '#ea580c', '#16a34a', '#ca8a04', '#0d9488', '#9333ea'];
export function dirColor(path: string): string {
  const top = path.split('/').slice(0, -1).join('/') || path;
  let h = 0;
  for (let i = 0; i < top.length; i++) h = (h * 31 + top.charCodeAt(i)) >>> 0;
  return DIR_PALETTE[h % DIR_PALETTE.length];
}

/**
 * Deterministic Fruchterman–Reingold force layout with centre gravity.
 * Computed once (no animation); returns a node-id → position map.
 */
export function forceLayout(
  ids: string[],
  edges: { from: string; to: string }[],
  W = 1200,
  H = 760,
): Map<string, { x: number; y: number }> {
  const N = ids.length;
  if (N === 0) return new Map();
  const pos = ids.map((_, i) => ({
    x: W / 2 + Math.cos((i / N) * 2 * Math.PI) * Math.min(W, H) * 0.36 + (i % 5) * 3,
    y: H / 2 + Math.sin((i / N) * 2 * Math.PI) * Math.min(W, H) * 0.36 + (i % 3) * 3,
  }));
  const idx = new Map(ids.map((id, i) => [id, i]));
  const k = Math.sqrt((W * H) / Math.max(N, 1)) * 0.95;
  const GRAV = 0.018;
  const ITER = 420;
  for (let it = 0; it < ITER; it++) {
    const temp = (1 - it / ITER) * (W * 0.06);
    const disp = pos.map(() => ({ x: 0, y: 0 }));
    for (let i = 0; i < N; i++) {
      for (let j = i + 1; j < N; j++) {
        const dx = pos[i].x - pos[j].x, dy = pos[i].y - pos[j].y;
        const d = Math.hypot(dx, dy) || 0.01;
        const f = (k * k) / d, ux = dx / d, uy = dy / d;
        disp[i].x += ux * f; disp[i].y += uy * f;
        disp[j].x -= ux * f; disp[j].y -= uy * f;
      }
    }
    for (const e of edges) {
      const a = idx.get(e.from), b = idx.get(e.to);
      if (a == null || b == null) continue;
      const dx = pos[a].x - pos[b].x, dy = pos[a].y - pos[b].y;
      const d = Math.hypot(dx, dy) || 0.01;
      const f = (d * d) / k, ux = dx / d, uy = dy / d;
      disp[a].x -= ux * f; disp[a].y -= uy * f;
      disp[b].x += ux * f; disp[b].y += uy * f;
    }
    for (let i = 0; i < N; i++) {
      disp[i].x += (W / 2 - pos[i].x) * GRAV * k;
      disp[i].y += (H / 2 - pos[i].y) * GRAV * k;
    }
    for (let i = 0; i < N; i++) {
      const dl = Math.hypot(disp[i].x, disp[i].y) || 0.01;
      pos[i].x += (disp[i].x / dl) * Math.min(dl, temp);
      pos[i].y += (disp[i].y / dl) * Math.min(dl, temp);
      pos[i].x = Math.max(40, Math.min(W - 40, pos[i].x));
      pos[i].y = Math.max(40, Math.min(H - 40, pos[i].y));
    }
  }
  return new Map(ids.map((id, i) => [id, pos[i]]));
}

/** Compact relative time, e.g. "vừa xong", "5m", "3h", "2d". */
export function timeAgo(unixSecs: number): string {
  if (!unixSecs) return '';
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
  if (diff < 60) return 'vừa xong';
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
