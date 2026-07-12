// Typed fetch client for the CRM backend (/api/*).

export interface Customer {
  id: number
  name: string
  email: string
  phone: string
  company: string
  title: string
  avatar_url: string
  notes: string
  tags: string[]
  role: string
  source: string
  address: string
  birthday: string
  created_at: number
  updated_at: number
  interaction_count: number
  last_interaction_at: number | null
}

export interface Relationship {
  id: number
  from_id: number
  from_name: string
  to_id: number
  to_name: string
  kind: string
  note: string
  confidence: number
  source: string
  created_at: number
}

export interface GraphNode {
  id: number
  name: string
  role: string
  company: string
  avatar_url: string
  interaction_count: number
}

export interface SearchHit {
  entity_type: 'customer' | 'interaction' | 'mention'
  entity_id: number
  customer_id: number | null
  customer_name: string | null
  title: string
  snippet: string
}

export interface CustomerChannel {
  id: number
  customer_id: number
  kind: string
  value: string
  label: string
  created_at: number
  updated_at: number
}

export interface Mention {
  id: number
  source_customer_id: number
  source_customer_name: string
  name: string
  role_guess: string
  kind_guess: string
  context: string
  confidence: number
  resolved_customer_id: number | null
  created_at: number
}

export interface Interaction {
  id: number
  customer_id: number
  kind: string
  summary: string
  details: string
  occurred_at: number
  created_at: number
}

export interface Stats {
  customers: number
  interactions: number
  open_tasks: number
  overdue_tasks: number
  open_deals: number
  pipeline_value: number
  won_value: number
  by_role: Record<string, number>
  by_stage: Record<string, { count: number; value: number }>
}

export interface Deal {
  id: number
  customer_id: number
  customer_name: string
  title: string
  amount: number
  currency: string
  stage: string
  probability: number
  expected_close_at: number | null
  closed_at: number | null
  notes: string
  created_at: number
  updated_at: number
}

export interface Task {
  id: number
  customer_id: number | null
  customer_name: string | null
  title: string
  details: string
  due_at: number | null
  done: boolean
  done_at: number | null
  created_at: number
  updated_at: number
}

export interface ActivityItem {
  id: number
  customer_id: number
  customer_name: string
  kind: string
  summary: string
  details: string
  occurred_at: number
}

export interface Upcoming {
  now: number
  window_days: number
  tasks: Array<{ id: number; title: string; due_at: number; customer_id: number | null; customer_name: string | null }>
  birthdays: Array<{ customer_id: number; customer_name: string; birthday: string; next_at: number }>
}

export interface CustomerDetail {
  customer: Customer
  interactions: Interaction[]
}

export type CustomerInput = Partial<Omit<Customer, 'id' | 'created_at' | 'updated_at' | 'interaction_count' | 'last_interaction_at'>> & {
  name?: string
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`/api${path}`, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) },
  })
  if (!r.ok) {
    let msg = `HTTP ${r.status}`
    try {
      const b = await r.json()
      if (b?.error) msg = b.error
    } catch {}
    throw new Error(msg)
  }
  if (r.status === 204) return undefined as T
  return r.json() as Promise<T>
}

export const api = {
  status: () => req<{ ok: boolean }>('/status'),
  stats: () => req<Stats>('/stats'),
  tags: () => req<{ tags: string[] }>('/tags').then((r) => r.tags),
  listCustomers: (params: { q?: string; tag?: string; role?: string; limit?: number } = {}) => {
    const qs = new URLSearchParams()
    if (params.q) qs.set('q', params.q)
    if (params.tag) qs.set('tag', params.tag)
    if (params.role) qs.set('role', params.role)
    if (params.limit) qs.set('limit', String(params.limit))
    const s = qs.toString()
    return req<{ customers: Customer[]; count: number }>(`/customers${s ? '?' + s : ''}`).then((r) => r.customers)
  },
  getCustomer: (id: number) => req<CustomerDetail>(`/customers/${id}`),
  createCustomer: (body: CustomerInput) =>
    req<Customer>('/customers', { method: 'POST', body: JSON.stringify(body) }),
  updateCustomer: (id: number, patch: CustomerInput & { change_note?: string }) =>
    req<Customer>(`/customers/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteCustomer: (id: number) => req<{ ok: boolean }>(`/customers/${id}`, { method: 'DELETE' }),
  listInteractions: (id: number) =>
    req<{ interactions: Interaction[] }>(`/customers/${id}/interactions`).then((r) => r.interactions),
  addInteraction: (id: number, body: { kind: string; summary: string; details?: string; occurred_at?: number }) =>
    req<Interaction>(`/customers/${id}/interactions`, { method: 'POST', body: JSON.stringify(body) }),
  deleteInteraction: (id: number) =>
    req<{ ok: boolean }>(`/interactions/${id}`, { method: 'DELETE' }),
  summarize: (id: number) => req<{ text: string; model: string }>(`/customers/${id}/summary`, { method: 'POST' }),
  nextStep: (id: number) => req<{ text: string; model: string }>(`/customers/${id}/next-step`, { method: 'POST' }),

  listDeals: (stage?: string) => {
    const qs = stage ? `?stage=${encodeURIComponent(stage)}` : ''
    return req<{ deals: Deal[]; count: number }>(`/deals${qs}`).then((r) => r.deals)
  },
  customerDeals: (id: number) => req<{ deals: Deal[] }>(`/customers/${id}/deals`).then((r) => r.deals),
  createDeal: (customerId: number, body: Partial<Deal>) =>
    req<Deal>(`/customers/${customerId}/deals`, { method: 'POST', body: JSON.stringify(body) }),
  updateDeal: (id: number, patch: Partial<Deal> & { change_note?: string }) =>
    req<{ deal: Deal }>(`/deals/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteDeal: (id: number) => req<{ ok: boolean }>(`/deals/${id}`, { method: 'DELETE' }),

  listTasks: (params: { open_only?: boolean; limit?: number } = {}) => {
    const qs = new URLSearchParams()
    if (params.open_only !== undefined) qs.set('open_only', String(params.open_only))
    if (params.limit) qs.set('limit', String(params.limit))
    const s = qs.toString()
    return req<{ tasks: Task[] }>(`/tasks${s ? '?' + s : ''}`).then((r) => r.tasks)
  },
  customerTasks: (id: number) => req<{ tasks: Task[] }>(`/customers/${id}/tasks`).then((r) => r.tasks),
  createTask: (body: { title: string; details?: string; due_at?: number; customer_id?: number }) => {
    const { customer_id, ...rest } = body
    const path = customer_id != null ? `/customers/${customer_id}/tasks` : '/tasks'
    return req<Task>(path, { method: 'POST', body: JSON.stringify(rest) })
  },
  toggleTask: (id: number, done: boolean) =>
    req<{ ok: boolean }>(`/tasks/${id}`, { method: 'PATCH', body: JSON.stringify({ done }) }),
  deleteTask: (id: number) => req<{ ok: boolean }>(`/tasks/${id}`, { method: 'DELETE' }),

  upcoming: (days = 14) => req<Upcoming>(`/upcoming?days=${days}`),
  activity: (limit = 100) => req<{ items: ActivityItem[] }>(`/activity?limit=${limit}`).then((r) => r.items),

  customerRelationships: (id: number) =>
    req<{ relationships: Relationship[] }>(`/customers/${id}/relationships`).then((r) => r.relationships),
  createRelationship: (body: { from_id: number; to_id: number; kind: string; note?: string; confidence?: number }) =>
    req<Relationship>(`/relationships`, { method: 'POST', body: JSON.stringify({ ...body, source: 'user' }) }),
  deleteRelationship: (id: number) => req<{ ok: boolean }>(`/relationships/${id}`, { method: 'DELETE' }),
  graph: () => req<{ nodes: GraphNode[]; edges: Relationship[] }>(`/graph`),
  graphPath: (from: number, to: number) =>
    req<{ found: boolean; hops: number; path_ids: number[] | null; nodes: GraphNode[]; edges: Relationship[] }>(
      `/graph/path?from=${from}&to=${to}`,
    ),
  graphExpand: (focus: number, hops: number) =>
    req<{ focus: number; hops: number; nodes: GraphNode[]; edges: Relationship[] }>(
      `/graph/expand?focus=${focus}&hops=${hops}`,
    ),
  listChannels: (customerId: number) =>
    req<{ channels: CustomerChannel[] }>(`/customers/${customerId}/channels`).then((r) => r.channels),
  addChannel: (customerId: number, body: { kind: string; value: string; label?: string }) =>
    req<CustomerChannel>(`/customers/${customerId}/channels`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  updateChannel: (id: number, patch: { kind?: string; value?: string; label?: string }) =>
    req<{ ok: boolean }>(`/channels/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteChannel: (id: number) => req<{ ok: boolean }>(`/channels/${id}`, { method: 'DELETE' }),

  pathAi: (from: number, to: number) =>
    req<{
      from: number
      to: number
      model: string
      summary: string
      connections: Array<{ type: string; detail: string; strength: string }>
      bfs_path_ids: number[] | null
      bfs_path_names: string[] | null
    }>(`/graph/path_ai?from=${from}&to=${to}`, { method: 'POST' }),
  getState: <T = unknown>(key: string) =>
    req<{ key: string; value: T | null }>(`/state/${encodeURIComponent(key)}`).then((r) => r.value),
  putState: (key: string, value: unknown) =>
    req<{ ok: boolean }>(`/state/${encodeURIComponent(key)}`, {
      method: 'PUT',
      body: JSON.stringify(value),
    }),
  deleteState: (key: string) =>
    req<{ ok: boolean }>(`/state/${encodeURIComponent(key)}`, { method: 'DELETE' }),
  findCommon: (id: number) =>
    req<{
      focus_id: number
      model: string
      themes: Array<{ theme: string; why: string; customer_ids: number[] }>
      highlight_ids: number[]
    }>(`/customers/${id}/find_common`, { method: 'POST' }),
  similar: (id: number) =>
    req<{
      similar: Array<{ customer: Customer; score: number; reasons: string[] }>
      count: number
    }>(`/customers/${id}/similar`),
  search: (q: string, limit = 30) => {
    const qs = new URLSearchParams({ q, limit: String(limit) })
    return req<{ hits: SearchHit[]; count: number }>(`/search?${qs}`).then((r) => r.hits)
  },
  mentions: (unresolved_only = false, limit = 100) => {
    const qs = new URLSearchParams({ unresolved_only: String(unresolved_only), limit: String(limit) })
    return req<{ mentions: Mention[] }>(`/mentions?${qs}`).then((r) => r.mentions)
  },
  extract: (id: number) =>
    req<{ model: string; extracted: number; mentions_saved: number; relationships_created: number; resolved: Array<{ name: string; resolved_customer_id: number; kind: string; confidence: number }> }>(
      `/customers/${id}/extract`,
      { method: 'POST' },
    ),

  aggregateReport: () =>
    req<{
      text: string
      model: string
      generated_at: number
      grounding: {
        customers: number
        open_deals: number
        pipeline_value: number
        top_deals: number
        recent_events: number
        overdue_tasks: number
      }
    }>(`/report`, { method: 'POST' }),
}

export function formatMoney(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(amount)
  } catch {
    return `${amount.toLocaleString()} ${currency}`
  }
}

export function fmtDate(secs: number | null | undefined): string {
  if (!secs) return '—'
  const d = new Date(secs * 1000)
  return d.toLocaleDateString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit' })
}

export function fmtDateTime(secs: number | null | undefined): string {
  if (!secs) return '—'
  const d = new Date(secs * 1000)
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/// Deterministic initials for the avatar fallback (skip diacritics-only words).
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0]!.slice(0, 2).toUpperCase()
  return (parts[0]![0]! + parts[parts.length - 1]![0]!).toUpperCase()
}

/// Deterministic HSL hue for a customer, from the name → stable colour without
/// pulling a colour library.
export function hueFromName(name: string): number {
  let h = 0
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) | 0
  return ((h % 360) + 360) % 360
}
