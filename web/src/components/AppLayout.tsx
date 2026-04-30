import { Outlet } from 'react-router-dom';
import { Layout } from 'antd';
import { Sidebar } from './Sidebar';
import type { WsHook } from '../hooks/useWebSocket';
import { theme } from 'antd';

interface Props {
  ws: WsHook;
  isDarkMode: boolean;
  toggleTheme: () => void;
  sidebarContent: React.ReactNode;
}

export function AppLayout({ ws, isDarkMode, toggleTheme, sidebarContent }: Props) {
  const { token } = theme.useToken();

  return (
    <Layout className="h-screen overflow-hidden" style={{ background: token.colorBgBase }}>
      <Sidebar
        status={ws.status}
        sidebarContent={sidebarContent}
        isDarkMode={isDarkMode}
        toggleTheme={toggleTheme}
      />
      <Layout className="bg-transparent relative">
        <Outlet />
      </Layout>
    </Layout>
  );
}
