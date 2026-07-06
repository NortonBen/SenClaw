// Typed fetch client for the DeepWiki App's own backend (/api/*).

export interface Sym {
  id: number;
  file_id: number;
  path: string;
  name: string;
  kind: string;
  parent: string | null;
  start_line: number;
  end_line: number;
  signature: string;
  doc?: string | null;
}

export interface CallLink {
  name: string;
  path: string;
  start_line: number;
  kind: string;
}

export interface Edge {
  kind: string;
  src_path: string;
  src_symbol: string | null;
  target: string;
  line: number;
}

export interface FileRec {
  id: number;
  path: string;
  lang: string;
  hash: string;
  mtime: number;
  loc: number;
}

export interface Stats {
  files: number;
  symbols: number;
  edges: number;
  by_lang: [string, number][];
  last_indexed: number;
}

export interface Status {
  root: string | null;
  stats: Stats;
  pages: number;
  exclude?: string[];
}

export interface RootInfo {
  path: string;
  last_indexed: number;
  files: number;
  symbols: number;
}

export interface IndexReport {
  root: string;
  scanned: number;
  indexed: number;
  skipped: number;
  removed: number;
  excluded: number;
  symbols: number;
  edges: number;
  errors: string[];
}

export interface WikiPage {
  slug: string;
  title: string;
  parent: string | null;
  content: string;
  ord: number;
  updated_at: number;
}

export interface Outline {
  root: string | null;
  stats: Stats;
  directories: { name: string; files: number }[];
  top_files: { path: string; lang: string; symbols: number }[];
  architectural_types: { name: string; kind: string; path: string }[];
  hot_symbols: { name: string; called: number }[];
}

export interface Exploration {
  query: string;
  matches: Sym[];
  callers: CallLink[];
  callees: CallLink[];
  blast_radius: CallLink[];
}

export interface SymbolResult {
  name: string;
  definitions: Sym[];
  callers: CallLink[];
  callees: CallLink[];
}

export interface GraphNode {
  id: string;
  kind: string;
  path: string;
  line: number;
  depth: number;
  external: boolean;
}
export interface GraphEdge {
  from: string;
  to: string;
}
export interface InvestigationGraph {
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface FileNode {
  path: string;
  lang: string;
  loc: number;
  symbols: number;
}
export interface FileEdge {
  from: string;
  to: string;
  weight: number;
}
export interface FileGraphData {
  nodes: FileNode[];
  edges: FileEdge[];
}

export interface SymGraphNode {
  name: string;
  kind: string;
  path: string;
  line: number;
}
export interface SymbolGraphData {
  nodes: SymGraphNode[];
  edges: FileEdge[];
}

export interface Investigation {
  query: string;
  focus: string | null;
  matches: Sym[];
  callers: CallLink[];
  callees: CallLink[];
  nodes: GraphNode[];
  edges: GraphEdge[];
}

export interface AskResult {
  id?: number | null;
  question: string;
  answer: string;
  model: string | null;
  focus: string | null;
  matches: Sym[];
  graph: InvestigationGraph;
  created_at?: number;
}

export interface Settings {
  defaultExcludes: string[];
  customExcludes: string[];
  minifiedMaxLine: number;
  factoryExcludes: string[];
}

export interface LlmInfo {
  ok: boolean;
  daemon: string;
  tier?: string;
  model?: string | null;
  provider?: string | null;
  error?: string;
}

export interface AskHistoryItem {
  id: number;
  question: string;
  model: string | null;
  focus: string | null;
  created_at: number;
}

export interface ContextResult {
  query: string;
  matches: Sym[];
  callers: CallLink[];
  callees: CallLink[];
  files: { path: string; imports: Edge[]; symbols: Sym[] }[];
  instruction: string;
}

// Vendored into SenClaw Code: DeepWiki's backend is nested under /api/deepwiki
// (the IDE serves its own API at /api), so rewrite the leading /api segment.
async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const url = path.replace(/^\/api\b/, '/api/deepwiki');
  const res = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    let msg = text;
    try {
      const j = JSON.parse(text);
      msg = j.error ?? text;
    } catch { /* keep raw text */ }
    throw new Error(msg || `${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

function qs(params: Record<string, string | number | undefined>): string {
  const s = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== undefined && v !== '') s.set(k, String(v));
  }
  return s.toString();
}

export const api = {
  // index + status
  status: () => apiFetch<Status>('/api/status'),
  recents: () => apiFetch<RootInfo[]>('/api/recents'),
  getSettings: () => apiFetch<Settings>('/api/settings'),
  saveSettings: (s: Partial<Pick<Settings, 'defaultExcludes' | 'customExcludes' | 'minifiedMaxLine'>>) =>
    apiFetch<{ success: boolean }>('/api/settings', { method: 'POST', body: JSON.stringify(s) }),
  llmInfo: () => apiFetch<LlmInfo>('/api/llm-info'),
  index: (path: string, exclude?: string[]) =>
    apiFetch<IndexReport>('/api/index', { method: 'POST', body: JSON.stringify({ path, exclude }) }),
  investigate: (q: string, depth = 2) => apiFetch<Investigation>(`/api/investigate?${qs({ q, depth })}`),
  fileGraph: () => apiFetch<FileGraphData>('/api/file-graph'),
  symbolGraph: () => apiFetch<SymbolGraphData>('/api/symbol-graph'),

  // wiki
  outline: () => apiFetch<Outline>('/api/outline'),
  pages: () => apiFetch<WikiPage[]>('/api/pages'),
  getPage: (slug: string) => apiFetch<WikiPage>(`/api/page?${qs({ slug })}`),
  savePage: (p: { slug: string; title: string; content: string; parent?: string; ord?: number }) =>
    apiFetch<{ success: boolean; slug: string }>('/api/page', { method: 'POST', body: JSON.stringify(p) }),
  deletePage: (slug: string) =>
    apiFetch<{ success: boolean }>(`/api/page?${qs({ slug })}`, { method: 'DELETE' }),
  context: (q: string, depth = 4) => apiFetch<ContextResult>(`/api/context?${qs({ q, depth })}`),
  generateWiki: () => apiFetch<{ created: string[]; errors: string[] }>('/api/generate-wiki', { method: 'POST' }),
  ask: (q: string) => apiFetch<AskResult>('/api/ask', { method: 'POST', body: JSON.stringify({ q }) }),
  askHistory: () => apiFetch<AskHistoryItem[]>('/api/ask-history'),
  getAsk: (id: number) => apiFetch<AskResult>(`/api/ask-history/${id}`),
  deleteAsk: (id: number) => apiFetch<{ success: boolean }>(`/api/ask-history/${id}`, { method: 'DELETE' }),

  // code graph
  search: (q: string, limit = 40, path?: string) => apiFetch<Sym[]>(`/api/search?${qs({ q, limit, path })}`),
  symbol: (name: string) => apiFetch<SymbolResult>(`/api/symbol?${qs({ name })}`),
  explore: (q: string, depth = 4) => apiFetch<Exploration>(`/api/explore?${qs({ q, depth })}`),
  files: () => apiFetch<FileRec[]>('/api/files'),
  fileOutline: (path: string) =>
    apiFetch<{ path: string; outline: Sym[]; imports: Edge[] }>(`/api/file?${qs({ path })}`),
  snippet: (args: { name?: string; path?: string; start?: number; end?: number; context?: number }) =>
    apiFetch<{ path: string; start_line: number; end_line: number; code: string; name?: string; kind?: string; signature?: string }>(
      `/api/snippet?${qs(args as Record<string, string | number | undefined>)}`,
    ),
};
