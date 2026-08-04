import { createContext, useContext } from 'react'

export type ThemeMode = 'dark' | 'light'

export const ThemeCtx = createContext<{ mode: ThemeMode; toggle: () => void }>({
  mode: 'dark',
  toggle: () => {},
})

export const useThemeMode = () => useContext(ThemeCtx)
