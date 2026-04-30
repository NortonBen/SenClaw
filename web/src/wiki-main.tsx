import { StrictMode, Suspense, useState, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter } from 'react-router-dom';
import { ConfigProvider, Layout, theme } from 'antd';
import './index.css';
import WikiView from './wiki/WikiView';
import { WikiSidebar } from './wiki/WikiSidebar';
import { Sidebar } from './components/Sidebar';
import { useWiki } from './hooks/useWiki';

function WikiApp() {
  const [isDark, setIsDark] = useState(() => localStorage.getItem('theme') === 'dark');

  useEffect(() => {
    document.documentElement.classList.toggle('dark', isDark);
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
        <WikiWrapper isDark={isDark} toggleTheme={() => setIsDark(v => !v)} />
      </BrowserRouter>
    </ConfigProvider>
  );
}

function WikiWrapper({ isDark, toggleTheme }: { isDark: boolean; toggleTheme: () => void }) {
  const wiki = useWiki();
  const [innerView, setInnerView] = useState<'home' | 'doc' | 'stats' | 'categories'>('home');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const { token } = theme.useToken();

  return (
    <Layout className="h-screen overflow-hidden" style={{ background: token.colorBgBase }}>
      <Sidebar
        status="disconnected"
        isDarkMode={isDark}
        toggleTheme={toggleTheme}
        sidebarContent={
          <WikiSidebar
            tree={wiki.tree}
            treeLoading={wiki.treeLoading}
            searchResults={wiki.searchResults}
            searching={wiki.searching}
            selectedPath={selectedPath}
            activeView={innerView}
            onSelectDoc={(path) => { setSelectedPath(path); setInnerView('doc'); }}
            onSearch={wiki.search}
            onClearSearch={wiki.clearSearch}
            onShowStats={() => setInnerView('stats')}
            onShowCategories={() => setInnerView('categories')}
            onShowHome={() => { setInnerView('home'); setSelectedPath(null); }}
          />
        }
      />
      <Layout className="bg-transparent relative">
        <WikiView
          wiki={wiki}
          innerView={innerView}
          setInnerView={setInnerView}
          selectedPath={selectedPath}
          setSelectedPath={setSelectedPath}
        />
      </Layout>
    </Layout>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Suspense fallback={<div className="flex h-screen items-center justify-center text-sm text-gray-400 bg-white">Loading Wiki...</div>}>
      <WikiApp />
    </Suspense>
  </StrictMode>
);
