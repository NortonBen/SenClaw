export interface Team {
  key: string
  name: string
  description: string
  sort: number
}

export interface Agent {
  key: string
  name: string
  role: string
  duty: string
  kind: 'manager' | 'worker' | 'qa' | string
  team: string
  enabled: boolean
  auto_assign: boolean
  skills: string[]
  status: 'idle' | 'working' | 'done' | 'handoff' | string
  status_note: string
  sort: number
}

export interface KnowledgeSummary {
  space: string
  count: number
}

export interface InventoryItem {
  name: string
  description: string
}

export interface SkillsInventory {
  skills: InventoryItem[]
  personas: InventoryItem[]
}

export interface OfficeFeatures {
  memory: boolean
  wiki: boolean
  workspace: boolean
  tools: boolean
  autocontinue: boolean
}

export interface OfficeSettings {
  workspaceDir: string
  workspaceFiles: number
  workspaceIsDefault: boolean
  features: OfficeFeatures
}

export interface WorkspaceFile {
  rel: string
  size: number
  modified: number
  text: boolean
}

export interface WorkspaceListing {
  dir: string
  files: WorkspaceFile[]
}

export interface DirListing {
  path: string
  parent: string | null
  home: string
  dirs: string[]
}

export interface Task {
  id: number
  title: string
  mode: 'demo' | 'live' | string
  team: string
  status: 'pending' | 'planning' | 'running' | 'review' | 'done' | 'error' | string
  report: string
  llm_calls: number
  llm_model: string
  tokens_in: number
  tokens_out: number
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
  tokensIn: number
  tokensOut: number
  lastModel: string
}
