import { useState, useRef, useEffect } from 'react';
import { Layout, Tooltip, theme, Button, Badge, Popover, List, Typography, Dropdown } from 'antd';
import {
  SettingOutlined,
  PlusOutlined,
  PushpinOutlined,
  PushpinFilled,
  MoreOutlined,
  DeleteOutlined,
  EditOutlined,
  CopyOutlined,
  ApiOutlined,
  BulbOutlined,
  BookOutlined,
  BulbFilled,
  BellOutlined,
  BellFilled,
  CheckOutlined,
  ClockCircleOutlined,
  DeploymentUnitOutlined,
  MessageOutlined,
  AppstoreOutlined,
  CoffeeOutlined,
  DownOutlined,
  RightOutlined,
  FolderOutlined,
} from '@ant-design/icons';
import { useNavigate, useLocation } from 'react-router-dom';
import type { WsStatus, EventNotification, GroupInfo } from '../types';

const { Sider } = Layout;

interface Props {
  status: WsStatus;
  sidebarContent: React.ReactNode;
  isDarkMode: boolean;
  toggleTheme: () => void;
  notifications: EventNotification[];
  onMarkRead: (id: string) => void;
  onClearAll: () => void;
}

const STATUS_DOT: Record<WsStatus, { color: string; animate: boolean }> = {
  connected:    { color: '#52c41a', animate: false },
  connecting:   { color: '#faad14', animate: true  },
  disconnected: { color: '#f5222d', animate: false },
};

export function Sidebar({ status, isDarkMode, toggleTheme, sidebarContent, notifications, onMarkRead, onClearAll }: Props) {
  const navigate = useNavigate();
  const location = useLocation();
  const { token } = theme.useToken();
  const { color, animate } = STATUS_DOT[status];
  const unreadCount = notifications.filter(n => !n.read).length;

  const isSettings  = location.pathname.startsWith('/settings');
  const isWiki      = location.pathname.startsWith('/wiki');
  const isPlugins   = location.pathname.startsWith('/plugins');
  const isCognitive = location.pathname.startsWith('/cognitive');
  const isChats     = location.pathname === '/' || location.pathname.startsWith('/chats');
  const isSpace     = location.pathname.startsWith('/space');
  const isCowork    = location.pathname.startsWith('/cowork');

  const notifContent = (
    <div style={{ width: 300, maxHeight: 400, overflowY: 'auto' }}>
      <div className="flex items-center justify-between mb-2 px-1">
        <Typography.Text strong>Thông báo</Typography.Text>
        {notifications.length > 0 && (
          <Button size="small" type="text" icon={<CheckOutlined />} onClick={onClearAll}>Xóa tất cả</Button>
        )}
      </div>
      {notifications.length === 0 ? (
        <div className="text-center py-6" style={{ color: token.colorTextSecondary }}>Không có thông báo</div>
      ) : (
        <List
          size="small"
          dataSource={[...notifications].reverse()}
          renderItem={(n) => {
            const d = new Date(n.startAt);
            const timeStr = d.toLocaleTimeString('vi-VN', { hour: '2-digit', minute: '2-digit' });
            const dateStr = d.toLocaleDateString('vi-VN', { day: '2-digit', month: '2-digit' });
            const isLate  = (n.delayedMs ?? 0) > 60_000;
            return (
              <List.Item
                style={{ background: n.read ? 'transparent' : token.colorPrimaryBg, borderRadius: 6, marginBottom: 4, padding: '6px 10px', cursor: 'pointer' }}
                onClick={() => onMarkRead(n.id)}
              >
                <div className="flex gap-2 w-full">
                  <ClockCircleOutlined style={{
                    color: n.kind === 'renotify' ? token.colorWarning : n.kind === 'pending' ? token.colorTextSecondary : n.kind === 'start' ? token.colorSuccess : token.colorPrimary,
                    marginTop: 2, flexShrink: 0,
                  }} />
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate" style={{ color: token.colorText }}>{n.title}</div>
                    <div className="text-xs" style={{ color: token.colorTextSecondary }}>
                      {n.kind === 'pending' ? `📅 Sắp nhắc · ${dateStr} ${timeStr}` : n.kind === 'start' ? '🔔 Bắt đầu ngay' : n.kind === 'renotify' ? '🔁 Đang diễn ra' : '⏰ Nhắc nhở'}
                      {n.kind !== 'pending' && ` · ${dateStr} ${timeStr}`}{isLate && ' · trễ'}
                    </div>
                  </div>
                  {!n.read && <span className="w-2 h-2 rounded-full flex-shrink-0 mt-1" style={{ background: token.colorPrimary }} />}
                </div>
              </List.Item>
            );
          }}
        />
      )}
    </div>
  );

  return (
    <Sider
      width={260}
      className="h-screen flex flex-col select-none"
      style={{ background: token.colorBgContainer, borderRight: `1px solid ${token.colorBorderSecondary}` }}
    >
      <div className="flex flex-col h-full">
        {/* Header */}
        <div className="px-4 py-3 flex items-center gap-2" style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
          <img src="/logo.svg" alt="SenClaw" className="w-6 h-6 object-contain" />
          <span className="font-semibold text-sm tracking-tight" style={{ color: token.colorTextHeading }}>SenClaw</span>
          <Tooltip title={status}>
            <span className={`w-1.5 h-1.5 rounded-full ml-0.5 ${animate ? 'animate-pulse' : ''}`} style={{ background: color }} />
          </Tooltip>
          <div className="ml-auto">
            <Popover content={notifContent} trigger="click" placement="bottomRight" arrow={false}>
              <Badge count={unreadCount} size="small" offset={[-2, 2]}>
                <Button type="text" size="small"
                  icon={unreadCount > 0 ? <BellFilled style={{ color: token.colorPrimary }} /> : <BellOutlined />}
                />
              </Badge>
            </Popover>
          </div>
        </div>

        {/* Top nav tabs */}
        <div className="flex items-center justify-around px-2 py-1.5" style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
          <Tooltip title="Chat">
            <Button type={isChats ? 'primary' : 'text'} size="small" icon={<MessageOutlined />} onClick={() => navigate('/chats')} className="flex-1" />
          </Tooltip>
          <Tooltip title="Space">
            <Button type={isSpace ? 'primary' : 'text'} size="small" icon={<AppstoreOutlined />} onClick={() => navigate('/space')} className="flex-1" />
          </Tooltip>
          <Tooltip title="Cowork">
            <Button type={isCowork ? 'primary' : 'text'} size="small" icon={<CoffeeOutlined />} onClick={() => navigate('/cowork')} className="flex-1" />
          </Tooltip>
        </div>

        {/* Session list injected by ChatPage */}
        <div className="flex-1 overflow-y-auto min-h-0 py-1">{sidebarContent}</div>

        {/* Bottom nav */}
        <div style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}>
          <div className="flex items-center justify-around py-2 px-2">
            <Tooltip title="Wiki">
              <Button type={isWiki ? 'primary' : 'text'} size="small" icon={<BookOutlined />} onClick={() => navigate('/wiki')} />
            </Tooltip>
            <Tooltip title="Plugins">
              <Button type={isPlugins ? 'primary' : 'text'} size="small" icon={<ApiOutlined />} onClick={() => navigate('/plugins')} />
            </Tooltip>
            <Tooltip title="Memory">
              <Button type={isCognitive ? 'primary' : 'text'} size="small" icon={<DeploymentUnitOutlined />} onClick={() => navigate('/cognitive')} />
            </Tooltip>
            <Tooltip title="Settings">
              <Button type={isSettings ? 'primary' : 'text'} size="small" icon={<SettingOutlined />} onClick={() => navigate('/settings')} />
            </Tooltip>
            <Tooltip title="Toggle theme">
              <Button type="text" size="small" icon={isDarkMode ? <BulbFilled /> : <BulbOutlined />} onClick={toggleTheme} />
            </Tooltip>
          </div>
        </div>
      </div>
    </Sider>
  );
}

// ─── SessionList ─────────────────────────────────────────────────────────────

const ACTIVE_STATES = new Set(['thinking', 'executing', 'processing', 'waiting_permission', 'waiting_question']);

export type GroupByMode = 'workspace' | 'day' | 'week' | 'none';

const GROUPBY_KEY = 'senclaw:sessionlist-groupby';
function loadGroupBy(): GroupByMode {
  try {
    const v = localStorage.getItem(GROUPBY_KEY);
    return (v === 'day' || v === 'week' || v === 'workspace' || v === 'none') ? v : 'workspace';
  } catch { return 'workspace'; }
}
function saveGroupBy(m: GroupByMode) {
  try { localStorage.setItem(GROUPBY_KEY, m); } catch {}
}

export interface SessionListProps {
  groups: GroupInfo[];
  selectedJid: string | null;
  agentStates: Record<string, string>;
  pinnedJids: Set<string>;
  onSelect: (jid: string) => void;
  onNewChat: () => void;
  onPin: (jid: string) => void;
  onRename: (jid: string, name: string) => void;
  onDelete: (jid: string) => void;
}

export function SessionList({ groups, selectedJid, agentStates, pinnedJids, onSelect, onNewChat, onPin, onRename, onDelete }: SessionListProps) {
  const { token } = theme.useToken();
  const [renamingJid, setRenamingJid] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [groupBy, setGroupBy] = useState<GroupByMode>(loadGroupBy);
  const renameInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renamingJid && renameInputRef.current) renameInputRef.current.focus();
  }, [renamingJid]);

  const pinned   = groups.filter(g => pinnedJids.has(g.jid));
  const unpinned = groups.filter(g => !pinnedJids.has(g.jid));

  const setMode = (m: GroupByMode) => { setGroupBy(m); saveGroupBy(m); };

  const renderItem = (g: GroupInfo, isPinned = false) => {
    const isSelected = g.jid === selectedJid;
    const state      = agentStates[g.jid] ?? 'idle';
    const isActive   = ACTIVE_STATES.has(state);
    const isRenaming = renamingJid === g.jid;

    const menuItems = [
      {
        key: 'pin',
        icon: isPinned ? <PushpinFilled /> : <PushpinOutlined />,
        label: isPinned ? 'Bỏ ghim' : 'Ghim',
        onClick: () => onPin(g.jid),
      },
      {
        key: 'rename',
        icon: <EditOutlined />,
        label: 'Đổi tên',
        onClick: () => { setRenamingJid(g.jid); setRenameValue(g.name || ''); },
      },
      {
        key: 'copy',
        icon: <CopyOutlined />,
        label: 'Copy ID',
        onClick: () => navigator.clipboard?.writeText(g.jid),
      },
      { type: 'divider' as const },
      {
        key: 'delete',
        icon: <DeleteOutlined />,
        label: 'Xoá',
        danger: true,
        onClick: () => onDelete(g.jid),
      },
    ];

    return (
      <div
        key={g.jid}
        onClick={() => !isRenaming && onSelect(g.jid)}
        className="group flex items-center gap-2 px-3 py-1.5 rounded-md mx-2 my-0.5 cursor-pointer"
        style={{ background: isSelected ? `${token.colorPrimary}18` : 'transparent', transition: 'background 0.15s' }}
        onMouseEnter={e => { if (!isSelected) (e.currentTarget as HTMLDivElement).style.background = token.colorFillAlter; }}
        onMouseLeave={e => { if (!isSelected) (e.currentTarget as HTMLDivElement).style.background = 'transparent'; }}
      >
        <span
          className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${isActive ? 'animate-pulse' : ''}`}
          style={{ background: isActive ? token.colorWarning : isSelected ? token.colorPrimary : token.colorTextQuaternary }}
        />
        {isRenaming ? (
          <input
            ref={renameInputRef}
            value={renameValue}
            onChange={e => setRenameValue(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'Enter') { onRename(g.jid, renameValue); setRenamingJid(null); }
              if (e.key === 'Escape') setRenamingJid(null);
            }}
            onBlur={() => { onRename(g.jid, renameValue); setRenamingJid(null); }}
            onClick={e => e.stopPropagation()}
            className="flex-1 min-w-0 text-xs outline-none bg-transparent border-b"
            style={{ color: token.colorText, borderColor: token.colorPrimary }}
          />
        ) : (
          <span
            className="flex-1 min-w-0 truncate text-xs"
            style={{ color: isSelected ? token.colorPrimary : token.colorText, fontWeight: isSelected ? 500 : 400 }}
          >
            {g.name || g.jid}
          </span>
        )}
        <Dropdown menu={{ items: menuItems }} trigger={['click']} placement="bottomRight">
          <Button
            type="text" size="small" icon={<MoreOutlined />}
            className="opacity-0 group-hover:opacity-100 flex-shrink-0"
            style={{ minWidth: 20, height: 20, padding: 0 }}
            onClick={e => e.stopPropagation()}
          />
        </Dropdown>
      </div>
    );
  };

  const sectionLabel = (label: string) => (
    <div className="px-4 pt-3 pb-1">
      <span className="text-[10px] font-semibold tracking-widest uppercase" style={{ color: token.colorTextTertiary }}>{label}</span>
    </div>
  );

  return (
    <div className="flex flex-col h-full">
      <div className="px-3 py-2">
        <Button type="dashed" size="small" block icon={<PlusOutlined />} onClick={onNewChat} style={{ borderRadius: 8, fontSize: 12 }}>
          New Chat
        </Button>
      </div>

      {pinned.length > 0 && <>{sectionLabel('Pinned')}{pinned.map(g => renderItem(g, true))}</>}

      {/* Group-by toggle: Workspace · Day · Week · None */}
      <div className="px-3 pt-2 pb-1 flex items-center gap-1">
        {(['workspace', 'day', 'week', 'none'] as GroupByMode[]).map(m => {
          const active = groupBy === m;
          return (
            <button
              key={m}
              type="button"
              onClick={() => setMode(m)}
              className="flex-1 text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded-md transition-colors"
              style={{
                background: active ? `${token.colorPrimary}1a` : 'transparent',
                color: active ? token.colorPrimary : token.colorTextTertiary,
                border: 'none', cursor: 'pointer', fontWeight: active ? 600 : 400,
              }}
            >
              {m === 'workspace' ? 'WS' : m === 'day' ? 'Day' : m === 'week' ? 'Week' : 'Flat'}
            </button>
          );
        })}
      </div>

      <SessionGroups
        groups={unpinned}
        groupBy={groupBy}
        renderItem={renderItem}
        token={token}
      />
    </div>
  );
}

// ─── Folder grouping ─────────────────────────────────────────────────────────

const COLLAPSED_KEY = 'senclaw:collapsed-folders';

function loadCollapsed(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]')); } catch { return new Set(); }
}
function saveCollapsed(s: Set<string>) {
  try { localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...s])); } catch {}
}

// Compute the bucket key + display label for a session under each mode.
// "Day" / "Week" use the last-active timestamp if present (server-touched on
// every message), else fall back to creation time (best-effort — frontend
// doesn't always have created_at, so just bucket as "Recent" in that case).
function bucketKey(g: GroupInfo, mode: GroupByMode): { key: string; sortKey: string; label: string } {
  if (mode === 'workspace') {
    const f = g.folder || '(unknown)';
    return { key: f, sortKey: f, label: f };
  }
  if (mode === 'none') return { key: 'all', sortKey: '0', label: 'Sessions' };

  // For day/week we want recent at top → use INVERSE sort key.
  // GroupInfo doesn't carry timestamps in the current type, so we synthesize a
  // pseudo-timestamp from the JID's trailing base36 suffix (set by new chat
  // creation) — for any JID that isn't `*:<base36>`, we group under "Older".
  // JID format: web:<folder>:<base36ts>-<rand6>  (new chats)
  //         OR:  web:<folder>:<base36ts>          (legacy / current build)
  // Capture just the timestamp portion so the bucket key reflects creation
  // time regardless of which suffix shape was used.
  const m = g.jid.match(/:([0-9a-z]{6,})(?:-[0-9a-z]{4,})?$/i);
  if (!m) return { key: 'older', sortKey: 'zzz', label: 'Older' };
  const ms = parseInt(m[1], 36);
  if (!Number.isFinite(ms) || ms <= 0) {
    return { key: 'older', sortKey: 'zzz', label: 'Older' };
  }
  const d = new Date(ms);
  const now = new Date();
  const isSameDay = d.toDateString() === now.toDateString();
  const yest = new Date(now); yest.setDate(now.getDate() - 1);
  const isYesterday = d.toDateString() === yest.toDateString();
  if (mode === 'day') {
    if (isSameDay)     return { key: 'today',     sortKey: '0', label: 'Today' };
    if (isYesterday)   return { key: 'yesterday', sortKey: '1', label: 'Yesterday' };
    // last 7 days else flat
    const diffDays = Math.floor((now.getTime() - d.getTime()) / 86_400_000);
    if (diffDays <= 7) return { key: 'past7',  sortKey: '2', label: 'Previous 7 days' };
    if (diffDays <= 30) return { key: 'past30', sortKey: '3', label: 'Previous 30 days' };
    return { key: 'older', sortKey: '4', label: 'Older' };
  }
  // week
  const diffMs = now.getTime() - d.getTime();
  if (diffMs < 7 * 86_400_000)  return { key: 'this-week', sortKey: '0', label: 'This week' };
  if (diffMs < 14 * 86_400_000) return { key: 'last-week', sortKey: '1', label: 'Last week' };
  if (diffMs < 30 * 86_400_000) return { key: 'past-month', sortKey: '2', label: 'Past month' };
  return { key: 'older', sortKey: '3', label: 'Older' };
}

function SessionGroups({
  groups, groupBy, renderItem, token,
}: {
  groups: GroupInfo[];
  groupBy: GroupByMode;
  renderItem: (g: GroupInfo) => React.ReactNode;
  token: ReturnType<typeof theme.useToken>['token'];
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);

  type Bucket = { key: string; sortKey: string; label: string; items: GroupInfo[] };
  const buckets = new Map<string, Bucket>();
  for (const g of groups) {
    const { key, sortKey, label } = bucketKey(g, groupBy);
    let b = buckets.get(key);
    if (!b) { b = { key, sortKey, label, items: [] }; buckets.set(key, b); }
    b.items.push(g);
  }
  const sorted = [...buckets.values()].sort((a, b) => a.sortKey.localeCompare(b.sortKey));

  if (sorted.length === 0) {
    return (
      <>
        <div className="px-4 pt-3 pb-1">
          <span className="text-[10px] font-semibold tracking-widest uppercase" style={{ color: token.colorTextTertiary }}>
            Sessions
          </span>
        </div>
        <div className="px-4 py-1 text-xs" style={{ color: token.colorTextTertiary }}>
          No chats yet. Click + New Chat above.
        </div>
      </>
    );
  }

  const toggle = (key: string) => {
    setCollapsed(prev => {
      const next = new Set(prev);
      const k = `${groupBy}:${key}`;
      if (next.has(k)) next.delete(k); else next.add(k);
      saveCollapsed(next);
      return next;
    });
  };

  return (
    <>
      {sorted.map(b => {
        const collapsedKey = `${groupBy}:${b.key}`;
        const isCollapsed = collapsed.has(collapsedKey);
        return (
          <div key={collapsedKey} className="mt-1">
            <button
              type="button"
              onClick={() => toggle(b.key)}
              className="w-full flex items-center gap-1.5 px-4 pt-2 pb-1 text-left"
              style={{ background: 'transparent', border: 'none', cursor: 'pointer' }}
            >
              {isCollapsed
                ? <RightOutlined style={{ fontSize: 8, color: token.colorTextTertiary }} />
                : <DownOutlined  style={{ fontSize: 8, color: token.colorTextTertiary }} />}
              {groupBy === 'workspace' && <FolderOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} />}
              <span
                className="text-[10px] font-semibold tracking-widest uppercase truncate flex-1"
                style={{ color: token.colorTextTertiary }}
                title={b.label}
              >
                {b.label}
              </span>
              <span className="text-[10px]" style={{ color: token.colorTextQuaternary }}>
                {b.items.length}
              </span>
            </button>
            {!isCollapsed && b.items.map(g => renderItem(g))}
          </div>
        );
      })}
    </>
  );
}
