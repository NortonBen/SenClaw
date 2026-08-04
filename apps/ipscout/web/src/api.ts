export type Severity = 'critical' | 'high' | 'medium' | 'low' | 'info'

export const SEV_ORDER: Severity[] = ['critical', 'high', 'medium', 'low', 'info']

export const SEV_LABEL: Record<Severity, string> = {
  critical: 'Nghiêm trọng',
  high: 'Cao',
  medium: 'Trung bình',
  low: 'Thấp',
  info: 'Thông tin',
}

export const SEV_COLOR: Record<Severity, string> = {
  critical: 'red',
  high: 'volcano',
  medium: 'orange',
  low: 'gold',
  info: 'blue',
}

/// Độ tin của địa lý là chuỗi tiếng Việt do backend đặt — ánh xạ sang màu để
/// người đọc thấy ngay "không dùng được" khác hẳn "cao", thay vì phải đọc chữ.
export const CONF_COLOR: Record<string, string> = {
  cao: 'green',
  'trung bình': 'gold',
  thấp: 'orange',
  'không dùng được': 'red',
  'không có': 'default',
}

export interface Project {
  id: number
  name: string
  note: string
  targets: number
  created_at: string
}

export interface Target {
  id: number
  project_id: number
  input: string
  host: string
  label: string
  created_at: string
}

export interface Run {
  id: number
  target_id: number
  layer: 'profile' | 'ports'
  status: 'running' | 'done' | 'failed'
  ip: string | null
  started_at: string
  finished_at: string | null
  error: string | null
  summary: Record<string, any>
}

export interface Finding {
  id: number
  run_id: number
  target_id: number
  fingerprint: string
  severity: Severity
  category: string
  title: string
  detail: string
  evidence: Record<string, any>
  fix: string
  first_seen: string
  last_seen: string
}

export interface PortRow {
  port: number
  state?: string
  service: string | null
  product: string | null
  version: string | null
  banner: string
  severity: Severity
  why?: string
  fix?: string
  latency_ms?: number
  tls?: {
    subject: string
    issuer: string
    san: string[]
    not_before: string
    not_after: string
    expired: boolean
    self_signed: boolean
  } | null
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`/api${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

const post = (path: string, body?: unknown) =>
  req<any>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) })

export const api = {
  status: () => req<any>('/status'),
  capabilities: () => req<any>('/capabilities'),

  projects: async () => (await req<any>('/projects')).projects as Project[],
  addProject: (name: string, note = '') => post('/projects', { name, note }),
  deleteProject: (id: number) => post(`/projects/${id}/delete`),

  targets: async (projectId?: number) =>
    ((await req<any>(`/targets${projectId ? `?project_id=${projectId}` : ''}`)).targets ??
      []) as Target[],
  addTarget: (target: string, project_id: number, label = '') =>
    post('/targets', { target, project_id, label }),
  deleteTarget: (id: number) => post(`/targets/${id}/delete`),

  profile: (target_id: number) => post('/profile', { target_id }),
  scan: (target_id: number, profile?: string, ports?: string) =>
    post('/scan', { target_id, profile, ports }),
  trace: (target_id: number, max_hops?: number) => post('/trace', { target_id, max_hops }),

  runs: async (target_id?: number) =>
    ((await req<any>(`/runs${target_id ? `?target_id=${target_id}` : ''}`)).runs ?? []) as Run[],
  run: (id: number) => req<any>(`/runs/${id}`),
  diff: (from_run: number, to_run: number) =>
    req<any>(`/diff?from_run=${from_run}&to_run=${to_run}`),
  findings: async (target_id?: number) =>
    ((await req<any>(`/findings${target_id ? `?target_id=${target_id}` : ''}`)).findings ??
      []) as Finding[],
  activity: async () => (await req<any>('/activity')).activity ?? [],
}

/// Ngày ISO → giờ địa phương dễ đọc. Backend luôn trả ISO có 'Z' nên `Date`
/// tự đổi múi giờ; hiển thị thẳng chuỗi UTC sẽ lệch 7 tiếng ở Việt Nam.
export function when(iso: string | null | undefined): string {
  if (!iso) return '—'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString('vi-VN')
}
