import { useState, useEffect, useMemo } from 'react';
import { Routes, Route, useNavigate, useLocation } from 'react-router-dom';
import { useWebSocket } from './hooks/useWebSocket';
import { useWiki } from './hooks/useWiki';
import { AppLayout } from './components/AppLayout';
import { ChatPage } from './pages/ChatPage';
import { SettingsPage } from './pages/SettingsPage';
import WikiView from './wiki/WikiView';
import PluginsView from './plugins/PluginsView';
import { CoworkPage } from './pages/CoworkPage';
import { CodePage } from './pages/CodePage';
import { ConfigProvider, theme } from 'antd';
import { AgentSidebar } from './components/AgentSidebar';
import { WikiSidebar } from './wiki/WikiSidebar';
import { PluginsSidebar, PluginsNavItem } from './plugins/PluginsSidebar';

export function App() {
  const [selectedJid, setSelectedJid] = useState<string | null>(null);
  const [isDarkMode, setIsDarkMode] = useState(() => {
    const saved = localStorage.getItem('theme');
    return saved ? saved === 'dark' : true;
  });
  
  // Wiki state
  const [wikiPath, setWikiPath] = useState<string | null>(null);
  const [wikiView, setWikiView] = useState<'home' | 'doc' | 'stats' | 'categories'>('home');
  const [pluginsView, setPluginsView] = useState<PluginsNavItem>('skills');

  const ws = useWebSocket();
  const wiki = useWiki();
  const navigate = useNavigate();
  const location = useLocation();

  const { dispatchParents, subscribeAll } = ws;

  // When dispatch is active, subscribe to all agents to receive their permission/todo events
  useEffect(() => {
    const hasActive = dispatchParents.some(p => p.status === 'active' || p.status === 'queued');
    if (hasActive && ws.groups.length > 0) subscribeAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dispatchParents, ws.groups.length]);

  // Auto-select admin (main) group on first load; fall back to first group if missing
  useEffect(() => {
    if (!selectedJid && ws.groups.length > 0) {
      const admin = ws.groups.find(g => g.isAdmin);
      const jid = (admin ?? ws.groups[0]).jid;
      setSelectedJid(jid);
      if (!ws.subscribed.has(jid)) ws.subscribe(jid);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ws.groups.length]);

  const handleSelect = (jid: string) => {
    setSelectedJid(jid);
    if (!ws.subscribed.has(jid)) ws.subscribe(jid);
    if (location.pathname !== '/chats') {
      navigate('/chats');
    }
  };

  useEffect(() => {
    localStorage.setItem('theme', isDarkMode ? 'dark' : 'light');
    document.documentElement.classList.toggle('dark', isDarkMode);
  }, [isDarkMode]);

  const toggleTheme = () => setIsDarkMode(prev => !prev);

  // Determine sidebar content based on route
  const sidebarContent = useMemo(() => {
    if (location.pathname.startsWith('/wiki')) {
      return (
        <WikiSidebar
          tree={wiki.tree}
          treeLoading={wiki.treeLoading}
          searchResults={wiki.searchResults}
          searching={wiki.searching}
          selectedPath={wikiPath}
          activeView={wikiView}
          onSelectDoc={(path) => { setWikiPath(path); setWikiView('doc'); }}
          onSearch={wiki.search}
          onClearSearch={wiki.clearSearch}
          onShowStats={() => setWikiView('stats')}
          onShowCategories={() => setWikiView('categories')}
          onShowHome={() => { setWikiView('home'); setWikiPath(null); }}
        />
      );
    }

    if (location.pathname.startsWith('/plugins')) {
      return (
        <PluginsSidebar
          activeNav={pluginsView}
          onSelect={setPluginsView}
        />
      );
    }

    return (
      <AgentSidebar 
        ws={ws} 
        selectedJid={selectedJid} 
        onSelect={handleSelect} 
      />
    );
  }, [location.pathname, ws, wiki, selectedJid, wikiPath, wikiView]);

  return (
    <ConfigProvider
      theme={{
        algorithm: isDarkMode ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: {
          colorPrimary: '#5BBFE8',
          colorBgBase: isDarkMode ? '#0D0D1F' : '#F0F2F5',
          colorBgContainer: isDarkMode ? 'rgba(255, 255, 255, 0.04)' : '#FFFFFF',
          colorBorder: isDarkMode ? 'rgba(255, 255, 255, 0.05)' : 'rgba(0, 0, 0, 0.05)',
        },
      }}
    >
      <Routes>
        <Route element={
          <AppLayout 
            ws={ws} 
            isDarkMode={isDarkMode} 
            toggleTheme={toggleTheme} 
            sidebarContent={sidebarContent}
          />
        }>
          <Route path="chats" element={<ChatPage ws={ws} selectedJid={selectedJid} />} />
          <Route index element={<ChatPage ws={ws} selectedJid={selectedJid} />} />
          <Route path="wiki" element={
            <WikiView 
              wiki={wiki} 
              innerView={wikiView} 
              setInnerView={setWikiView}
              selectedPath={wikiPath}
              setSelectedPath={setWikiPath}
            />
          } />
          <Route path="plugins" element={<PluginsView activeNav={pluginsView} />} />
          <Route path="settings" element={<SettingsPage ws={ws} />} />
          <Route path="cowork" element={<CoworkPage />} />
          <Route path="code" element={<CodePage />} />
        </Route>
      </Routes>
    </ConfigProvider>
  );
}
