import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'

/** What the user asked for. `system` = follow the host, or the OS when standalone. */
export type ThemeMode = 'system' | 'light' | 'dark'
/** What actually gets rendered. */
export type Resolved = 'light' | 'dark'

const KEY = 'study.theme'

function readStored(): ThemeMode {
  const v = localStorage.getItem(KEY)
  return v === 'light' || v === 'dark' || v === 'system' ? v : 'system'
}

interface Ctx {
  mode: ThemeMode
  resolved: Resolved
  setMode: (m: ThemeMode) => void
  /** True while running inside the SenClaw shell (an iframe). */
  embedded: boolean
}

const ThemeCtx = createContext<Ctx>({
  mode: 'system',
  resolved: 'dark',
  setMode: () => {},
  embedded: false,
})

export const useTheme = () => useContext(ThemeCtx)

/**
 * Theme resolution, in priority order:
 *
 * 1. **The user's explicit choice inside this app.** A toggle that the host can
 *    override is a toggle that lies — and this is a reading app, where wanting
 *    a light page inside a dark shell (or the reverse) is a real preference,
 *    not a mistake.
 * 2. **The host**, when embedded. SenClaw pushes `senclaw:init` / `senclaw:theme`
 *    over postMessage; following it keeps the app from clashing with the shell.
 * 3. **The OS**, when standalone — and it keeps following it, because
 *    `matchMedia` read once at startup misses the user switching at sunset.
 */
export function ThemeProvider({ children }: { children: (r: Resolved) => ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(readStored)
  const [hostTheme, setHostTheme] = useState<Resolved | null>(null)
  const [osTheme, setOsTheme] = useState<Resolved>(() =>
    window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  )
  const embedded = useMemo(() => {
    try {
      return window.parent !== window
    } catch {
      return true
    }
  }, [])

  // Keep following the OS rather than sampling it once at startup.
  useEffect(() => {
    const mq = window.matchMedia?.('(prefers-color-scheme: dark)')
    if (!mq) return
    const on = (e: MediaQueryListEvent) => setOsTheme(e.matches ? 'dark' : 'light')
    mq.addEventListener('change', on)
    return () => mq.removeEventListener('change', on)
  }, [])

  // The host announces its theme on init and again whenever it changes.
  useEffect(() => {
    const onMsg = (e: MessageEvent) => {
      const t = e.data?.theme ?? e.data?.env?.theme
      if (t === 'dark' || t === 'light') setHostTheme(t)
    }
    window.addEventListener('message', onMsg)
    window.parent?.postMessage({ type: 'senclaw:ready' }, '*')
    return () => window.removeEventListener('message', onMsg)
  }, [])

  const resolved: Resolved =
    mode === 'system' ? (embedded ? (hostTheme ?? osTheme) : osTheme) : mode

  // Tell the browser too: without `color-scheme`, native scrollbars, form
  // controls and the canvas behind the app stay light while everything drawn
  // on top is dark.
  useEffect(() => {
    const root = document.documentElement
    root.dataset.theme = resolved
    root.style.colorScheme = resolved
  }, [resolved])

  const setMode = useCallback((m: ThemeMode) => {
    setModeState(m)
    localStorage.setItem(KEY, m)
  }, [])

  const value = useMemo(
    () => ({ mode, resolved, setMode, embedded }),
    [mode, resolved, setMode, embedded],
  )

  return <ThemeCtx.Provider value={value}>{children(resolved)}</ThemeCtx.Provider>
}
