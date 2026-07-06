// Typed fetch client for the SenClaw Code backend (/api/*).

export interface TreeEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

export interface FileContent {
  path: string;
  content: string;
  lang: string;
  too_large: boolean;
  binary: boolean;
  size: number;
}

export interface SearchHit {
  path: string;
  line: number;
  text: string;
}

export interface Recent {
  path: string;
  name: string;
  openedAt: number;
}

export interface Pin {
  path: string;
  start_line: number;
  end_line: number;
  code: string;
  lang?: string;
}

export interface PlanStep {
  title: string;
  status: 'pending' | 'running' | 'done' | 'error';
  result?: string;
}

export interface ChatMsg {
  role: 'user' | 'assistant';
  content: string;
  /** Wall-clock seconds the response took (assistant only; UI-only field). */
  ms?: number;
  /** Devin-style plan steps parsed from a Plan-mode response (UI-only). */
  steps?: PlanStep[];
  /** True while the plan is being executed step-by-step. */
  executing?: boolean;
}

export interface ModelInfo {
  id: string;
  modelName: string | null;
  provider: string | null;
}

export type RunMode = 'chat' | 'plan' | 'agent' | 'dag';

export interface GitCommit { hash: string; parents: string[]; author: string; time: number; refs: string[]; subject: string }

/** A saved chat conversation (localStorage, UI-only). */
export interface Conversation { id: string; title: string; messages: ChatMsg[]; at: number }

// ---- DeepWiki (in-process) ----
export interface DwStatus {
  root: string | null;
  stats: { files?: number; symbols?: number; edges?: number } | null;
  pages: number;
}
export interface DwIndexReport { indexed: number; symbols: number; edges: number; root: string }
export interface DwPage { slug: string; title: string; parent: string | null; content?: string; ord?: number }
export interface DwFile { path: string; lang: string; loc: number }
export interface DwSym { name: string; path: string; kind: string; start_line: number; signature?: string }
export interface DwMatch { name: string; path: string; start_line: number; kind: string }
export interface DwAsk { id?: number; question: string; answer: string; model: string | null; matches: DwMatch[] }
export interface DwAskHistItem { id: number; question: string; created_at?: number }
export interface DwGraphNode { id: string; depth: number; external?: boolean }
export interface DwGraphEdge { from: string; to: string }
export interface DwInvestigation {
  focus: string | null;
  matches: DwMatch[];
  nodes: DwGraphNode[];
  edges: DwGraphEdge[];
}

export interface DwFileNode { path: string; lang: string; loc: number; symbols: number }
export interface DwSymNode { name: string; kind: string; path: string; line: number }
export interface DwEdge { from: string; to: string; weight: number }

/** A saved Codemap — a Devin-style deep exploration from a starting point. */
export interface Codemap {
  id: string;
  start: string;
  title: string;
  narrative: string;
  matches: DwMatch[];
  nodes: DwGraphNode[];
  focus: string | null;
  at: number;
}

async function req<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers ?? {}) },
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((data as { error?: string }).error ?? `HTTP ${res.status}`);
  return data as T;
}

export const api = {
  status: () => req<{ root: string | null; name: string | null; hasRoot: boolean }>('/api/status'),
  llmInfo: () => req<{ ok: boolean; model?: string | null; error?: string }>('/api/llm-info'),
  recents: () => req<Recent[]>('/api/recents'),
  browse: (path?: string) =>
    req<{ path: string; parent: string | null; dirs: { name: string; path: string }[] }>(
      `/api/browse${path ? `?path=${encodeURIComponent(path)}` : ''}`,
    ),
  open: (path: string) =>
    req<{ root: string; name: string; tree: TreeEntry[] }>('/api/open', {
      method: 'POST',
      body: JSON.stringify({ path }),
    }),
  tree: (path: string) => req<TreeEntry[]>(`/api/tree?path=${encodeURIComponent(path)}`),
  file: (path: string) => req<FileContent>(`/api/file?path=${encodeURIComponent(path)}`),
  files: () => req<string[]>('/api/files'),
  save: (path: string, content: string) =>
    req<{ success: boolean }>('/api/save', { method: 'POST', body: JSON.stringify({ path, content }) }),
  create: (path: string, dir: boolean) =>
    req<{ success: boolean }>('/api/create', { method: 'POST', body: JSON.stringify({ path, dir }) }),
  rename: (from: string, to: string) =>
    req<{ success: boolean }>('/api/rename', { method: 'POST', body: JSON.stringify({ from, to }) }),
  remove: (path: string) =>
    req<{ success: boolean }>('/api/delete', { method: 'POST', body: JSON.stringify({ path }) }),
  search: (q: string, limit = 100) =>
    req<SearchHit[]>(`/api/search?q=${encodeURIComponent(q)}&limit=${limit}`),
  gitStatus: () => req<{ files: Record<string, string> }>('/api/git-status'),
  git: {
    status: () => req<{ files: Record<string, string> }>('/api/git-status'),
    head: () => req<{ branch: string }>('/api/git/head'),
    filediff: (path: string, staged = false) =>
      req<{ path: string; original: string; modified: string }>(`/api/git/filediff?path=${encodeURIComponent(path)}&staged=${staged}`),
    stage: (paths: string[]) => req<{ success: boolean }>('/api/git/stage', { method: 'POST', body: JSON.stringify({ paths }) }),
    unstage: (paths: string[]) => req<{ success: boolean }>('/api/git/unstage', { method: 'POST', body: JSON.stringify({ paths }) }),
    discard: (paths: string[]) => req<{ success: boolean }>('/api/git/discard', { method: 'POST', body: JSON.stringify({ paths }) }),
    commit: (message: string) => req<{ success: boolean; output: string }>('/api/git/commit', { method: 'POST', body: JSON.stringify({ message }) }),
    log: (limit = 120) => req<{ commits: GitCommit[] }>(`/api/git/log?limit=${limit}`),
  },
  chat: (messages: ChatMsg[], pins: Pin[], activeFile: string | null, mode: RunMode = 'chat') =>
    req<{ text: string; model: string }>('/api/chat', {
      method: 'POST',
      body: JSON.stringify({ messages, pins, active_file: activeFile, mode }),
    }),
  models: () => req<{ activeId: string | null; configs: ModelInfo[] }>('/api/models'),
  // Fire-and-forget: index the given folder into the in-process DeepWiki.
  deepwikiIndex: (path: string) =>
    fetch('/api/deepwiki/index', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path }),
    }).catch(() => {}),

  // In-process DeepWiki (code intelligence + wiki), rendered natively.
  dw: {
    status: () => req<DwStatus>('/api/deepwiki/status'),
    index: (path: string) =>
      req<DwIndexReport>('/api/deepwiki/index', { method: 'POST', body: JSON.stringify({ path }) }),
    pages: () => req<DwPage[]>('/api/deepwiki/pages'),
    page: (slug: string) => req<DwPage>(`/api/deepwiki/page?slug=${encodeURIComponent(slug)}`),
    deletePage: (slug: string) =>
      req<{ success: boolean }>(`/api/deepwiki/page?slug=${encodeURIComponent(slug)}`, { method: 'DELETE' }),
    generateWiki: (instructions?: string) =>
      req<{ created: string[]; errors: string[] }>('/api/deepwiki/generate-wiki', {
        method: 'POST',
        body: JSON.stringify({ instructions: instructions ?? '' }),
      }),
    files: () => req<DwFile[]>('/api/deepwiki/files'),
    search: (q: string) => req<DwSym[]>(`/api/deepwiki/search?q=${encodeURIComponent(q)}&limit=40`),
    ask: (q: string) => req<DwAsk>('/api/deepwiki/ask', { method: 'POST', body: JSON.stringify({ q }) }),
    askHistory: () => req<DwAskHistItem[]>('/api/deepwiki/ask-history'),
    deleteAsk: (id: number) => req<{ success: boolean }>(`/api/deepwiki/ask-history/${id}`, { method: 'DELETE' }),
    fileOutline: (path: string) =>
      req<{ path: string; outline: DwSym[]; imports: unknown[] }>(`/api/deepwiki/file?path=${encodeURIComponent(path)}`),
    // Multi-hop call-graph investigation from a starting point (powers Codemap).
    investigate: (q: string, depth = 2) =>
      req<DwInvestigation>(`/api/deepwiki/investigate?q=${encodeURIComponent(q)}&depth=${depth}`),
    fileGraph: () => req<{ nodes: DwFileNode[]; edges: DwEdge[] }>('/api/deepwiki/file-graph'),
    symbolGraph: () => req<{ nodes: DwSymNode[]; edges: DwEdge[] }>('/api/deepwiki/symbol-graph'),
    snippet: (name: string) =>
      req<{ code?: string; path?: string; start_line?: number; kind?: string }>(`/api/deepwiki/snippet?name=${encodeURIComponent(name)}&context=2`),
  },
  setModel: (id: string) =>
    req<{ success: boolean; activeId: string }>('/api/model-active', {
      method: 'POST',
      body: JSON.stringify({ id }),
    }),
};
