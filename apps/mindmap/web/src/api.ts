// Typed fetch client for the SenClaw Mindmap backend (/api/*).

export type Layout = 'mindmap' | 'org' | 'outline' | 'right'
export type Shape = 'rounded' | 'rect' | 'pill' | 'ellipse' | 'line'

export interface MapMeta {
  id: number
  title: string
  description: string
  layout: Layout
  created_at: number
  updated_at: number
  node_count: number
}

export interface TreeNode {
  id: number
  text: string
  note: string
  color: string | null
  shape: Shape | null
  fill: boolean
  icon: string | null
  pos_x: number | null
  pos_y: number | null
  collapsed: boolean
  children: TreeNode[]
}

/** A flat node for restoring a whole map (undo/redo). */
export interface RestoreNode {
  id: number
  parent_id: number | null
  text: string
  note: string
  color: string | null
  shape: Shape | null
  fill: boolean
  icon: string | null
  pos_x: number | null
  pos_y: number | null
  collapsed: boolean
  ord: number
}

/** A parsed node for import (matches the backend GenNode shape). */
export interface ImportNode {
  text: string
  note?: string
  color?: string | null
  shape?: Shape | null
  fill?: boolean
  icon?: string | null
  children?: ImportNode[]
}

export interface TemplateInfo {
  id: string
  name: string
  icon: string
  category: string
  description: string
  layout: Layout
}

export interface ChatSession {
  id: number
  map_id: number
  title: string
  created_at: number
  updated_at: number
  message_count: number
}

export interface ChatMessageRow {
  id: number
  role: 'user' | 'assistant' | 'system'
  content: string
  model: string | null
  created_at: number
}

export interface ModelInfo {
  id: string
  modelName: string | null
  provider: string | null
}

export interface ChatMsg {
  role: 'user' | 'assistant'
  content: string
  /** Wall-clock ms the response took (assistant only; UI-only). */
  ms?: number
  /** Model tag for an assistant reply (UI-only). */
  model?: string
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
  status: () => req<{ ok: boolean }>('/api/status'),
  llmInfo: () => req<{ ok: boolean; model?: string | null; error?: string }>('/api/llm-info'),

  maps: () => req<MapMeta[]>('/api/maps'),
  createMap: (title: string, description = '', layout: Layout = 'mindmap') =>
    req<{ id: number; rootId: number; layout: Layout }>('/api/maps', {
      method: 'POST',
      body: JSON.stringify({ title, description, layout }),
    }),
  templates: () => req<TemplateInfo[]>('/api/templates'),
  createFromTemplate: (templateId: string, title?: string) =>
    req<{ id: number; rootId: number; layout: Layout; added: number }>('/api/maps/from-template', {
      method: 'POST',
      body: JSON.stringify({ template_id: templateId, title }),
    }),
  getMap: (id: number) => req<{ meta: MapMeta; tree: TreeNode | null }>(`/api/map?id=${id}`),
  renameMap: (id: number, title: string, description = '') =>
    req<{ success: boolean }>('/api/map/rename', {
      method: 'POST',
      body: JSON.stringify({ id, title, description }),
    }),
  setLayout: (id: number, layout: Layout) =>
    req<{ success: boolean; layout: Layout }>('/api/map/layout', {
      method: 'POST',
      body: JSON.stringify({ id, layout }),
    }),
  deleteMap: (id: number) =>
    req<{ success: boolean }>('/api/map/delete', { method: 'POST', body: JSON.stringify({ id }) }),

  addNode: (parentId: number, text: string, note = '', color: string | null = null) =>
    req<{ id: number }>('/api/node/add', {
      method: 'POST',
      body: JSON.stringify({ parent_id: parentId, text, note, color }),
    }),
  updateNode: (
    id: number,
    patch: {
      text?: string
      note?: string
      color?: string | null
      shape?: Shape | null
      fill?: boolean
      icon?: string | null
      collapsed?: boolean
    },
  ) =>
    req<{ success: boolean }>('/api/node/update', {
      method: 'POST',
      body: JSON.stringify({ id, ...patch }),
    }),
  deleteNode: (id: number) =>
    req<{ success: boolean }>('/api/node/delete', { method: 'POST', body: JSON.stringify({ id }) }),
  moveNode: (id: number, newParent: number) =>
    req<{ success: boolean }>('/api/node/move', {
      method: 'POST',
      body: JSON.stringify({ id, new_parent: newParent }),
    }),
  savePositions: (items: { id: number; x: number; y: number }[]) =>
    req<{ success: boolean }>('/api/positions', {
      method: 'POST',
      body: JSON.stringify({ items }),
    }),
  clearPositions: (mapId: number) =>
    req<{ success: boolean }>('/api/map/clear-positions', {
      method: 'POST',
      body: JSON.stringify({ id: mapId }),
    }),
  restoreMap: (mapId: number, layout: Layout, nodes: RestoreNode[]) =>
    req<{ success: boolean }>('/api/map/restore', {
      method: 'POST',
      body: JSON.stringify({ map_id: mapId, layout, nodes }),
    }),
  importMap: (title: string, layout: Layout, children: ImportNode[]) =>
    req<{ id: number; rootId: number; added: number }>('/api/maps/import', {
      method: 'POST',
      body: JSON.stringify({ title, layout, children }),
    }),

  aiNote: (nodeId: number) =>
    req<{ note: string; model: string }>('/api/node/ai-note', {
      method: 'POST',
      body: JSON.stringify({ node_id: nodeId }),
    }),
  generate: (
    parentId: number,
    opts: { topic?: string; instruction?: string; source?: string; replace?: boolean } = {},
  ) =>
    req<{ added: number; model: string }>('/api/generate', {
      method: 'POST',
      body: JSON.stringify({ parent_id: parentId, ...opts }),
    }),

  // ---- chat sessions ----
  sessions: (mapId: number) => req<ChatSession[]>(`/api/chat/sessions?map_id=${mapId}`),
  createSession: (mapId: number, title?: string) =>
    req<{ id: number; title: string }>('/api/chat/sessions', {
      method: 'POST',
      body: JSON.stringify({ map_id: mapId, title }),
    }),
  renameSession: (id: number, title: string) =>
    req<{ success: boolean }>('/api/chat/session/rename', {
      method: 'POST',
      body: JSON.stringify({ id, title }),
    }),
  deleteSession: (id: number) =>
    req<{ success: boolean }>('/api/chat/session/delete', {
      method: 'POST',
      body: JSON.stringify({ id }),
    }),
  messages: (sessionId: number) => req<ChatMessageRow[]>(`/api/chat/messages?session_id=${sessionId}`),
  chatSend: (sessionId: number, content: string, mapOutline: string | null) =>
    req<{ text: string; model: string }>('/api/chat', {
      method: 'POST',
      body: JSON.stringify({ session_id: sessionId, content, map_outline: mapOutline }),
    }),

  /** Upload a file → extracted text (OCR for images via the daemon). */
  importFile: async (file: File) => {
    const fd = new FormData()
    fd.append('file', file)
    const res = await fetch('/api/import', { method: 'POST', body: fd })
    const data = await res.json().catch(() => ({}))
    if (!res.ok) throw new Error((data as { error?: string }).error ?? `HTTP ${res.status}`)
    return data as { text: string; name: string; chars: number; ocr: boolean }
  },

  models: () => req<{ activeId: string | null; configs: ModelInfo[] }>('/api/models'),
  setModel: (id: string) =>
    req<{ success: boolean; activeId: string }>('/api/model-active', {
      method: 'POST',
      body: JSON.stringify({ id }),
    }),
}

/** Flatten a tree into a plain-text markdown outline (grounds the chat). */
export function outline(node: TreeNode | null, depth = 0): string {
  if (!node) return ''
  let out = `${'  '.repeat(depth)}- ${node.text}\n`
  for (const c of node.children) out += outline(c, depth + 1)
  return out
}
