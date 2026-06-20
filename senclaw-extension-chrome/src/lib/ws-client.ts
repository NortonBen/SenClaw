// WebSocket client with auto-reconnect, heartbeat, and lifecycle events.
import type { DaemonMessage, ExtensionMessage } from '../types/protocol';

const HEARTBEAT_INTERVAL = 15_000;
const RECONNECT_BACKOFF = [1, 2, 4, 8, 16, 30]; // seconds

export type ConnectionState =
  | 'idle'
  | 'connecting'
  | 'connected'
  | 'disconnected'
  | 'reconnecting';

type MessageHandler = (msg: DaemonMessage) => void;
type StatusHandler = (state: ConnectionState, detail?: string) => void;

export class WSClient {
  private ws: WebSocket | null = null;
  private host: string;
  private port: number;
  private messageHandler: MessageHandler | null = null;
  private statusHandler: StatusHandler | null = null;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private reconnectAttempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private activeTabId: string | null = null;
  private state: ConnectionState = 'idle';
  private disposed = false;

  constructor(host: string, port: number) {
    this.host = host;
    this.port = port;
  }

  private get wsUrl(): string {
    return `ws://${this.host}:${this.port}/browser`;
  }

  /** Returns true if the WebSocket is OPEN. */
  isConnected(): boolean {
    return this.ws?.readyState === WebSocket.OPEN;
  }

  getState(): ConnectionState {
    return this.state;
  }

  getEndpoint(): string {
    return this.wsUrl;
  }

  onMessage(handler: MessageHandler): void {
    this.messageHandler = handler;
  }

  onStatusChange(handler: StatusHandler): void {
    this.statusHandler = handler;
  }

  setActiveTabId(tabId: string | null): void {
    this.activeTabId = tabId;
  }

  /** Update endpoint and force a fresh connection. */
  setEndpoint(host: string, port: number): void {
    const same = host === this.host && port === this.port;
    this.host = host;
    this.port = port;
    if (same) return;
    this.reconnectAttempt = 0;
    this.disconnect(/* permanent */ false);
    this.connect();
  }

  /** Open (or no-op if already opening/open). */
  connect(): void {
    if (this.disposed) return;
    if (
      this.ws &&
      (this.ws.readyState === WebSocket.OPEN ||
        this.ws.readyState === WebSocket.CONNECTING)
    ) {
      return;
    }
    // Cancel any pending reconnect — we're acting now.
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }

    this.setState('connecting', this.wsUrl);

    let socket: WebSocket;
    try {
      socket = new WebSocket(this.wsUrl);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      this.setState('disconnected', `construct failed: ${msg}`);
      this.scheduleReconnect();
      return;
    }
    this.ws = socket;

    socket.onopen = () => {
      this.reconnectAttempt = 0;
      this.setState('connected', this.wsUrl);
      this.startHeartbeat();
    };

    socket.onmessage = (event) => {
      try {
        const msg: DaemonMessage = JSON.parse(event.data as string);
        this.messageHandler?.(msg);
      } catch (e) {
        const detail = e instanceof Error ? e.message : String(e);
        this.statusHandler?.(this.state, `parse error: ${detail}`);
      }
    };

    socket.onclose = (event) => {
      this.stopHeartbeat();
      this.ws = null;
      const wasConnected = this.state === 'connected';
      const reason =
        event.reason ||
        (event.code === 1006
          ? 'abnormal close (daemon down or network)'
          : `code ${event.code}`);
      this.setState('disconnected', wasConnected ? `closed: ${reason}` : reason);
      this.scheduleReconnect();
    };

    socket.onerror = () => {
      // onerror fires before onclose with no detail in MV3; flag and let close handle reconnect.
      this.statusHandler?.(this.state, 'socket error');
    };
  }

  send(msg: ExtensionMessage): boolean {
    if (this.ws?.readyState === WebSocket.OPEN) {
      try {
        this.ws.send(JSON.stringify(msg));
        return true;
      } catch (e) {
        const detail = e instanceof Error ? e.message : String(e);
        this.statusHandler?.(this.state, `send failed: ${detail}`);
        return false;
      }
    }
    this.statusHandler?.(this.state, 'send dropped: not connected');
    return false;
  }

  private startHeartbeat(): void {
    this.stopHeartbeat();
    this.heartbeatTimer = setInterval(() => {
      if (this.ws?.readyState !== WebSocket.OPEN) {
        this.stopHeartbeat();
        return;
      }
      chrome.tabs.query({}, (allTabs) => {
        this.send({
          type: 'Heartbeat',
          tab_count: allTabs.length,
          active_tab_id: this.activeTabId ?? undefined,
        });
      });
    }, HEARTBEAT_INTERVAL);
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer) {
      clearInterval(this.heartbeatTimer);
      this.heartbeatTimer = null;
    }
  }

  private scheduleReconnect(): void {
    if (this.disposed) return;
    if (this.reconnectTimer) return;
    const idx = Math.min(this.reconnectAttempt, RECONNECT_BACKOFF.length - 1);
    const delayMs = RECONNECT_BACKOFF[idx] * 1000;
    this.reconnectAttempt++;
    this.setState(
      'reconnecting',
      `retry #${this.reconnectAttempt} in ${delayMs / 1000}s`,
    );
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delayMs);
  }

  private setState(next: ConnectionState, detail?: string): void {
    if (this.state === next && !detail) return;
    this.state = next;
    this.statusHandler?.(next, detail);
  }

  disconnect(permanent = true): void {
    if (permanent) this.disposed = true;
    this.stopHeartbeat();
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      try {
        // Drop handlers BEFORE closing so onclose doesn't trigger a reconnect.
        this.ws.onopen = null;
        this.ws.onclose = null;
        this.ws.onerror = null;
        this.ws.onmessage = null;
        this.ws.close();
      } catch {
        /* ignore */
      }
      this.ws = null;
    }
    if (permanent) this.setState('disconnected', 'disposed');
  }
}
