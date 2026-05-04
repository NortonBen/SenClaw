import { useState, useMemo, useEffect, useCallback, useRef } from 'react';
import { theme, Typography, Button, Badge, Space, Card, Tag } from 'antd';
import {
  SettingOutlined,
  CloseOutlined,
  LeftOutlined,
} from '@ant-design/icons';
import type { AgentState, DispatchParent, DispatchTask, AgentTodosEntry, PermissionMessage, GroupInfo, ChatMessage } from '../types';
import { DispatchTree } from './DispatchTree';
import { AgentTodoPanel } from './AgentTodoPanel';

const { Text } = Typography;

const COLLAPSED_W = 56;
const DEFAULT_W = 320;
const MIN_W = 240;
const MAX_W = 600;

const glassStyle = (token: any) => ({
  background: token.colorBgElevated,
  backdropFilter: 'blur(16px)',
  WebkitBackdropFilter: 'blur(16px)',
  borderLeft: `1px solid ${token.colorBorderSecondary}`,
  boxShadow: '-4px 0 15px rgba(0, 0, 0, 0.05)',
});
const WIDTH_KEY = 'agent-console-width';

function loadWidth(): number {
  try {
    const v = localStorage.getItem(WIDTH_KEY);
    if (v) {
      const n = parseInt(v, 10);
      if (n >= MIN_W && n <= MAX_W) return n;
    }
  } catch {}
  return DEFAULT_W;
}

function saveWidth(w: number) {
  try { localStorage.setItem(WIDTH_KEY, String(w)); } catch {}
}

interface AgentConsoleProps {
  dispatchParents: DispatchParent[];
  agentTodos: Record<string, AgentTodosEntry>;
  messages: Record<string, ChatMessage[]>;
  groups: GroupInfo[];
  agentStates: Record<string, AgentState>;
  resolvePermission: (requestId: string, optionKey: string) => void;
}

export function AgentConsole({ dispatchParents, agentTodos, messages, groups, agentStates, resolvePermission }: AgentConsoleProps) {
  const { token } = theme.useToken();
  const activeParents = dispatchParents.filter(p => p.status === 'active');
  const queuedParents = dispatchParents.filter(p => p.status === 'queued');
  const hasActivity = activeParents.length > 0 || queuedParents.length > 0;

  // Derive main admin agent state from active/queued parents
  const adminFolder = activeParents[0]?.adminFolder ?? queuedParents[0]?.adminFolder ?? null;
  const adminJid = adminFolder ? (groups.find(g => g.folder === adminFolder)?.jid ?? null) : null;
  const adminState: AgentState = adminJid ? (agentStates[adminJid] ?? 'idle') : 'idle';
  const adminPaused = adminState === 'paused';

  // Start expanded if dispatch is already active on mount; otherwise collapsed
  const [collapsed, setCollapsed] = useState(() => !dispatchParents.some(
    p => p.status === 'active' || p.status === 'queued'
  ));
  const [selectedTask, setSelectedTask] = useState<DispatchTask | null>(null);
  const [width, setWidth] = useState(loadWidth);
  const widthRef = useRef(width);
  widthRef.current = width;
  const dragging = useRef(false);
  const dragStartX = useRef(0);
  const dragStartW = useRef(0);

  // Pending permissions from ALL agents (scan all message lists)
  const pendingPermissions = useMemo(() => {
    const result: Array<PermissionMessage & { agentJid: string; agentName: string }> = [];
    for (const [jid, msgs] of Object.entries(messages)) {
      const agentName = groups.find(g => g.jid === jid)?.name ?? jid;
      for (const msg of msgs) {
        if (msg.role === 'permission' && !msg.resolved) {
          result.push({ ...(msg as PermissionMessage), agentJid: jid, agentName });
        }
      }
    }
    return result;
  }, [messages, groups]);

  // Auto-expand when dispatch becomes active
  useEffect(() => {
    if (hasActivity) setCollapsed(false);
  }, [hasActivity]);

  // Resize: mouse events (refs avoid re-registering listeners)
  const onMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    dragStartX.current = e.clientX;
    dragStartW.current = widthRef.current;
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }, []);

  useEffect(() => {
    const onMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const delta = dragStartX.current - e.clientX; // Inverted: moving mouse left increases width
      const next = Math.min(MAX_W, Math.max(MIN_W, dragStartW.current + delta));
      widthRef.current = next;
      setWidth(next);
    };
    const onMouseUp = () => {
      if (dragging.current) {
        dragging.current = false;
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
        saveWidth(widthRef.current);
      }
    };
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
    return () => {
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
    };
  }, []);

  const hasTodos = Object.keys(agentTodos).length > 0;

  if (collapsed) {
    const totalPending = pendingPermissions.length;
    return (
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          gap: '12px',
          width: COLLAPSED_W,
          flexShrink: 0,
          borderLeft: `1px solid ${token.colorBorderSecondary}`,
          padding: '16px 0',
          background: token.colorBgContainer,
          zIndex: 20
        }}
      >
        <TooltipIcon
          title="Open Agent Console"
          onClick={() => setCollapsed(false)}
          icon={<SettingOutlined style={{ fontSize: '18px' }} />}
          badge={totalPending}
          active={hasActivity}
          hasContent={hasTodos}
          token={token}
        />
      </div>
    );
  }

  return (
    <div
      className="agent-console-expanded"
      style={{
        display: 'flex',
        flexDirection: 'column',
        width,
        flexShrink: 0,
        ...glassStyle(token),
        overflow: 'hidden',
        position: 'relative',
        zIndex: 20,
        transition: dragging.current ? 'none' : 'width 0.3s cubic-bezier(0.4, 0, 0.2, 1)'
      }}
    >
      {/* Header */}
      <div style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '12px 16px',
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
        background: token.colorBgContainer,
        flexShrink: 0
      }}>
        <Space size={8}>
          <Text strong style={{ fontSize: '14px' }}>Agent Console</Text>
          {hasActivity && (
            <Badge status={adminPaused ? 'warning' : 'processing'} text={
              <Text style={{ fontSize: '11px', color: adminPaused ? token.colorWarning : token.colorSuccess }}>
                {adminPaused ? 'Paused' : 'Live'}
              </Text>
            } />
          )}
        </Space>
        <Button
          type="text"
          size="small"
          icon={<LeftOutlined style={{ fontSize: '10px' }} />}
          onClick={() => setCollapsed(true)}
        >
          Hide
        </Button>
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '12px' }}>
        <Space direction="vertical" style={{ width: '100%' }} size={16}>

          {/* Permissions Section */}
          {pendingPermissions.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <Text style={{ fontSize: '11px', textTransform: 'uppercase', color: token.colorTextDescription, letterSpacing: '0.5px' }}>
                Pending Permissions ({pendingPermissions.length})
              </Text>
              {pendingPermissions.map(perm => (
                <Card
                  key={perm.requestId}
                  size="small"
                  style={{
                    borderLeft: `3px solid ${token.colorPrimary}`,
                    borderRadius: '8px',
                    boxShadow: '0 2px 8px rgba(0,0,0,0.05)'
                  }}
                  bodyStyle={{ padding: '10px' }}
                >
                  <div style={{ marginBottom: '8px' }}>
                    <Text strong style={{ fontSize: '12px', display: 'block' }}>{perm.title}</Text>
                    <Text type="secondary" style={{ fontSize: '11px' }}>{perm.agentName}</Text>
                  </div>
                  <pre style={{
                    fontSize: '10px',
                    background: token.colorFillAlter,
                    padding: '6px',
                    borderRadius: '4px',
                    marginBottom: '10px',
                    whiteSpace: 'pre-wrap',
                    fontFamily: 'monospace'
                  }}>
                    {perm.content.length > 120 ? perm.content.slice(0, 120) + '...' : perm.content}
                  </pre>
                  <Space style={{ width: '100%' }}>
                    {perm.options.map(opt => (
                      <Button
                        key={opt.key}
                        size="small"
                        type={opt.key === 'allow' || opt.key === 'yes' ? 'primary' : 'default'}
                        danger={opt.key === 'deny' || opt.key === 'no'}
                        onClick={() => resolvePermission(perm.requestId, opt.key)}
                        style={{ fontSize: '11px', flex: 1 }}
                      >
                        {opt.label}
                      </Button>
                    ))}
                  </Space>
                </Card>
              ))}
            </div>
          )}

          {/* Workflow Section */}
          {hasActivity && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <Text style={{ fontSize: '11px', textTransform: 'uppercase', color: token.colorTextDescription, letterSpacing: '0.5px' }}>
                Active Workflow
              </Text>
              <div style={{
                background: token.colorBgContainer,
                borderRadius: '8px',
                padding: '8px',
                border: `1px solid ${token.colorBorderSecondary}`
              }}>
                <DispatchTree
                  parents={dispatchParents}
                  onSelectTask={setSelectedTask}
                  selectedTaskId={selectedTask?.id}
                  adminPaused={adminPaused}
                />
              </div>
            </div>
          )}

          {/* Task Detail Card */}
          {selectedTask && (
            <Card
              size="small"
              title={<Text style={{ fontSize: '12px' }}>Task Details</Text>}
              extra={<CloseOutlined onClick={() => setSelectedTask(null)} style={{ fontSize: '10px', cursor: 'pointer' }} />}
              style={{ borderRadius: '8px' }}
            >
              <Space direction="vertical" style={{ width: '100%' }} size={4}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <Text strong style={{ fontSize: '12px' }}>{selectedTask.label}</Text>
                  <Tag color={selectedTask.status === 'done' ? 'success' : 'processing'} style={{ margin: 0, fontSize: '10px' }}>
                    {selectedTask.status}
                  </Tag>
                </div>
                <Text type="secondary" style={{ fontSize: '11px' }}>
                  Agent: {selectedTask.isVirtual ? (selectedTask.personaName ?? selectedTask.agentId) : selectedTask.agentId}
                </Text>
                <Text style={{ fontSize: '11px', color: token.colorText }}>{selectedTask.prompt}</Text>
                {selectedTask.result && (
                  <div style={{
                    marginTop: '4px',
                    padding: '6px',
                    background: token.colorSuccessBg,
                    borderRadius: '4px',
                    border: `1px solid ${token.colorSuccessBorder}`
                  }}>
                    <Text style={{ fontSize: '10px', color: token.colorSuccessText }}>{selectedTask.result}</Text>
                  </div>
                )}
              </Space>
            </Card>
          )}

          {/* Todos Section */}
          {hasTodos && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <Text style={{ fontSize: '11px', textTransform: 'uppercase', color: token.colorTextDescription, letterSpacing: '0.5px' }}>
                Agent Todos
              </Text>
              <AgentTodoPanel agentTodos={agentTodos} groups={groups} />
            </div>
          )}

        </Space>
      </div>

      {/* Resize handle (placed on the left edge now) */}
      <div
        onMouseDown={onMouseDown}
        style={{
          position: 'absolute',
          top: 0,
          left: 0,
          width: '4px',
          height: '100%',
          cursor: 'col-resize',
          zIndex: 30,
          transition: 'background 0.2s',
        }}
        onMouseEnter={(e) => (e.currentTarget.style.background = token.colorPrimary + '66')}
        onMouseLeave={(e) => (e.currentTarget.style.background = 'transparent')}
      />
    </div>
  );
}

function TooltipIcon({ title, onClick, icon, badge, active, hasContent, token }: any) {
  return (
    <div
      onClick={onClick}
      style={{
        position: 'relative',
        cursor: 'pointer',
        color: active ? token.colorPrimary : token.colorTextTertiary,
        transition: 'all 0.2s'
      }}
      onMouseEnter={(e) => e.currentTarget.style.color = token.colorPrimary}
      onMouseLeave={(e) => e.currentTarget.style.color = active ? token.colorPrimary : token.colorTextTertiary}
    >
      {icon}
      {badge > 0 && (
        <div style={{
          position: 'absolute',
          top: '-6px',
          right: '-6px',
          background: token.colorError,
          color: 'white',
          fontSize: '9px',
          width: '14px',
          height: '14px',
          borderRadius: '50%',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          fontWeight: 'bold',
          border: `2px solid ${token.colorBgContainer}`
        }}>
          {badge}
        </div>
      )}
      {active && (
        <div style={{
          position: 'absolute',
          bottom: '-8px',
          left: '50%',
          transform: 'translateX(-50%)',
          width: '4px',
          height: '4px',
          borderRadius: '50%',
          background: token.colorSuccess,
          boxShadow: `0 0 8px ${token.colorSuccess}`
        }} />
      )}
    </div>
  );
}
