import { StrictMode, useState, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { ConfigProvider, Layout, theme } from 'antd';
import './index.css';
import PluginsView from './plugins/PluginsView';
import { PluginsSidebar, PluginsNavItem } from './plugins/PluginsSidebar';
import { Sidebar } from './components/Sidebar';

function PluginsApp() {
  const [activeNav, setActiveNav] = useState<PluginsNavItem>('skills');
  const [isDark, setIsDark] = useState(() => localStorage.getItem('theme') === 'dark');

  useEffect(() => {
    const html = document.documentElement;
    html.classList.toggle('dark', isDark);
    localStorage.setItem('theme', isDark ? 'dark' : 'light');
  }, [isDark]);

  return (
    <ConfigProvider
      theme={{
        algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: {
          colorPrimary: '#5BBFE8',
          colorBgBase: isDark ? '#0D0D1F' : '#F0F2F5',
          colorBgContainer: isDark ? 'rgba(255, 255, 255, 0.04)' : '#FFFFFF',
          colorBorder: isDark ? 'rgba(255, 255, 255, 0.05)' : 'rgba(0, 0, 0, 0.05)',
        },
      }}
    >
      <BrowserRouter>
        <PluginsInner
          activeNav={activeNav}
          setActiveNav={setActiveNav}
          isDark={isDark}
          toggleTheme={() => setIsDark(v => !v)}
        />
      </BrowserRouter>
    </ConfigProvider>
  );
}

function PluginsInner({
  activeNav,
  setActiveNav,
  isDark,
  toggleTheme,
}: {
  activeNav: PluginsNavItem;
  setActiveNav: (v: PluginsNavItem) => void;
  isDark: boolean;
  toggleTheme: () => void;
}) {
  const { token } = theme.useToken();
  return (
    <Layout className="h-screen overflow-hidden" style={{ background: token.colorBgBase }}>
      <Sidebar
        status="disconnected"
        isDarkMode={isDark}
        toggleTheme={toggleTheme}
        sidebarContent={
          <PluginsSidebar activeNav={activeNav} onSelect={setActiveNav} />
        }
      />
      <Layout className="bg-transparent relative">
        <PluginsView activeNav={activeNav} />
      </Layout>
    </Layout>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PluginsApp />
  </StrictMode>
);
