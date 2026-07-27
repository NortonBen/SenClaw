import { useEffect, useRef } from 'react'
import { wsUrl } from './api'

// Envelope server LUÔN là `{type, data, timestamp}` — KHÔNG phải `event`
// (video-flow từng chết vì đọc sai key). type ∈ run:status | step:progress |
// dataset:updated | hello.
export interface DashEvent {
  type: string
  data: Record<string, unknown>
  timestamp: string
}

export function useDashboardWS(onEvent: (e: DashEvent) => void) {
  const cbRef = useRef(onEvent)
  cbRef.current = onEvent

  useEffect(() => {
    let ws: WebSocket | undefined
    let closed = false
    let timer: number | undefined

    function connect() {
      try {
        ws = new WebSocket(wsUrl())
      } catch {
        timer = window.setTimeout(connect, 3000)
        return
      }
      ws.onmessage = (e) => {
        try {
          cbRef.current(JSON.parse(e.data as string) as DashEvent)
        } catch {
          /* bỏ frame hỏng */
        }
      }
      ws.onclose = () => {
        if (!closed) timer = window.setTimeout(connect, 3000)
      }
      ws.onerror = () => ws?.close()
    }
    connect()

    return () => {
      closed = true
      if (timer) window.clearTimeout(timer)
      ws?.close()
    }
  }, [])
}
