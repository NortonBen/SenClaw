// Chế độ sáng/tối: mặc định theo hệ điều hành, lưu lựa chọn vào localStorage,
// đồng bộ 2 tầng — antd ConfigProvider (algorithm) + CSS vars (data-theme trên <html>).

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { App as AntApp, ConfigProvider, theme as antdTheme } from 'antd'
import viVN from 'antd/locale/vi_VN'

export type ThemeMode = 'dark' | 'light'

const ThemeCtx = createContext<{ mode: ThemeMode; toggle: () => void }>({
  mode: 'dark',
  toggle: () => {},
})

// eslint-disable-next-line react-refresh/only-export-components
export const useTheme = () => useContext(ThemeCtx)

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setMode] = useState<ThemeMode>(() => {
    const saved = localStorage.getItem('discuss-theme')
    if (saved === 'dark' || saved === 'light') return saved
    return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
  })

  useEffect(() => {
    document.documentElement.dataset.theme = mode
    localStorage.setItem('discuss-theme', mode)
  }, [mode])

  const ctx = useMemo(
    () => ({ mode, toggle: () => setMode((m) => (m === 'dark' ? 'light' : 'dark')) }),
    [mode],
  )

  return (
    <ThemeCtx.Provider value={ctx}>
      <ConfigProvider
        locale={viVN}
        theme={
          mode === 'dark'
            ? {
                algorithm: antdTheme.darkAlgorithm,
                token: {
                  colorPrimary: '#4c8dff',
                  colorBgBase: '#12151c',
                  colorBgContainer: '#1c212c',
                  colorBorder: '#2c3342',
                  borderRadius: 8,
                  fontSize: 14,
                },
              }
            : {
                algorithm: antdTheme.defaultAlgorithm,
                token: {
                  colorPrimary: '#3b76e0',
                  borderRadius: 8,
                  fontSize: 14,
                },
              }
        }
      >
        <AntApp>{children}</AntApp>
      </ConfigProvider>
    </ThemeCtx.Provider>
  )
}
