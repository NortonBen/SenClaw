export interface Discussion {
  id: number
  title: string
  requirement: string
  status: 'draft' | 'running' | 'paused' | 'review' | 'done'
  mode: 'sequential' | 'parallel'
  pace_secs: number
  max_rounds: number
  round: number
  manager_score: number
  manager_missing: string[]
  created_at: number
  updated_at: number
  concluded_at: number | null
}

export interface Member {
  id: number
  key: string
  name: string
  role: 'member' | 'manager' | 'secretary'
  expertise: string
  style: string
  hat: string
  use_tools: boolean
  tools: string[] | null
  model: string | null
  enabled: boolean
  sort: number
  created_at: number
}

export interface Citation {
  kind: 'doc' | 'url' | 'tool'
  ref: string
  quote: string
  verified: boolean
}

export interface Message {
  id: number
  discussion_id: number
  round: number
  author_kind: 'boss' | 'member' | 'manager' | 'secretary' | 'system'
  member_id: number | null
  kind: 'opinion' | 'reaction' | 'boss' | 'manager_note' | 'minutes_note' | 'system' | 'result_note'
  content: string
  claim_type: 'evidence' | 'inference' | 'creative' | null
  provability: 'practical' | 'theoretical' | null
  hat: string | null
  stance: 'agree' | 'disagree' | null
  reply_to: number | null
  citations: Citation[]
  flags: Record<string, unknown>
  created_at: number
}

export interface DocMeta {
  id: number
  discussion_id: number | null
  title: string
  filename: string
  source: string
  created_by: string
  created_at: number
  preview: string
  chars: number
}

export interface MinutesRow {
  id: number
  discussion_id: number
  round: number
  content: string
  created_at: number
}

export interface ResultRow {
  id: number
  discussion_id: number
  content: string
  status: 'draft' | 'approved' | 'rejected'
  feedback: string
  created_at: number
}

export interface Participation {
  member_id: number
  key: string
  name: string
  message_count: number
  last_round: number
  silent_rounds: number
}

export interface Progress {
  status: string
  round: number
  max_rounds: number
  manager_score: number
  manager_missing: string[]
  participation: Participation[]
  open_opinions: { id: number; content: string }[]
  member_statuses: Record<string, string>
}

export interface LlmProfile {
  id: string
  name: string
  model: string
  provider: string
  active: boolean
}

export interface ToolInfo {
  server: string
  tool: string
  full: string
  description: string
  builtin: boolean
  status: string
}

/** Mũ thiên hướng lưu dạng chuỗi phẩy ("black,red") — tách thành mảng sạch. */
export function splitHats(h: string | null | undefined): string[] {
  return (h || '')
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
}

export const HAT_COLORS: Record<string, string> = {
  white: '#e8ecf1',
  red: '#e5484d',
  black: '#3a3f4b',
  yellow: '#f5c518',
  green: '#46a758',
  blue: '#4c8dff',
}

export const HAT_NAMES: Record<string, string> = {
  white: 'Mũ trắng · dữ kiện',
  red: 'Mũ đỏ · trực giác',
  black: 'Mũ đen · rủi ro',
  yellow: 'Mũ vàng · lợi ích',
  green: 'Mũ xanh lá · sáng tạo',
  blue: 'Mũ xanh dương · quy trình',
}

export const CLAIM_LABEL: Record<string, string> = {
  evidence: '🔎 dẫn chứng',
  inference: '🧠 suy diễn',
  creative: '💡 sáng tạo',
}

export const PROV_LABEL: Record<string, string> = {
  practical: 'THỰC TIỄN',
  theoretical: 'LÝ THUYẾT',
}
