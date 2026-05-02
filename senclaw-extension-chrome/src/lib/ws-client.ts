// WebSocket client with auto-reconnect and heartbeat.
import type { DaemonMessage, ExtensionMessage } from '../types/protocol';

const DEFAULT_WS_PORT = 18789;
const HEARTBEAT_INTERVAL = 15_000;
const RECONNECT_BACKOFF = [1, 2, 4, 8, 16, 30]; // seconds

type MessageHandler = (msg: DaemonMessage) => void;
type StatusHandler = (connected: boolean) => void;

export class WSClient {
  private ws: WebSocket | null = null;
  private wsUrl: string;
  private messageHandler: MessageHandler | null = null;
  private statusHandler: StatusHandler | null = null;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private reconnectAttempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private activeTabId: string | null = null;

  constructor(port?: number) {
    this.wsUrl = `ws://127.0.0.1:${port ?? DEFAULT_WS_PORT}/browser`;
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

  connect(): void {
    if (this.ws?.readyState === WebSocket.OPEN) return;

    try {
      this.ws = new WebSocket(this.wsUrl);
    } catch {
      this.scheduleReconnect();
      return;
    }

    this.ws.onopen = () => {
      console.log('[SenClaw] WebSocket connected');
      this.reconnectAttempt = 0;
      this.statusHandler?.(true);
      this.startHeartbeat();
    };

    this.ws.onmessage = (event) => {
      try {
        const msg: DaemonMessage = JSON.parse(event.data as string);
        this.messageHandler?.(msg);
      } catch (e) {
        console.warn('[SenClaw] Failed to parse WS message:', e);
      }
    };

    this.ws.onclose = () => {
      console.log('[SenClaw] WebSocket disconnected');
      this.stopHeartbeat();
      this.statusHandler?.(false);
      this.scheduleReconnect();
    };

    this.ws.onerror = (e) => {
      console.error('[SenClaw] WebSocket error:', e);
    };
  }

  send(msg: ExtensionMessage): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    } else {
      console.warn('[SenClaw] Cannot send, WS not connected');
    }
  }

  private startHeartbeat(): void {
    this.stopHeartbeat();
    this.heartbeatTimer = setInterval(() => {
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
    if (this.reconnectTimer) return;
    const delay = RECONNECT_BACKOFF[Math.min(this.reconnectAttempt, RECONNECT_BACKOFF.length - 1)] * 1000;
    console.log(`[SenClaw] Reconnecting in ${delay}ms (attempt ${this.reconnectAttempt + 1})`);
    this.reconnectAttempt++;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, delay);
  }

  disconnect(): void {
    this.stopHeartbeat();
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    this.ws?.close();
    this.ws = null;
  }
}
