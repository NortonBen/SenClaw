// The UI is served both directly (:4570) and under the daemon proxy
// (/api/space/apps/zeach/proxy/), so every call must be relative to the page.
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
  /** core = có sẵn trong app; optional = đến từ app/MCP đã cài. */
  tier: 'core' | 'optional'
  origin: 'builtin' | 'preset' | 'discovered' | 'user'
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
  /** Absent on persisted history rows — derive from `tier`. */
  tier_label?: string
  confidence: number
  independent_count: number
  agreement: number
  high_stakes: boolean
  supports: string[]
  refutes: string[]
  /** Absent on persisted history rows. */
  dropped_citations?: string[]
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

export type Depth = 'quick' | 'standard' | 'deep'

export interface SavedRef {
  target: string
  status?: string
  report_id?: string
  version?: number
  detail?: string
}

/** Verdict of the post-synthesis checkpoint (`review::review_report`). */
export interface ReportReview {
  answers: boolean
  score: number
  issues: string[]
  missing: string[]
  /** False when the reviewer itself could not run — `answers` is then a default. */
  used_llm: boolean
}

export interface ResearchOutcome {
  query: string
  depth: Depth
  sub_queries: string[]
  evidence: Evidence[]
  /** Retrieved but judged off topic — never used to derive a claim. */
  off_topic?: Evidence[]
  /** ok | off_topic (report does not answer the question) | insufficient. */
  status?: 'ok' | 'off_topic' | 'insufficient'
  review?: ReportReview | null
  sources: SourceOutcome[]
  unknown_sources: string[]
  claims: Claim[]
  contradictions: Contradiction[]
  report_title: string
  report_markdown: string
  report_llm: boolean
  confidence_note: string
  rounds: number
  total_before_dedupe: number
  deepened: number
  warnings: string[]
  ms: number
  run_id?: string | null
  saved?: SavedRef[]
}

export interface ReportSummary {
  run_id: string
  title: string
  version: number
  created_at: string
  query: string
}

/** A saved report read back, with its run (evidence) + verified claims folded in. */
export interface ReportDetail {
  id: string
  run_id: string
  version: number
  title: string
  body_md: string
  created_at: string
  query: string
  run: SearchOutcome & { id: string }
  claims: Claim[]
  contradictions: Contradiction[]
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
  verify_level?: string
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
  research: (body: {
    query: string
    depth?: Depth
    sources?: string[]
    lang?: string
    max_evidence?: number
    save_wiki?: boolean
    save_knowledge?: boolean
  }) => req<ResearchOutcome>('/research', { method: 'POST', body: JSON.stringify(body) }),
  reports: (limit = 50) => req<{ reports: ReportSummary[] }>(`/reports?limit=${limit}`),
  report: (id: string) => req<ReportDetail>(`/reports/${encodeURIComponent(id)}`),
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
  runs: (limit = 50) => req<{ runs: RunSummary[] }>(`/runs?limit=${limit}`),
  run: (id: string) => req<SearchOutcome & { id: string }>(`/runs/${encodeURIComponent(id)}`),
  deleteRun: (id: string) =>
    req<{ ok: boolean }>(`/runs/${encodeURIComponent(id)}`, { method: 'DELETE' }),
}
