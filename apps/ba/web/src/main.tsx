import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { App as AntApp, ConfigProvider, theme } from 'antd'
import 'antd/dist/reset.css'
import App from './App'
import './index.css'

function Root() {
  const [dark, setDark] = useState(() => (localStorage.getItem('ba-theme') ?? 'dark') !== 'light')
  useEffect(() => {
    document.documentElement.dataset.appTheme = dark ? 'dark' : 'light'
    try {
      localStorage.setItem('ba-theme', dark ? 'dark' : 'light')
    } catch {
      /* private mode */
    }
  }, [dark])
  return (
    <ConfigProvider
      theme={{
        algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: { colorPrimary: '#7c5cff', borderRadius: 10 },
      }}
    >
      <AntApp>
        <App dark={dark} onToggleTheme={() => setDark((d) => !d)} />
      </AntApp>
    </ConfigProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
