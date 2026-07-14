import type { Agent, OfficeEvent, Stats, Step, Task } from './types'

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
  agents: () => get<{ agents: Agent[] }>('agents'),
  updateAgent: (key: string, patch: Partial<Pick<Agent, 'name' | 'role' | 'duty'>>) =>
    send<{ ok: boolean }>('PATCH', `agents/${key}`, patch),
  tasks: (limit = 30) => get<{ tasks: Task[] }>(`tasks?limit=${limit}`),
  task: (id: number) => get<{ task: Task; steps: Step[] }>(`tasks/${id}`),
  createTask: (title: string, mode: string) =>
    send<{ task: Task }>('POST', 'tasks', { title, mode }),
  events: (taskId: number, after = 0) =>
    get<{ events: OfficeEvent[] }>(`tasks/${taskId}/events?after=${after}`),
  recentEvents: (limit = 40) => get<{ events: OfficeEvent[] }>(`events/recent?limit=${limit}`),
  stats: () => get<Stats>('stats'),
  llmInfo: () => get<{ available: boolean; config?: unknown }>('llm-info'),
}
