// Typed fetch client for the mini-browser backend REST API.

export interface TabInfo { index: number; url: string; title: string; active: boolean }
/** The accessibility tree, as the AI sees it. `tree` is the rendered text. */
export interface Snapshot {
  url: string; title: string; count: number; new: number; truncated: boolean; tree: string
}
export interface HistRow { id: number; url: string; title: string; at: number }
export interface Dialog { type: string; message: string; defaultText?: string }
export interface ConsoleLine { level: string; text: string; at: number }
export interface NetRow { method: string; url: string; type: string; status?: number | string; failed?: string }
/** One live-view frame. `w`/`h` are the page's real viewport, used to map clicks. */
export interface Frame { data: string; url: string; title: string; w: number; h: number }
export interface ChatRow { id: number; role: string; content: string; run_id: number | null; at: number }
export interface RunRow {
  id: number; goal: string; status: string; plans_used: number; outcome: string
  verified: boolean | null; source: string; started_at: number; finished_at: number | null
}
/// One thing the agent learned from a run that worked.
export interface Lesson {
  id: number; host: string; note: string; kind: string
  uses: number; wins: number; losses: number; run_id: number | null; at: number
}
export interface Settings {
  max_plans: number; hard_max_plans: number; default_max_plans: number
  learning: boolean; headful: boolean; accept_language: string
}
export interface StepRow {
  id: number; plan_no: number; step_no: number; kind: string; detail: string; ok: boolean; at: number
}
/** Live progress from a run: plan made, step started, action taken, check done. */
export interface AgentEvent {
  type: 'agent'; run?: number; kind: string; plan?: number
  body?: { step?: number; detail?: string; ok?: boolean; text?: string; goal?: string }
}

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
    post<{ answer: string; mode: string; run?: number; achieved?: boolean; plans_used?: number }>(
      '/chat', { messages, page_context }),
  chatHistory: () => get<ChatRow[]>('/chat/history'),
  chatClear: () => post('/chat/clear'),
  act: (instruction: string) => post<any>('/act', { instruction }),
  runs: () => get<RunRow[]>('/act/runs'),
  run: (id: number) => get<{ run: RunRow; steps: StepRow[] }>(`/act/run/${id}`),
  settings: () => get<Settings>('/settings'),
  saveSettings: (patch: { max_plans?: number; learning?: boolean }) =>
    post<{ max_plans: number; learning: boolean }>('/settings', patch),
  takeover: () => get<{ takeover: boolean; remaining: number | null }>('/takeover'),
  pingTakeover: () => post<{ takeover: boolean; remaining: number | null }>('/takeover/ping'),
  setTakeover: (on: boolean, url?: string) =>
    post<{ takeover: boolean; note: string; url: string }>('/takeover', { on, url }),
  knowledge: () => get<Lesson[]>('/knowledge'),
  forgetLesson: (id: number) => post('/knowledge/forget', { id }),
  extract: (request: string, schema?: string) =>
    post<{ answer: string; model: string }>('/extract', { request, schema }),
  find: (text: string) => post<{ matches: string | null }>('/find', { text }),
  console: () => get<ConsoleLine[]>('/console'),
  network: () => get<NetRow[]>('/network'),
  answerDialog: (accept: boolean, prompt_text?: string) =>
    post('/dialog', { accept, prompt_text }),
  highlight: (ref: string, ms?: number) => post('/highlight', { ref, ms }),
}

export function connectLiveView(
  onFrame: (f: Frame) => void,
  onDialog?: (d: Dialog | null) => void,
  onAgent?: (e: AgentEvent) => void,
): WebSocket {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const ws = new WebSocket(`${proto}://${location.host}/api/ws`)
  ws.onmessage = (ev) => {
    try {
      const m = JSON.parse(ev.data)
      // While a dialog is up the renderer is suspended, so no frames arrive at
      // all. The server sends this instead, which is the only thing that keeps
      // the view from looking simply frozen.
      if (m.type === 'dialog') onDialog?.(m.dialog)
      else if (m.type === 'agent') onAgent?.(m)
      else if (m.type === 'frame') { onDialog?.(null); onFrame(m) }
    } catch {}
  }
  return ws
}
