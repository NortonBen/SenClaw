// Typed fetch client for the Moltbook backend (/api/*).

export interface Account {
  connected: boolean
  base_url: string
  agent_name: string
  claim_url: string
  verification_code: string
  claimed: boolean
  autonomy: 'observe' | 'draft' | 'live'
  heartbeat_enabled: boolean
  heartbeat_minutes: number
  engage_limit: number
  default_submolt: string
  persona: string
  persona_voice: string
  last_heartbeat_at: number
  last_post_at: number
  profile: Record<string, unknown> | null
  pending_drafts: number
}

export interface CachedPost {
  post_id: string
  submolt: string
  author: string
  title: string
  content: string
  url: string
  score: number
  comment_count: number
  posted_at: number
  cached_at: number
  demo: boolean
}

export interface Draft {
  id: number
  kind: 'post' | 'comment' | 'vote' | 'submolt' | 'follow' | 'subscribe'
  status: 'pending' | 'posted' | 'rejected' | 'error'
  submolt: string
  title: string
  content: string
  url: string
  target_post_id: string
  target_title: string
  parent_id: string
  vote_dir: string
  target_name: string
  reason: string
  source: string
  model: string
  posted_ref: string
  error: string
  created_at: number
  decided_at: number | null
}

export interface Activity {
  id: number
  kind: string
  text: string
  ref: string
  created_at: number
}

export interface FeedResponse {
  posts: CachedPost[]
  source: 'live' | 'cache' | 'demo'
  connected: boolean
  warning: string | null
  count: number
}

export interface HeartbeatResult {
  ok: boolean
  mode?: string
  fetched?: number
  considered?: number
  drafted?: number
  published?: number
  errors?: number
  note?: string
  reason?: string
  model?: string
}

export interface ModelConfig {
  id: string
  modelName?: string
  provider?: string
  adapt?: string
}
export interface ModelsResponse {
  activeId?: string | null
  configs?: ModelConfig[]
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
    } catch {
      /* ignore */
    }
    throw new Error(msg)
  }
  if (r.status === 204) return undefined as T
  return r.json() as Promise<T>
}

export const api = {
  status: () => req<{ ok: boolean }>('/status'),
  account: () => req<Account>('/account'),
  register: (name: string, description: string) =>
    req<{ ok: boolean; claim_url: string; verification_code: string; note: string; raw: unknown; account: Account }>(
      '/account/register',
      { method: 'POST', body: JSON.stringify({ name, description }) },
    ),
  claimInfo: () =>
    req<{ ok: boolean; claim_url: string; claimed: boolean; status: unknown; me: unknown; last_register_response: unknown }>(
      '/account/claim-info',
      { method: 'POST' },
    ),
  connect: (api_key: string, base_url?: string) =>
    req<{ ok: boolean; profile: Record<string, unknown>; account: Account }>('/account/connect', {
      method: 'POST',
      body: JSON.stringify({ api_key, base_url }),
    }),
  refresh: () => req<{ ok: boolean; profile: Record<string, unknown>; account: Account }>('/account/refresh', { method: 'POST' }),
  disconnect: () => req<Account>('/account/disconnect', { method: 'POST' }),

  getSettings: () => req<Account>('/settings'),
  putSettings: (patch: Partial<Account>) =>
    req<Account>('/settings', { method: 'PUT', body: JSON.stringify(patch) }),

  feed: (params: { sort?: string; filter?: string; refresh?: boolean; limit?: number } = {}) => {
    const qs = new URLSearchParams()
    if (params.sort) qs.set('sort', params.sort)
    if (params.filter) qs.set('filter', params.filter)
    if (params.refresh !== undefined) qs.set('refresh', String(params.refresh))
    if (params.limit) qs.set('limit', String(params.limit))
    const s = qs.toString()
    return req<FeedResponse>(`/feed${s ? '?' + s : ''}`)
  },
  home: () => req<Record<string, unknown>>('/home'),
  post: (id: string) => req<{ post: Record<string, unknown>; comments: Record<string, unknown> }>(`/posts/${encodeURIComponent(id)}`),
  search: (q: string, type = 'all') =>
    req<Record<string, unknown>>(`/search?q=${encodeURIComponent(q)}&type=${type}`),
  submolts: () => req<Record<string, unknown>>('/submolts'),
  notifications: () => req<Record<string, unknown>>('/notifications'),
  activity: (limit = 100) => req<{ items: Activity[] }>(`/activity?limit=${limit}`).then((r) => r.items),

  listDrafts: (status?: string) => {
    const qs = status ? `?status=${status}` : ''
    return req<{ drafts: Draft[]; count: number }>(`/drafts${qs}`).then((r) => r.drafts)
  },
  draftsCount: () => req<{ pending: number }>('/drafts/count').then((r) => r.pending),
  composeReply: (body: { target_post_id: string; post_title?: string; post_content?: string; instruction?: string }) =>
    req<{ ok: boolean; draft: Draft }>('/drafts/compose', { method: 'POST', body: JSON.stringify(body) }),
  composePost: (body: { submolt?: string; topic?: string }) =>
    req<{ ok: boolean; draft: Draft }>('/drafts/compose-post', { method: 'POST', body: JSON.stringify(body) }),
  createDraft: (body: Partial<Draft> & { kind: string }) =>
    req<{ ok: boolean; draft: Draft }>('/drafts', { method: 'POST', body: JSON.stringify(body) }),
  approveDraft: (id: number) =>
    req<{ ok: boolean; draft: Draft; ref?: string; error?: string }>(`/drafts/${id}/approve`, { method: 'POST' }),
  rejectDraft: (id: number) => req<{ ok: boolean; draft: Draft }>(`/drafts/${id}/reject`, { method: 'POST' }),
  deleteDraft: (id: number) => req<{ ok: boolean }>(`/drafts/${id}`, { method: 'DELETE' }),

  vote: (post_id: string, dir: 'up' | 'down', title?: string) =>
    req<Record<string, unknown>>('/actions/vote', { method: 'POST', body: JSON.stringify({ post_id, dir, title }) }),
  comment: (post_id: string, content: string, title?: string) =>
    req<Record<string, unknown>>('/actions/comment', { method: 'POST', body: JSON.stringify({ post_id, content, title }) }),
  createPost: (body: { submolt?: string; title: string; content?: string; url?: string }) =>
    req<Record<string, unknown>>('/actions/post', { method: 'POST', body: JSON.stringify(body) }),
  follow: (name: string) => req<Record<string, unknown>>('/actions/follow', { method: 'POST', body: JSON.stringify({ name }) }),
  subscribe: (name: string) =>
    req<Record<string, unknown>>('/actions/subscribe', { method: 'POST', body: JSON.stringify({ name }) }),

  runHeartbeat: () => req<HeartbeatResult>('/engine/run', { method: 'POST' }),
  seedDemo: () => req<{ ok: boolean; seeded: number }>('/demo/seed', { method: 'POST' }),

  models: () => req<ModelsResponse>('/models'),
  setModel: (id: string) => req<{ ok: boolean; id: string }>('/models', { method: 'POST', body: JSON.stringify({ id }) }),
}

export function fmtDateTime(secs: number | null | undefined): string {
  if (!secs) return '—'
  return new Date(secs * 1000).toLocaleString(undefined, {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

export function fmtRelative(secs: number | null | undefined): string {
  if (!secs) return '—'
  const diff = Date.now() / 1000 - secs
  if (diff < 60) return 'vừa xong'
  if (diff < 3600) return `${Math.floor(diff / 60)} phút trước`
  if (diff < 86400) return `${Math.floor(diff / 3600)} giờ trước`
  return `${Math.floor(diff / 86400)} ngày trước`
}

/// Deterministic HSL hue from a name → a stable avatar colour.
export function hueFromName(name: string): number {
  let h = 0
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) | 0
  return ((h % 360) + 360) % 360
}
