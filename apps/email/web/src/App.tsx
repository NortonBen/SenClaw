import { useEffect, useState } from 'react';
import { App as AntApp, ConfigProvider, Layout, Segmented, Typography, theme } from 'antd';
import { InboxOutlined, MailOutlined, SettingOutlined } from '@ant-design/icons';
import { InboxView } from './components/InboxView';
import { AccountsView } from './components/AccountsView';

const { Header, Content } = Layout;
const { Title } = Typography;

type Tab = 'inbox' | 'accounts';
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
  const [tab, setTab] = useState<Tab>('inbox');

  // Keep the page background in sync with the theme (avoids a flash behind the Layout).
  useEffect(() => {
    document.body.style.background = token.colorBgLayout;
  }, [token.colorBgLayout]);

  return (
    <Layout style={{ height: '100vh', background: token.colorBgLayout }}>
      <Header
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 16,
          background: token.colorBgContainer,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          paddingInline: 20,
          height: 58,
          lineHeight: '58px',
        }}
      >
        <Title level={5} style={{ margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
          <MailOutlined style={{ color: token.colorPrimary }} /> Email
        </Title>
        <Segmented<Tab>
          value={tab}
          onChange={setTab}
          options={[
            { label: 'Hộp thư', value: 'inbox', icon: <InboxOutlined /> },
            { label: 'Tài khoản', value: 'accounts', icon: <SettingOutlined /> },
          ]}
        />
      </Header>
      <Content style={{ minHeight: 0, padding: 24, background: token.colorBgLayout }}>
        <div
          style={{
            height: '100%',
            minHeight: 0,
            overflow: 'auto',
            background: token.colorBgContainer,
            border: `1px solid ${token.colorBorderSecondary}`,
            borderRadius: token.borderRadiusLG,
            boxShadow: token.boxShadowTertiary,
          }}
        >
          {tab === 'inbox' ? <InboxView onConfigure={() => setTab('accounts')} /> : <AccountsView />}
        </div>
      </Content>
    </Layout>
  );
}
