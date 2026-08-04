// Tiny fetch wrapper for the Capital app REST API (served from the same origin
// in production; proxied to :4600 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export interface Status {
  ok: boolean
  sources_active: number
  debt_outstanding: number
  overdue_count: number
}

export interface Source {
  id: number
  name: string
  kind: string
  provider: string
  total_amount: number
  currency: string
  interest_rate: number
  rate_type: string
  start_date: string
  end_date: string
  status: string
  note: string
  disbursed: number
  repaid_principal: number
  interest_paid: number
  fees_paid: number
  outstanding: number
  available: number
  is_debt: boolean
}

export interface Tx {
  id: number
  source_id: number
  source_name: string
  alloc_id: number | null
  alloc_name: string | null
  kind: string
  amount: number
  tx_date: string
  note: string
  currency: string
}

export interface ScheduleItem {
  id: number
  source_id: number
  source_name: string
  seq: number
  due_date: string
  principal_due: number
  interest_due: number
  total_due: number
  status: string
  paid_at: number | null
  currency: string
}

export interface Alloc {
  id: number
  name: string
  description: string
  target_amount: number
  status: string
  used: number
  remaining: number
}

export interface Dashboard {
  today: string
  sources_active: number
  sources_total: number
  equity_in: number
  debt_outstanding: number
  total_committed: number
  total_disbursed: number
  available: number
  interest_paid: number
  fees_paid: number
  weighted_debt_rate: number
  de_ratio: number | null
  upcoming_30d: { count: number; total_due: number; items: ScheduleItem[] }
  overdue: { count: number; total_due: number; items: ScheduleItem[] }
  cashflow_12m: CashflowRow[]
  sources: Source[]
}

export interface CashflowRow {
  month: string
  inflow: number
  repay_principal: number
  repay_interest: number
  fees: number
  outflow: number
  net: number
}

export interface Finding {
  severity: 'good' | 'warn' | 'crit'
  title: string
  detail: string
}

export interface Insight {
  score: number
  grade: string
  label: string
  today: string
  metrics: {
    debt_outstanding: number
    equity_in: number
    available: number
    weighted_debt_rate: number
    de_ratio: number | null
    due_30d: number
    due_90d: number
  }
  findings: Finding[]
}

export interface SimSide {
  debt_outstanding: number
  de_ratio: number | null
  weighted_debt_rate: number
  due_30d: number
  score: number
  grade: string
  monthly_due_12m: { month: string; total_due: number }[]
}

export interface SimResult {
  error?: string
  scenario?: string
  loan?: {
    first_payment: number
    total_interest: number
    total_cost: number
    schedule_preview: { seq: number; due_date: string; principal: number; interest: number }[]
  }
  estimate?: { interest_saved: number; note: string }
  before?: SimSide
  after?: SimSide
}

export interface GoalStep {
  id: number
  seq: number
  title: string
  due_date: string
  amount: number
  status: 'todo' | 'done'
  source: 'manual' | 'ai' | 'auto'
}

export interface Goal {
  id: number
  name: string
  kind: string
  target_amount: number
  baseline: number
  source_id: number | null
  deadline: string
  status: string
  note: string
  created_date: string
  current: number
  progress_pct: number
  elapsed_pct: number
  remaining: number
  months_left: number
  pace_per_month: number
  eval_status: string
  steps: GoalStep[]
}

export interface UsageSignal {
  severity: 'good' | 'warn' | 'crit'
  title: string
  detail: string
}

export interface Usage {
  total_disbursed: number
  allocated: number
  unallocated: number
  unallocated_pct: number
  by_allocation: {
    id: number
    name: string
    status: string
    used: number
    target_amount: number
    share_pct: number
    budget_used_pct: number | null
    over_budget: boolean
  }[]
  by_source: {
    id: number
    name: string
    kind: string
    committed: number
    disbursed: number
    utilization_pct: number
    idle: number
  }[]
  signals: UsageSignal[]
}

export interface RatingFactor {
  impact: '+' | '-' | '0'
  delta: number
  text: string
}

export interface SourceRating {
  id: number
  name: string
  kind: string
  is_debt: boolean
  outstanding: number
  interest_rate: number
  score: number
  grade: string
  verdict: string
  factors: RatingFactor[]
}

export const GOAL_KIND_LABELS: Record<string, string> = {
  reduce_debt: 'Giảm dư nợ',
  payoff_source: 'Tất toán khoản vay',
  raise_equity: 'Tăng vốn chủ',
  raise_funding: 'Huy động vốn',
  build_reserve: 'Tăng dự phòng khả dụng',
}

export const GOAL_STATUS_LABELS: Record<string, { label: string; color: string }> = {
  on_track: { label: 'đúng tiến độ', color: 'green' },
  behind: { label: 'chậm tiến độ', color: 'orange' },
  at_risk: { label: 'nguy cơ trễ', color: 'red' },
  achieved: { label: 'đã đạt', color: 'green' },
  overdue: { label: 'quá hạn', color: 'red' },
  in_progress: { label: 'đang thực hiện', color: 'blue' },
  done: { label: 'hoàn thành', color: 'green' },
  cancelled: { label: 'đã huỷ', color: 'default' },
}

export const SOURCE_KIND_LABELS: Record<string, string> = {
  equity: 'Vốn chủ sở hữu',
  investor: 'Vốn góp NĐT',
  bank_loan: 'Vay ngân hàng',
  credit_line: 'Hạn mức tín dụng',
  personal_loan: 'Vay cá nhân',
  bond: 'Trái phiếu',
  grant: 'Tài trợ',
  other: 'Khác',
}

export const TX_KIND_LABELS: Record<string, string> = {
  disburse: 'Giải ngân / nhận vốn',
  repay_principal: 'Trả gốc',
  repay_interest: 'Trả lãi',
  fee: 'Phí',
}

export function fmtMoney(v: number | null | undefined, currency = 'VND'): string {
  if (v === null || v === undefined) return '—'
  const s = new Intl.NumberFormat('vi-VN', { maximumFractionDigits: 2 }).format(v)
  return `${s} ${currency === 'VND' ? 'đ' : currency}`
}

export const api = {
  status: () => j<Status>('/api/status'),
  dashboard: () => j<Dashboard>('/api/dashboard'),
  sources: (status?: string) => j<{ sources: Source[] }>(`/api/sources${status ? `?status=${status}` : ''}`),
  sourceGet: (id: number) =>
    j<{ source: Source; transactions: Tx[]; schedule: ScheduleItem[]; error?: string }>(`/api/sources/${id}`),
  sourceAdd: (body: Partial<Source>) =>
    j<{ ok?: boolean; source?: Source; error?: string }>('/api/sources', { method: 'POST', body: JSON.stringify(body) }),
  sourceUpdate: (id: number, patch: Partial<Source>) =>
    j<{ ok?: boolean; source?: Source; error?: string }>(`/api/sources/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  txList: (q: { source_id?: number; kind?: string; alloc_id?: number } = {}) => {
    const p = new URLSearchParams()
    if (q.source_id) p.set('source_id', String(q.source_id))
    if (q.kind) p.set('kind', q.kind)
    if (q.alloc_id) p.set('alloc_id', String(q.alloc_id))
    return j<{ transactions: Tx[] }>(`/api/transactions?${p}`)
  },
  txAdd: (body: { source_id: number; kind: string; amount: number; tx_date?: string; alloc_id?: number; note?: string }) =>
    j<{ ok?: boolean; tx_id?: number; error?: string }>('/api/transactions', { method: 'POST', body: JSON.stringify(body) }),
  txDelete: (id: number) => j<{ ok?: boolean; error?: string }>(`/api/transactions/${id}/delete`, { method: 'POST' }),
  schedule: (q: { source_id?: number; status?: string } = {}) => {
    const p = new URLSearchParams()
    if (q.source_id) p.set('source_id', String(q.source_id))
    if (q.status) p.set('status', q.status)
    return j<{ schedule: ScheduleItem[] }>(`/api/schedule?${p}`)
  },
  scheduleGenerate: (body: {
    source_id: number
    method: string
    periods: number
    principal?: number
    annual_rate?: number
    start_date?: string
    freq_months?: number
  }) => j<{ ok?: boolean; schedule?: ScheduleItem[]; error?: string }>('/api/schedule/generate', { method: 'POST', body: JSON.stringify(body) }),
  schedulePay: (id: number, create_tx = true) =>
    j<{ ok?: boolean; error?: string }>(`/api/schedule/${id}/pay`, { method: 'POST', body: JSON.stringify({ create_tx }) }),
  allocs: () => j<{ allocations: Alloc[] }>('/api/allocations'),
  allocAdd: (body: { name: string; description?: string; target_amount?: number }) =>
    j<{ ok?: boolean; alloc_id?: number; error?: string }>('/api/allocations', { method: 'POST', body: JSON.stringify(body) }),
  allocUpdate: (id: number, patch: Partial<Alloc>) =>
    j<{ ok?: boolean; error?: string }>(`/api/allocations/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  cashflow: (months = 12) => j<{ cashflow: CashflowRow[] }>(`/api/report/cashflow?months=${months}`),
  insight: () => j<Insight>('/api/insight'),
  usage: () => j<Usage>('/api/report/usage'),
  ratings: () => j<{ weighted_debt_rate: number; ratings: SourceRating[] }>('/api/report/source-ratings'),
  goals: (status?: string) => j<{ goals: Goal[] }>(`/api/goals${status ? `?status=${status}` : ''}`),
  goalAdd: (body: { name: string; kind: string; target_amount?: number; deadline?: string; source_id?: number; note?: string }) =>
    j<{ ok?: boolean; goal?: Goal; error?: string }>('/api/goals', { method: 'POST', body: JSON.stringify(body) }),
  goalUpdate: (id: number, patch: Record<string, unknown>) =>
    j<{ ok?: boolean; goal?: Goal; error?: string }>(`/api/goals/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  goalPlan: (id: number, ai = true) =>
    j<{ ok?: boolean; source?: string; model?: string; steps?: GoalStep[]; error?: string }>(`/api/goals/${id}/plan`, {
      method: 'POST',
      body: JSON.stringify({ ai }),
    }),
  goalStep: (
    goalId: number,
    body: { action: 'add' | 'done' | 'todo' | 'delete'; step_id?: number; title?: string; due_date?: string; amount?: number },
  ) => j<{ ok?: boolean; steps?: GoalStep[]; error?: string }>(`/api/goals/${goalId}/steps`, { method: 'POST', body: JSON.stringify(body) }),
  simulate: (body: {
    scenario: 'new_loan' | 'early_repay'
    amount: number
    annual_rate?: number
    periods?: number
    method?: string
    freq_months?: number
    source_id?: number
  }) => j<SimResult>('/api/simulate', { method: 'POST', body: JSON.stringify(body) }),
  analyze: (question = '') =>
    j<{ analysis: string; model: string }>('/api/analyze', { method: 'POST', body: JSON.stringify({ question }) }),
  activity: () => j<{ activity: any[] }>('/api/activity'),
}
