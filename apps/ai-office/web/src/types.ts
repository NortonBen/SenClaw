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
  status: 'inbox' | 'pending' | 'planning' | 'running' | 'review' | 'done' | 'error' | string
  report: string
  llm_calls: number
  llm_model: string
  tokens_in: number
  tokens_out: number
  /** Mục tiêu quý việc này phục vụ — null = "lạc hướng". */
  goal_id: number | null
  /** '' | 'waiting' (chờ Sếp duyệt) | 'approved' (hoàn tất) | 'returned'. */
  approval: string
  approved_at: number | null
  boss_note: string
  created_at: number
  finished_at: number | null
}

export interface KeyResult {
  text: string
  done: boolean
}

export interface Goal {
  id: number
  title: string
  quarter: string
  key_results: KeyResult[]
  archived: boolean
  created_at: number
  progress: number
  taskCount?: number
  openTaskCount?: number
}

export interface Meeting {
  id: number
  kind: 'morning' | 'evening' | string
  day: string
  content: string
  created_at: number
}

export interface Dashboard {
  date: string
  alignment: { open: number; aligned: number; percent: number | null }
  goals: { count: number; avgProgress: number }
  waiting: number
  streak: { days: number; morningToday: boolean; eveningToday: boolean }
  budget: { monthTokens: number; openTasks: number }
}

export interface Board {
  columns: { inbox: Task[]; doing: Task[]; waiting: Task[]; done: Task[] }
  goals: Record<string, { title: string; quarter: string }>
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
