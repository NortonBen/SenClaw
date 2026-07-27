import { useEffect, useRef } from "react";

export interface DashEvent {
  /** The envelope key is `type`, matching what the Rust hub emits. */
  type: string;
  data: Record<string, unknown>;
  timestamp: string;
}

/**
 * Subscribe to the app's dashboard socket.
 *
 * Analysis runs for minutes, so the UI listens for completion rather than
 * polling hard; the poll in App.tsx is only a slow safety net.
 */
export function useDashboardWS(onEvent: (e: DashEvent) => void) {
  const handler = useRef(onEvent);
  handler.current = onEvent;

  useEffect(() => {
    let socket: WebSocket | null = null;
    let timer: number | undefined;
    let closed = false;

    const connect = () => {
      if (closed) return;
      const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${proto}//${window.location.host}/ws/dashboard`);
      socket.onmessage = (ev) => {
        try {
          handler.current(JSON.parse(ev.data) as DashEvent);
        } catch {
          /* ignore malformed frames */
        }
      };
      socket.onclose = () => {
        if (!closed) timer = window.setTimeout(connect, 3000);
      };
      socket.onerror = () => socket?.close();
    };

    connect();
    return () => {
      closed = true;
      if (timer) window.clearTimeout(timer);
      socket?.close();
    };
  }, []);
}
