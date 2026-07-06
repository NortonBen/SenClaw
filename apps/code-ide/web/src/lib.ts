// Small UI helpers shared across the IDE.

export function basename(path: string): string {
  return path.split('/').pop() ?? path;
}

export function dirname(path: string): string {
  const i = path.lastIndexOf('/');
  return i < 0 ? '' : path.slice(0, i);
}

/** A file-type glyph for the explorer/tabs (emoji — no icon font needed). */
export function fileIcon(name: string, isDir = false, open = false): string {
  if (isDir) return open ? '📂' : '📁';
  const ext = name.split('.').pop()?.toLowerCase() ?? '';
  switch (ext) {
    case 'rs': return '🦀';
    case 'ts': case 'tsx': return '🟦';
    case 'js': case 'jsx': case 'mjs': case 'cjs': return '🟨';
    case 'py': return '🐍';
    case 'go': return '🐹';
    case 'json': return '🔧';
    case 'md': case 'markdown': return '📝';
    case 'css': case 'scss': return '🎨';
    case 'html': return '🌐';
    case 'toml': case 'yaml': case 'yml': return '⚙️';
    case 'sh': case 'bash': return '📜';
    case 'lock': return '🔒';
    case 'png': case 'jpg': case 'jpeg': case 'gif': case 'svg': case 'webp': return '🖼️';
    default: return '📄';
  }
}

export function langFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  const map: Record<string, string> = {
    rs: 'rust', ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    mjs: 'javascript', cjs: 'javascript', py: 'python', go: 'go', java: 'java', c: 'c',
    h: 'c', cpp: 'cpp', cc: 'cpp', hpp: 'cpp', cs: 'csharp', rb: 'ruby', php: 'php',
    swift: 'swift', kt: 'kotlin', scala: 'scala', sh: 'shell', bash: 'shell', json: 'json',
    toml: 'toml', yaml: 'yaml', yml: 'yaml', md: 'markdown', html: 'html', css: 'css',
    scss: 'scss', sql: 'sql', xml: 'xml', dart: 'dart',
  };
  return map[ext] ?? 'plaintext';
}

/** Stable accent colour per symbol kind (for the symbol/function graph). */
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

/** Directory-stable accent colour for a file path (for the file graph). */
const DIR_PALETTE = ['#2563eb', '#0891b2', '#7c3aed', '#db2777', '#ea580c', '#16a34a', '#ca8a04', '#0d9488', '#9333ea'];
export function dirColor(path: string): string {
  const top = path.split('/').slice(0, -1).join('/') || path;
  let h = 0;
  for (let i = 0; i < top.length; i++) h = (h * 31 + top.charCodeAt(i)) >>> 0;
  return DIR_PALETTE[h % DIR_PALETTE.length];
}

export function timeAgo(unixSecs: number): string {
  if (!unixSecs) return '';
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
  if (diff < 60) return 'vừa xong';
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
