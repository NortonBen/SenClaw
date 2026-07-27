// The UI is served both directly (:4530) and under the daemon proxy
// (/api/space/apps/search/proxy/), so every call must be relative to the page.
const base = new URL('.', window.location.href).pathname.replace(/\/$/, '')

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${base}/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...init,
  })
  const text = await res.text()
  let body: unknown
  try {
    body = JSON.parse(text)
  } catch {
    // An unstyled HTML body here means the SPA fallback answered — i.e. the
    // route does not exist on this build.
    throw new Error(`${path}: phản hồi không phải JSON (HTTP ${res.status})`)
  }
  if (!res.ok) {
    const msg = (body as { error?: string })?.error ?? `HTTP ${res.status}`
    throw new Error(msg)
  }
  return body as T
}

export type Health =
  | { state: 'ready' }
  | { state: 'degraded'; reason: string }
  | { state: 'unavailable'; reason: string }

export interface SourceInfo {
  id: string
  label: string
  kind: string
  enabled: boolean
  weight: number
  max_results: number
  timeout_ms: number
  health: Health
}

export interface SourceHit {
  source_id: string
  kind: string
  rank: number
  raw_score: number
}

export interface Evidence {
  id: string
  title: string
  url?: string
  canonical_url?: string
  domain?: string
  snippet: string
  full_text?: string
  published_at?: number
  retrieved_at: number
  hits: SourceHit[]
  fused_score: number
  independent_kinds: number
  meta?: unknown
}

export interface SourceOutcome {
  source_id: string
  sub_query: string
  status: 'ok' | 'timeout' | 'error' | 'skipped'
  item_count: number
  dropped_count: number
  ms: number
  error?: string
}

export type Tier = 'verified' | 'supported' | 'single-source' | 'disputed' | 'unverified'

export interface Claim {
  id: string
  text: string
  tier: Tier
  tier_label: string
  confidence: number
  independent_count: number
  agreement: number
  high_stakes: boolean
  supports: string[]
  refutes: string[]
  dropped_citations: string[]
}

export interface Contradiction {
  id: string
  claim_a: string
  claim_b: string
  summary: string
}

export interface SearchOutcome {
  query: string
  evidence: Evidence[]
  sources: SourceOutcome[]
  unknown_sources: string[]
  total_before_dedupe: number
  deepened: number
  ms: number
  run_id?: string | null
  // Present only for /ask.
  claims?: Claim[]
  contradictions?: Contradiction[]
  confidence_note?: string
  /** Evidence was found but claim extraction failed. */
  claims_error?: string
  /** There was no evidence to extract claims from. */
  claims_note?: string
}

export interface CorpusDoc {
  id: string
  name: string
  mime: string
  bytes: number
  status: string
  uploaded_at: string
  chunks: number
}

export interface UploadResult {
  added: { name: string; chunks?: number; note?: string; message?: string; duplicate?: boolean }[]
  failed: { name: string; error: string }[]
}

export interface RunSummary {
  id: string
  query: string
  status: string
  evidence_count: number
  total_before_dedupe: number
  ms: number
  created_at: string
}

export interface SourceTemplate {
  id: string
  label: string
  app_id: string
  tool: string
  why: string
  required_args: { name: string; hint: string }[]
}

export interface SyncReport {
  id: string
  registered: boolean
  reason: string
}

export interface McpToolInfo {
  name: string
  description?: string
  inputSchema?: { properties?: Record<string, unknown>; required?: string[] }
}

export interface NewMcpSource {
  id: string
  label?: string
  app_id?: string
  rpc_url?: string
  tool: string
  query_arg?: string
  limit_arg?: string
  kind?: string
  weight?: number
  extra_args?: Record<string, unknown>
  map?: Record<string, string>
}

export const api = {
  sources: () => req<{ sources: SourceInfo[] }>('/sources'),
  templates: () => req<{ templates: SourceTemplate[] }>('/source-templates'),
  sync: () => req<{ sources: SyncReport[] }>('/sync', { method: 'POST' }),
  mcpTools: (target: { app_id?: string; rpc_url?: string }) => {
    const qs = new URLSearchParams(
      Object.entries(target).filter(([, v]) => v) as [string, string][],
    )
    return req<{ rpc_url: string; tools: McpToolInfo[] }>(`/mcp-tools?${qs}`)
  },
  addSource: (body: NewMcpSource) =>
    req<{ ok: boolean; source: string; health: Health }>('/sources/mcp', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  removeSource: (id: string) =>
    req<{ ok: boolean }>(`/sources/mcp/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  search: (body: {
    query: string
    sources?: string[]
    limit?: number
    depth?: number
    lang?: string
  }) => req<SearchOutcome>('/search', { method: 'POST', body: JSON.stringify(body) }),
  ask: (body: { query: string; sources?: string[]; limit?: number; depth?: number }) =>
    req<SearchOutcome>('/ask', { method: 'POST', body: JSON.stringify(body) }),
  setSource: (id: string, patch: Partial<Pick<SourceInfo, 'enabled' | 'weight'>>) =>
    req<{ ok: boolean }>(`/sources/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(patch),
    }),
  corpus: () => req<{ documents: CorpusDoc[] }>('/corpus'),
  removeDoc: (id: string) =>
    req<{ ok: boolean }>(`/corpus/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  uploadDocs: async (files: FileList) => {
    const form = new FormData()
    for (const f of Array.from(files)) form.append('file', f)
    // No content-type header: the browser must set the multipart boundary.
    const res = await fetch(`${base}/api/corpus`, { method: 'POST', body: form })
    const body = (await res.json()) as UploadResult
    if (!res.ok && !body.added?.length) {
      throw new Error(body.failed?.[0]?.error ?? `HTTP ${res.status}`)
    }
    return body
  },
  runs: () => req<{ runs: RunSummary[] }>('/runs?limit=30'),
  run: (id: string) => req<SearchOutcome & { id: string }>(`/runs/${encodeURIComponent(id)}`),
}
