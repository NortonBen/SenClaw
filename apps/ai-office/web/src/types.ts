export interface Agent {
  key: string
  name: string
  role: string
  duty: string
  status: 'idle' | 'working' | 'done' | 'handoff' | string
  status_note: string
  sort: number
}

export interface Task {
  id: number
  title: string
  mode: 'demo' | 'live' | string
  status: 'pending' | 'planning' | 'running' | 'review' | 'done' | 'error' | string
  report: string
  llm_calls: number
  llm_model: string
  created_at: number
  finished_at: number | null
}

export interface Step {
  id: number
  task_id: number
  agent_key: string
  title: string
  status: string
  result: string
  ord: number
}

export interface OfficeEvent {
  id: number
  task_id: number | null
  kind: 'chat' | 'assign' | 'handoff' | 'report' | 'bubble' | 'system' | string
  actor: string
  target: string
  text: string
  created_at: number
}

export interface Stats {
  tasksTotal: number
  tasksDone: number
  tasksLive: number
  llmCalls: number
  lastModel: string
}
