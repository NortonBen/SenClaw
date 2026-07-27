// SenClaw Social — extension service worker (WXT + TypeScript).
//
// Responsibilities:
//   1. Keep a WebSocket to the app's extension bridge (ws://127.0.0.1:9224).
//   2. Sniff each platform's session/auth tokens off the page's own requests
//      (webRequest.onBeforeSendHeaders) and keep them LOCALLY — never sent to
//      the app. The app only learns which hosts have a live session + caps.
//   3. Cache page-VM tokens (Meta fb_dtsg/lsd + real identity) forwarded from
//      the MAIN-world content script, so `WhoAmI`/replay can use them.
//   4. Handle RPC commands: ReplayApi, OpenLogin, WhoAmI, Ping.
//   5. Heartbeat every 15s with hosts_ready + per-platform capabilities.

import {
  ADAPTERS,
  adapterById,
  adapterForHost,
  credentialedFetch,
  type Capabilities,
  type Identity,
  type MetaTokens,
} from '../adapters'

const DEFAULT_WS = 'ws://127.0.0.1:9224'

const CAPTURE_HEADERS = new Set(
  ADAPTERS.flatMap((a) => (a.captureHeaders || []).map((h) => h.toLowerCase())).concat([
    'authorization',
    'x-csrf-token',
  ]),
)
const MATCH_URLS = ADAPTERS.flatMap((a) => a.hosts.map((d) => `*://*.${d}/*`))

interface Captured {
  headers: Record<string, string>
  at: number
}

export default defineBackground(() => {
  let ws: WebSocket | null = null
  let callbackSecret: string | null = null
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null
  const captured: Record<string, Captured> = {} // platform -> headers
  const metaCache: Record<string, MetaTokens> = {} // platform -> page-VM tokens
  // A learned Facebook composer request (all form fields of a real
  // ComposerStoryCreateMutation the user sent) — replayed to post via the API.
  let fbComposerTemplate: { fields: Record<string, string>; at: number } | null = null
  // Learned Facebook FEED pagination requests, keyed by kind (home|profile|
  // page|groups). Captured passively as the user scrolls their own feed, then
  // replayed (with fresh tokens) to read stories. doc_id rotates per deploy, so
  // the freshest capture wins — this is the self-healing "scan" path.
  const fbFeedTpl: Record<string, { fields: Record<string, string>; friendly: string; at: number }> = {}

  // ---------------- Persistence (survive MV3 SW termination) ----------------
  //
  // The background service worker is killed when idle, wiping in-memory state —
  // so a WhoAmI a minute after login would find an empty metaCache. Mirror the
  // captured headers + page-VM tokens to chrome.storage.session (kept for the
  // browser session, never written to disk) and hydrate on startup.

  let persistTimer: ReturnType<typeof setTimeout> | null = null
  function persist() {
    if (persistTimer) return
    persistTimer = setTimeout(() => {
      persistTimer = null
      // Volatile session tokens stay in session storage (cleared when the browser
      // closes — they rotate anyway and are refreshed on the next page load).
      chrome.storage.session.set({ captured, metaCache }).catch(() => {})
      // The learned FB composer template is DURABLE: persist it to local storage
      // so a Chrome restart doesn't lose it and force the fragile DOM fallback.
      // Its rotating fields (fb_dtsg/jazoest/lsd) are overwritten with fresh
      // metaCache values at replay time, so keeping the seed long-term is safe.
      chrome.storage.local.set({ fbTpl: fbComposerTemplate, fbFeedTpl }).catch(() => {})
    }, 400)
  }
  const hydrated = Promise.all([
    chrome.storage.session.get(['captured', 'metaCache']).catch(() => ({}) as Record<string, unknown>),
    chrome.storage.local.get(['fbTpl', 'fbFeedTpl']).catch(() => ({}) as Record<string, unknown>),
  ])
    .then(([sess, local]) => {
      Object.assign(captured, (sess.captured as typeof captured) || {})
      Object.assign(metaCache, (sess.metaCache as typeof metaCache) || {})
      // Prefer the durable local copy; fall back to a legacy session copy if an
      // older build wrote it there.
      const tpl = (local.fbTpl as typeof fbComposerTemplate) || null
      if (tpl) fbComposerTemplate = tpl
      Object.assign(fbFeedTpl, (local.fbFeedTpl as typeof fbFeedTpl) || {})
    })
    .catch(() => {})

  // ---------------- WebSocket ----------------

  function connect() {
    chrome.storage.local.get(['ws_url'], ({ ws_url }) => {
      const url = ws_url || DEFAULT_WS
      try {
        ws = new WebSocket(url)
      } catch {
        scheduleReconnect()
        return
      }
      ws.onopen = () => {
        console.log('[social] connected', url)
        // Announce identity so the app can show which extension is driving it.
        const m = chrome.runtime.getManifest()
        send({ type: 'hello', name: m.name, version: m.version, ext_id: chrome.runtime.id })
      }
      ws.onclose = () => scheduleReconnect()
      ws.onerror = () => {
        try {
          ws?.close()
        } catch {
          /* ignore */
        }
      }
      ws.onmessage = (ev) => {
        let msg: Record<string, unknown>
        try {
          msg = JSON.parse(ev.data as string)
        } catch {
          return
        }
        void handle(msg)
      }
    })
  }

  function scheduleReconnect() {
    if (reconnectTimer) return
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null
      connect()
    }, 3000)
  }

  function send(obj: unknown) {
    if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(obj))
  }

  // ---------------- Command handling ----------------

  async function handle(msg: Record<string, unknown>) {
    if (msg.type === 'callback_secret') {
      callbackSecret = String(msg.secret)
      return
    }
    if (msg.type === 'pong') return
    if (msg.id && msg.method) {
      try {
        const result = await dispatch(String(msg.method), (msg.params as Record<string, unknown>) || {})
        send({ id: msg.id, result })
      } catch (e) {
        send({ id: msg.id, error: e instanceof Error ? e.message : String(e) })
      }
    }
  }

  async function dispatch(method: string, params: Record<string, unknown>): Promise<unknown> {
    switch (method) {
      case 'ReplayApi':
        return await replayApi(params)
      case 'OpenLogin':
        return await openLogin(params)
      case 'WhoAmI':
        return await whoAmI(params)
      case 'GetFbTemplate':
        return await fbTemplateInfo()
      case 'FbTestPost':
        return await fbTestPost(params)
      case 'Ping':
        return { pong: true }
      default:
        throw new Error('unknown method: ' + method)
    }
  }

  // ---------------- Replay ----------------

  async function replayApi(params: Record<string, unknown>) {
    const platform = String(params.platform || '')
    const op = String(params.op || '')
    const adapter = adapterById(ADAPTERS, platform)
    const strategy = adapter?.capabilities[capabilityOf(op) as keyof Capabilities]

    // Personal Facebook post: internal-GraphQL replay first, DOM composer fallback.
    if (op === 'post') return await postFacebook(params)

    // Personal Facebook feed/group scan: replay a learned feed-pagination query.
    if (platform === 'facebook' && (op === 'feed' || op === 'groups' || op === 'browse')) {
      return await scanFacebookFeed(op, params)
    }

    if (!params.url) {
      // Feed / group / page browsing over a personal session has no stable
      // wired path: mbasic HTML is being decommissioned (Meta redirects it to
      // Comet since Oct 2024) and internal-GraphQL feed queries rotate their
      // doc_id every deploy. Point the caller at the reliable, ToS-clean route.
      const isBrowse = op === 'feed' || op === 'groups' || op === 'browse' || capabilityOf(op) === 'browse'
      return {
        not_wired: true,
        platform,
        op,
        strategy: strategy || 'unknown',
        hint:
          isBrowse && platform === 'facebook'
            ? 'Quét Page ổn định: dùng tool social_page_scan (Graph API) cho Page Sếp quản trị (cần official_config {page_id, access_token}). Quét feed/nhóm cá nhân qua phiên web chưa nối vì mbasic đang bị Meta gỡ và GraphQL feed đổi doc_id liên tục — cần bật riêng, dễ hỏng.'
            : strategy === 'page-sign'
              ? 'Endpoint này cần chữ ký từ trang (MAIN-world signer chưa nối) — chưa replay được.'
              : 'Truyền params.url (+method/body) để replay, hoặc bổ sung adapter cho op này.',
      }
    }
    return await credentialedFetch(params as { url: string })
  }

  function capabilityOf(op: string): keyof Capabilities {
    if (!op) return 'browse'
    if (op.startsWith('post')) return 'post'
    if (op.startsWith('send_dm') || op === 'dm') return 'dm'
    if (op.startsWith('search')) return 'search'
    return 'browse'
  }

  // ---------------- Login flow ----------------

  // Open (and focus) the platform's login page in a real Chrome tab so the user
  // signs in with their own credentials. Once the session cookie lands, the next
  // heartbeat reports the host as ready.
  async function openLogin(params: Record<string, unknown>) {
    const platform = String(params.platform || '')
    const adapter = adapterById(ADAPTERS, platform)
    if (!adapter) throw new Error('unknown platform: ' + platform)
    const url = adapter.loginUrl || `https://${adapter.hosts[0]}/`
    const tab = await chrome.tabs.create({ url, active: true })
    try {
      if (tab.windowId != null) await chrome.windows.update(tab.windowId, { focused: true })
    } catch {
      /* window focus is best-effort */
    }
    return { opened: true, url, tab_id: tab.id }
  }

  // Post to a personal Facebook profile. Prefer replaying FB's own internal
  // GraphQL composer request (learned from a real post); fall back to driving the
  // DOM composer when no template has been learned yet.
  // Does an internal-GraphQL error look like a stale-session/token problem
  // (fb_dtsg rotated, session needs re-scrape) rather than a rejected request?
  // Those are the ones a token refresh + retry can fix.
  function isStaleTokenError(msg: string): boolean {
    return /fb_dtsg|dtsg|session|hết hạn|expired|135700\d|1357\d{3}|đóng và mở lại|close.*reopen|try again|thử lại|login|đăng nhập|not.?logged/i.test(
      msg,
    )
  }

  // Ensure a facebook.com tab is loaded and wait for the MAIN-world reader to
  // post a FRESH fb_dtsg (it rotates per session; the SW cache can be stale or
  // empty after a restart). Returns true once a newer token lands.
  async function refreshFbTokens(timeoutMs = 7000): Promise<boolean> {
    const before = metaCache['facebook']?.at || 0
    const tabs = await chrome.tabs.query({ url: ['*://*.facebook.com/*'] })
    let tab = tabs.find((t) => t.id != null)
    if (!tab) {
      // Open the home feed in the background — metasign runs on load and posts
      // fresh tokens within a second or two.
      tab = await chrome.tabs.create({ url: 'https://www.facebook.com/', active: false })
    } else if (tab.id != null) {
      // Force the MAIN-world reader to re-read + re-post tokens now, rather than
      // waiting up to 20s for its periodic tick.
      await chrome.scripting
        .executeScript({
          target: { tabId: tab.id },
          world: 'MAIN',
          func: () => {
            try {
              const req = (window as unknown as { require?: (n: string) => { token?: string } }).require
              const dtsg = req?.('DTSGInitialData')?.token || ''
              const lsd = req?.('LSD')?.token || ''
              let sum = 0
              for (let i = 0; i < dtsg.length; i++) sum += dtsg.charCodeAt(i)
              window.postMessage(
                { __senclaw_social: 'meta_tokens', platform: 'facebook', fb_dtsg: dtsg, lsd, jazoest: dtsg ? '2' + sum : '' },
                '*',
              )
            } catch {
              /* best-effort */
            }
          },
        })
        .catch(() => {})
    }
    const t0 = Date.now()
    while (Date.now() - t0 < timeoutMs) {
      const cur = metaCache['facebook']
      if (cur?.fb_dtsg && (cur.at || 0) > before) return true
      await new Promise((r) => setTimeout(r, 300))
    }
    return !!metaCache['facebook']?.fb_dtsg
  }

  // Post to a personal Facebook profile. PRIMARY method is DOM control — drive
  // the real composer in the user's logged-in tab, exactly like a human. It
  // carries no fb_dtsg/doc_id tokens, so it never hits the stale-token 1357004
  // ("đóng và mở lại trình duyệt") or doc_id-rotation failures the API path had,
  // and the text insert is de-duplicated (no more "TestTestTestTest").
  //
  // `params.use_api` opts into the internal-GraphQL fast path first (when a
  // composer template has been learned), falling back to DOM control on any
  // failure so a post never just errors out.
  async function postFacebook(params: Record<string, unknown>) {
    const text = String(params.text || '')
    if (!text.trim()) throw new Error('thiếu nội dung')
    await hydrated

    if (params.use_api && fbComposerTemplate?.fields?.doc_id) {
      console.log('[social] FB post via GraphQL fast-path, doc_id=', fbComposerTemplate.fields['doc_id'])
      let r = await replayGraphqlPost(text)
      for (let attempt = 0; attempt < 2 && !r.ok && isStaleTokenError(r.error || ''); attempt++) {
        const refreshed = await refreshFbTokens()
        if (!refreshed && attempt === 0) break
        r = await replayGraphqlPost(text)
      }
      if (r.ok) {
        console.log('[social] FB API post OK, ref=', r.ref)
        return r
      }
      console.warn('[social] FB API post failed → fallback to DOM control:', r.error)
      // fall through to DOM control
    }

    console.log('[social] FB post via DOM control')
    const dom = await postDom({ ...params, platform: 'facebook' })
    return { ...dom, via: 'dom' }
  }

  // Diagnostics: safe metadata about the learned composer template (no secrets).
  async function fbTemplateInfo() {
    await hydrated
    const t = fbComposerTemplate
    if (!t?.fields?.doc_id) return { ready: false }
    let inputKeys: string[] = []
    try {
      inputKeys = Object.keys((JSON.parse(t.fields.variables).input as Record<string, unknown>) || {})
    } catch {
      /* ignore */
    }
    return {
      ready: true,
      friendly: t.fields['fb_api_req_friendly_name'],
      doc_id: t.fields['doc_id'],
      field_keys: Object.keys(t.fields),
      input_keys: inputKeys,
      has_fb_dtsg: !!metaCache['facebook']?.fb_dtsg,
      learned_at: t.at,
    }
  }

  // Diagnostics: run the GraphQL replay once and return the raw outcome so we can
  // see FB's actual response/error. Publishes a real post — use deliberately.
  async function fbTestPost(params: Record<string, unknown>) {
    await hydrated
    const text = String(params.text || '')
    if (!text.trim()) throw new Error('thiếu text')
    return await replayGraphqlPost(text)
  }

  // Deep-search a GraphQL response for the created post's id.
  function deepFindPostId(o: unknown): string | null {
    if (!o || typeof o !== 'object') return null
    const rec = o as Record<string, unknown>
    for (const key of ['post_id', 'legacy_story_hideable_id']) {
      if (typeof rec[key] === 'string') return rec[key] as string
    }
    for (const k in rec) {
      const r = deepFindPostId(rec[k])
      if (r) return r
    }
    return null
  }

  // Replay the learned composer mutation with new text + fresh tokens/ids.
  async function replayGraphqlPost(
    text: string,
  ): Promise<{ ok?: boolean; ref?: string; via?: string; error?: string; raw?: string }> {
    const tpl = fbComposerTemplate
    if (!tpl?.fields?.doc_id) return { error: 'chưa học template' }
    const fields: Record<string, string> = { ...tpl.fields }
    const meta = metaCache['facebook']
    if (meta?.fb_dtsg) fields['fb_dtsg'] = meta.fb_dtsg
    if (meta?.jazoest) fields['jazoest'] = meta.jazoest
    if (meta?.lsd) fields['lsd'] = meta.lsd

    let vars: { input?: Record<string, unknown> }
    try {
      vars = JSON.parse(fields['variables'])
    } catch {
      return { error: 'template variables hỏng — đăng tay 1 bài để học lại.' }
    }
    const input = (vars.input ||= {})
    input.message = { ranges: [], text }
    try {
      input.composer_session_id = crypto.randomUUID()
    } catch {
      /* keep the seed's id */
    }
    input.client_mutation_id = String(Date.now() % 1000000)
    // Strip anything media/attachment-related from the seed → text-only post.
    delete input.attachments
    delete input.attachment
    delete input.media
    delete input.attached_story_attachment
    fields['variables'] = JSON.stringify(vars)

    // Match a genuine Comet request: the web app sends the friendly name and the
    // LSD token as HEADERS (x-fb-friendly-name / x-fb-lsd), not just in the body.
    // Some GraphQL endpoints reject a bare form POST that omits these.
    const headers: Record<string, string> = { 'content-type': 'application/x-www-form-urlencoded' }
    if (fields['fb_api_req_friendly_name']) headers['x-fb-friendly-name'] = fields['fb_api_req_friendly_name']
    const lsdTok = fields['lsd'] || meta?.lsd
    if (lsdTok) headers['x-fb-lsd'] = lsdTok

    let resp: Response
    try {
      resp = await fetch('https://www.facebook.com/api/graphql/', {
        method: 'POST',
        headers,
        body: new URLSearchParams(fields).toString(),
        credentials: 'include',
      })
    } catch (e) {
      return { error: 'lỗi mạng khi gọi FB GraphQL: ' + (e instanceof Error ? e.message : String(e)) }
    }
    const raw = (await resp.text()).replace(/^for\s*\(;;\);/, '').trim()
    console.log('[social] FB GraphQL HTTP', resp.status, 'len', raw.length)
    // FB's error shape varies: a GraphQL `errors[]` array, OR a top-level
    // `error` that is a NUMBER code (e.g. 1357004) alongside `errorSummary` /
    // `errorDescription`. Model both so we never collapse it to "lỗi không rõ".
    type FbResp = {
      data?: unknown
      errors?: { message?: string; code?: number }[]
      error?: number | { message?: string; code?: number; description?: string }
      errorSummary?: string
      errorDescription?: string
    }
    let json: FbResp | null = null
    // FB streams one JSON object per line (and prepends anti-hijack junk). Try
    // each line; then fall back to trimming to the first '{' (jdcodes pattern).
    for (const line of raw.split('\n')) {
      try {
        json = JSON.parse(line)
        break
      } catch {
        /* try next line */
      }
    }
    if (!json) {
      const brace = raw.indexOf('{')
      if (brace >= 0) {
        try {
          json = JSON.parse(raw.slice(brace))
        } catch {
          /* give up below */
        }
      }
    }
    if (!json) {
      return { error: 'FB trả về không phải JSON (phiên hết hạn?) — đăng nhập lại/refresh tab FB.', raw: raw.slice(0, 400) }
    }
    if (json.errors?.length || json.error || json.errorSummary) {
      const e0 = json.errors?.[0]
      const errObj = typeof json.error === 'object' ? json.error : undefined
      const code = typeof json.error === 'number' ? json.error : (e0?.code ?? errObj?.code)
      const summary = e0?.message || json.errorSummary || errObj?.message
      const desc = json.errorDescription || errObj?.description
      const parts = [summary, desc && desc !== summary ? desc : ''].filter(Boolean).join(' — ')
      return {
        error: `FB${code ? ' ' + code : ''}: ${parts || 'lỗi không rõ'}`,
        raw: raw.slice(0, 400),
      }
    }
    const id = deepFindPostId(json.data)
    if (!id) return { ok: true, ref: 'graphql', via: 'graphql', raw: raw.slice(0, 300) }
    return { ok: true, ref: id, via: 'graphql' }
  }

  // ---------------- Feed scan (session-based) ----------------

  // Classify a GraphQL friendly-name into the feed kind it paginates, or null.
  // These are the query names Comet uses for the four feed surfaces (2024-2026).
  function feedKindOf(friendly: string): string | null {
    if (/GroupsComet.*Feed.*Pagination|Group.*Feed.*Pagination/i.test(friendly)) return 'groups'
    if (/PageFeed.*Pagination|CometModernPageFeed/i.test(friendly)) return 'page'
    if (/ProfileComet.*(Timeline|Feed).*(Refetch|Pagination)|Timeline.*Feed.*Refetch/i.test(friendly))
      return 'profile'
    if (/NewsFeed.*Pagination|CometNewsFeed|FeedPaginationQuery/i.test(friendly)) return 'home'
    return null
  }

  // Deep-walk a GraphQL response collecting story-like nodes. Path-independent
  // (Meta rotates the exact edge paths), so we match on shape: any object that
  // carries readable text plus a post/story id. Returns up to `cap` stories.
  function extractStories(root: unknown, cap: number): Array<Record<string, unknown>> {
    const out: Array<Record<string, unknown>> = []
    const seen = new Set<string>()
    const textOf = (o: Record<string, unknown>): string => {
      const m = o.message as { text?: string } | undefined
      if (m && typeof m.text === 'string' && m.text.trim()) return m.text
      if (typeof o.text === 'string' && (o.text as string).trim()) return o.text as string
      return ''
    }
    const idOf = (o: Record<string, unknown>): string => {
      for (const k of ['post_id', 'legacy_story_hideable_id', 'ft_ent_identifier']) {
        if (typeof o[k] === 'string') return o[k] as string
      }
      return ''
    }
    const walk = (o: unknown) => {
      if (!o || typeof o !== 'object' || out.length >= cap) return
      if (Array.isArray(o)) {
        for (const v of o) walk(v)
        return
      }
      const rec = o as Record<string, unknown>
      const text = textOf(rec)
      const id = idOf(rec)
      if (text && (id || !seen.has(text))) {
        const key = id || text.slice(0, 60)
        if (!seen.has(key)) {
          seen.add(key)
          const fb = rec.feedback as Record<string, unknown> | undefined
          out.push({
            id,
            text: text.slice(0, 2000),
            url: typeof rec.url === 'string' ? rec.url : (typeof rec.wwwURL === 'string' ? rec.wwwURL : ''),
            reactions: countOf(fb, ['reaction_count', 'reactors']),
            comments: countOf(fb, ['comment_count', 'total_comment_count', 'comments']),
            shares: countOf(fb, ['share_count', 'reshares']),
          })
        }
      }
      for (const k in rec) walk(rec[k])
    }
    walk(root)
    return out
  }

  function countOf(fb: Record<string, unknown> | undefined, keys: string[]): number | null {
    if (!fb) return null
    for (const k of keys) {
      const v = fb[k]
      if (typeof v === 'number') return v
      if (v && typeof v === 'object' && typeof (v as { count?: number }).count === 'number')
        return (v as { count: number }).count
      if (v && typeof v === 'object' && typeof (v as { total_count?: number }).total_count === 'number')
        return (v as { total_count: number }).total_count
    }
    return null
  }

  // Read the user's Facebook feed/groups by replaying a learned feed-pagination
  // query with fresh tokens. Requires the user to have scrolled that surface at
  // least once (so the query was captured). Best-effort + self-healing on stale
  // tokens; the parser is path-independent to survive Meta's edge-path churn.
  async function scanFacebookFeed(op: string, params: Record<string, unknown>) {
    await hydrated
    const limit = Math.max(1, Math.min(Number(params.limit) || 15, 60))
    // Pick the template that best matches the requested surface, else the newest.
    const want = op === 'groups' ? ['groups'] : ['home', 'profile', 'page']
    const pool = Object.entries(fbFeedTpl)
    if (!pool.length) {
      return {
        not_wired: true,
        op,
        hint:
          'Chưa học được truy vấn feed. Mở facebook.com và CUỘN feed (hoặc nhóm) 1 lần để app học, rồi thử lại. Cách ổn định hơn cho Page Sếp quản trị: dùng social_page_scan (Graph API).',
      }
    }
    const chosen =
      want.map((k) => fbFeedTpl[k]).find(Boolean) ||
      pool.map(([, v]) => v).sort((a, b) => b.at - a.at)[0]
    if (!chosen) {
      return { not_wired: true, op, hint: 'Chưa học được truy vấn feed — cuộn feed Facebook 1 lần rồi thử lại.' }
    }

    let r = await replayFeedQuery(chosen.fields, chosen.friendly)
    if (r.error && isStaleTokenError(r.error)) {
      if (await refreshFbTokens()) r = await replayFeedQuery(chosen.fields, chosen.friendly)
    }
    if (r.error) throw new Error(r.error)
    const stories = extractStories(r.data, limit)
    return {
      via: 'graphql',
      learned: chosen.friendly,
      count: stories.length,
      stories,
      note: stories.length ? undefined : 'Không trích được bài nào từ phản hồi (FB có thể đã đổi cấu trúc feed).',
    }
  }

  // Replay a captured feed query with fresh tokens; return parsed `data` or error.
  async function replayFeedQuery(
    template: Record<string, string>,
    friendly: string,
  ): Promise<{ data?: unknown; error?: string }> {
    const fields: Record<string, string> = { ...template }
    const meta = metaCache['facebook']
    if (meta?.fb_dtsg) fields['fb_dtsg'] = meta.fb_dtsg
    if (meta?.jazoest) fields['jazoest'] = meta.jazoest
    if (meta?.lsd) fields['lsd'] = meta.lsd
    const headers: Record<string, string> = { 'content-type': 'application/x-www-form-urlencoded' }
    if (friendly) headers['x-fb-friendly-name'] = friendly
    if (fields['lsd']) headers['x-fb-lsd'] = fields['lsd']

    let resp: Response
    try {
      resp = await fetch('https://www.facebook.com/api/graphql/', {
        method: 'POST',
        headers,
        body: new URLSearchParams(fields).toString(),
        credentials: 'include',
      })
    } catch (e) {
      return { error: 'lỗi mạng khi đọc feed FB: ' + (e instanceof Error ? e.message : String(e)) }
    }
    const raw = (await resp.text()).replace(/^for\s*\(;;\);/, '').trim()
    // Feed responses stream multiple JSON objects (one per line). Merge their
    // `data` so the extractor sees every story chunk.
    const datas: unknown[] = []
    for (const line of raw.split('\n')) {
      if (!line.trim()) continue
      try {
        const j = JSON.parse(line) as { data?: unknown; errors?: { message?: string }[] }
        if (j.errors?.length) return { error: 'FB: ' + (j.errors[0]?.message || 'lỗi feed') }
        if (j.data) datas.push(j.data)
      } catch {
        /* skip non-JSON line */
      }
    }
    if (!datas.length) {
      const brace = raw.indexOf('{')
      if (brace >= 0) {
        try {
          const j = JSON.parse(raw.slice(brace)) as { data?: unknown }
          if (j.data) datas.push(j.data)
        } catch {
          /* fall through */
        }
      }
    }
    if (!datas.length) return { error: 'FB trả về không phải JSON (phiên hết hạn?) — refresh tab FB rồi thử lại.' }
    return { data: datas }
  }

  // Post to a personal Facebook profile by driving the on-page composer in the
  // user's own tab (no post API exists for personal timelines). Finds/opens a
  // facebook.com tab, then asks the content script to fill + submit.
  async function postDom(params: Record<string, unknown>) {
    const platform = String(params.platform || '')
    const text = String(params.text || '')
    if (platform !== 'facebook') throw new Error('post_dom hiện chỉ hỗ trợ facebook')
    if (!text.trim()) throw new Error('thiếu nội dung')

    // Prefer a tab already on the home feed (where the composer reliably exists);
    // otherwise accept any facebook.com tab and steer it home; else open one.
    const home = await chrome.tabs.query({ url: ['*://*.facebook.com/', '*://*.facebook.com/?*'] })
    let tab = home.find((t) => t.id != null)
    if (!tab) {
      const any = await chrome.tabs.query({ url: ['*://*.facebook.com/*'] })
      tab = any.find((t) => t.id != null)
      if (tab?.id != null) {
        // Navigate an off-feed FB tab to the home feed so the composer is present.
        await chrome.tabs.update(tab.id, { url: 'https://www.facebook.com/', active: true })
        await waitTabComplete(tab.id)
      }
    }
    if (!tab) {
      tab = await chrome.tabs.create({ url: 'https://www.facebook.com/', active: true })
      await waitTabComplete(tab.id!)
    }

    const res = await sendComposeWithInject(tab.id!, text)
    if (res?.error) throw new Error(res.error)
    return { ok: true, ref: res?.ref || 'dom' }
  }

  // Send the compose_post command, self-healing the "Receiving end does not
  // exist" case: a Facebook tab opened before the extension loaded (or after a
  // reload) has no relay content script, so the first sendMessage rejects. When
  // that happens, inject the content script on demand and retry once.
  async function sendComposeWithInject(
    tabId: number,
    text: string,
  ): Promise<{ ok?: boolean; ref?: string; error?: string }> {
    const trySend = () =>
      chrome.tabs.sendMessage(tabId, { type: 'compose_post', text }) as Promise<{
        ok?: boolean
        ref?: string
        error?: string
      }>
    try {
      return await trySend()
    } catch (e) {
      const msg = String((e as { message?: string })?.message || e)
      if (!/Receiving end does not exist|Could not establish connection/i.test(msg)) {
        return { error: msg }
      }
    }
    // No content script in the tab — inject it, then retry.
    try {
      await chrome.scripting.executeScript({ target: { tabId }, files: ['content-scripts/relay.js'] })
    } catch (e) {
      return {
        error:
          'Không nối được với tab Facebook (content script chưa nạp). Mở/refresh 1 tab facebook.com rồi thử lại. ' +
          String((e as { message?: string })?.message || e),
      }
    }
    // Give the freshly-injected script a tick to register its message listener.
    await new Promise((r) => setTimeout(r, 400))
    try {
      return await trySend()
    } catch (e) {
      return { error: String((e as { message?: string })?.message || e) }
    }
  }

  function waitTabComplete(tabId: number, timeoutMs = 20000): Promise<void> {
    return new Promise((resolve) => {
      const start = Date.now()
      const tick = () => {
        chrome.tabs.get(tabId).then((t) => {
          if (t.status === 'complete' || Date.now() - start > timeoutMs) setTimeout(resolve, 1500)
          else setTimeout(tick, 400)
        }).catch(() => resolve())
      }
      tick()
    })
  }

  // Report whether a platform has a live session and, best-effort, who is logged
  // in. Identity resolution is per-adapter and may use cached page-VM tokens.
  async function whoAmI(params: Record<string, unknown>) {
    const platform = String(params.platform || '')
    const adapter = adapterById(ADAPTERS, platform)
    if (!adapter) throw new Error('unknown platform: ' + platform)

    // Make sure a fresh SW has restored the captured tokens before answering.
    await hydrated

    let present = false
    for (const domain of adapter.hosts) {
      const c = await chrome.cookies.get({ url: `https://${domain}/`, name: adapter.sessionCookie }).catch(() => null)
      if (c && c.value) {
        present = true
        break
      }
    }
    if (!present) return { logged_in: false }

    const ctx = {
      fetch: credentialedFetch,
      headers: captured[adapter.id]?.headers || {},
      cookie: async (name: string, domain: string) => {
        const c = await chrome.cookies.get({ url: `https://${domain}/`, name }).catch(() => null)
        return c && c.value ? c.value : null
      },
      meta: metaCache[adapter.id] || null,
    }
    let ident: Identity = {}
    try {
      ident = (adapter.whoami && (await adapter.whoami(ctx))) || {}
    } catch {
      ident = {}
    }
    return { logged_in: true, platform, ...ident }
  }

  // ---------------- Token capture ----------------

  chrome.webRequest.onBeforeSendHeaders.addListener(
    (details) => {
      try {
        const u = new URL(details.url)
        const adapter = adapterForHost(ADAPTERS, u.hostname)
        if (!adapter) return
        const grab: Record<string, string> = {}
        for (const h of details.requestHeaders || []) {
          if (CAPTURE_HEADERS.has(h.name.toLowerCase())) grab[h.name.toLowerCase()] = h.value || ''
        }
        let changed = false
        if (Object.keys(grab).length) {
          captured[adapter.id] = {
            headers: { ...(captured[adapter.id]?.headers || {}), ...grab },
            at: Date.now(),
          }
          changed = true
        }
        // A Graph access token (EAA…) sometimes rides the page's own request
        // URL — grab it so replays have it even without the content script.
        const at = u.searchParams.get('access_token')
        if (at && /^EAA/.test(at)) {
          metaCache[adapter.id] = { ...(metaCache[adapter.id] || { platform: adapter.id }), access_token: at, at: Date.now() }
          changed = true
        }
        if (changed) persist()
      } catch {
        /* ignore malformed requests */
      }
    },
    { urls: MATCH_URLS },
    ['requestHeaders', 'extraHeaders'],
  )

  // Learn the Facebook composer request: when the user posts manually, FB sends a
  // ComposerStoryCreateMutation to /api/graphql/. We capture the whole form body
  // (doc_id + variables + tokens) as a template to replay for API posting.
  chrome.webRequest.onBeforeRequest.addListener(
    (details) => {
      try {
        if (details.method !== 'POST') return
        const fd = details.requestBody?.formData as Record<string, string[]> | undefined
        if (!fd) return
        const friendly = fd['fb_api_req_friendly_name']?.[0] || ''
        const varsStr = fd['variables']?.[0] || ''
        const docId = fd['doc_id']?.[0]
        if (!docId || !varsStr) return
        const fields: Record<string, string> = {}
        for (const k in fd) fields[k] = fd[k][0]

        // Learn FEED pagination queries (for the "scan" path) by friendly name.
        // These fire as the user scrolls their own feed/profile/page/group.
        const feedKind = feedKindOf(friendly)
        if (feedKind) {
          fbFeedTpl[feedKind] = { fields, friendly, at: Date.now() }
          persist()
          console.log('[social] learned FB feed template:', feedKind, friendly)
          return
        }

        // Recognise a status-post composer mutation by the friendly name OR by
        // its variables shape (actor_id + message), while excluding comments.
        const isComment = /comment/i.test(friendly) || /"feedback_id"/.test(varsStr)
        const looksLikePost =
          (/composer/i.test(friendly) && /create|publish|story/i.test(friendly)) ||
          (/"actor_id"/.test(varsStr) && /"message"/.test(varsStr))
        if (isComment || !looksLikePost) return
        fbComposerTemplate = { fields, at: Date.now() }
        persist()
        console.log('[social] learned FB composer template:', friendly)
      } catch {
        /* ignore */
      }
    },
    { urls: ['*://*.facebook.com/api/graphql/*', '*://*.facebook.com/api/graphql/'] },
    ['requestBody'],
  )

  // ---------------- Page-VM tokens (from the MAIN-world content script) ----

  chrome.runtime.onMessage.addListener((msg: { type?: string } & Partial<MetaTokens>) => {
    if (msg && msg.type === 'meta_tokens' && msg.platform) {
      const prev = metaCache[msg.platform] || { platform: msg.platform }
      // Merge, keeping any previously-captured field a later (thinner) page
      // load might omit — e.g. an access_token grabbed from a request URL.
      metaCache[msg.platform] = {
        platform: msg.platform,
        name: msg.name || prev.name,
        id: msg.id || prev.id,
        fb_dtsg: msg.fb_dtsg || prev.fb_dtsg,
        lsd: msg.lsd || prev.lsd,
        jazoest: msg.jazoest || prev.jazoest,
        access_token: msg.access_token || prev.access_token,
        at: Date.now(),
      }
      persist()
    }
  })

  // ---------------- Heartbeat ----------------

  async function hostsReady(): Promise<string[]> {
    const ready: string[] = []
    for (const a of ADAPTERS) {
      for (const domain of a.hosts) {
        const c = await chrome.cookies.get({ url: `https://${domain}/`, name: a.sessionCookie }).catch(() => null)
        if (c && c.value) {
          ready.push(a.id)
          break
        }
      }
    }
    return ready
  }

  function capabilitiesMap(): Record<string, Capabilities> {
    const m: Record<string, Capabilities> = {}
    for (const a of ADAPTERS) m[a.id] = a.capabilities
    return m
  }

  chrome.alarms.create('heartbeat', { periodInMinutes: 0.25 })
  chrome.alarms.onAlarm.addListener(async (alarm) => {
    if (alarm.name !== 'heartbeat') return
    if (!ws || ws.readyState !== WebSocket.OPEN) return
    const ready = await hostsReady()
    const m = chrome.runtime.getManifest()
    send({
      type: 'heartbeat',
      name: m.name,
      version: m.version,
      hosts_ready: ready,
      captured_hosts: Object.keys(captured),
      meta_hosts: Object.keys(metaCache),
      fb_composer_ready: !!fbComposerTemplate?.fields?.doc_id,
      capabilities: capabilitiesMap(),
    })
  })

  // Silence "unused" while keeping the secret available for a future HTTP fallback.
  void callbackSecret

  connect()
})
