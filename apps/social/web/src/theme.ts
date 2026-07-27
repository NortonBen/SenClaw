// Dark/light mode: explicit user choice, falling back to the OS preference.

import { useEffect, useState } from 'react'

export type Mode = 'light' | 'dark'
const KEY = 'social.theme'

function systemMode(): Mode {
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

/** Stored choice, or the OS preference when the user hasn't chosen. */
export function initialMode(): Mode {
  const saved = localStorage.getItem(KEY)
  return saved === 'light' || saved === 'dark' ? saved : systemMode()
}

export function useThemeMode() {
  const [mode, setMode] = useState<Mode>(initialMode)

  // Keep following the OS while the user hasn't made an explicit choice.
  useEffect(() => {
    if (localStorage.getItem(KEY)) return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => setMode(systemMode())
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  // Let the page background follow the theme too.
  useEffect(() => {
    document.documentElement.style.colorScheme = mode
  }, [mode])

  const toggle = () => {
    const next: Mode = mode === 'dark' ? 'light' : 'dark'
    localStorage.setItem(KEY, next)
    setMode(next)
  }

  return { mode, toggle }
}
