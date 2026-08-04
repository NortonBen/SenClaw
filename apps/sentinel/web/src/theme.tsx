// Sáng / tối / theo hệ thống. Lựa chọn lưu ở localStorage và đồng bộ lên
// `/api/settings` để agent hay thiết bị khác đọc được; chế độ "hệ thống" bám
// `prefers-color-scheme` và đổi ngay khi OS đổi, không cần tải lại trang.

import { createContext, useContext, useEffect, useMemo, useState } from 'react'
import { App as AntApp, ConfigProvider, theme as antdTheme } from 'antd'

export type ThemeMode = 'light' | 'dark' | 'system'

const KEY = 'senclaw-sentinel-theme'

interface Ctx {
  mode: ThemeMode
  setMode: (m: ThemeMode) => void
  /** Chế độ đang hiển thị thật sự (đã giải quyết 'system'). */
  dark: boolean
}

const ThemeCtx = createContext<Ctx>({ mode: 'system', setMode: () => {}, dark: true })

export const useTheme = () => useContext(ThemeCtx)

const prefersDark = () =>
  typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(() => {
    const saved = localStorage.getItem(KEY)
    return saved === 'light' || saved === 'dark' || saved === 'system' ? saved : 'system'
  })
  const [systemDark, setSystemDark] = useState(prefersDark)

  // Giữ listener luôn bật kể cả khi đang ở chế độ cố định, để lúc quay lại
  // 'system' là đúng ngay chứ không phải đợi OS đổi lần nữa.
  useEffect(() => {
    const mq = window.matchMedia?.('(prefers-color-scheme: dark)')
    if (!mq) return
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  const dark = mode === 'system' ? systemDark : mode === 'dark'

  const setMode = (m: ThemeMode) => {
    localStorage.setItem(KEY, m)
    setModeState(m)
    // Đồng bộ lên server là tiện ích, không phải điều kiện — hỏng thì bỏ qua
    // chứ không được chặn việc đổi giao diện.
    fetch('/api/settings', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ key: 'theme', value: m }),
    }).catch(() => {})
  }

  // Điều khiển phần CSS thuần: nền body, thanh cuộn, biểu đồ tự vẽ.
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? 'dark' : 'light'
    document.documentElement.style.colorScheme = dark ? 'dark' : 'light'
  }, [dark])

  const value = useMemo(() => ({ mode, setMode, dark }), [mode, dark])

  return (
    <ThemeCtx.Provider value={value}>
      <ConfigProvider
        theme={{
          algorithm: dark ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
          token: { colorPrimary: '#6366f1', borderRadius: 10 },
        }}
      >
        <AntApp>{children}</AntApp>
      </ConfigProvider>
    </ThemeCtx.Provider>
  )
}
