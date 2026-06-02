import type { DispatchParent, AgentTodosEntry, ChatMessage, GroupInfo, PermissionMessage, WorkbenchState } from '../types';
import { useMemo } from 'react';
import { theme } from 'antd';

interface Props {
  expanded: 'agent' | 'workbench' | null;
  onToggle: (which: 'agent' | 'workbench') => void;
  // AgentConsole badge data
  dispatchParents: DispatchParent[];
  agentTodos: Record<string, AgentTodosEntry>;
  messages: Record<string, ChatMessage[]>;
  groups: GroupInfo[];
  // Workbench badge data
  workbenchState: WorkbenchState | null;
}

export function DockBadges(p: Props) {
  const { token } = theme.useToken();

  // ===== AgentConsole signals =====
  const pendingPermissions = useMemo(() => {
    let n = 0;
    for (const msgs of Object.values(p.messages)) {
      for (const m of msgs) {
        if (m.role === 'permission' && !(m as PermissionMessage).resolved) n++;
      }
    }
    return n;
  }, [p.messages]);
  const hasActivity = p.dispatchParents.some(d => d.status === 'active' || d.status === 'queued');
  const hasTodos = Object.keys(p.agentTodos).length > 0;
  void p.groups; // signature parity only; reserved for future targeted navigation

  // ===== Workbench signals =====
  const wbCurrent = p.workbenchState?.current ?? null;
  const wbHasRunning = p.workbenchState?.history.some(h => h.process?.status === 'ready') ?? false;

  return (
    <div
      className="flex flex-col items-center gap-3 w-10 flex-shrink-0 border-l py-3"
      style={{
        background: token.colorBgContainer,
        borderColor: token.colorBorderSecondary,
      }}
    >
      <BadgeButton
        active={p.expanded === 'agent'}
        onClick={() => p.onToggle('agent')}
        title="Agent Console"
        icon={<GearIcon />}
        countBadge={pendingPermissions > 0 ? pendingPermissions : null}
        dot={hasActivity ? 'live' : hasTodos ? 'info' : null}
        token={token}
      />
      <BadgeButton
        active={p.expanded === 'workbench'}
        onClick={() => p.onToggle('workbench')}
        title="Workbench"
        icon={<MonitorIcon />}
        countBadge={null}
        dot={wbCurrent ? 'info' : wbHasRunning ? 'live' : null}
        token={token}
      />
    </div>
  );
}

interface BadgeButtonProps {
  active: boolean;
  onClick: () => void;
  title: string;
  icon: React.ReactNode;
  countBadge: number | null;
  /** Status dot: 'live' = pulsing green; 'info' = static blue. */
  dot: 'live' | 'info' | null;
  token: ReturnType<typeof theme.useToken>['token'];
}

function BadgeButton({ active, onClick, title, icon, countBadge, dot, token }: BadgeButtonProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="relative flex flex-col items-center gap-1 w-7 h-7 justify-center rounded transition-colors"
      style={{
        background: active ? token.colorPrimaryBg : 'transparent',
        color: active ? token.colorPrimary : token.colorTextTertiary,
      }}
      onMouseEnter={(e) => {
        if (!active) e.currentTarget.style.color = token.colorPrimary;
      }}
      onMouseLeave={(e) => {
        if (!active) e.currentTarget.style.color = token.colorTextTertiary;
      }}
    >
      {icon}
      {countBadge != null && (
        <span className="absolute -top-1 -right-1 min-w-[14px] h-[14px] px-1 rounded-full bg-amber-500 text-white text-[9px] font-bold flex items-center justify-center">
          {countBadge}
        </span>
      )}
      {dot === 'live' && (
        <span className="absolute -bottom-0.5 w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
      )}
      {dot === 'info' && (
        <span className="absolute -bottom-0.5 w-1.5 h-1.5 rounded-full bg-[#5BBFE8]" />
      )}
    </button>
  );
}

// ===== Icons (same visual weight as the gear: 16x16, stroke 1.8) =====

function GearIcon() {
  // Reuse the existing unicode gear to keep the original look.
  return <span className="text-base leading-none select-none">⚙</span>;
}

function MonitorIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <rect x="3" y="4" width="18" height="13" rx="1.2" />
      <path d="M9 21h6" />
      <path d="M12 17v4" />
    </svg>
  );
}
