// Shell: theme + a two-view router driven by `?chain=<id>` so the browser back
// button works without pulling in react-router. A query string (rather than a
// path segment) keeps the relative `./api/...` base intact on reload.

import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, ConfigProvider, theme as antTheme } from 'antd'
import viVN from 'antd/locale/vi_VN'
import { useHostTheme } from './theme'
import ChainList from './views/ChainList'
import ChainEditor from './views/ChainEditor'

/** The page itself is transparent by default, which leaks the browser's own
 *  light/dark canvas colour; paint it from the active AntD theme instead. */
function Surface({ children }: { children: React.ReactNode }) {
  const { token } = antTheme.useToken()
  return (
    <div
      style={{
        minHeight: '100%',
        background: token.colorBgLayout,
        color: token.colorText,
      }}
    >
      {children}
    </div>
  )
}

function readChainId(): number | null {
  const raw = new URLSearchParams(window.location.search).get('chain')
  if (!raw) return null
  const id = Number(raw)
  return Number.isFinite(id) && id > 0 ? id : null
}

export default function App() {
  const [mode] = useHostTheme()
  const [chainId, setChainId] = useState<number | null>(() => readChainId())

  useEffect(() => {
    const onPop = () => setChainId(readChainId())
    window.addEventListener('popstate', onPop)
    return () => window.removeEventListener('popstate', onPop)
  }, [])

  const open = useCallback((id: number) => {
    const url = `${window.location.pathname}?chain=${id}`
    window.history.pushState({ chain: id }, '', url)
    setChainId(id)
  }, [])

  const back = useCallback(() => {
    window.history.pushState({}, '', window.location.pathname)
    setChainId(null)
  }, [])

  return (
    <ConfigProvider
      locale={viVN}
      theme={{
        algorithm: mode === 'dark' ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: { colorPrimary: '#4da3ff', borderRadius: 8 },
      }}
    >
      <AntApp style={{ height: '100%' }}>
        <Surface>
          {chainId === null ? (
            <ChainList onOpen={open} />
          ) : (
            <ChainEditor key={chainId} chainId={chainId} onBack={back} />
          )}
        </Surface>
      </AntApp>
    </ConfigProvider>
  )
}
