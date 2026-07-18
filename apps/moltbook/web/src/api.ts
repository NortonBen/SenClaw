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
  /** SenClaw integrations: knowledge = trí nhớ, wiki = kho thông tin. */
  memory_enabled: boolean
  wiki_enabled: boolean
  wiki_archive: boolean
  knowledge_space: string
  /** LLM profile this app composes with; "" = follow the daemon's active model. */
  llm_profile: string
  /** "all" = engage with the whole feed; "focus" = only the listed subjects. */
  topic_mode: 'all' | 'focus'
  /** Each heartbeat also harvests feedback and refreshes the wiki docs. */
  harvest_enabled: boolean
  /** Write a trending briefing into the wiki once a day. */
  trending_daily: boolean
}

/** A day's trending briefing written into the wiki. */
export interface TrendingDigest {
  day: string
  wiki_path: string
  post_count: number
  topic_count: number
  summary: string
  topics: string[]
  runs: number
  created_at: number
  updated_at: number
}

export interface TrendingRun {
  ok: boolean
  day?: string
  posts?: number
  topics?: number
  wiki_path?: string
  summary?: string
  note?: string
  reason?: string
  topic_list?: Array<{ name: string; why: string; takeaway: string; relevant: boolean; post_count: number }>
}

/** One of our published posts + the state of every feedback check on it. */
export interface TrackedPost {
  post_id: string
  title: string
  submolt: string
  wiki_path: string
  posted_at: number
  last_checked_at: number | null
  checks: number
  last_comment_count: number
  last_score: number
  last_synced_at: number | null
  synced_comment_count: number
  synthesis: string
  last_error: string
  /** New agent comments the wiki doc hasn't absorbed yet. */
  doc_is_stale: boolean
}

export interface HarvestResult {
  ok: boolean
  checked?: number
  updated?: number
  discovered?: number
  errors?: number
  note?: string
  reason?: string
}

/** A steering entry: a subject to engage with, and/or something to post/ask about. */
export interface Topic {
  id: number
  text: string
  kind: 'engage' | 'post' | 'both'
  enabled: boolean
  used_at: number | null
  created_at: number
}

export interface Integrations {
  daemon: string
  wiki: { available: boolean }
  knowledge: { available: boolean; space: string; error: string | null }
}

export interface RecallResult {
  space: string
  answer: string
  grounded: boolean
  hits: Array<{ name: string; summary: string; score: number }>
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

/** One LLM profile configured in SenClaw. `label` is the profile name (e.g. "MoltClaw"). */
export interface ModelConfig {
  id: string
  label?: string
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

  digests: () => req<{ digests: TrendingDigest[]; count: number }>('/trending').then((r) => r.digests),
  runTrending: (write_wiki = true) =>
    req<TrendingRun>('/trending', { method: 'POST', body: JSON.stringify({ write_wiki }) }),

  tracked: () => req<{ posts: TrackedPost[]; count: number }>('/tracked').then((r) => r.posts),
  trackPost: (post_id: string, title = '', submolt = '') =>
    req<{ ok: boolean; post: TrackedPost }>('/tracked', { method: 'POST', body: JSON.stringify({ post_id, title, submolt }) }),
  untrackPost: (post_id: string) => req<{ ok: boolean }>(`/tracked/${encodeURIComponent(post_id)}`, { method: 'DELETE' }),
  harvest: (post_id?: string) =>
    req<HarvestResult>('/harvest', { method: 'POST', body: JSON.stringify(post_id ? { post_id } : {}) }),

  topics: () => req<{ topics: Topic[]; topic_mode: string; count: number }>('/topics').then((r) => r.topics),
  addTopic: (text: string, kind: Topic['kind'] = 'both') =>
    req<{ ok: boolean; topic: Topic }>('/topics', { method: 'POST', body: JSON.stringify({ text, kind }) }),
  patchTopic: (id: number, patch: Partial<Pick<Topic, 'text' | 'kind' | 'enabled'>>) =>
    req<{ ok: boolean; topic: Topic }>(`/topics/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteTopic: (id: number) => req<{ ok: boolean }>(`/topics/${id}`, { method: 'DELETE' }),

  integrations: () => req<Integrations>('/integrations'),
  memoryRecall: (query: string) => req<RecallResult>('/memory/recall', { method: 'POST', body: JSON.stringify({ query }) }),
  memorySave: (text: string, tags: string[] = []) =>
    req<{ ok: boolean; space: string }>('/memory/save', { method: 'POST', body: JSON.stringify({ text, tags }) }),
  wikiArchive: (post_id: string) =>
    req<{ ok: boolean; path: string }>('/wiki/archive', { method: 'POST', body: JSON.stringify({ post_id }) }),

  runHeartbeat: () => req<HeartbeatResult>('/engine/run', { method: 'POST' }),
  seedDemo: () => req<{ ok: boolean; seeded: number }>('/demo/seed', { method: 'POST' }),

  // Read-only: the app never changes the daemon's active model. Picking a
  // profile for Moltbook is a local setting (putSettings({ llm_profile })).
  models: () => req<ModelsResponse>('/models'),
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
