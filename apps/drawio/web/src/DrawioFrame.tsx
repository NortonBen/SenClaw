import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
} from 'react'

/**
 * Thin wrapper around the self-hosted draw.io editor iframe using the official
 * embed postMessage JSON protocol (embed=1&proto=json). The editor webapp is
 * served same-origin at /drawio/ after the first-run download; stealth=1 keeps
 * it from making any external calls.
 */
export interface DrawioFrameHandle {
  /** Latest XML the editor reported (autosave) — may be ahead of the DB. */
  getXml(): string
  /** Replace the whole diagram. */
  loadXml(xml: string): void
  /** Merge XML into the current diagram (AI additions). */
  mergeXml(xml: string): void
  /** Load Mermaid source — the editor converts it into editable shapes. */
  loadMermaid(code: string): void
  /** Ask the editor for an SVG snapshot (arrives via onSvg). */
  exportSvg(): void
}

interface Props {
  initialXml: string
  dark: boolean
  /** Diagram changed inside the editor (also fired on explicit save). */
  onAutosave(xml: string): void
  /** SVG snapshot decoded to raw text. */
  onSvg(svg: string): void
  /** Editor finished init and loaded the initial XML. */
  onReady(): void
}

/** Decode the editor's `data:image/svg+xml;base64,…` export to raw SVG text. */
function decodeSvgDataUri(data: string): string | null {
  const m = data.match(/^data:image\/svg\+xml;base64,(.*)$/s)
  if (m) {
    try {
      const bytes = Uint8Array.from(atob(m[1]), (c) => c.charCodeAt(0))
      return new TextDecoder().decode(bytes)
    } catch {
      return null
    }
  }
  if (data.startsWith('data:image/svg+xml,')) {
    try {
      return decodeURIComponent(data.slice('data:image/svg+xml,'.length))
    } catch {
      return null
    }
  }
  return data.startsWith('<svg') || data.startsWith('<?xml') ? data : null
}

export const DrawioFrame = forwardRef<DrawioFrameHandle, Props>(function DrawioFrame(
  { initialXml, dark, onAutosave, onSvg, onReady },
  ref,
) {
  const iframe = useRef<HTMLIFrameElement>(null)
  // Tracks the freshest XML: seeded with the prop, then updated on every
  // autosave, so a src-triggered reload (theme switch) restores current work.
  const xmlRef = useRef(initialXml)

  // Callbacks in refs so the message listener never goes stale.
  const cbs = useRef({ onAutosave, onSvg, onReady })
  cbs.current = { onAutosave, onSvg, onReady }

  const post = (msg: Record<string, unknown>) => {
    iframe.current?.contentWindow?.postMessage(JSON.stringify(msg), '*')
  }

  useEffect(() => {
    const onMsg = (e: MessageEvent) => {
      // Only the editor iframe speaks stringified-JSON here; host (SenClaw)
      // messages are plain objects and are handled in App.
      if (e.source !== iframe.current?.contentWindow || typeof e.data !== 'string') return
      let m: { event?: string; xml?: string; format?: string; data?: string }
      try {
        m = JSON.parse(e.data)
      } catch {
        return
      }
      switch (m.event) {
        case 'init':
          post({ action: 'load', xml: xmlRef.current || '', autosave: 1, dark: dark ? 1 : 0 })
          break
        case 'load':
          // XML loads keep the stored viewport, which may sit far from the
          // content (AI-generated diagrams especially) — always re-fit.
          post({ action: 'fit' })
          cbs.current.onReady()
          break
        case 'autosave':
          if (typeof m.xml === 'string') {
            xmlRef.current = m.xml
            cbs.current.onAutosave(m.xml)
          }
          break
        case 'save':
          if (typeof m.xml === 'string') {
            xmlRef.current = m.xml
            cbs.current.onAutosave(m.xml)
          }
          post({ action: 'export', format: 'svg' })
          break
        case 'export': {
          if (typeof m.data === 'string') {
            const svg = decodeSvgDataUri(m.data)
            if (svg) cbs.current.onSvg(svg)
          }
          break
        }
      }
    }
    window.addEventListener('message', onMsg)
    return () => window.removeEventListener('message', onMsg)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dark])

  useImperativeHandle(ref, () => ({
    getXml() {
      return xmlRef.current
    },
    loadXml(xml: string) {
      xmlRef.current = xml
      post({ action: 'load', xml, autosave: 1, dark: dark ? 1 : 0 })
    },
    mergeXml(xml: string) {
      post({ action: 'merge', xml })
    },
    loadMermaid(code: string) {
      post({ action: 'load', descriptor: { format: 'mermaid', data: code }, autosave: 1 })
    },
    exportSvg() {
      post({ action: 'export', format: 'svg' })
    },
  }))

  const src = `drawio/index.html?embed=1&proto=json&spin=1&libraries=1&stealth=1${dark ? '&dark=1' : ''}`
  return <iframe ref={iframe} className="drawio-frame" title="draw.io" src={src} />
})
