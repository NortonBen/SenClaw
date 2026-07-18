// One EventSource for the whole app, fanned out to subscribers.
//
// Each page used to open its own `new EventSource('/api/events')`. Under
// HTTP/1.1 a browser allows only ~6 concurrent connections per origin, and an
// SSE stream holds its connection open forever — so a handful of pages each
// opening one starves the pool and ordinary fetches start failing (a 502 from
// the dev proxy, or a request that simply never starts). One shared stream
// keeps exactly one connection in use no matter how many pages listen.

type Handler = (data: string) => void

let source: EventSource | null = null
const handlers = new Set<Handler>()

function ensureOpen() {
  if (source) return
  source = new EventSource('/api/events')
  source.onmessage = (e) => {
    for (const h of handlers) {
      try {
        h(e.data)
      } catch {
        // A throwing subscriber must not take down the others.
      }
    }
  }
  // EventSource reconnects on its own; a dropped frame just means the next
  // refetch is slightly staler, which is the whole point of using the stream
  // as a nudge rather than as the source of truth.
  source.onerror = () => {}
}

function closeIfIdle() {
  if (handlers.size === 0 && source) {
    source.close()
    source = null
  }
}

/// Subscribe to the server event stream. Returns an unsubscribe function —
/// call it from a `useEffect` cleanup.
export function subscribeEvents(handler: Handler): () => void {
  handlers.add(handler)
  ensureOpen()
  return () => {
    handlers.delete(handler)
    closeIfIdle()
  }
}
