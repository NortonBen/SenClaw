import React from 'react'
import ReactDOM from 'react-dom/client'
import { App as AntApp, ConfigProvider, theme } from 'antd'
import viVN from 'antd/locale/vi_VN'
import App from './App'
import { ThemeProvider, type Resolved } from './theme'
import './index.css'
import '@xterm/xterm/css/xterm.css'

/**
 * Paint the page surface from the resolved antd tokens.
 *
 * `<body>` has no background of its own, so without this the app's panels are
 * themed but the canvas behind them is whatever the browser or host defaults
 * to — a light strip under a dark app, and a white flash on every reload.
 */
function Surface({ children }: { children: React.ReactNode }) {
  const { token } = theme.useToken()
  React.useEffect(() => {
    document.body.style.background = token.colorBgLayout
    document.body.style.color = token.colorText
    document.documentElement.style.setProperty('--sbx-code-bg', token.colorFillQuaternary)
    document.documentElement.style.setProperty('--sbx-border', token.colorBorderSecondary)
  }, [token])
  return <>{children}</>
}

function Themed({ mode }: { mode: Resolved }) {
  return (
    <ConfigProvider
      locale={viVN}
      theme={{
        algorithm: mode === 'dark' ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: { colorPrimary: '#00a37a' },
      }}
    >
      <AntApp>
        <Surface>
          <App mode={mode} />
        </Surface>
      </AntApp>
    </ConfigProvider>
  )
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ThemeProvider>{(mode) => <Themed mode={mode} />}</ThemeProvider>
  </React.StrictMode>,
)
