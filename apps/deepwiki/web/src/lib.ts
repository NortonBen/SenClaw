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
