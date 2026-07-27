import { useEffect, useState } from 'react'

/** House accent (SenClaw purple), shared with crm/deepwiki. */
export const ACCENT = '#5e4ae3'

export const FONT_FAMILY =
  "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"

/** AntD ConfigProvider token — matches the repo house style. */
export const themeToken = {
  colorPrimary: ACCENT,
  colorInfo: ACCENT,
  colorSuccess: '#34c759',
  colorWarning: '#ff9500',
  colorError: '#ff3b30',
  borderRadius: 8,
  fontFamily: FONT_FAMILY,
}

export type ThemeMode = 'light' | 'dark'

/**
 * Follow the host. The app runs in an iframe under the daemon, so dark/light
 * comes from the `senclaw:init`/`senclaw:theme` postMessage handshake first,
 * and falls back to the OS preference when run standalone.
 */
export function useThemeMode(): ThemeMode {
  const [mode, setMode] = useState<ThemeMode>(() =>
    window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  )
  useEffect(() => {
    const mq = window.matchMedia?.('(prefers-color-scheme: dark)')
    const onMq = (e: MediaQueryListEvent) => setMode(e.matches ? 'dark' : 'light')
    mq?.addEventListener('change', onMq)

    const onMsg = (e: MessageEvent) => {
      const d = e.data as { type?: string; theme?: string; env?: { theme?: string } }
      if (!d || typeof d !== 'object') return
      const t = d.theme ?? d.env?.theme
      if ((d.type === 'senclaw:init' || d.type === 'senclaw:theme') && (t === 'dark' || t === 'light')) {
        setMode(t)
      }
    }
    window.addEventListener('message', onMsg)
    try {
      window.parent?.postMessage({ type: 'senclaw:ready' }, '*')
    } catch {
      /* not in an iframe */
    }
    return () => {
      mq?.removeEventListener('change', onMq)
      window.removeEventListener('message', onMsg)
    }
  }, [])
  return mode
}

/** Vietnamese label for a tier — provenance, not truth. Mirrors the Rust
 *  `Tier::label_vi`, used when reading history rows that store only `tier`. */
export function tierLabelVi(tier: string): string {
  switch (tier) {
    case 'verified':
      return 'nhiều nguồn độc lập'
    case 'supported':
      return 'có nguồn hậu thuẫn'
    case 'single-source':
      return 'chỉ một nguồn'
    case 'disputed':
      return 'các nguồn mâu thuẫn'
    default:
      return 'không có bằng chứng'
  }
}

/** AntD Tag preset color for an evidence/claim tier — provenance strength. */
export function tierColor(tier: string): string {
  switch (tier) {
    case 'verified':
      return 'success'
    case 'supported':
      return 'processing'
    case 'disputed':
      return 'warning'
    case 'unverified':
      return 'error'
    default:
      return 'default' // single-source
  }
}

/** AntD Tag preset color for a source family. */
export function kindColor(kind: string): string {
  switch (kind) {
    case 'web':
      return 'blue'
    case 'internal':
      return 'purple'
    case 'social':
      return 'magenta'
    case 'docs':
      return 'gold'
    case 'code':
      return 'cyan'
    default:
      return 'default'
  }
}

/** AntD dot/badge status for a source health state. */
export function healthStatus(state: string): 'success' | 'warning' | 'error' | 'default' {
  if (state === 'ready') return 'success'
  if (state === 'degraded') return 'warning'
  if (state === 'unavailable') return 'error'
  return 'default'
}
