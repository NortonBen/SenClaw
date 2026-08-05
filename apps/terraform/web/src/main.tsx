import { StrictMode, useEffect, useMemo, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { App as AntApp, ConfigProvider, theme } from 'antd'
import 'antd/dist/reset.css'
import App from './App'
import './index.css'

type Mode = 'dark' | 'light'
export type ThemePref = 'auto' | Mode

function systemLight(): boolean {
  try {
    return window.matchMedia('(prefers-color-scheme: light)').matches
  } catch {
    return false
  }
}

function Root() {
  // Ưu tiên: user chọn tay (lưu localStorage) → host SenClaw đẩy postMessage →
  // theme hệ điều hành. Desktop shell (webview) không gửi postMessage nên nút
  // chọn tay trên header là đường chắc chắn luôn hoạt động.
  const [pref, setPrefState] = useState<ThemePref>(() => {
    const v = localStorage.getItem('tf-theme')
    return v === 'dark' || v === 'light' || v === 'auto' ? v : 'auto'
  })
  const [hostTheme, setHostTheme] = useState<Mode | null>(null)
  const [sysLight, setSysLight] = useState(systemLight)

  const setPref = (v: ThemePref) => {
    setPrefState(v)
    try {
      localStorage.setItem('tf-theme', v)
    } catch {
      /* webview chặn storage thì thôi — chỉ mất persist */
    }
  }

  useEffect(() => {
    const onMsg = (e: MessageEvent) => {
      const d = e.data as { type?: string; theme?: string } | null
      if (d && (d.type === 'senclaw:init' || d.type === 'senclaw:theme')) {
        if (d.theme === 'dark' || d.theme === 'light') setHostTheme(d.theme)
      }
    }
    window.addEventListener('message', onMsg)
    if (window.parent !== window) {
      window.parent.postMessage({ type: 'senclaw:ready' }, '*')
    }
    const mq = window.matchMedia('(prefers-color-scheme: light)')
    const onMq = (ev: MediaQueryListEvent) => setSysLight(ev.matches)
    mq.addEventListener?.('change', onMq)
    return () => {
      window.removeEventListener('message', onMsg)
      mq.removeEventListener?.('change', onMq)
    }
  }, [])

  const mode: Mode = useMemo(() => {
    if (pref !== 'auto') return pref
    if (hostTheme) return hostTheme
    return sysLight ? 'light' : 'dark'
  }, [pref, hostTheme, sysLight])

  useEffect(() => {
    document.documentElement.dataset.theme = mode
  }, [mode])

  return (
    <ConfigProvider
      theme={{
        algorithm: mode === 'dark' ? theme.darkAlgorithm : theme.defaultAlgorithm,
        // Tím Terraform — đậm hơn trên nền sáng cho đủ tương phản.
        token: { colorPrimary: mode === 'dark' ? '#a78bfa' : '#7c3aed', borderRadius: 10 },
      }}
    >
      <AntApp>
        <App themePref={pref} onThemePref={setPref} />
      </AntApp>
    </ConfigProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
