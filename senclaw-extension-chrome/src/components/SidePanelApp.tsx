import React, { useEffect, useState } from 'react';

interface TabInfo {
  id: string;
  url: string;
  title: string;
  status: string;
}

type ConnectionState = 'disconnected' | 'connecting' | 'connected';

export function SidePanelApp(): React.ReactElement {
  const [connectionState, setConnectionState] = useState<ConnectionState>('disconnected');
  const [tabs, setTabs] = useState<TabInfo[]>([]);
  const [wsPort, setWsPort] = useState(18789);
  const [log, setLog] = useState<string[]>([]);
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    chrome.storage.local.get('ws_port').then((result) => {
      if (result.ws_port) setWsPort(result.ws_port);
    });

    const tabListener = (_tabId: number, changeInfo: chrome.tabs.TabChangeInfo) => {
      if (changeInfo.status || changeInfo.url || changeInfo.title) {
        refreshTabs();
      }
    };
    chrome.tabs.onUpdated.addListener(tabListener);
    chrome.tabs.onCreated.addListener(refreshTabs);
    chrome.tabs.onRemoved.addListener(refreshTabs);

    refreshTabs();

    const interval = setInterval(checkConnection, 3000);
    checkConnection();

    // Load initial logs
    chrome.runtime.sendMessage({ type: 'get-activity-logs' }).then((response) => {
      if (response?.logs) setLog(response.logs);
    });

    // Listen for new logs
    const logListener = (message: any) => {
      if (message.type === 'activity-log') {
        setLog((prev) => [...prev.slice(-49), message.entry]);
      }
    };
    chrome.runtime.onMessage.addListener(logListener);

    return () => {
      chrome.tabs.onUpdated.removeListener(tabListener);
      chrome.tabs.onCreated.removeListener(refreshTabs);
      chrome.tabs.onRemoved.removeListener(refreshTabs);
      chrome.runtime.onMessage.removeListener(logListener);
      clearInterval(interval);
    };
  }, [wsPort]);

  async function refreshTabs(): Promise<void> {
    const allTabs = await chrome.tabs.query({});
    setTabs(
      allTabs.map((t) => ({
        id: String(t.id ?? ''),
        url: t.url ?? '',
        title: t.title ?? '',
        status: t.status ?? 'loading',
      })),
    );
  }

  async function checkConnection(): Promise<void> {
    try {
      const response = await chrome.runtime.sendMessage({ type: 'get-connection-status' });
      setConnectionState(response?.connected ? 'connected' : 'disconnected');
    } catch {
      setConnectionState('disconnected');
    }
  }

  async function savePort(port: number): Promise<void> {
    setWsPort(port);
    chrome.storage.local.set({ ws_port: port });
  };

  const statusColor = connectionState === 'connected' ? '#a6e3a1'
    : connectionState === 'connecting' ? '#f9e2af'
    : '#f38ba8';

  const logoUrl = chrome.runtime.getURL('icon.png');

  if (showSettings) {
    return React.createElement('div', {
      style: {
        display: 'flex', flexDirection: 'column', gap: '16px',
        minHeight: 'calc(100vh - 32px)',
      }
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
        React.createElement('label', { style: { fontSize: '12px', color: '#a6adc8' } }, 'WebSocket Port:'),
        React.createElement('input', {
          type: 'number',
          value: wsPort,
          onChange: (e: React.ChangeEvent<HTMLInputElement>) => savePort(parseInt(e.target.value) || 18789),
          style: {
            width: '100%', padding: '8px 12px', borderRadius: '4px',
            border: '1px solid #45475a', background: '#313244', color: '#cdd6f4', fontSize: '13px',
          },
        }),
      ),

      React.createElement('div', {
        style: {
          fontSize: '10px', color: '#585b70', textAlign: 'center',
          marginTop: 'auto', paddingTop: '8px'
        }
      },
        'SenClaw v0.1.0 — Remote Browser Control',
      ),
    );
  }

  return React.createElement('div', {
    style: {
      display: 'flex', flexDirection: 'column', gap: '12px',
      minHeight: 'calc(100vh - 32px)',
    }
  },
    // Header
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
        }, '⚙️'),
        React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: '4px' } },
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

    // Activity Log
    React.createElement('div', {
      style: {
        fontSize: '11px', fontWeight: 600, color: '#a6adc8',
        borderBottom: '1px solid #45475a', paddingBottom: '6px',
      },
    }, 'Activity Log'),
    React.createElement('div', { style: { flex: 1, display: 'flex', flexDirection: 'column' } },
      React.createElement('div', {
        style: {
          flex: 1, maxHeight: 'calc(100vh - 160px)', overflowY: 'auto', background: '#11111b',
          padding: '6px 8px', borderRadius: '4px', fontSize: '10px',
          fontFamily: 'monospace',
        },
      },
        ...(log.length > 0
          ? log.map((entry, i) => React.createElement('div', { key: i }, entry))
          : [React.createElement('div', { key: 'empty', style: { color: '#6c7086' } }, 'Waiting for agent activity...')]
        ),
      ),
    ),

    // Footer
    React.createElement('div', {
      style: {
        fontSize: '10px', color: '#585b70', textAlign: 'center',
        marginTop: 'auto', paddingTop: '8px'
      }
    },
      'SenClaw v0.1.0 — Remote Browser Control',
    ),
  );
}
