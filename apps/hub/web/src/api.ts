// Thin client for the hub app's own REST proxy (served by the Rust binary).

export interface ServerSettings {
  base_url: string
  namespace: string
  username: string
}

export interface ConnStatus {
  configured: boolean
  connected: boolean
  base_url: string
  username: string
  message: string
}

export interface Device {
  id: string
  name: string
  model: string
  online: boolean
  last_seen: string | null
  attributes: Record<string, unknown>
}

export interface TelemetryPoint {
  ts: string
  field: string
  value: number | string | boolean | null
}

export interface AlertItem {
  id: string
  device_id: string
  device_name: string
  level: string
  message: string
  ts: string
}

export interface HtmlPanel {
  id: number
  name: string
  html: string
  updated_at: string
}

async function j<T>(res: Response): Promise<T> {
  if (!res.ok) {
    let msg = `HTTP ${res.status}`
    try {
      const body = await res.json()
      if (body && typeof body.error === 'string') msg = body.error
    } catch {
      /* keep status message */
    }
    throw new Error(msg)
  }
  return res.json() as Promise<T>
}

const post = (url: string, body: unknown) =>
  fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })

export const api = {
  status: () => fetch('/api/hub/status').then((r) => j<ConnStatus>(r)),
  getSettings: () => fetch('/api/hub/settings').then((r) => j<ServerSettings>(r)),
  saveSettings: (s: { base_url: string; namespace: string }) =>
    post('/api/hub/settings', s).then((r) => j<ConnStatus>(r)),
  login: (username: string, password: string) =>
    post('/api/hub/login', { username, password }).then((r) => j<ConnStatus>(r)),
  logout: () => post('/api/hub/logout', {}).then((r) => j<ConnStatus>(r)),
  devices: (q = '') =>
    fetch(`/api/hub/devices${q ? `?q=${encodeURIComponent(q)}` : ''}`).then((r) =>
      j<Device[]>(r),
    ),
  device: (id: string) =>
    fetch(`/api/hub/devices/${encodeURIComponent(id)}`).then((r) => j<Device>(r)),
  telemetry: (id: string, field = '', limit = 50) =>
    fetch(
      `/api/hub/devices/${encodeURIComponent(id)}/telemetry?limit=${limit}${
        field ? `&field=${encodeURIComponent(field)}` : ''
      }`,
    ).then((r) => j<TelemetryPoint[]>(r)),
  sendCommand: (id: string, command: string, params: Record<string, unknown>) =>
    post(`/api/hub/devices/${encodeURIComponent(id)}/command`, { command, params }).then((r) =>
      j<{ ok: boolean; detail: string }>(r),
    ),
  alerts: (limit = 30) => fetch(`/api/hub/alerts?limit=${limit}`).then((r) => j<AlertItem[]>(r)),
  panels: () => fetch('/api/hub/panels').then((r) => j<HtmlPanel[]>(r)),
  savePanel: (p: { id?: number; name: string; html: string }) =>
    post('/api/hub/panels', p).then((r) => j<HtmlPanel>(r)),
  deletePanel: (id: number) =>
    fetch(`/api/hub/panels/${id}`, { method: 'DELETE' }).then((r) => j<{ ok: boolean }>(r)),
}
