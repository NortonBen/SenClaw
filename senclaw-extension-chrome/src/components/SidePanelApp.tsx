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

    return () => {
      chrome.tabs.onUpdated.removeListener(tabListener);
      chrome.tabs.onCreated.removeListener(refreshTabs);
      chrome.tabs.onRemoved.removeListener(refreshTabs);
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
    await chrome.storage.local.set({ ws_port: port });
    addLog(`Port changed to ${port}. Reconnecting...`);
  }

  function addLog(message: string): void {
    setLog((prev) => [...prev.slice(-50), `[${new Date().toLocaleTimeString()}] ${message}`]);
  }

  const statusColor =
    connectionState === 'connected' ? '#a6e3a1'
    : connectionState === 'connecting' ? '#f9e2af'
    : '#f38ba8';

  return React.createElement('div', { style: { display: 'flex', flexDirection: 'column', gap: '12px' } },
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between' } },
      React.createElement('h2', { style: { fontSize: '16px', fontWeight: 600 } }, 'SenClaw Extension'),
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: '6px' } },
        React.createElement('div', {
          style: {
            width: '8px', height: '8px', borderRadius: '50%',
            backgroundColor: statusColor, display: 'inline-block',
          },
        }),
        React.createElement('span', { style: { fontSize: '11px', color: '#a6adc8' } }, connectionState),
      ),
    ),

    React.createElement('div', { style: { display: 'flex', gap: '8px', alignItems: 'center' } },
      React.createElement('label', { style: { fontSize: '12px', color: '#a6adc8' } }, 'WS Port:'),
      React.createElement('input', {
        type: 'number',
        value: wsPort,
        onChange: (e: React.ChangeEvent<HTMLInputElement>) => savePort(parseInt(e.target.value) || 18789),
        style: {
          width: '70px', padding: '4px 8px', borderRadius: '4px',
          border: '1px solid #45475a', background: '#313244', color: '#cdd6f4', fontSize: '12px',
        },
      }),
    ),

    React.createElement('div', {},
      React.createElement('h3', { style: { fontSize: '13px', marginBottom: '6px', color: '#a6adc8' } }, `Open Tabs (${tabs.length})`),
      React.createElement('div', { style: { maxHeight: '300px', overflowY: 'auto', display: 'flex', flexDirection: 'column', gap: '4px' } },
        ...tabs.map((tab) =>
          React.createElement('div', {
            key: tab.id,
            style: {
              padding: '6px 8px', borderRadius: '4px', background: '#313244',
              fontSize: '11px', cursor: 'pointer',
            },
            onClick: () => chrome.tabs.update(parseInt(tab.id), { active: true }),
          },
            React.createElement('div', { style: { fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, tab.title || tab.url),
            React.createElement('div', { style: { color: '#6c7086', fontSize: '10px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, tab.url),
            React.createElement('div', { style: { color: tab.status === 'complete' ? '#a6e3a1' : '#f9e2af', fontSize: '10px' } }, tab.status),
          ),
        ),
      ),
    ),

    React.createElement('div', {},
      React.createElement('h3', { style: { fontSize: '13px', marginBottom: '6px', color: '#a6adc8' } }, 'Activity Log'),
      React.createElement('div', {
        style: {
          maxHeight: '150px', overflowY: 'auto', background: '#11111b',
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

    React.createElement('div', { style: { fontSize: '10px', color: '#585b70', textAlign: 'center', marginTop: '8px' } },
      'SenClaw v0.1.0 — Remote Browser Control',
    ),
  );
}
