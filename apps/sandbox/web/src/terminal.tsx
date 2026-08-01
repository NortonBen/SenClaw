import { useEffect, useRef } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import type { Resolved } from './theme'

/**
 * Interactive shell inside a sandbox.
 *
 * Frame protocol matches the server (`src/pty.rs`) and the terminal in
 * apps/code-ide: `{"d": keys}` for input, `{"r": [cols, rows]}` on resize,
 * binary frames back are raw PTY bytes.
 */
export function SandboxTerminal({ sandboxId, mode }: { sandboxId: string; mode: Resolved }) {
  const host = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!host.current) return

    const term = new Terminal({
      convertEol: true,
      fontSize: 12.5,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      theme:
        mode === 'dark'
          ? { background: '#141414', foreground: '#e6e6e6', cursor: '#00a37a' }
          : { background: '#fafafa', foreground: '#1f1f1f', cursor: '#00a37a' },
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host.current)
    fit.fit()

    const proto = location.protocol === 'https:' ? 'wss' : 'ws'
    const ws = new WebSocket(`${proto}://${location.host}/api/sandboxes/${sandboxId}/terminal`)
    ws.binaryType = 'arraybuffer'

    const sendSize = () => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify({ r: [term.cols, term.rows] }))
      }
    }

    ws.onopen = sendSize
    ws.onmessage = (e) => {
      if (typeof e.data === 'string') term.write(e.data)
      else term.write(new Uint8Array(e.data))
    }
    // A closed socket with no explanation reads as a frozen terminal, so say so
    // in the terminal itself.
    ws.onclose = () => term.write('\r\n\x1b[2m[phiên đã đóng]\x1b[0m\r\n')
    ws.onerror = () => term.write('\r\n\x1b[31m[mất kết nối tới sandbox]\x1b[0m\r\n')

    const sub = term.onData((d) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ d }))
    })

    // The pane resizes with the window and with the surrounding layout, so a
    // window listener alone misses the common case of a sidebar toggling.
    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
        sendSize()
      } catch {
        /* the pane can be measured at zero size while hidden */
      }
    })
    ro.observe(host.current)

    return () => {
      ro.disconnect()
      sub.dispose()
      ws.close()
      term.dispose()
    }
  }, [sandboxId, mode])

  return <div className="sbx-term" ref={host} style={{ background: mode === 'dark' ? '#141414' : '#fafafa' }} />
}
