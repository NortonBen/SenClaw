// Tiny fetch wrapper for the Thinking app REST API (served from the same
// origin in production; proxied to :4650 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export interface Status {
  ok: boolean
  problems_total: number
  open: number
  analyzing: number
  decided: number
  attention_count: number
}

export interface Problem {
  id: number
  title: string
  description: string
  context: string
  goal: string
  priority: string
  status: string
  tags: string
  synthesis: string
  decision: string
  decided_solution_id: number | null
  created_at: number
  updated_at: number
  w_filled: number
  hats_filled: number
  solution_count: number
  completeness: number
}

export interface Entry {
  content: string
  source: string
  updated_at: number
}

export interface Evaluation {
  benefit: number
  risk: number
  feasibility: number
  effort: number
  overall: number
  verdict: string
  detail: string
  source: string
  updated_at: number
}

export interface Solution {
  id: number
  problem_id: number
  title: string
  description: string
  status: string
  source: string
  created_at: number
  updated_at: number
  evaluation: Evaluation | null
}

export interface Detail {
  problem: Problem
  five_w: Record<string, Entry>
  hats: Record<string, Entry>
  solutions: Solution[]
  error?: string
}

export interface Compare {
  problem_id: number
  title: string
  ranked: Solution[]
  unevaluated: { id: number; title: string }[]
  best: Solution | null
  error?: string
}

export interface Dashboard {
  problems_total: number
  by_status: { open: number; analyzing: number; decided: number; closed: number }
  solutions_total: number
  recent: Problem[]
  attention: Problem[]
  activity: ActivityRow[]
}

export interface ActivityRow {
  kind: string
  text: string
  ref: string
  created_at: number
}

export const W_KEYS = ['who', 'what', 'when', 'where', 'why'] as const
export const W_LABELS: Record<string, string> = {
  who: 'Who — Ai liên quan',
  what: 'What — Vấn đề là gì',
  when: 'When — Khi nào xảy ra',
  where: 'Where — Xảy ra ở đâu',
  why: 'Why — Tại sao xảy ra',
}

export const HAT_KEYS = ['white', 'red', 'black', 'yellow', 'green', 'blue'] as const
export const HAT_LABELS: Record<string, string> = {
  white: '⚪ Mũ Trắng — Dữ kiện & số liệu',
  red: '🔴 Mũ Đỏ — Cảm xúc & trực giác',
  black: '⚫ Mũ Đen — Rủi ro & phản biện',
  yellow: '🟡 Mũ Vàng — Lợi ích & giá trị',
  green: '🟢 Mũ Xanh Lá — Sáng tạo & lựa chọn mới',
  blue: '🔵 Mũ Xanh Dương — Điều phối & tổng kết',
}
export const HAT_COLORS: Record<string, string> = {
  white: '#9ca3af',
  red: '#ef4444',
  black: '#6b7280',
  yellow: '#f59e0b',
  green: '#22c55e',
  blue: '#3b82f6',
}

export const STATUS_LABELS: Record<string, string> = {
  open: 'Mới',
  analyzing: 'Đang phân tích',
  decided: 'Đã quyết định',
  closed: 'Đã đóng',
}
export const STATUS_COLORS: Record<string, string> = {
  open: 'gold',
  analyzing: 'processing',
  decided: 'success',
  closed: 'default',
}

export const PRIORITY_LABELS: Record<string, string> = {
  low: 'Thấp',
  normal: 'Bình thường',
  high: 'Cao',
}
export const PRIORITY_COLORS: Record<string, string> = {
  low: 'default',
  normal: 'blue',
  high: 'red',
}

export const SOLUTION_STATUS_LABELS: Record<string, string> = {
  proposed: 'Đề xuất',
  chosen: '✅ Đã chọn',
  rejected: 'Loại',
}

export function fmtTime(ts: number): string {
  if (!ts) return '—'
  return new Date(ts * 1000).toLocaleString('vi-VN', { hour12: false })
}

export const api = {
  status: () => j<Status>('/api/status'),
  dashboard: () => j<Dashboard>('/api/dashboard'),
  activity: () => j<{ activity: ActivityRow[] }>('/api/activity'),
  problems: (q: { q?: string; status?: string } = {}) => {
    const p = new URLSearchParams()
    if (q.q) p.set('q', q.q)
    if (q.status) p.set('status', q.status)
    return j<{ problems: Problem[] }>(`/api/problems?${p}`)
  },
  problemAdd: (body: Partial<Problem>) =>
    j<{ ok?: boolean; problem?: Problem; error?: string }>('/api/problems', { method: 'POST', body: JSON.stringify(body) }),
  problemGet: (id: number) => j<Detail>(`/api/problems/${id}`),
  problemUpdate: (id: number, patch: Partial<Problem>) =>
    j<{ ok?: boolean; problem?: Problem; error?: string }>(`/api/problems/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  problemDelete: (id: number) =>
    j<{ ok?: boolean; error?: string }>(`/api/problems/${id}/delete`, { method: 'POST' }),
  wSet: (id: number, body: Record<string, string>) =>
    j<Detail>(`/api/problems/${id}/w`, { method: 'POST', body: JSON.stringify(body) }),
  wGenerate: (id: number, force = false) =>
    j<{ ok?: boolean; filled?: string[]; note?: string; error?: string }>(`/api/problems/${id}/w/generate`, {
      method: 'POST',
      body: JSON.stringify({ force }),
    }),
  hatsSet: (id: number, body: Record<string, string>) =>
    j<Detail>(`/api/problems/${id}/hats`, { method: 'POST', body: JSON.stringify(body) }),
  hatsGenerate: (id: number, hat = '', force = false) =>
    j<{ ok?: boolean; filled?: string[]; note?: string; error?: string }>(`/api/problems/${id}/hats/generate`, {
      method: 'POST',
      body: JSON.stringify({ hat, force }),
    }),
  solutionAdd: (id: number, body: { title: string; description?: string }) =>
    j<{ ok?: boolean; solution?: Solution; error?: string }>(`/api/problems/${id}/solutions`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  solutionsGenerate: (id: number, count = 3) =>
    j<{ ok?: boolean; added?: any[]; error?: string }>(`/api/problems/${id}/solutions/generate`, {
      method: 'POST',
      body: JSON.stringify({ count }),
    }),
  solutionUpdate: (id: number, patch: Partial<Solution>) =>
    j<{ ok?: boolean; solution?: Solution; error?: string }>(`/api/solutions/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  solutionDelete: (id: number) =>
    j<{ ok?: boolean; error?: string }>(`/api/solutions/${id}/delete`, { method: 'POST' }),
  solutionEvaluate: (id: number, body: Partial<Evaluation> = {}) =>
    j<{ ok?: boolean; solution?: Solution; model?: string; error?: string }>(`/api/solutions/${id}/evaluate`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  compare: (id: number) => j<Compare>(`/api/problems/${id}/compare`),
  decide: (id: number, solution_id: number, rationale: string) =>
    j<{ ok?: boolean; error?: string }>(`/api/problems/${id}/decide`, {
      method: 'POST',
      body: JSON.stringify({ solution_id, rationale }),
    }),
  analyze: (id: number, question = '') =>
    j<{ ok?: boolean; synthesis?: string; steps?: any[]; error?: string }>(`/api/problems/${id}/analyze`, {
      method: 'POST',
      body: JSON.stringify({ question }),
    }),
  report: (id: number) => j<{ report?: string; error?: string }>(`/api/problems/${id}/report`),
}
