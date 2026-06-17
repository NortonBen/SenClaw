import { useState, useEffect, useCallback } from 'react';
import { Layout, theme } from 'antd';
import { Sidebar } from './Sidebar';
import { AgentConsole } from './AgentConsole';
import { Workbench } from './Workbench';
import { DockBadges } from './DockBadges';
import { useAppContext } from '../contexts/AppContext';
import type { WsStatus } from '../types';

interface Props {
  sidebar: React.ReactNode;
  children: React.ReactNode;
  status?: WsStatus;
}

type ExpandedDock = 'agent' | 'workbench' | null;

export function AppLayout({ sidebar, children, status }: Props) {
  const { ws, isDarkMode, toggleTheme, embed } = useAppContext();
  const { token } = theme.useToken();

  // ===== Right-dock: mutually-exclusive AgentConsole / Workbench =====
  const [expandedDock, setExpandedDock] = useState<ExpandedDock>(() =>
    ws.dispatchParents.some(p => p.status === 'active' || p.status === 'queued') ? 'agent' : null,
  );
  // After the user manually collapses, suppress this latest's auto-foreground.
  const [suppressedLatestAt, setSuppressedLatestAt] = useState<number | null>(null);

  const activeJid = ws.activeJid;
  const workbenchState = activeJid ? (ws.workbench[activeJid] ?? null) : null;

  // A new artifact for the active chat grabs the foreground (unless suppressed).
  useEffect(() => {
    const latest = ws.workbenchLatest;
    if (!latest) return;
    if (suppressedLatestAt === latest.at) return;
    if (latest.jid !== activeJid) return;
    setExpandedDock('workbench');
  }, [ws.workbenchLatest, suppressedLatestAt, activeJid]);

  // Workbench callbacks pinned to the active chat jid.
  const { workbenchReadFile, workbenchClose, workbenchMarkViewed, workbenchSetCurrent, workbenchLatest } = ws;
  const wbReadFile = useCallback((artifactId: string, path: string) => {
    if (!activeJid) return Promise.resolve({ error: 'no_jid' });
    return workbenchReadFile(activeJid, artifactId, path);
  }, [activeJid, workbenchReadFile]);
  const wbClose = useCallback((artifactId: string) => {
    if (activeJid) workbenchClose(activeJid, artifactId);
  }, [activeJid, workbenchClose]);
  const wbMarkViewed = useCallback((artifactId: string) => {
    if (activeJid) workbenchMarkViewed(activeJid, artifactId);
  }, [activeJid, workbenchMarkViewed]);
  const wbSelect = useCallback((artifactId: string) => {
    if (activeJid) workbenchSetCurrent(activeJid, artifactId);
  }, [activeJid, workbenchSetCurrent]);

  const collapseWorkbench = useCallback(() => {
    setExpandedDock(d => (d === 'workbench' ? null : d));
    if (workbenchLatest) setSuppressedLatestAt(workbenchLatest.at);
  }, [workbenchLatest]);

  // Badge toggle: click active → collapse; click inactive → switch.
  const onToggleDock = useCallback((which: 'agent' | 'workbench') => {
    setExpandedDock(prev => {
      if (prev === which) {
        if (which === 'workbench' && workbenchLatest) setSuppressedLatestAt(workbenchLatest.at);
        return null;
      }
      return which;
    });
  }, [workbenchLatest]);

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
        dispatchActivity={ws.dispatchActivity}
        agentTodos={ws.agentTodos}
        messages={ws.messages}
        groups={ws.groups}
        agentStates={ws.agentStates}
        resolvePermission={ws.resolvePermission}
        expanded={expandedDock === 'agent'}
        onExpand={() => setExpandedDock('agent')}
        onCollapse={() => setExpandedDock(d => (d === 'agent' ? null : d))}
      />
      <Workbench
        state={workbenchState}
        expanded={expandedDock === 'workbench'}
        onCollapse={collapseWorkbench}
        readFile={wbReadFile}
        closeArtifact={wbClose}
        selectArtifact={wbSelect}
        markViewed={wbMarkViewed}
      />
      <DockBadges
        expanded={expandedDock}
        onToggle={onToggleDock}
        dispatchParents={ws.dispatchParents}
        dispatchActivity={ws.dispatchActivity}
        agentTodos={ws.agentTodos}
        messages={ws.messages}
        groups={ws.groups}
        workbenchState={workbenchState}
      />
    </Layout>
  );
}
