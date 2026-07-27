// UI phục vụ cả trực tiếp (:4560) lẫn dưới proxy daemon
// (/api/space/apps/lakehouse/proxy/) → mọi call phải page-relative.
import type {
  ConnectionListResp,
  DatasetDetail,
  DatasetListResp,
  FlowGetResp,
  FlowListResp,
  IntrospectResp,
  Page,
  PreviewResp,
  RunGetResp,
  RunListResp,
  RunLogsResp,
  Settings,
  StatusResp,
} from './types'

export const base = new URL('.', window.location.href).pathname.replace(/\/$/, '')

export class ApiError extends Error {
  status: number
  details?: unknown
  constructor(message: string, status: number, details?: unknown) {
    super(message)
    this.name = 'ApiError'
    this.status = status
    this.details = details
  }
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${base}/api${path}`, {
    headers: { 'content-type': 'application/json' },
    ...init,
  })
  const text = await res.text()
  let body: unknown
  try {
    body = text ? JSON.parse(text) : {}
  } catch {
    // HTML body ⇒ SPA fallback trả lời ⇒ route không tồn tại trên build này.
    throw new ApiError(
      `Điểm cuối chưa khả dụng (HTTP ${res.status})`,
      res.status === 200 ? 404 : res.status,
    )
  }
  if (!res.ok) {
    const b = body as { error?: string; details?: unknown }
    throw new ApiError(b?.error ?? `HTTP ${res.status}`, res.status, b?.details)
  }
  return body as T
}

export const wsUrl = () => {
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${proto}//${window.location.host}${base}/api/ws/dashboard`
}

// ---- status ----
export const getStatus = () => req<StatusResp>('/status')

// ---- datasets ----
export const listDatasets = (namespace?: string, limit = 200, offset = 0) => {
  const q = new URLSearchParams({ limit: String(limit), offset: String(offset) })
  if (namespace) q.set('namespace', namespace)
  return req<DatasetListResp>(`/datasets?${q}`)
}
export const getDataset = (ns: string, name: string) =>
  req<DatasetDetail>(`/datasets/${encodeURIComponent(ns)}/${encodeURIComponent(name)}`)
export const previewDataset = (ns: string, name: string, limit = 50) =>
  req<PreviewResp>(
    `/datasets/${encodeURIComponent(ns)}/${encodeURIComponent(name)}/preview?limit=${limit}`,
  )
export const deleteDataset = (ns: string, name: string) =>
  req<{ ok: boolean; deleted: string }>(
    `/datasets/${encodeURIComponent(ns)}/${encodeURIComponent(name)}`,
    { method: 'DELETE' },
  )
export const importFile = (body: {
  filename: string
  contentBase64: string
  namespace?: string
  dataset?: string
}) => req<{ ok: boolean; run_id: string; datasets: unknown[] }>('/import', {
  method: 'POST',
  body: JSON.stringify(body),
})

// ---- query ----
export const runQuery = (sql: string, limit: number, offset: number) =>
  req<Page>('/query', { method: 'POST', body: JSON.stringify({ sql, limit, offset }) })
export const explainQuery = (sql: string) =>
  req<{ plan: unknown; next?: string }>('/query/explain', {
    method: 'POST',
    body: JSON.stringify({ sql }),
  })

// ---- settings ----
export const getSettings = () => req<Settings>('/settings')
export const putSettings = (patch: Settings) =>
  req<Settings>('/settings', { method: 'PUT', body: JSON.stringify(patch) })

// ---- connections ----
export const listConnections = () => req<ConnectionListResp>('/connections')
export const addConnection = (body: { id?: string; kind: string; dsn: string }) =>
  req<{ ok: boolean; connection: unknown }>('/connections', {
    method: 'POST',
    body: JSON.stringify(body),
  })
export const testConnection = (id: string) =>
  req<{ ok: boolean; connection_id: string }>(
    `/connections/${encodeURIComponent(id)}/test`,
    { method: 'POST' },
  )
export const introspectConnection = (id: string, schema?: string) => {
  const q = schema ? `?schema=${encodeURIComponent(schema)}` : ''
  return req<IntrospectResp>(`/connections/${encodeURIComponent(id)}/introspect${q}`)
}
export const deleteConnection = (id: string) =>
  req<{ ok: boolean; deleted: string }>(`/connections/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })

// ---- flows ----
export const listFlows = () => req<FlowListResp>('/flows')
export const getFlow = (id: string) => req<FlowGetResp>(`/flows/${encodeURIComponent(id)}`)
export const createFlow = (def: unknown, enable = false) =>
  req<{ ok: boolean; flow: unknown; dag: string[] | null }>('/flows', {
    method: 'POST',
    body: JSON.stringify({ def, enable }),
  })
export const updateFlow = (id: string, def: unknown, confirm_reset = false) =>
  req<{ ok: boolean; flow: unknown; impact: unknown }>(`/flows/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify({ def, confirm_reset }),
  })
export const deleteFlow = (id: string) =>
  req<{ ok: boolean; deleted: string }>(`/flows/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  })
export const runFlow = (id: string) =>
  req<{ ok: boolean; run_id: string }>(`/flows/${encodeURIComponent(id)}/run`, {
    method: 'POST',
  })
export const enableFlow = (id: string, enabled: boolean) =>
  req<{ ok: boolean; flow_id: string; enabled: boolean }>(
    `/flows/${encodeURIComponent(id)}/enable`,
    { method: 'POST', body: JSON.stringify({ enabled }) },
  )
// Có thể chưa khả dụng — caller bắt ApiError.status===404 → "chưa khả dụng".
export const generateFlow = (prompt: string) =>
  req<{ ok: boolean; flow?: unknown; dag?: string[] | null }>('/flows/generate', {
    method: 'POST',
    body: JSON.stringify({ prompt }),
  })
export const backfillFlow = (id: string, start: string, end: string) =>
  req<{ ok: boolean; run_id?: string }>(`/flows/${encodeURIComponent(id)}/backfill`, {
    method: 'POST',
    body: JSON.stringify({ start, end }),
  })

// ---- runs ----
export const listRuns = (opts: {
  flow_id?: string
  status?: string
  limit?: number
  offset?: number
} = {}) => {
  const q = new URLSearchParams()
  if (opts.flow_id) q.set('flow_id', opts.flow_id)
  if (opts.status) q.set('status', opts.status)
  q.set('limit', String(opts.limit ?? 100))
  q.set('offset', String(opts.offset ?? 0))
  return req<RunListResp>(`/runs?${q}`)
}
export const getRun = (id: string) => req<RunGetResp>(`/runs/${encodeURIComponent(id)}`)
export const cancelRun = (id: string) =>
  req<{ ok: boolean; run_id: string }>(`/runs/${encodeURIComponent(id)}/cancel`, {
    method: 'POST',
  })
export const getRunLogs = (id: string, tail = 200) =>
  req<RunLogsResp>(`/runs/${encodeURIComponent(id)}/logs?tail=${tail}`)
