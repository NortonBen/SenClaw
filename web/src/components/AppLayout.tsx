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
  const { ws, isDarkMode, toggleTheme } = useAppContext();
  const { token } = theme.useToken();

  return (
    <Layout className="h-screen overflow-hidden" style={{ background: token.colorBgBase }}>
      <Sidebar
        status={status ?? ws.status}
        sidebarContent={sidebar}
        isDarkMode={isDarkMode}
        toggleTheme={toggleTheme}
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
