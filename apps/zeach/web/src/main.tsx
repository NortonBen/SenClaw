import 'antd/dist/reset.css'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { App as AntApp, ConfigProvider, theme as antTheme } from 'antd'
import viVN from 'antd/locale/vi_VN'
import App from './App'
import './index.css'
import { installExternalLinkHook } from './openExternal'
import { themeToken, useThemeMode } from './theme'

// Link ngoài phải mở trên trình duyệt hệ thống, không điều hướng webview nhúng.
installExternalLinkHook()

function Root() {
  const mode = useThemeMode()
  return (
    <ConfigProvider
      locale={viVN}
      theme={{
        algorithm: mode === 'dark' ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: themeToken,
      }}
    >
      <AntApp>
        <App />
      </AntApp>
    </ConfigProvider>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
)
