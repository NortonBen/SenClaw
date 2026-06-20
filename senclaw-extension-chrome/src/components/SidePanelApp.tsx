import React, { useEffect, useState } from 'react';

type LogLevel = 'info' | 'event' | 'warn' | 'error';
interface LogEntry {
  ts: string;
  level: LogLevel;
  message: string;
}

type ConnectionState = 'idle' | 'connecting' | 'connected' | 'disconnected' | 'reconnecting';

const DEFAULT_HOST = '127.0.0.1';
const DEFAULT_PORT = 18789;

const LEVEL_COLOR: Record<LogLevel, string> = {
  info:  '#a6adc8',
  event: '#a6e3a1',
  warn:  '#f9e2af',
  error: '#f38ba8',
};

const STATE_COLOR: Record<ConnectionState, string> = {
  idle:          '#6c7086',
  connecting:    '#f9e2af',
  connected:     '#a6e3a1',
  reconnecting:  '#fab387',
  disconnected:  '#f38ba8',
};

function formatTime(ts: string): string {
  // ISO → HH:MM:SS
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString([], { hour12: false });
  } catch {
    return ts;
  }
}

export function SidePanelApp(): React.ReactElement {
  const [connectionState, setConnectionState] = useState<ConnectionState>('idle');
  const [connectionDetail, setConnectionDetail] = useState<string>('');
  const [endpoint, setEndpoint] = useState<string>('');
  const [wsHost, setWsHost] = useState(DEFAULT_HOST);
  const [wsPort, setWsPort] = useState(DEFAULT_PORT);
  const [hostInput, setHostInput] = useState(DEFAULT_HOST);
  const [portInput, setPortInput] = useState<string>(String(DEFAULT_PORT));
  const [log, setLog] = useState<LogEntry[]>([]);
  const [showSettings, setShowSettings] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [levelFilter, setLevelFilter] = useState<Set<LogLevel>>(
    new Set<LogLevel>(['info', 'event', 'warn', 'error']),
  );

  useEffect(() => {
    let cancelled = false;

    (async () => {
      const s = await chrome.storage.local.get(['ws_host', 'ws_port']);
      if (cancelled) return;
      const h = typeof s.ws_host === 'string' && s.ws_host ? s.ws_host : DEFAULT_HOST;
      const p = Number(s.ws_port);
      const port = Number.isFinite(p) && p > 0 ? p : DEFAULT_PORT;
      setWsHost(h);
      setWsPort(port);
      setHostInput(h);
      setPortInput(String(port));
    })();

    refreshStatus();
    refreshLogs();

    const tabListener = (_tabId: number, changeInfo: chrome.tabs.TabChangeInfo) => {
      if (changeInfo.status || changeInfo.url || changeInfo.title) {
        // no-op; tabs view not displayed in this simplified panel.
      }
    };
    chrome.tabs.onUpdated.addListener(tabListener);

    const interval = setInterval(refreshStatus, 3000);

    const runtimeListener = (message: any) => {
      if (!message) return;
      if (message.type === 'activity-log' && message.entry) {
        setLog((prev) => {
          const next = prev.concat([message.entry as LogEntry]);
          if (next.length > 500) next.splice(0, next.length - 500);
          return next;
        });
      } else if (message.type === 'activity-logs-cleared') {
        setLog([]);
      } else if (message.type === 'connection-state') {
        setConnectionState(message.state as ConnectionState);
        setConnectionDetail(message.detail ?? '');
      }
    };
    chrome.runtime.onMessage.addListener(runtimeListener);

    return () => {
      cancelled = true;
      chrome.tabs.onUpdated.removeListener(tabListener);
      chrome.runtime.onMessage.removeListener(runtimeListener);
      clearInterval(interval);
    };
  }, []);

  async function refreshStatus(): Promise<void> {
    try {
      const response = await chrome.runtime.sendMessage({ type: 'get-connection-status' });
      if (response?.state) setConnectionState(response.state as ConnectionState);
      else setConnectionState(response?.connected ? 'connected' : 'disconnected');
      if (typeof response?.endpoint === 'string') setEndpoint(response.endpoint);
    } catch {
      setConnectionState('disconnected');
    }
  }

  async function refreshLogs(): Promise<void> {
    try {
      const response = await chrome.runtime.sendMessage({ type: 'get-activity-logs' });
      if (Array.isArray(response?.logs)) setLog(response.logs as LogEntry[]);
    } catch {
      /* ignore */
    }
  }

  async function applyEndpoint(): Promise<void> {
    const host = hostInput.trim() || DEFAULT_HOST;
    const port = Number(portInput);
    const portValid = Number.isFinite(port) && port > 0 && port < 65536;
    await chrome.storage.local.set({
      ws_host: host,
      ws_port: portValid ? port : DEFAULT_PORT,
    });
    setWsHost(host);
    setWsPort(portValid ? port : DEFAULT_PORT);
  }

  async function clearLogs(): Promise<void> {
    setLog([]);
    try {
      await chrome.runtime.sendMessage({ type: 'clear-activity-logs' });
    } catch {
      /* background may be respawning — UI already cleared */
    }
  }

  async function reconnectNow(): Promise<void> {
    try {
      await chrome.runtime.sendMessage({ type: 'reconnect-now' });
    } catch {
      /* ignore */
    }
  }

  function toggleLevel(level: LogLevel): void {
    setLevelFilter((prev) => {
      const next = new Set(prev);
      if (next.has(level)) next.delete(level);
      else next.add(level);
      return next;
    });
  }

  const statusColor = STATE_COLOR[connectionState];
  const logoUrl = chrome.runtime.getURL('icon.png');
  const filtered = log.filter((e) => levelFilter.has(e.level));

  // ===== Settings view =====
  if (showSettings) {
    return React.createElement('div', {
      style: {
        display: 'flex', flexDirection: 'column', gap: '16px',
        minHeight: 'calc(100vh - 32px)', padding: '4px',
      },
    },
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: '8px' } },
        React.createElement('button', {
          onClick: () => setShowSettings(false),
          style: {
            background: 'none', border: 'none', color: '#a6adc8', cursor: 'pointer',
            fontSize: '18px', padding: '0 4px',
          },
        }, '←'),
        React.createElement('h2', { style: { fontSize: '16px', fontWeight: 600 } }, 'Settings'),
      ),

      React.createElement('div', { style: { display: 'flex', flexDirection: 'column', gap: '8px' } },
        React.createElement('label', { style: { fontSize: '12px', color: '#a6adc8' } }, 'Daemon Host:'),
        React.createElement('input', {
          type: 'text',
          value: hostInput,
          placeholder: DEFAULT_HOST,
          onChange: (e: React.ChangeEvent<HTMLInputElement>) => setHostInput(e.target.value),
          style: {
            width: '100%', padding: '8px 12px', borderRadius: '4px',
            border: '1px solid #45475a', background: '#313244', color: '#cdd6f4', fontSize: '13px',
          },
        }),
        React.createElement('div', { style: { fontSize: '10px', color: '#6c7086' } },
          'Hostname or IP. Use 127.0.0.1 for a local daemon.',
        ),
      ),

      React.createElement('div', { style: { display: 'flex', flexDirection: 'column', gap: '8px' } },
        React.createElement('label', { style: { fontSize: '12px', color: '#a6adc8' } }, 'WebSocket Port:'),
        React.createElement('input', {
          type: 'number',
          value: portInput,
          min: 1,
          max: 65535,
          placeholder: String(DEFAULT_PORT),
          onChange: (e: React.ChangeEvent<HTMLInputElement>) => setPortInput(e.target.value),
          style: {
            width: '100%', padding: '8px 12px', borderRadius: '4px',
            border: '1px solid #45475a', background: '#313244', color: '#cdd6f4', fontSize: '13px',
          },
        }),
      ),

      React.createElement('div', { style: { display: 'flex', gap: '8px' } },
        React.createElement('button', {
          onClick: applyEndpoint,
          style: {
            flex: 1, padding: '8px 12px', borderRadius: '4px', border: 'none',
            background: '#89b4fa', color: '#1e1e2e', fontSize: '12px', fontWeight: 600, cursor: 'pointer',
          },
        }, 'Save & Reconnect'),
        React.createElement('button', {
          onClick: () => { setHostInput(DEFAULT_HOST); setPortInput(String(DEFAULT_PORT)); },
          style: {
            padding: '8px 12px', borderRadius: '4px', border: '1px solid #45475a',
            background: '#313244', color: '#cdd6f4', fontSize: '12px', cursor: 'pointer',
          },
        }, 'Reset'),
      ),

      React.createElement('div', {
        style: {
          fontSize: '11px', color: '#a6adc8', padding: '8px 10px',
          background: '#313244', borderRadius: '4px',
        },
      },
        React.createElement('div', null, `Active endpoint: ws://${wsHost}:${wsPort}/browser`),
        React.createElement('div', { style: { color: statusColor, marginTop: '4px' } },
          `Status: ${connectionState}${connectionDetail ? ` — ${connectionDetail}` : ''}`,
        ),
      ),

      React.createElement('div', {
        style: {
          fontSize: '10px', color: '#585b70', textAlign: 'center',
          marginTop: 'auto', paddingTop: '8px',
        },
      }, 'SenClaw v0.1.0 — Remote Browser Control'),
    );
  }

  // ===== Main view =====
  const filterChip = (level: LogLevel, label: string) => {
    const active = levelFilter.has(level);
    return React.createElement('button', {
      key: level,
      onClick: () => toggleLevel(level),
      title: `Toggle ${label}`,
      style: {
        fontSize: '10px', padding: '2px 8px', borderRadius: '999px',
        border: `1px solid ${active ? LEVEL_COLOR[level] : '#45475a'}`,
        background: active ? `${LEVEL_COLOR[level]}22` : 'transparent',
        color: active ? LEVEL_COLOR[level] : '#6c7086',
        cursor: 'pointer', fontFamily: 'inherit',
      },
    }, label);
  };

  return React.createElement('div', {
    style: {
      display: 'flex', flexDirection: 'column', gap: '10px',
      minHeight: 'calc(100vh - 32px)', padding: '4px',
    },
  },
    // ===== Header =====
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between' } },
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: '10px' } },
        React.createElement('img', { src: logoUrl, style: { width: '40px', height: '40px', borderRadius: '8px' } }),
        React.createElement('h2', { style: { fontSize: '18px', fontWeight: 700 } }, 'SenClaw'),
      ),
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: '8px' } },
        React.createElement('button', {
          onClick: () => setShowSettings(true),
          style: {
            background: 'none', border: 'none', color: '#a6adc8', cursor: 'pointer',
            fontSize: '14px', padding: '2px', display: 'flex', alignItems: 'center',
          },
          title: 'Settings',
        }, '⚙'),
        React.createElement('div', {
          style: { display: 'flex', alignItems: 'center', gap: '4px' },
          title: connectionDetail || connectionState,
        },
          React.createElement('div', {
            style: {
              width: '8px', height: '8px', borderRadius: '50%',
              backgroundColor: statusColor, display: 'inline-block',
            },
          }),
          React.createElement('span', { style: { fontSize: '11px', color: '#a6adc8' } }, connectionState),
        ),
      ),
    ),

    // ===== Connection bar (endpoint + reconnect) =====
    React.createElement('div', {
      style: {
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        fontSize: '10px', color: '#6c7086', background: '#181825',
        padding: '4px 8px', borderRadius: '4px',
      },
    },
      React.createElement('span', { style: { fontFamily: 'monospace' } },
        endpoint || `ws://${wsHost}:${wsPort}/browser`,
      ),
      React.createElement('button', {
        onClick: reconnectNow,
        style: {
          background: 'none', border: '1px solid #45475a', color: '#a6adc8',
          fontSize: '10px', padding: '2px 8px', borderRadius: '4px',
          cursor: 'pointer',
        },
        title: 'Force reconnect now',
      }, 'Reconnect'),
    ),

    connectionDetail
      ? React.createElement('div', {
          style: {
            fontSize: '10px', color: STATE_COLOR[connectionState],
            background: '#181825', padding: '4px 8px', borderRadius: '4px',
            fontFamily: 'monospace',
          },
        }, connectionDetail)
      : null,

    // ===== Log toolbar =====
    React.createElement('div', {
      style: {
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        borderBottom: '1px solid #45475a', paddingBottom: '6px',
      },
    },
      React.createElement('div', { style: { fontSize: '11px', fontWeight: 600, color: '#a6adc8' } },
        `Activity Log (${filtered.length}/${log.length})`,
      ),
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: '6px' } },
        React.createElement('label', {
          style: { fontSize: '10px', color: '#a6adc8', display: 'flex', alignItems: 'center', gap: '4px' },
          title: 'Auto-scroll to newest entry',
        },
          React.createElement('input', {
            type: 'checkbox',
            checked: autoScroll,
            onChange: (e: React.ChangeEvent<HTMLInputElement>) => setAutoScroll(e.target.checked),
            style: { margin: 0 },
          }),
          'auto',
        ),
        React.createElement('button', {
          onClick: clearLogs,
          style: {
            background: 'none', border: '1px solid #45475a', color: '#a6adc8',
            fontSize: '10px', padding: '2px 8px', borderRadius: '4px', cursor: 'pointer',
          },
          title: 'Clear log',
        }, 'Clear'),
      ),
    ),

    React.createElement('div', {
      style: {
        display: 'flex', gap: '6px', flexWrap: 'wrap',
      },
    },
      filterChip('event', 'event'),
      filterChip('info',  'info'),
      filterChip('warn',  'warn'),
      filterChip('error', 'error'),
    ),

    // ===== Log view =====
    React.createElement('div', { style: { flex: 1, display: 'flex', flexDirection: 'column' } },
      React.createElement('div', {
        ref: (el: HTMLDivElement | null) => {
          if (el && autoScroll) el.scrollTop = el.scrollHeight;
        },
        style: {
          flex: 1, maxHeight: 'calc(100vh - 240px)', overflowY: 'auto', background: '#11111b',
          padding: '6px 8px', borderRadius: '4px', fontSize: '10px',
          fontFamily: 'monospace', lineHeight: 1.5,
        },
      },
        ...(filtered.length > 0
          ? filtered.map((entry, i) =>
              React.createElement('div', {
                key: i,
                style: { color: LEVEL_COLOR[entry.level], whiteSpace: 'pre-wrap' },
              },
                React.createElement('span', { style: { color: '#585b70' } }, `[${formatTime(entry.ts)}] `),
                entry.message,
              ),
            )
          : [React.createElement('div', { key: 'empty', style: { color: '#6c7086' } }, 'Waiting for agent activity…')]
        ),
      ),
    ),

    // ===== Footer =====
    React.createElement('div', {
      style: {
        fontSize: '10px', color: '#585b70', textAlign: 'center',
        marginTop: 'auto', paddingTop: '8px',
      },
    }, 'SenClaw v0.1.0 — Remote Browser Control'),
  );
}
