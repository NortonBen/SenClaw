// Theme comes from the SenClaw host when embedded in an iframe; falls back to
// a locally remembered choice when opened standalone.

import { useEffect, useState } from 'react'

export type ThemeMode = 'light' | 'dark'

const KEY = 'rule-engine-theme'

interface HostMessage {
  type?: string
  theme?: string
}

export function useHostTheme(): [ThemeMode, (m: ThemeMode) => void] {
  const [mode, setMode] = useState<ThemeMode>(
    () => (localStorage.getItem(KEY) as ThemeMode | null) ?? 'dark',
  )

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const d = e.data as HostMessage | null
      if (!d || typeof d !== 'object') return
      if (
        (d.type === 'senclaw:init' || d.type === 'senclaw:theme') &&
        (d.theme === 'dark' || d.theme === 'light')
      ) {
        setMode(d.theme)
      }
    }
    window.addEventListener('message', onMessage)
    window.parent?.postMessage({ type: 'senclaw:ready' }, '*')
    return () => window.removeEventListener('message', onMessage)
  }, [])

  useEffect(() => {
    localStorage.setItem(KEY, mode)
    document.documentElement.dataset.theme = mode
    // Keeps scrollbars and native controls in step with the AntD algorithm.
    document.documentElement.style.colorScheme = mode
  }, [mode])

  return [mode, setMode]
}
