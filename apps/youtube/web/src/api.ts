// Thin fetch wrapper over the app's REST API (served by the Rust binary under /api).

export interface Status {
  ok: boolean
  app: string
  status: {
    extensionConnected: boolean
    bridge: { connected: boolean; connects: number; disconnects: number; uptime_s: number }
    auth: { hasSapisid?: boolean; loggedIn?: boolean; [k: string]: unknown }
  }
}

export interface VideoItem {
  videoId: string
  title: string
  channel: string
  published: string
  views: string
}

export interface Draft {
  id: string
  kind: string
  target: string
  body: string
  status: string
  result: string | null
  created_at: number
  updated_at: number
}

export interface CommentStats {
  videoId: string
  total: number
  analyzed: number
  spam: number
  avgSentiment: number | null
  sentiment: Record<string, number>
  intent: Record<string, number>
  lang: Record<string, number>
  topAuthors: Record<string, number>
}

export interface CachedComment {
  id: string
  author: string
  text: string
  like_count: number | null
  sentiment: string | null
  intent: string | null
}

export interface ModelInfo {
  id: string
  modelName: string
  provider: string
}

export interface Identity {
  channelId: string
  title: string
  thumbnail: string
}

export interface OAuthStatus {
  configured: boolean
  authorized: boolean
  expiresAt: number | null
  redirectUri: string
  scope: string
  identity: Identity | null
}

async function jget<T>(url: string): Promise<T> {
  const r = await fetch(url)
  const body = await r.json()
  if (!r.ok) throw new Error(body?.error || `HTTP ${r.status}`)
  return body as T
}

async function jpost<T>(url: string, data: unknown): Promise<T> {
  const r = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(data),
  })
  const body = await r.json()
  if (!r.ok) throw new Error(body?.error || `HTTP ${r.status}`)
  return body as T
}

export const api = {
  // status / models
  status: () => jget<Status>('/api/status'),
  llmInfo: () => jget<{ ok: boolean; daemon: string; model?: string | null }>('/api/llm-info'),
  models: () => jget<{ activeId: string; configs: ModelInfo[] }>('/api/models'),
  setModel: (id: string) => jpost<{ success: boolean; activeId: string }>('/api/model-active', { id }),

  // search
  search: (q: string) =>
    jget<{ query: string; count: number; items: VideoItem[] }>(`/api/search?q=${encodeURIComponent(q)}`),

  // comments + analytics
  syncComments: (videoId: string, maxPages = 3) =>
    jpost<{ fetched: number; new: number; pages: number }>('/api/comments/sync', { video_id: videoId, max_pages: maxPages }),
  analyzeComments: (max = 60) => jpost<{ analyzed: number; model: string }>('/api/comments/analyze', { max }),
  commentStats: (videoId: string) => jget<CommentStats>(`/api/comments/stats?video_id=${encodeURIComponent(videoId)}`),
  cachedComments: (videoId: string, limit = 500) =>
    jget<{ count: number; comments: CachedComment[] }>(`/api/comments/cached?video_id=${encodeURIComponent(videoId)}&limit=${limit}`),
  commentAction: (commentId: string, action: string, confirm = false) =>
    jpost<{ ok: boolean; action: string }>('/api/comment/action', { comment_id: commentId, action, confirm }),
  indexComments: (videoId: string) => jpost<{ indexed: number }>('/api/comments/index', { video_id: videoId }),

  // drafts
  listDrafts: (status?: string) => jget<Draft[]>(`/api/drafts${status ? `?status=${status}` : ''}`),
  aiDraft: (kind: string, context: string, target = '', instruction = '') =>
    jpost<{ id: string; body: string; model: string }>('/api/draft/ai', { kind, context, target, instruction }),
  approveDraft: (id: string) => jpost<{ id: string; status: string }>('/api/draft/approve', { id }),
  sendDraft: (id: string) => jpost<{ id: string; status: string }>('/api/draft/send', { id }),

  // moderation / oauth
  oauthStatus: () => jget<OAuthStatus>('/api/oauth/status'),
  oauthConfig: (client_id: string, client_secret: string) =>
    jpost<{ ok: boolean; authUrl: string }>('/api/oauth/config', { client_id, client_secret }),
  moderate: (commentId: string, status: string, banAuthor = false) =>
    jpost<{ ok: boolean }>('/api/moderate', { comment_id: commentId, status, ban_author: banAuthor }),
  oauthMe: () => jget<Identity>('/api/oauth/me'),
  oauthLogout: () => jpost<{ ok: boolean }>('/api/oauth/logout', {}),
}
