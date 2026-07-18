import { useEffect, useState } from 'react';
import { App as AntApp, ConfigProvider, theme } from 'antd';
import { MailboxView } from './components/MailboxView';

type Mode = 'dark' | 'light';

/** Resolve an initial theme before any host message arrives. */
function detectInitialMode(): Mode {
  // The app is served same-origin via senclaw's proxy, so it shares senclaw's
  // localStorage 'theme' key when embedded.
  try {
    const saved = localStorage.getItem('theme');
    if (saved === 'dark' || saved === 'light') return saved;
  } catch { /* ignore */ }
  if (typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches) {
    return 'dark';
  }
  return 'light';
}

export default function App() {
  const [mode, setMode] = useState<Mode>(detectInitialMode);

  // Follow senclaw's theme: listen for the host's init/theme postMessages.
  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const d = e.data;
      if (!d || typeof d !== 'object') return;
      const t = d.theme ?? d.env?.theme;
      if ((d.type === 'senclaw:init' || d.type === 'senclaw:theme') && (t === 'dark' || t === 'light')) {
        setMode(t);
      }
    };
    window.addEventListener('message', onMessage);
    // Tell the host we're ready so it sends senclaw:init (with the current theme).
    try {
      window.parent?.postMessage({ type: 'senclaw:ready' }, '*');
    } catch { /* ignore */ }
    return () => window.removeEventListener('message', onMessage);
  }, []);

  const isDark = mode === 'dark';

  return (
    <ConfigProvider
      theme={{
        algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: { colorPrimary: '#2563eb', borderRadius: 8 },
        components: {
          // The list rows carry their own selected state; keep AntD from
          // double-painting a background behind them.
          Segmented: { itemSelectedBg: 'transparent' },
        },
      }}
    >
      <AntApp>
        <Shell />
      </AntApp>
    </ConfigProvider>
  );
}

function Shell() {
  const { token } = theme.useToken();

  // Bridge the tokens that only CSS can use — :hover on the hand-rolled nav
  // rows, and the scrollbar colours — and keep the page background in sync so
  // there's no flash behind the panes on load or theme switch.
  useEffect(() => {
    const root = document.documentElement;
    document.body.style.background = token.colorBgContainer;
    root.style.setProperty('--email-hover', token.colorFillTertiary);
    root.style.setProperty('--email-scrollbar', token.colorFill);
    root.style.setProperty('--email-scrollbar-hover', token.colorFillSecondary);
  }, [token]);

  return <MailboxView />;
}
