// Cùng origin với backend của app (iframe được nạp từ runtime.url), nên chỉ cần
// đường dẫn tuyệt đối /api/... — không SDK, không bridge, không auth ở tầng này.

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}
const post = (url: string, body?: any) =>
  j(url, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) })

const qs = (o: Record<string, any>) => {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(o)) if (v !== undefined && v !== null && v !== '') p.set(k, String(v))
  const s = p.toString()
  return s ? `?${s}` : ''
}

export type Severity = 'critical' | 'high' | 'medium' | 'low' | 'info'
export type FindingStatus = 'open' | 'triaged' | 'accepted_risk' | 'false_positive' | 'resolved'

export interface Ev {
  id: number
  ts: string
  source: string
  kind: string
  actor: string
  agent_id: string
  tool_name: string | null
  ok: boolean | null
  summary: string
  detail: any
}

export interface Finding {
  id: number
  rule_id: string
  severity: Severity
  score: number
  title: string
  detail: string
  actor: string | null
  first_ts: string
  last_ts: string
  evidence: number[]
  standards: string[]
  status: FindingStatus
  note: string
  case_id: number | null
  created_at: string
}

export interface Rule {
  id: string
  group: string
  title: string
  severity: Severity
  default_severity: Severity
  standards: string[]
  about: string
  enabled: boolean
  params: any
}

export interface CaseRow {
  id: number
  title: string
  summary: string
  status: string
  severity: string
  hypothesis: string
  created_at: string
  finding_count: number
}

export const api = {
  status: () => j('/api/status'),
  dashboard: () => j('/api/dashboard'),
  sources: () => j('/api/sources'),

  ingest: () => post('/api/ingest/run'),
  scan: () => post('/api/scan'),

  events: (q: Record<string, any> = {}) => j<{ count: number; events: Ev[] }>('/api/events' + qs(q)),
  event: (id: number) => j('/api/events/' + id),
  pivot: (id: number, mode: string, minutes = 30) =>
    j('/api/events/' + id + '/pivot' + qs({ mode, minutes })),

  findings: (q: Record<string, any> = {}) =>
    j<{ count: number; findings: Finding[] }>('/api/findings' + qs(q)),
  finding: (id: number) => j('/api/findings/' + id),
  setFindingStatus: (id: number, status: FindingStatus, note?: string) =>
    post(`/api/findings/${id}/status`, { status, note }),
  explain: (id: number) => post(`/api/findings/${id}/explain`),

  rules: () => j<{ rules: Rule[] }>('/api/rules'),
  setRule: (id: string, body: any) => post('/api/rules/' + id, body),

  snapshots: (q: Record<string, any> = {}) => j('/api/snapshots' + qs(q)),
  takeSnapshot: () => post('/api/snapshots/take'),
  diffs: (q: Record<string, any> = {}) => j('/api/snapshots/diff' + qs(q)),

  cases: (q: Record<string, any> = {}) => j<{ cases: CaseRow[] }>('/api/cases' + qs(q)),
  createCase: (b: any) => post('/api/cases', b),
  case: (id: number) => j('/api/cases/' + id),
  updateCase: (id: number, patch: any) => post('/api/cases/' + id, patch),
  caseNote: (id: number, body: string) => post(`/api/cases/${id}/notes`, { body }),
  caseAttach: (id: number, finding_ids: number[]) => post(`/api/cases/${id}/attach`, { finding_ids }),
  caseHypothesis: (id: number) => post(`/api/cases/${id}/hypothesis`),
  caseReport: (id: number) => post(`/api/cases/${id}/report`),

  ask: (question: string, from?: string, to?: string) => post('/api/ask', { question, from, to }),
  verifyChain: () => j('/api/verify-chain'),
  suppressions: () => j('/api/suppressions'),
  addSuppression: (b: any) => post('/api/suppressions', b),
  delSuppression: (id: number) => post(`/api/suppressions/${id}/delete`),
  toolArgs: (q: Record<string, any> = {}) => j('/api/tool-args' + qs(q)),
}

export const SEV_COLOR: Record<Severity, string> = {
  critical: 'red',
  high: 'volcano',
  medium: 'gold',
  low: 'blue',
  info: 'default',
}

export const SEV_LABEL: Record<Severity, string> = {
  critical: 'Nghiêm trọng',
  high: 'Cao',
  medium: 'Trung bình',
  low: 'Thấp',
  info: 'Thông tin',
}

export const STATUS_LABEL: Record<FindingStatus, string> = {
  open: 'Chưa xử lý',
  triaged: 'Đã xem',
  accepted_risk: 'Chấp nhận rủi ro',
  false_positive: 'Dương tính giả',
  resolved: 'Đã xử lý',
}

/** Giờ địa phương, gọn — mốc lưu trong kho là UTC. */
export function fmtTs(ts?: string | null) {
  if (!ts) return '—'
  const d = new Date(ts)
  if (isNaN(d.getTime())) return ts
  return d.toLocaleString('vi-VN', { hour12: false })
}
