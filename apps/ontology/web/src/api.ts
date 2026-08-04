// Typed fetch helpers for the SenClaw Ontology backend.

export type Project = {
  id: number
  name: string
  description: string
  baseIri: string
  prefixes: Record<string, string>
  createdAt: number
  updatedAt: number
  tripleCount: number
}

export type Column = {
  name: string
  datatype: string
  xsdDatatype: string
  nullRatio: number
  distinctCount: number
  isUnique: boolean
  isEnum: boolean
  role: string
  samples: string[]
}

export type Source = {
  id: number
  name: string
  /** Storage kind the pipeline understands: csv | json | text. */
  kind: string
  /** Format the file actually arrived in (xlsx, pdf, jsonl, …). */
  origin: string
  /** One line on what the ingest sniffer did. */
  note: string
  columns: Column[]
  rowCount: number
  createdAt: number
}

export type IngestedSource = {
  id: number
  name: string
  kind: string
  origin: string
  note: string
  rowCount: number
  columns: Column[]
}

export type AutoStep = {
  key: string
  label: string
  status: 'pending' | 'running' | 'ok' | 'warn' | 'error' | 'skipped'
  detail: string
}

export type AutoJob = {
  id: string
  projectId: number
  steps: AutoStep[]
  done: boolean
  error: string | null
  result: {
    sources?: unknown[]
    tbox?: { classes: number; properties: number }
    lift?: { triples: number; subjects: number; skippedRows: number; batch: string }
    repairs?: string[]
    extracted?: number
    validation?: { conforms: boolean; violationCount: number }
    competency?: { total: number; passed: number }
    reason?: { inferred: number }
    tripleCount?: number
  }
  startedAt: number
  updatedAt: number
}

export type AskResult = {
  question: string
  sparql: string
  repaired: string | null
  head: string[]
  rows: SparqlResult['rows']
  boolean?: boolean
  count: number
  answer: string
  model: string
}

/** Where the user is when they ask AIP Assist — this is what re-ranks retrieval. */
export type AssistContext = { tab?: string; source?: string; selection?: string }

export type AssistCitation = {
  n: number
  id: string
  kind: string
  title: string
  tab: string | null
  reason: string
  score: number
}

export type AssistResult = {
  question: string
  answer: string
  citations: AssistCitation[]
  context: string
  /** True when the question wants values, which Assist deliberately cannot see. */
  dataQuestion: boolean
  model: string
}

// ---- AIP Logic: typed-action functions + proposal queue --------------------

export type LogicFunction = {
  id: number
  name: string
  kind: 'extract' | 'classify' | 'resolve'
  inputKind: string
  target: string
  instruction: string
  autoApply: boolean
  createdAt: number
}

export type RunReport = {
  proposed: number
  invalid: number
  applied: number
  skippedInputs: number
  preview: Array<{
    action: unknown
    summary: string
    valid: boolean
    invalidReason: string
    confidence: number
    rationale: string
  }>
  errors: string[]
  batch: string
}

export type Proposal = {
  id: number
  functionId: number | null
  action: Record<string, unknown>
  summary: string
  rationale: string
  confidence: number
  status: 'pending' | 'approved' | 'rejected' | 'invalid'
  valid: boolean
  invalidReason: string
  batch: string
  createdAt: number
}

export type LiveSchema = {
  classes: Array<{ class: string; count?: string }>
  predicates: Array<{ predicate: string; count?: string; example?: string }>
}

export type TboxClass = { iri: string; label?: string; super?: string }
export type TboxProp = { iri: string; kind?: string; label?: string; domain?: string; range?: string }
export type Tbox = { classes: TboxClass[]; properties: TboxProp[] }

export type CompetencyQuestion = { id: number; question: string; sparql: string; expect: string }
export type Batch = {
  iri: string
  label: string
  source: string
  activity: string
  generatedAt: string
  tripleCount: number
}

export type SparqlResult = {
  head?: string[]
  rows?: Array<Record<string, { type: string; value: string; datatype?: string; lang?: string }>>
  boolean?: boolean
}

export type GraphViz = {
  nodes: Array<{ id: string; label: string; kind: string; type?: string }>
  edges: Array<{ source: string; target: string; label: string }>
}

async function req<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch('/api' + path, {
    headers: { 'content-type': 'application/json' },
    ...opts,
  })
  const text = await res.text()
  let data: unknown = null
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    throw new Error(text || res.statusText)
  }
  // Only a *truthy* `error` means failure — an autobuild job legitimately
  // carries `error: null` while it is running, and a presence check would
  // reject every successful poll.
  const err = (data as { error?: string } | null)?.error
  if (!res.ok || err) {
    throw new Error(err || res.statusText)
  }
  return data as T
}

const get = <T>(p: string) => req<T>(p)
const post = <T>(p: string, body?: unknown) => req<T>(p, { method: 'POST', body: JSON.stringify(body ?? {}) })
const put = <T>(p: string, body?: unknown) => req<T>(p, { method: 'PUT', body: JSON.stringify(body ?? {}) })
const del = <T>(p: string, body?: unknown) => req<T>(p, { method: 'DELETE', body: JSON.stringify(body ?? {}) })

export const api = {
  status: () => get<{ ok: boolean; name: string; version: string; projects: number }>('/status'),

  models: () =>
    get<{
      activeId?: string
      configs?: Array<{ id: string; label?: string; modelName?: string; provider?: string; baseURL?: string; adapt?: string }>
    }>('/models'),
  getSettings: () => get<{ llmProfile: string; resolved: string }>('/settings'),
  setLlmProfile: (llmProfile: string) => put<{ ok: boolean; llmProfile: string }>('/settings', { llmProfile }),

  listProjects: () => get<Project[]>('/projects'),
  createProject: (name: string, description: string, baseIri?: string) =>
    post<{ id: number; baseIri: string }>('/projects', { name, description, baseIri }),
  getProject: (id: number) => get<Project>(`/projects/${id}`),
  deleteProject: (id: number) => del(`/projects/${id}`),
  setPrefixes: (id: number, prefixes: Record<string, string>) => put(`/projects/${id}/prefixes`, { prefixes }),

  /** Universal ingest: any file, auto-detected. `content` for text, `contentBase64` for binaries. */
  ingest: (id: number, filename: string, payload: { content?: string; contentBase64?: string }) =>
    post<{ sources: IngestedSource[] }>(`/projects/${id}/ingest`, { filename, ...payload }),
  formats: () => get<{ extensions: string[] }>('/formats'),

  autobuild: (id: number, opts?: { reason?: boolean; extract?: boolean; maxChunks?: number }) =>
    post<{ jobId: string }>(`/projects/${id}/autobuild`, opts ?? {}),
  autobuildStatus: (id: number, job: string) => get<AutoJob>(`/projects/${id}/autobuild/${job}`),
  ask: (id: number, question: string) => post<AskResult>(`/projects/${id}/ask`, { question }),
  assist: (id: number, question: string, context: AssistContext) =>
    post<AssistResult>(`/projects/${id}/assist`, { question, context }),
  assistIndex: (id: number) =>
    get<{ count: number; documents: Array<{ id: string; kind: string; title: string; body: string }> }>(
      `/projects/${id}/assist/index`,
    ),
  liveSchema: (id: number) => get<LiveSchema>(`/projects/${id}/schema`),

  listFunctions: (id: number) =>
    get<{ functions: LogicFunction[]; proposalCounts: Record<string, number> }>(`/projects/${id}/functions`),
  createFunction: (
    id: number,
    body: { name: string; kind: string; target?: string; instruction: string; autoApply?: boolean },
  ) => post<{ id: number }>(`/projects/${id}/functions`, body),
  deleteFunction: (id: number, fid: number) => del(`/projects/${id}/functions/${fid}`),
  trialFunction: (id: number, fid: number) => post<RunReport>(`/projects/${id}/functions/${fid}/trial`),
  runFunction: (id: number, fid: number) => post<RunReport>(`/projects/${id}/functions/${fid}/run`),
  listProposals: (id: number, status?: string) =>
    get<{ proposals: Proposal[]; counts: Record<string, number> }>(
      `/projects/${id}/proposals${status ? `?status=${status}` : ''}`,
    ),
  approveProposals: (id: number, ids?: number[]) =>
    post<{ applied: number; triples: number; batch: string; staleRejected: number }>(
      `/projects/${id}/proposals/approve`,
      { ids: ids ?? [] },
    ),
  rejectProposals: (id: number, ids?: number[]) =>
    post<{ rejected: number }>(`/projects/${id}/proposals/reject`, { ids: ids ?? [] }),
  addEval: (id: number, fid: number, input: string, expect: string) =>
    post<{ id: number }>(`/projects/${id}/functions/${fid}/evals`, { input, expect }),
  listEvals: (id: number, fid: number) =>
    get<Array<{ id: number; input: string; expect: string }>>(`/projects/${id}/functions/${fid}/evals`),
  runEvals: (id: number, fid: number, profiles?: string[]) =>
    post<{ results: Array<{ model: string; passed: number; total: number; varied: number; cases: unknown[] }> }>(
      `/projects/${id}/functions/${fid}/evals/run`,
      { profiles: profiles ?? [] },
    ),

  listSources: (id: number) => get<Source[]>(`/projects/${id}/sources`),
  addSource: (id: number, name: string, content: string, kind?: string) =>
    post<{ id: number; columns: Column[]; rowCount: number }>(`/projects/${id}/sources`, { name, content, kind }),
  deleteSource: (id: number, sid: number) => del(`/projects/${id}/sources/${sid}`),
  profileSource: (id: number, sid: number, llm: boolean) =>
    post<{ columns: Column[]; rowCount: number; llm?: { roles: unknown; model: string }; llmError?: string }>(
      `/projects/${id}/sources/${sid}/profile${llm ? '?llm=1' : ''}`,
    ),

  getTbox: (id: number) => get<Tbox>(`/projects/${id}/tbox`),
  addClass: (id: number, def: TboxClass) => post(`/projects/${id}/tbox/class`, def),
  addProperty: (id: number, def: TboxProp) => post(`/projects/${id}/tbox/property`, def),
  applyTbox: (id: number, draft: unknown) => post<{ classes: number; properties: number }>(`/projects/${id}/tbox/apply`, { draft }),
  removeTerm: (id: number, iri: string) => del(`/projects/${id}/tbox/term`, { iri }),
  draftTbox: (id: number, sourceId?: number) => post<{ draft: unknown; model: string }>(`/projects/${id}/tbox/draft`, { sourceId }),
  tboxGraph: (id: number) => get<GraphViz>(`/projects/${id}/tbox/graph`),

  getMapping: (id: number) => get<Record<string, unknown>>(`/projects/${id}/mapping`),
  setMapping: (id: number, mapping: unknown) => put(`/projects/${id}/mapping`, { mapping }),
  previewMapping: (id: number, mapping?: unknown) =>
    post<{ triples: number; subjects: number; skippedRows: number; samples: string[][] }>(`/projects/${id}/mapping/preview`, { mapping }),
  liftMapping: (id: number, mapping?: unknown) =>
    post<{ batch: string; triples: number; subjects: number; skippedRows: number; totalTriples: number }>(`/projects/${id}/mapping/lift`, { mapping }),
  draftMapping: (id: number, sourceId?: number) => post<{ mapping: unknown; model: string }>(`/projects/${id}/mapping/draft`, { sourceId }),

  sparql: (id: number, query: string) => post<SparqlResult>(`/projects/${id}/sparql`, { query }),
  nl2sparql: (id: number, question: string) => post<{ sparql: string; model: string }>(`/projects/${id}/nl2sparql`, { question }),
  dataGraph: (id: number, limit = 250) => get<GraphViz>(`/projects/${id}/graph?limit=${limit}`),

  listCq: (id: number) => get<CompetencyQuestion[]>(`/projects/${id}/competency`),
  addCq: (id: number, question: string, sparql: string, expect: string) => post(`/projects/${id}/competency`, { question, sparql, expect }),
  updateCq: (id: number, cid: number, question: string, sparql: string, expect: string) => put(`/projects/${id}/competency/${cid}`, { question, sparql, expect }),
  deleteCq: (id: number, cid: number) => del(`/projects/${id}/competency/${cid}`),
  runCq: (id: number) => post<{ total: number; passed: number; results: Array<{ id: number; question: string; pass: boolean; count?: number; error?: string }> }>(`/projects/${id}/competency/run`),

  getShapes: (id: number) => get<Record<string, unknown>>(`/projects/${id}/shapes`),
  setShapes: (id: number, shapes: unknown) => put(`/projects/${id}/shapes`, { shapes }),
  draftShapes: (id: number) => post<{ shapes: unknown; model: string }>(`/projects/${id}/shapes/draft`),
  validate: (id: number, shapes?: unknown) =>
    post<{ conforms: boolean; violationCount: number; checked: number; violations: Array<{ focusNode: string; path: string; constraint: string; value: string; message: string }> }>(`/projects/${id}/validate`, { shapes }),

  materialize: (id: number) => post<{ inferred: number; iterations: number }>(`/projects/${id}/materialize`),
  clearInferred: (id: number) => post(`/projects/${id}/materialize/clear`),

  resolveCandidates: (id: number, cls: string, labelProp: string, threshold: number) =>
    post<{ count: number; pairs: Array<{ a: string; b: string; labelA: string; labelB: string; score: number }> }>(`/projects/${id}/resolve/candidates`, { class: cls, labelProp, threshold }),
  resolveApply: (id: number, predicate: string, pairs: string[][]) => post<{ applied: number }>(`/projects/${id}/resolve/apply`, { predicate, pairs }),

  listBatches: (id: number) => get<Batch[]>(`/projects/${id}/batches`),
  dropBatch: (id: number, iri: string) => del(`/projects/${id}/batches`, { iri }),

  extract: (id: number, text: string) => post<{ inserted: number; model: string; batch: string; raw: unknown }>(`/projects/${id}/extract`, { text }),

  exportUrl: (id: number) => `/api/projects/${id}/export`,
}
