import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { ConfigProvider, theme as antdTheme } from 'antd'
import 'antd/dist/reset.css'
import App from './App'
import { ThemeCtx, type ThemeMode } from './theme'
import './index.css'

/** Ưu tiên lựa chọn đã lưu; chưa có thì theo giao diện hệ thống. */
function initialMode(): ThemeMode {
  const saved = localStorage.getItem('cafe-theme')
  if (saved === 'dark' || saved === 'light') return saved
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function Root() {
  const [mode, setMode] = useState<ThemeMode>(initialMode)

  useEffect(() => {
    localStorage.setItem('cafe-theme', mode)
    // antd chỉ đổi màu component — scrollbar/input native + nền trang đổi ở đây.
    document.documentElement.style.colorScheme = mode
    document.body.style.background = mode === 'dark' ? '#141414' : '#f5f5f5'
  }, [mode])

  return (
    <ThemeCtx.Provider
      value={{ mode, toggle: () => setMode((m) => (m === 'dark' ? 'light' : 'dark')) }}
    >
      <ConfigProvider
        theme={{
          algorithm: mode === 'dark' ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
          token: { colorPrimary: '#d97706', borderRadius: 10 },
        }}
      >
        <App />
      </ConfigProvider>
    </ThemeCtx.Provider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
