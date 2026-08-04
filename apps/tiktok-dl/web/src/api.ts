// Tiny fetch wrapper for the TikTok Downloader REST API (served from the same
// origin in production; proxied to :4670 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

const post = (url: string, body?: any) =>
  j(url, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) })

export interface Counters {
  active: number
  queued: number
  done: number
  error: number
  total: number
  bytes_done: number
}

export interface Status {
  ok: boolean
  counters: Counters
  download_dir: string
  default_quality: string
  max_concurrent: string
}

export interface Stats {
  play_count: number
  digg_count: number
  comment_count: number
  share_count: number
  download_count: number
  collect_count: number
  create_time: number
  region: string
}

/** Kết quả phân giải một link (chưa tải). */
export interface Meta {
  video_id: string
  kind: 'video' | 'images'
  title: string
  region: string
  duration: number
  cover_url: string
  author_id: string
  author_name: string
  author_avatar: string
  play: string
  wmplay: string
  hdplay: string
  size: number
  wm_size: number
  hd_size: number
  music_url: string
  music_title: string
  images: string[]
  stats: Stats
}

export type Quality = 'nowm' | 'hd' | 'wm' | 'audio'

export interface DownloadRow {
  id: number
  input_url: string
  video_id: string
  kind: string
  quality: string
  title: string
  author_id: string
  author_name: string
  cover_url: string
  duration: number
  files: string[]
  dir: string
  total_bytes: number
  progress_bytes: number
  status: 'queued' | 'resolving' | 'downloading' | 'done' | 'error' | 'canceled'
  error: string
  stats: Partial<Stats>
  music_title: string
  created_at: string
  started_at: string
  finished_at: string
}

export interface FeedVideo {
  video_id: string
  url: string
  title: string
  duration: number
  size: number
  cover: string
  is_images: boolean
  play_count: number
  create_time: number
}

export const api = {
  status: () => j<Status>('/api/status'),
  resolve: (url: string) => post('/api/resolve', { url }) as Promise<{ ok?: boolean; error?: string; url?: string; meta?: Meta }>,
  download: (url: string, quality: string, force = false, meta?: Meta) =>
    post('/api/download', { url, quality, force, meta }),
  batch: (text: string, quality: string, force = false) =>
    post('/api/download/batch', { text, quality, force }),
  list: (p: { q?: string; status?: string; kind?: string; limit?: number; offset?: number } = {}) => {
    const qs = new URLSearchParams()
    if (p.q) qs.set('q', p.q)
    if (p.status) qs.set('status', p.status)
    if (p.kind) qs.set('kind', p.kind)
    qs.set('limit', String(p.limit ?? 300))
    if (p.offset) qs.set('offset', String(p.offset))
    return j<{ ok: boolean; counters: Counters; downloads: DownloadRow[] }>(`/api/downloads?${qs}`)
  },
  get: (id: number) => j<{ ok?: boolean; download?: DownloadRow; error?: string }>(`/api/downloads/${id}`),
  cancel: (id: number) => post(`/api/downloads/${id}/cancel`),
  retry: (id: number) => post(`/api/downloads/${id}/retry`),
  delete: (id: number, withFile: boolean) => post(`/api/downloads/${id}/delete`, { with_file: withFile }),
  clear: (status?: string, withFiles = false) => post('/api/downloads/clear', { status, with_files: withFiles }),
  open: (id: number, reveal: boolean) => post(`/api/downloads/${id}/open`, { reveal }),
  openDir: () => post('/api/settings/open_dir'),
  profileFeed: (unique_id: string, count = 30, cursor = '') =>
    post('/api/profile/feed', { unique_id, count, cursor }) as Promise<{
      ok?: boolean
      error?: string
      hint?: string
      feed?: { unique_id: string; videos: FeedVideo[]; cursor: string; has_more: boolean }
    }>,
  profileDownload: (unique_id: string, max: number, quality: string) =>
    post('/api/profile/download', { unique_id, max, quality }),
  avatar: (url: string) => post('/api/avatar', { url }),
  settings: () => j<{ ok: boolean; settings: Record<string, string> }>('/api/settings'),
  setSettings: (patch: Record<string, string | number | boolean>) => post('/api/settings', patch),
  activity: () => j<{ ok: boolean; activity: { kind: string; message: string; ref_id: string; at: string }[] }>('/api/activity'),
}

// ---- shared formatters ----

export const fmtBytes = (n?: number) => {
  if (!n || n <= 0) return '—'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}

export const fmtNum = (n?: number) => {
  if (n === undefined || n === null) return '—'
  if (n < 1000) return String(n)
  if (n < 1_000_000) return `${(n / 1000).toFixed(1)}K`
  return `${(n / 1_000_000).toFixed(1)}M`
}

export const fmtDuration = (s?: number) => {
  if (!s || s <= 0) return ''
  const m = Math.floor(s / 60)
  const ss = s % 60
  return m > 0 ? `${m}:${String(ss).padStart(2, '0')}` : `${ss}s`
}

export const QUALITY_LABEL: Record<string, string> = {
  nowm: 'Không logo',
  hd: 'HD',
  wm: 'Có logo',
  audio: 'Nhạc MP3',
  avatar: 'Avatar',
}

export const KIND_LABEL: Record<string, string> = {
  video: 'Video',
  images: 'Bộ ảnh',
  audio: 'Nhạc',
  avatar: 'Avatar',
}

/** Đếm nhanh link TikTok/Douyin trong một đoạn text (xấp xỉ phía client —
 *  backend mới là nơi lọc chuẩn). */
export const countLinks = (text: string) =>
  (text.match(/(https?:\/\/[^\s,;"'<>]*)?(tiktok\.com|douyin\.com)\/[^\s,;"'<>]+/g) ?? []).length
