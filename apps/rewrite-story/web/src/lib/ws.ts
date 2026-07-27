import { useEffect, useRef } from "react";

/**
 * The server envelope is `{type, data, timestamp}`.
 *
 * video-flow's client reads `event` here instead, which never matches, so its
 * live updates silently never fire and the UI falls back to polling. Keep this
 * as `type`.
 */
export interface DashEvent {
  type: string;
  data: Record<string, unknown>;
  timestamp: string;
}

export function useDashboardWS(onEvent: (e: DashEvent) => void) {
  const cbRef = useRef(onEvent);
  cbRef.current = onEvent;

  useEffect(() => {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${window.location.host}/ws/dashboard`;
    let ws: WebSocket | undefined;
    let closed = false;

    function connect() {
      ws = new WebSocket(url);
      ws.onmessage = (e) => {
        try {
          cbRef.current(JSON.parse(e.data as string) as DashEvent);
        } catch {
          /* ignore malformed frames */
        }
      };
      ws.onclose = () => {
        if (!closed) setTimeout(connect, 3000);
      };
    }
    connect();

    return () => {
      closed = true;
      ws?.close();
    };
  }, []);
}
