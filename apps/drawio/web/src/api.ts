// Typed fetch client for the SenClaw Diagrams backend (/api/*).

export type Kind =
  | 'flowchart'
  | 'sequence'
  | 'architecture'
  | 'er'
  | 'state'
  | 'class'
  | 'org'
  | 'network'
  | 'bpmn'

export interface DiagramMeta {
  id: number
  name: string
  kind: Kind
  cells: number
  svg_stale: boolean
  created_at: number
  updated_at: number
}

export interface Diagram extends DiagramMeta {
  xml: string
}

export interface EditorStatus {
  status: 'missing' | 'downloading' | 'extracting' | 'ready' | 'error'
  received?: number
  total?: number
  percent?: number
  version?: string
  message?: string
}

export interface StatusResp {
  ok: boolean
  app: string
  editor: EditorStatus
}

export interface ModelInfo {
  id: string
  modelName: string | null
  provider: string | null
}

async function req<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers ?? {}) },
  })
  const data = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error((data as { error?: string }).error ?? `HTTP ${res.status}`)
  return data as T
}

export const api = {
  status: () => req<StatusResp>('/api/status'),
  editorRetry: () => req<StatusResp>('/api/editor/retry', { method: 'POST' }),

  list: () => req<DiagramMeta[]>('/api/diagrams'),
  create: (name: string, kind?: Kind) =>
    req<{ id: number }>('/api/diagrams', { method: 'POST', body: JSON.stringify({ name, kind }) }),
  get: (id: number) => req<Diagram>(`/api/diagrams/${id}`),
  rename: (id: number, name: string) =>
    req<{ ok: boolean }>(`/api/diagrams/${id}/rename`, { method: 'POST', body: JSON.stringify({ name }) }),
  remove: (id: number) => req<{ ok: boolean }>(`/api/diagrams/${id}/delete`, { method: 'POST' }),
  putXml: (id: number, xml: string) =>
    req<{ ok: boolean }>(`/api/diagrams/${id}/xml`, { method: 'PUT', body: JSON.stringify({ xml }) }),
  putSvg: (id: number, svg: string) =>
    req<{ ok: boolean }>(`/api/diagrams/${id}/svg`, { method: 'PUT', body: JSON.stringify({ svg }) }),

  generate: (body: { prompt: string; kind?: Kind; mode?: 'mermaid' | 'xml'; diagram_id?: number }) =>
    req<{ mode: string; mermaid?: string; xml?: string; model: string }>('/api/generate', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  edit: (body: { diagram_id?: number; xml?: string; instruction: string }) =>
    req<{ xml: string; model: string }>('/api/edit', { method: 'POST', body: JSON.stringify(body) }),

  models: () => req<{ activeId: string | null; configs: ModelInfo[] }>('/api/models'),
  setActiveModel: (id: string) =>
    req<{ ok: boolean }>('/api/model-active', { method: 'POST', body: JSON.stringify({ id }) }),
}

/** Subscribe to backend events (MCP-driven changes). Returns a cleanup fn. */
export function subscribeEvents(onEvent: (ev: { type: string; id?: number }) => void): () => void {
  const es = new EventSource('/api/events')
  es.addEventListener('message', (e) => {
    try {
      onEvent(JSON.parse((e as MessageEvent).data))
    } catch {
      /* ignore malformed events */
    }
  })
  return () => es.close()
}
