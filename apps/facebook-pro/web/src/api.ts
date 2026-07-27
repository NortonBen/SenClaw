// Tiny fetch wrapper for the Facebook Pro app REST API (served from the same
// origin in production; proxied to :4590 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export interface Status {
  configured: boolean
  connected: boolean
  active_page_id: string
  pages: number
  autonomy: string
  pending_drafts: number
}

export interface SettingsPublic {
  app_id: string
  version: string
  autonomy: string
  active_page_id: string
  app_secret_set: boolean
  user_token_set: boolean
}

export interface Page {
  page_id: string
  name: string
  category: string
}

export interface Draft {
  id: number
  kind: string
  status: string
  page_id: string
  target_id: string
  message: string
  link: string
  image_url: string
  source: string
  model: string
  result_id: string
  error: string
}

export interface Trigger {
  id: number
  name: string
  page_id: string
  event: string
  match_type: string
  match_value: string
  action: string
  reply_hint: string
  enabled: boolean
}

const body = (b: unknown) => ({ method: 'POST', body: JSON.stringify(b) })

/**
 * Open a URL in the user's REAL system browser.
 *
 * Inside the SenClaw desktop app, a Space App runs in an embedded WKWebView /
 * WebView2. Facebook rejects OAuth inside embedded webviews (`disallowed_useragent`)
 * and `window.open` is unreliable there — so we hand the URL to the Flutter host
 * (`flutter_inappwebview.callHandler('senclawOpenExternal', url)`), which launches
 * the system browser. The OAuth redirect (`…:4590/api/oauth/callback`) then comes
 * back to this app's local server. In a plain browser we just `window.open`.
 */
export function openExternal(url: string) {
  const w = window as unknown as { flutter_inappwebview?: { callHandler?: (name: string, ...args: unknown[]) => unknown } }
  const fiw = w.flutter_inappwebview
  if (fiw && typeof fiw.callHandler === 'function') {
    try {
      fiw.callHandler('senclawOpenExternal', url)
      return
    } catch {
      /* fall through to window.open */
    }
  }
  window.open(url, '_blank', 'noopener')
}

export const api = {
  status: () => j<Status>('/api/status'),
  getSettings: () => j<SettingsPublic>('/api/settings'),
  setSettings: (b: Partial<SettingsPublic> & { app_secret?: string }) => j<SettingsPublic>('/api/settings', body(b)),
  oauthLink: (redirect: string) => j<{ url?: string; error?: string }>(`/api/oauth/link?redirect=${encodeURIComponent(redirect)}`),
  connectToken: (user_token: string) => j('/api/connect/token', body({ user_token })),
  pages: () => j<{ pages: Page[]; active_page_id: string }>('/api/pages'),
  selectPage: (page_id: string) => j('/api/pages/select', body({ page_id })),

  posts: (page_id?: string, limit = 15) => j(`/api/posts?limit=${limit}${page_id ? `&page_id=${page_id}` : ''}`),
  postGet: (id: string, page_id?: string) => j(`/api/posts/get?id=${encodeURIComponent(id)}${page_id ? `&page_id=${page_id}` : ''}`),
  createPost: (b: { message: string; link?: string; image_url?: string; page_id?: string }) => j('/api/posts', body(b)),
  uploadPhoto: async (file: File, message: string, page_id?: string) => {
    const fd = new FormData()
    fd.append('file', file)
    fd.append('message', message)
    if (page_id) fd.append('page_id', page_id)
    const r = await fetch('/api/posts/photo_upload', { method: 'POST', body: fd })
    return r.json()
  },
  editPost: (b: { post_id: string; message: string; page_id?: string }) => j('/api/posts/edit', body(b)),
  deletePost: (b: { post_id: string; page_id?: string }) => j('/api/posts/delete', body(b)),

  comments: (object_id: string, page_id?: string, limit = 25) => j(`/api/comments?object_id=${encodeURIComponent(object_id)}&limit=${limit}${page_id ? `&page_id=${page_id}` : ''}`),
  createComment: (b: { object_id: string; message: string; page_id?: string }) => j('/api/comments', body(b)),
  reply: (b: { comment_id: string; message?: string; comment_text?: string; hint?: string; page_id?: string }) => j('/api/comments/reply', body(b)),
  like: (b: { object_id: string; page_id?: string }) => j('/api/like', body(b)),

  overview: () => j('/api/overview'),
  conversations: (page_id?: string, limit = 25) => j(`/api/conversations?limit=${limit}${page_id ? `&page_id=${page_id}` : ''}`),
  conversationMessages: (id: string, page_id?: string) => j(`/api/conversations/messages?id=${encodeURIComponent(id)}${page_id ? `&page_id=${page_id}` : ''}`),
  messageReply: (b: { recipient_id: string; message?: string; customer_msg?: string; hint?: string; page_id?: string }) => j('/api/messages/reply', body(b)),

  analyze: (b: { post_id?: string; message?: string; page_id?: string }) => j('/api/analyze', body(b)),
  pageInsights: (b?: { page_id?: string; metric?: string; period?: string }) => {
    const q = new URLSearchParams(b as Record<string, string>).toString()
    return j(`/api/insights/page${q ? `?${q}` : ''}`)
  },
  postInsights: (id: string, page_id?: string) => j(`/api/insights/post?id=${encodeURIComponent(id)}${page_id ? `&page_id=${page_id}` : ''}`),

  adAccounts: () => j<{ accounts?: any[]; active_ad_account?: string; error?: string }>('/api/ads/accounts'),
  selectAdAccount: (account_id: string) => j('/api/ads/select', body({ account_id })),
  adCampaigns: (account_id?: string) => j(`/api/ads/campaigns${account_id ? `?account_id=${account_id}` : ''}`),
  adsInsights: (b: { object_id?: string; level?: string; date_preset?: string }) => {
    const q = new URLSearchParams(b as Record<string, string>).toString()
    return j(`/api/ads/insights${q ? `?${q}` : ''}`)
  },
  adsAnalyze: (b: { object_id?: string; level?: string; date_preset?: string; currency?: string }) => j('/api/ads/analyze', body(b)),
  adStatus: (b: { entity_id: string; status: string }) => j('/api/ads/status', body(b)),

  drafts: () => j<{ pending: Draft[] }>('/api/drafts'),
  approve: (id: number) => j(`/api/drafts/${id}/approve`, { method: 'POST' }),
  reject: (id: number) => j(`/api/drafts/${id}/reject`, { method: 'POST' }),

  triggers: () => j<{ triggers: Trigger[] }>('/api/triggers'),
  createTrigger: (b: Partial<Trigger>) => j('/api/triggers', body(b)),
  deleteTrigger: (id: number) => j(`/api/triggers/${id}/delete`, { method: 'POST' }),

  activity: () => j<{ activity: any[] }>('/api/activity'),
  tick: () => j('/api/engine/tick', { method: 'POST' }),
}
