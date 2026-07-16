import type {
  Agent,
  DirListing,
  KnowledgeSummary,
  OfficeEvent,
  OfficeFeatures,
  OfficeSettings,
  SkillsInventory,
  Stats,
  Step,
  Task,
  Team,
  WorkspaceListing,
} from './types'

const BASE = 'api'

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}/${path}`)
  if (!res.ok) throw new Error(`${res.status} ${await res.text()}`)
  return res.json() as Promise<T>
}

async function send<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${BASE}/${path}`, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) {
    let msg = `${res.status}`
    try {
      const j = await res.json()
      if (j.error) msg = j.error
    } catch {
      /* keep status code */
    }
    throw new Error(msg)
  }
  return res.json() as Promise<T>
}

export const api = {
  teams: () => get<{ teams: Team[] }>('teams'),
  addTeam: (body: { name: string; description?: string }) =>
    send<{ team: Team }>('POST', 'teams', body),
  updateTeam: (key: string, patch: { name?: string; description?: string }) =>
    send<{ ok: boolean }>('PATCH', `teams/${key}`, patch),
  deleteTeam: (key: string) => send<{ ok: boolean }>('DELETE', `teams/${key}`),
  agents: () => get<{ agents: Agent[] }>('agents'),
  updateAgent: (
    key: string,
    patch: Partial<Pick<Agent, 'name' | 'role' | 'duty' | 'enabled' | 'auto_assign' | 'skills'>>,
  ) => send<{ ok: boolean }>('PATCH', `agents/${key}`, patch),
  addAgent: (body: { name: string; role: string; duty: string; kind: string; team: string }) =>
    send<{ agent: Agent }>('POST', 'agents', body),
  deleteAgent: (key: string) => send<{ ok: boolean }>('DELETE', `agents/${key}`),
  agentKnowledge: (key: string) => get<KnowledgeSummary>(`agents/${key}/knowledge`),
  skillsInventory: () => get<SkillsInventory>('skills-inventory'),
  tasks: (limit = 30, team?: string) =>
    get<{ tasks: Task[] }>(`tasks?limit=${limit}${team ? `&team=${encodeURIComponent(team)}` : ''}`),
  task: (id: number) => get<{ task: Task; steps: Step[] }>(`tasks/${id}`),
  createTask: (title: string, team: string) =>
    send<{ task: Task; queued: boolean }>('POST', 'tasks', { title, team }),
  events: (taskId: number, after = 0) =>
    get<{ events: OfficeEvent[] }>(`tasks/${taskId}/events?after=${after}`),
  recentEvents: (limit = 40) => get<{ events: OfficeEvent[] }>(`events/recent?limit=${limit}`),
  stats: () => get<Stats>('stats'),
  llmInfo: () => get<{ available: boolean; config?: unknown }>('llm-info'),
  settings: () => get<OfficeSettings>('settings'),
  updateSettings: (patch: { workspaceDir?: string; features?: Partial<OfficeFeatures> }) =>
    send<OfficeSettings>('POST', 'settings', patch),
  queue: () => get<{ pending: Task[] }>('queue'),
  stt: async (blob: Blob): Promise<{ text: string }> => {
    const fd = new FormData()
    fd.append('audio', blob, 'audio.wav')
    fd.append('language', 'vi')
    const res = await fetch(`${BASE}/stt`, { method: 'POST', body: fd })
    if (!res.ok) {
      const j = await res.json().catch(() => ({ error: `${res.status}` }))
      throw new Error(j.error ?? `${res.status}`)
    }
    return res.json()
  },
  tts: async (text: string): Promise<Blob> => {
    const res = await fetch(`${BASE}/tts`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ text }),
    })
    if (!res.ok) {
      const j = await res.json().catch(() => ({ error: `${res.status}` }))
      throw new Error(j.error ?? `${res.status}`)
    }
    return res.blob()
  },
  workspaceFiles: () => get<WorkspaceListing>('workspace/files'),
  fsDirs: (path?: string) =>
    get<DirListing>(`fs/dirs${path ? `?path=${encodeURIComponent(path)}` : ''}`),
}
