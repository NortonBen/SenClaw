import { Layout, theme } from 'antd';
import { Sidebar } from './Sidebar';
import { AgentConsole } from './AgentConsole';
import { useAppContext } from '../contexts/AppContext';
import type { WsStatus } from '../types';

interface Props {
  sidebar: React.ReactNode;
  children: React.ReactNode;
  status?: WsStatus;
}

export function AppLayout({ sidebar, children, status }: Props) {
  const { ws, isDarkMode, toggleTheme, embed } = useAppContext();
  const { token } = theme.useToken();

  // Compact chat window (desktop app menu-bar): just the chat, no nav rail / console.
  if (embed) {
    return (
      <Layout className="h-screen overflow-hidden" style={{ background: token.colorBgBase }}>
        <Layout className="bg-transparent relative">{children}</Layout>
      </Layout>
    );
  }

  return (
    <Layout className="h-screen overflow-hidden" style={{ background: token.colorBgBase }}>
      <Sidebar
        status={status ?? ws.status}
        sidebarContent={sidebar}
        isDarkMode={isDarkMode}
        toggleTheme={toggleTheme}
        notifications={ws.notifications}
        onMarkRead={ws.markNotificationRead}
        onClearAll={ws.clearAllNotifications}
      />
      <Layout className="bg-transparent relative">
        {children}
      </Layout>
      <AgentConsole
        dispatchParents={ws.dispatchParents}
        agentTodos={ws.agentTodos}
        messages={ws.messages}
        groups={ws.groups}
        agentStates={ws.agentStates}
        resolvePermission={ws.resolvePermission}
      />
    </Layout>
  );
}
