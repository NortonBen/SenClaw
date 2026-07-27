// Thin REST layer over the social app's own API.

export type Capability = 'official' | 'replay' | 'page-sign' | 'dom' | 'none'

export interface PlatformCaps {
  post: Capability
  dm: Capability
  search: Capability
  browse: Capability
  note: string
}

export interface Status {
  ok: boolean
  app: string
  platforms: string[]
  capabilities: Record<string, PlatformCaps>
  autonomy: 'observe' | 'draft' | 'live'
  accounts: number
  drafts_pending: number
  posts_logged: number
  actions_logged: number
  extension_connected: boolean
  extension_hosts_ready: string[]
  fb_composer_ready?: boolean
  extension_uptime_s: number
  /** Identity of the Chrome extension remotely driving this app. */
  extension_name?: string
  extension_version?: string
  extension_label?: string
  port: string
  ext_ws_port: number
}

export interface Account {
  id: number
  platform: string
  handle: string
  display_name: string
  official_config: Record<string, unknown>
  session_present: boolean
  token_expiry: string
  created_at: string
  updated_at: string
}

/** Public profile URL for a platform handle (for the accounts table link). */
export function profileUrl(platform: string, handle: string): string | null {
  const h = handle.replace(/^@/, '').trim()
  if (!h) return null
  switch (platform) {
    case 'facebook':
      return `https://facebook.com/${h}`
    case 'x':
      return `https://x.com/${h}`
    case 'instagram':
      return `https://instagram.com/${h}`
    case 'threads':
      return `https://threads.net/@${h}`
    case 'tiktok':
      return `https://tiktok.com/@${h}`
    case 'youtube':
      return `https://youtube.com/@${h}`
    default:
      return null
  }
}

export interface Draft {
  id: number
  platform: string
  handle: string
  kind: string
  text: string
  thread_id: string
  status: 'pending' | 'sent' | 'rejected'
  ref_id: string
  detail: string
  media?: string[]
  created_at: string
}

/** Per-platform composing rules for input validation. */
export interface PlatformRule {
  maxChars: number
  mediaMax: number
  mediaRequired: boolean
  mediaNote?: string
}

export const PLATFORM_RULES: Record<string, PlatformRule> = {
  facebook: { maxChars: 63206, mediaMax: 4, mediaRequired: false },
  x: { maxChars: 280, mediaMax: 4, mediaRequired: false },
  threads: { maxChars: 500, mediaMax: 4, mediaRequired: false },
  instagram: { maxChars: 2200, mediaMax: 4, mediaRequired: true, mediaNote: 'Instagram bắt buộc có ảnh.' },
  tiktok: { maxChars: 2200, mediaMax: 1, mediaRequired: true, mediaNote: 'TikTok cần video (ảnh chỉ để nháp).' },
  youtube: { maxChars: 5000, mediaMax: 1, mediaRequired: true, mediaNote: 'YouTube cần video (ảnh chỉ để nháp).' },
}

export const platformRule = (p: string): PlatformRule =>
  PLATFORM_RULES[p] ?? { maxChars: 5000, mediaMax: 4, mediaRequired: false }

export interface InboxMsg {
  id: number
  platform: string
  external_id: string
  sender: string
  direction: 'in' | 'out'
  text: string
  created_at: string
}

export interface ActionRow {
  platform: string
  action: string
  status: string
  detail: string
  created_at: string
}

export interface PostRow {
  platform: string
  kind: string
  ref_id: string
  status: string
  detail: string
  created_at: string
}

export interface SessionRow {
  platform: string
  event: 'online' | 'offline'
  created_at: string
}

export interface ExtStatus {
  connected: boolean
  connects: number
  disconnects: number
  uptime_s: number
  hosts_ready: string[]
  name?: string
  version?: string
  ext_id?: string
  label?: string
}

/** Human-readable uptime like "2 giờ 5 phút". */
export function uptime(sec: number): string {
  if (!sec || sec < 0) return '—'
  const h = Math.floor(sec / 3600)
  const m = Math.floor((sec % 3600) / 60)
  const s = Math.floor(sec % 60)
  if (h) return `${h} giờ ${m} phút`
  if (m) return `${m} phút ${s}s`
  return `${s}s`
}

async function req<T>(path: string, init?: RequestInit): Promise<T | null> {
  try {
    const r = await fetch(path, init)
    return (await r.json()) as T
  } catch {
    return null
  }
}

/** POST/PUT/DELETE that reports success + the server's error message. */
export async function mutate(
  path: string,
  method: string,
  body?: unknown,
): Promise<{ ok: boolean; error?: string; data?: any }> {
  try {
    const r = await fetch(path, {
      method,
      headers: body ? { 'content-type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
    })
    const data = await r.json().catch(() => null)
    return r.ok ? { ok: true, data } : { ok: false, error: data?.error ?? `HTTP ${r.status}`, data }
  } catch (e) {
    return { ok: false, error: String(e) }
  }
}

/** Detected identity from the extension after a platform login. */
export interface WhoAmI {
  logged_in: boolean
  platform?: string
  handle?: string
  name?: string
  id?: string
  /** Which web-API credentials the extension captured (Facebook). */
  tokens?: { fb_dtsg?: boolean; lsd?: boolean; access_token?: boolean }
  /** Actual captured web-session values, to persist under official_config.web_session. */
  web_config?: Record<string, string>
}

/** Compose a post or DM through the autonomy gate (draft or send by mode). */
export const compose = (body: {
  platform: string
  handle: string
  kind: 'post' | 'dm'
  text: string
  thread_id?: string
  media?: string[]
}) => mutate('/api/compose', 'POST', body)

/** Ask the extension to open the platform's login page in the user's Chrome. */
export const extLogin = (platform: string) => mutate('/api/ext/login', 'POST', { platform })

/** Ask the extension whether a platform has a live session + who is logged in. */
export const extWhoami = (platform: string) =>
  mutate('/api/ext/whoami', 'POST', { platform }) as Promise<{ ok: boolean; error?: string; data?: WhoAmI }>

export const getStatus = () => req<Status>('/api/status')
export const getExtStatus = () => req<ExtStatus>('/api/ext/status')
export const getAccounts = () => req<{ accounts: Account[] }>('/api/accounts')
export const getDrafts = () => req<{ drafts: Draft[] }>('/api/drafts')
export const getInbox = () => req<{ messages: InboxMsg[] }>('/api/inbox')
export const getActions = () => req<{ actions: ActionRow[] }>('/api/actions')
export const getPosts = () => req<{ posts: PostRow[] }>('/api/logs')
export const getSessions = () => req<{ sessions: SessionRow[] }>('/api/sessions')

/** Config keys each platform's official API needs — shown as a hint in the UI. */
export const CFG_HINT: Record<string, string> = {
  facebook: '{"page_id":"…","access_token":"…"}',
  x: '{"access_token":"…"}',
  threads: '{"threads_user_id":"…","access_token":"…"}',
  instagram: '{"ig_user_id":"…","access_token":"…"}',
  tiktok: '{"access_token":"…"}',
  youtube: '{"api_key":"…"} hoặc {"access_token":"…"}',
}

export function ago(iso: string): string {
  const d = (Date.now() - new Date(iso).getTime()) / 1000
  if (isNaN(d)) return '—'
  if (d < 60) return `${Math.floor(d)}s`
  if (d < 3600) return `${Math.floor(d / 60)} phút`
  if (d < 86400) return `${Math.floor(d / 3600)} giờ`
  return new Date(iso).toLocaleDateString('vi')
}
