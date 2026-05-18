import { useState, useEffect } from 'react';
import { Routes, Route } from 'react-router-dom';
import { ConfigProvider, theme } from 'antd';
import { useWebSocket } from './hooks/useWebSocket';
import { AppContext } from './contexts/AppContext';
import { PlanExitDialog } from './components/PlanExitDialog';
import { ChatPage } from './pages/ChatPage';
import { WikiPage } from './pages/WikiPage';
import { PluginsPage } from './pages/PluginsPage';
import { SettingsPage } from './pages/SettingsPage';
import { CoworkPage } from './pages/CoworkPage';
import { CodePage } from './pages/CodePage';
import { SpacePage } from './pages/SpacePage';

export function App() {
  const [isDarkMode, setIsDarkMode] = useState(() => {
    const saved = localStorage.getItem('theme');
    return saved ? saved === 'dark' : true;
  });

  const ws = useWebSocket();

  useEffect(() => {
    localStorage.setItem('theme', isDarkMode ? 'dark' : 'light');
    document.documentElement.classList.toggle('dark', isDarkMode);
  }, [isDarkMode]);

  const toggleTheme = () => setIsDarkMode(prev => !prev);

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
      <AppContext.Provider value={{ ws, isDarkMode, toggleTheme }}>
        <Routes>
          <Route index element={<ChatPage />} />
          <Route path="chats" element={<ChatPage />} />
          <Route path="wiki/*" element={<WikiPage />} />
          <Route path="plugins" element={<PluginsPage />} />
          <Route path="settings" element={<SettingsPage />} />
          <Route path="cowork" element={<CoworkPage />} />
          <Route path="code" element={<CodePage />} />
          <Route path="space/*" element={<SpacePage />} />
        </Routes>
        {/* Global Plan-mode approval modal — surfaces when any agent calls ExitPlanMode. */}
        <PlanExitDialog
          request={ws.planExitRequest}
          onResolve={ws.resolvePlanExit}
          onDismiss={ws.dismissPlanExit}
        />
      </AppContext.Provider>
    </ConfigProvider>
  );
}
