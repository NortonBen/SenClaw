// PlatformAdapter type surface — shared by the service worker and adapters.

export type Strategy = 'official' | 'replay' | 'page-sign' | 'dom' | 'none'

export interface Capabilities {
  post: Strategy
  dm: Strategy
  search: Strategy
  browse: Strategy
}

/** A credentialed request the extension replays as the logged-in user. */
export interface FetchReq {
  url: string
  method?: string
  headers?: Record<string, string>
  body?: unknown
}

export interface FetchResult {
  status: number
  json: unknown
  text?: string
  /** Final URL after redirects — used to resolve a Facebook vanity via /me/. */
  url?: string
}

/** Page-VM tokens captured from a platform (Meta: fb_dtsg/lsd + identity). */
export interface MetaTokens {
  platform: string
  name?: string
  id?: string
  fb_dtsg?: string
  lsd?: string
  jazoest?: string
  /** A Graph access token (EAA…) captured from the page's own requests. */
  access_token?: string
  at?: number
}

export interface Identity {
  handle?: string
  name?: string
  id?: string
  /** Which web-API credentials the extension has captured for this account. */
  tokens?: { fb_dtsg?: boolean; lsd?: boolean; access_token?: boolean }
  /** Actual captured web-session values, to persist under official_config.web_session. */
  web_config?: Record<string, string>
}

/** What an adapter's `whoami` receives to resolve the logged-in identity. */
export interface WhoamiCtx {
  fetch: (req: FetchReq) => Promise<FetchResult>
  headers: Record<string, string>
  cookie: (name: string, domain: string) => Promise<string | null>
  meta?: MetaTokens | null
}

export interface Adapter {
  id: string
  hosts: string[]
  sessionCookie: string
  captureHeaders?: string[]
  sign?: 'none' | 'meta' | 'tiktok'
  endpointHint?: string
  loginUrl?: string
  capabilities: Capabilities
  whoami?: (ctx: WhoamiCtx) => Promise<Identity | null>
}
