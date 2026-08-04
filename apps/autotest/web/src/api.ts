// Tiny fetch wrapper for the AutoTest app REST API (served from the same
// origin in production; proxied to :4640 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

const post = (url: string, body: any) => j(url, { method: 'POST', body: JSON.stringify(body) })

export interface Status {
  ok: boolean
  suites: number
  cases: number
  running: number
  runs_today: number
  pass_rate_recent: number | null
  schedules_enabled: number
}

export interface Suite {
  id: number
  name: string
  description: string
  env_id: number | null
  status: string
  case_count?: number
  enabled_count?: number
  last_run_status?: string | null
  last_run_at?: number | null
  cases?: Case[]
  schedule?: Schedule | null
}

export interface Case {
  id: number
  suite_id: number
  name: string
  kind: 'http' | 'script' | 'web'
  position: number
  enabled: boolean
  timeout_ms: number
  config: any
  assertions: any[]
  extract: any[]
}

export interface Env {
  id: number
  name: string
  vars: Record<string, string>
}

export interface Run {
  id: number
  suite_id: number | null
  case_id: number | null
  env_id: number | null
  trigger: string
  status: string
  started_at: number
  finished_at: number | null
  total: number
  passed: number
  failed: number
  errors: number
  skipped: number
  target: string
  results?: RunResult[]
}

export interface RunResult {
  id: number
  case_id: number
  name: string
  kind: string
  status: string
  duration_ms: number
  log: string
  assertions: { desc: string; pass: boolean; actual: string; expected: string; type: string }[]
  error: string
}

export interface Schedule {
  id: number
  suite_id: number
  suite_name?: string
  interval_min: number
  enabled: boolean
  last_run_at: number | null
}

export const api = {
  status: () => j<Status>('api/status'),
  dashboard: () => j<any>('api/dashboard'),

  suites: (all = false) => j<{ suites: Suite[] }>(`api/suites?all=${all}`).then((r) => r.suites ?? []),
  suite: (id: number) => j<{ ok: boolean; suite: Suite; error?: string }>(`api/suites/${id}`),
  addSuite: (b: { name: string; description?: string; env_id?: number | null }) => post('api/suites', b),
  updateSuite: (id: number, b: any) => post(`api/suites/${id}`, b),
  deleteSuite: (id: number) => post(`api/suites/${id}/delete`, {}),

  addCase: (b: any) => post('api/cases', b),
  updateCase: (id: number, b: any) => post(`api/cases/${id}`, b),
  deleteCase: (id: number) => post(`api/cases/${id}/delete`, {}),

  envs: () => j<{ environments: Env[] }>('api/environments').then((r) => r.environments ?? []),
  setEnv: (b: { name: string; vars: any }) => post('api/environments', b),
  deleteEnv: (id: number) => post(`api/environments/${id}/delete`, {}),

  runSuite: (suite_id: number, env_id?: number | null) =>
    post('api/run/suite', { suite_id, env_id: env_id ?? null, wait: false }),
  runCase: (case_id: number, env_id?: number | null) => post('api/run/case', { case_id, env_id: env_id ?? null }),
  runs: (suite_id?: number | null, limit = 50) =>
    j<{ runs: Run[] }>(`api/runs?limit=${limit}${suite_id ? `&suite_id=${suite_id}` : ''}`).then((r) => r.runs ?? []),
  run: (id: number) => j<{ ok: boolean; run: Run; error?: string }>(`api/runs/${id}`),
  cancelRun: (id: number) => post(`api/runs/${id}/cancel`, {}),

  report: (suite_id?: number | null) => j<any>(`api/report${suite_id ? `?suite_id=${suite_id}` : ''}`),

  schedules: () => j<{ schedules: Schedule[] }>('api/schedules').then((r) => r.schedules ?? []),
  setSchedule: (b: { suite_id: number; interval_min: number; enabled: boolean }) => post('api/schedules', b),
  deleteSchedule: (suite_id: number) => post(`api/schedules/${suite_id}/delete`, {}),

  aiGenerate: (b: { suite_id: number; description: string; apply: boolean }) => post('api/ai/generate', b),
  aiDiagnose: (b: { run_id: number; question?: string }) => post('api/ai/diagnose', b),

  activity: () => j<{ activity: any[] }>('api/activity').then((r) => r.activity ?? []),
  settings: () => j<{ browser_url: string }>('api/settings'),
  saveSettings: (b: { browser_url: string }) => post('api/settings', b),
}

export const fmtTime = (ts?: number | null) => (ts ? new Date(ts * 1000).toLocaleString('vi-VN') : '—')

export const fmtDuration = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`)

export const statusColor: Record<string, string> = {
  pass: 'green',
  fail: 'red',
  error: 'volcano',
  running: 'processing',
  cancelled: 'default',
  skipped: 'default',
}

export const statusLabel: Record<string, string> = {
  pass: 'PASS',
  fail: 'FAIL',
  error: 'LỖI',
  running: 'đang chạy',
  cancelled: 'đã hủy',
  skipped: 'bỏ qua',
}
