export type Severity = 'critical' | 'high' | 'medium' | 'low' | 'info'

export interface Finding {
  id: number
  scan_id: number
  category: string
  severity: Severity
  fingerprint: string
  title: string
  detail: string
  fix: string
  evidence: unknown
  wstg: string | null
  cve: string | null
  kev: boolean
  epss: number | null
  status: string
  status_reason: string | null
  first_seen: string
  last_seen: string
}

export interface DiffEntry {
  fingerprint: string
  severity: Severity
  title: string
}

export interface Asset {
  id: number
  kind: string
  target: string
  label: string
  verify_method: string | null
  verify_token: string | null
  verified_at: string | null
  verify_error: string | null
  created_at: string
}

export interface Scan {
  id: number
  asset_id: number
  layer: string
  status: string
  score: number | null
  grade: string | null
  error: string | null
  started_at: string
  finished_at: string | null
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export const api = {
  assets: () => req<{ assets: Asset[] }>('/assets').then((d) => d.assets ?? []),

  addAsset: (kind: string, target: string, label: string) =>
    req<{ ok: boolean; id?: number; error?: string }>('/assets', {
      method: 'POST',
      body: JSON.stringify({ kind, target, label }),
    }),

  removeAsset: (id: number) =>
    req<{ ok: boolean }>(`/assets/${id}/delete`, { method: 'POST' }),

  verifyToken: (id: number, method: string) =>
    req<{ ok: boolean; token?: string; instructions?: string; error?: string }>(
      `/assets/${id}/verify-token`,
      { method: 'POST', body: JSON.stringify({ method }) },
    ),

  verify: (id: number) =>
    req<{ ok: boolean; verified?: boolean; error?: string }>(`/assets/${id}/verify`, {
      method: 'POST',
    }),

  scan: (asset_id: number) =>
    req<{ ok: boolean; scan_id?: number; score?: number; grade?: string; findings?: Finding[]; error?: string }>(
      '/scan/passive',
      { method: 'POST', body: JSON.stringify({ asset_id }) },
    ),

  scanActive: (asset_id: number) =>
    req<{ ok: boolean; scan_id?: number; score?: number; grade?: string; requests?: number; truncated?: boolean; error?: string }>(
      '/scan/active',
      { method: 'POST', body: JSON.stringify({ asset_id }) },
    ),

  scans: (asset_id?: number) =>
    req<{ scans: Scan[] }>(`/scans${asset_id ? `?asset_id=${asset_id}` : ''}`).then(
      (d) => d.scans ?? [],
    ),

  scan_get: (id: number) =>
    req<{ ok: boolean; scan?: Scan; findings?: Finding[] }>(`/scans/${id}`),

  setStatus: (id: number, status: string, reason?: string) =>
    req<{ ok: boolean }>(`/findings/${id}/status`, {
      method: 'POST',
      body: JSON.stringify({ status, reason }),
    }),

  // /diff trả bản rút gọn (fingerprint/severity/title), không phải Finding đầy đủ.
  rules: () =>
    req<{ total: number; implemented: number; rules: Rule[]; not_covered: string[] }>('/rules'),

  dashboard: (asset_id?: number) =>
    req<Dashboard>(`/dashboard${asset_id ? `?asset_id=${asset_id}` : ''}`),

  customRules: () =>
    req<{ custom: CustomRule[]; overrides: Override[] }>('/settings/rules'),

  addRule: (rule: unknown) =>
    req<{ ok: boolean; id?: string; error?: string }>('/settings/rules', {
      method: 'POST', body: JSON.stringify(rule),
    }),

  removeRule: (id: string) =>
    req<{ ok: boolean; error?: string }>(`/settings/rules/${encodeURIComponent(id)}/delete`, {
      method: 'POST',
    }),

  importRules: (body: { url?: string; json?: string; apply: boolean }) =>
    req<ImportReport>('/settings/rules/import', { method: 'POST', body: JSON.stringify(body) }),

  setOverride: (b: { rule_id: string; severity?: string | null; enabled: boolean; note?: string }) =>
    req<{ ok: boolean; error?: string }>('/settings/overrides', {
      method: 'POST', body: JSON.stringify(b),
    }),

  diff: (from: number, to: number) =>
    req<{ ok: boolean; new: DiffEntry[]; fixed: DiffEntry[]; unchanged: number }>(
      `/diff?from=${from}&to=${to}`,
    ),
}

export interface Rule {
  id: string
  category: string
  layer: string
  layer_label: string
  max_severity: Severity
  title: string
  rationale: string
  wstg: string
  implemented: boolean
}

export interface TrendPoint {
  scan_id: number
  at: string
  score: number | null
  grade: string | null
}

export interface Dashboard {
  ok: boolean
  assets_total: number
  assets_verified: number
  scans_total: number
  trend: TrendPoint[]
  latest_scan_id: number | null
  by_severity: Record<Severity, number>
  by_category: Record<string, number>
  top_open: Finding[]
  regressed: number
  acked: number
}

export interface CustomRule {
  id: string
  title: string
  category: string
  severity: Severity
  rationale?: string
  fix?: string
  check: { target: string; name?: string; op: string; value?: string }
  enabled: boolean
  source: string
}

export interface Override {
  rule_id: string
  severity: Severity | null
  enabled: boolean
  note: string | null
}

export interface ImportReport {
  ok: boolean
  error?: string
  source: string
  total: number
  accepted: number
  applied: boolean
  rules: { id: string; title: string; severity: Severity; target: string; name: string; op: string; value: string }[]
  rejected: { id: string; reason: string }[]
}

export const SEV_ORDER: Severity[] = ['critical', 'high', 'medium', 'low', 'info']

export const SEV_LABEL: Record<Severity, string> = {
  critical: 'Nghiêm trọng',
  high: 'Cao',
  medium: 'Trung bình',
  low: 'Thấp',
  info: 'Thông tin',
}

export const SEV_COLOR: Record<Severity, string> = {
  critical: '#a8071a',
  high: '#d4380d',
  medium: '#d46b08',
  low: '#7cb305',
  info: '#8c8c8c',
}

/** Hạng nào thì tô màu gì — theo cùng thang với mức nặng nhất mà nó ngụ ý. */
export function gradeColor(grade?: string | null): string {
  if (!grade) return '#8c8c8c'
  if (grade.startsWith('A')) return '#389e0d'
  if (grade.startsWith('B')) return '#7cb305'
  if (grade.startsWith('C')) return '#d46b08'
  if (grade.startsWith('D')) return '#d4380d'
  return '#a8071a'
}
