// Typed fetch client for the mini-browser backend REST API.

export interface TabInfo { index: number; url: string; title: string; active: boolean }
export interface SnapEl { idx: number; tag: string; type?: string; role?: string; text: string; x: number; y: number; w: number; h: number }
export interface Snapshot { url: string; title: string; count: number; elements: SnapEl[]; text: string }
export interface HistRow { id: number; url: string; title: string; at: number }

async function post<T = any>(path: string, body?: any): Promise<T> {
  const r = await fetch(`/api${path}`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body ?? {}),
  })
  if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error || r.statusText)
  return r.json()
}
async function get<T = any>(path: string): Promise<T> {
  const r = await fetch(`/api${path}`)
  if (!r.ok) throw new Error(r.statusText)
  return r.json()
}

export const api = {
  navigate: (url: string) => post('/navigate', { url }),
  back: () => post('/back'),
  forward: () => post('/forward'),
  reload: () => post('/reload'),
  info: () => get<{ url: string; title: string }>('/info'),
  snapshot: () => get<Snapshot>('/snapshot'),
  tabs: () => get<{ tabs: TabInfo[]; active: number }>('/tabs'),
  newTab: (url?: string) => post('/tabs/new', { url }),
  switchTab: (index: number) => post('/tabs/switch', { index }),
  closeTab: (index: number) => post('/tabs/close', { index }),
  history: () => get<HistRow[]>('/history'),
  bookmarks: () => get<HistRow[]>('/bookmarks'),
  addBookmark: (url: string, title: string) => post('/bookmark', { url, title }),
  removeBookmark: (url: string) => post('/bookmark/remove', { url }),
  chat: (messages: { role: string; content: string }[], page_context?: string) =>
    post<{ answer: string; model: string }>('/chat', { messages, page_context }),
  act: (instruction: string, max_steps?: number) =>
    post<any>('/act', { instruction, max_steps }),
  extract: (request: string) => post<{ answer: string; model: string }>('/extract', { request }),
}

export function connectLiveView(onFrame: (f: { data: string; url: string; title: string }) => void): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const ws = new WebSocket(`${proto}://${location.host}/api/ws`)
  ws.onmessage = (ev) => {
    try {
      const m = JSON.parse(ev.data)
      if (m.type === 'frame') onFrame(m)
    } catch {}
  }
  return ws
}
