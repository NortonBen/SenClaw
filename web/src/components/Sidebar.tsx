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
  ProjectOutlined,
  DownOutlined,
  RightOutlined,
  FolderOutlined,
  ShrinkOutlined,
  MenuOutlined,
  ReloadOutlined,
  ControlOutlined,
  SortAscendingOutlined,
} from '@ant-design/icons';
import { useNavigate, useLocation } from 'react-router-dom';
import type { WsStatus, EventNotification, GroupInfo } from '../types';
import { WorkflowSidebarSection } from './workflows/WorkflowSidebarSection';

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
  const isKanban    = location.pathname.startsWith('/kanban');

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
        <div className="px-4 py-4 flex items-center gap-2" style={{ borderBottom: `1px solid ${token.colorBorderSecondary}`, minHeight: 64 }}>
          <img src="/logo.svg" alt="SenClaw" className="w-6 h-6 object-contain" />
          <span className="font-semibold text-sm tracking-tight" style={{ color: token.colorTextHeading }}>SenClaw</span>
          <Tooltip title={status}>
            <span className={`w-1.5 h-1.5 rounded-full ml-0.5 ${animate ? 'animate-pulse' : ''}`} style={{ background: color }} />
          </Tooltip>
          <div className="ml-auto flex items-center gap-0.5">
            <Tooltip title="Toggle theme">
              <Button type="text" size="small" icon={isDarkMode ? <BulbFilled /> : <BulbOutlined />} onClick={toggleTheme} />
            </Tooltip>
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
        <div className="flex items-center justify-around gap-1.5 px-2 py-1.5" style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
          <Tooltip title="Chat">
            <Button type={isChats ? 'primary' : 'text'} icon={<MessageOutlined style={{ fontSize: 16 }} />} onClick={() => navigate('/chats')} className="flex-1" style={{ height: 38, borderRadius: 10 }} />
          </Tooltip>
          <Tooltip title="Space">
            <Button type={isSpace ? 'primary' : 'text'} icon={<AppstoreOutlined style={{ fontSize: 16 }} />} onClick={() => navigate('/space')} className="flex-1" style={{ height: 38, borderRadius: 10 }} />
          </Tooltip>
          <Tooltip title="Cowork">
            <Button type={isCowork ? 'primary' : 'text'} icon={<CoffeeOutlined style={{ fontSize: 16 }} />} onClick={() => navigate('/cowork')} className="flex-1" style={{ height: 38, borderRadius: 10 }} />
          </Tooltip>
          <Tooltip title="Kanban">
            <Button type={isKanban ? 'primary' : 'text'} icon={<ProjectOutlined style={{ fontSize: 16 }} />} onClick={() => navigate('/kanban')} className="flex-1" style={{ height: 38, borderRadius: 10 }} />
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
            <Tooltip title="Knowledge">
              <Button type={isCognitive ? 'primary' : 'text'} size="small" icon={<DeploymentUnitOutlined />} onClick={() => navigate('/cognitive')} />
            </Tooltip>
            <Tooltip title="Settings">
              <Button type={isSettings ? 'primary' : 'text'} size="small" icon={<SettingOutlined />} onClick={() => navigate('/settings')} />
            </Tooltip>
          </div>
        </div>
      </div>
    </Sider>
  );
}

// ─── SessionList ─────────────────────────────────────────────────────────────

const ACTIVE_STATES = new Set(['thinking', 'executing', 'processing', 'waiting_permission', 'waiting_question']);

/** Sidebar "Group by" axis: bucket by project folder, by date, or not at all. */
export type GroupMode = 'project' | 'date' | 'none';
/** Sidebar "Sort by" axis: last activity, creation time, or name A–Z. */
export type SortMode = 'created' | 'updated' | 'name';

const ORGANIZE_KEY = 'senclaw:sessionlist-organize';
const SORT_KEY = 'senclaw:sessionlist-sort';

function loadGroupMode(): GroupMode {
  try {
    const v = localStorage.getItem(ORGANIZE_KEY);
    if (v === 'project' || v === 'date' || v === 'none') return v;
    // Legacy 4-way organize values ('project-recent' collapses into 'project'
    // — bucket order now follows the sort mode).
    if (v === 'project-recent') return 'project';
    if (v === 'chronological') return 'date';
    if (v === 'flat') return 'none';
  } catch {}
  return 'project';
}
function saveGroupMode(m: GroupMode) { try { localStorage.setItem(ORGANIZE_KEY, m); } catch {} }

function loadSort(): SortMode {
  try {
    const v = localStorage.getItem(SORT_KEY);
    if (v === 'created' || v === 'updated' || v === 'name') return v;
  } catch {}
  return 'updated';
}
function saveSort(m: SortMode) { try { localStorage.setItem(SORT_KEY, m); } catch {} }

/** Best-effort creation timestamp parsed from the JID's trailing base36 suffix. */
function jidCreatedAt(jid: string): number {
  const m = jid.match(/:([0-9a-z]{6,})(?:-[0-9a-z]{4,})?$/i);
  if (!m) return 0;
  const ms = parseInt(m[1], 36);
  return Number.isFinite(ms) && ms > 0 ? ms : 0;
}

// "Recent activity" = the chat's last message/agent response time from the
// server (NOT when the user last opened it — opening must not reorder).
// Sort:name still needs a timestamp for date buckets — use activity time.
function itemTimestamp(g: GroupInfo, sort: SortMode): number {
  if (sort === 'created') return jidCreatedAt(g.jid);
  return g.lastActivity ?? jidCreatedAt(g.jid);
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
  /** Re-fetch the chat list from the server (reload button beside New Chat). */
  onReload?: () => void;
}

export function SessionList({ groups, selectedJid, agentStates, pinnedJids, onSelect, onNewChat, onPin, onRename, onDelete, onReload }: SessionListProps) {
  const { token } = theme.useToken();
  const [renamingJid, setRenamingJid] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const [groupMode, setGroupModeState] = useState<GroupMode>(loadGroupMode);
  const [sort, setSort] = useState<SortMode>(loadSort);
  // collapseSignal/expandSignal are bumped from the menu to broadcast
  // "collapse all" / "expand all" intents to <SessionGroups/>.
  const [collapseSignal, setCollapseSignal] = useState(0);
  const [expandSignal, setExpandSignal] = useState(0);
  // Brief spin after clicking reload — the refresh is fire-and-forget over WS,
  // so we just animate for a moment to acknowledge the click.
  const [reloading, setReloading] = useState(false);
  const renameInputRef = useRef<HTMLInputElement>(null);

  const handleReload = () => {
    if (!onReload) return;
    onReload();
    setReloading(true);
    setTimeout(() => setReloading(false), 600);
  };

  useEffect(() => {
    if (renamingJid && renameInputRef.current) renameInputRef.current.focus();
  }, [renamingJid]);

  const pinned   = groups.filter(g => pinnedJids.has(g.jid));
  const unpinned = groups.filter(g => !pinnedJids.has(g.jid));

  const setGroupMode = (m: GroupMode) => { setGroupModeState(m); saveGroupMode(m); };
  const setSortMode  = (m: SortMode) => { setSort(m); saveSort(m); };

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
      <div className="px-3 py-2 flex items-center gap-1.5">
        <Button type="dashed" size="small" block icon={<PlusOutlined />} onClick={onNewChat} style={{ borderRadius: 8, fontSize: 12 }} className="flex-1">
          New Session
        </Button>
        {onReload && (
          <Tooltip title="Tải lại danh sách chat">
            <Button
              type="dashed" size="small"
              icon={<ReloadOutlined spin={reloading} />}
              onClick={handleReload}
              style={{ borderRadius: 8 }}
            />
          </Tooltip>
        )}
      </div>

      {pinned.length > 0 && <>{sectionLabel('Pinned')}{pinned.map(g => renderItem(g, true))}</>}

      {/* Workflow runs surface as sessions too — selecting one shows the
          flow-activity pane in place of the chat conversation. */}
      <WorkflowSidebarSection selectedJid={selectedJid} onSelect={onSelect} />

      {/* Header row: section title + collapse-all + group/sort menu. */}
      <div className="px-4 pt-3 pb-1 flex items-center gap-1">
        <span
          className="text-[10px] font-semibold tracking-widest uppercase flex-1"
          style={{ color: token.colorTextTertiary }}
        >
          {groupMode === 'none' ? 'Sessions' : groupMode === 'date' ? 'Chats' : 'Projects'}
        </span>
        <Tooltip title="Thu gọn tất cả">
          <Button
            type="text" size="small"
            icon={<ShrinkOutlined style={{ fontSize: 11 }} />}
            onClick={() => setCollapseSignal(n => n + 1)}
            style={{ width: 22, height: 22, padding: 0, color: token.colorTextTertiary }}
          />
        </Tooltip>
        {/* Flat two-section menu (Group by / Sort by) — no nested submenus. */}
        <Dropdown
          trigger={['click']}
          placement="bottomRight"
          menu={{
            items: [
              {
                key: 'group-by',
                type: 'group' as const,
                label: 'Nhóm theo',
                children: [
                  { key: 'g-project', label: 'Dự án',      icon: <FolderOutlined />,      onClick: () => setGroupMode('project'), extra: groupMode === 'project' ? <CheckOutlined /> : undefined },
                  { key: 'g-date',    label: 'Ngày',       icon: <ClockCircleOutlined />, onClick: () => setGroupMode('date'),    extra: groupMode === 'date'    ? <CheckOutlined /> : undefined },
                  { key: 'g-none',    label: 'Không nhóm', icon: <MenuOutlined />,        onClick: () => setGroupMode('none'),    extra: groupMode === 'none'    ? <CheckOutlined /> : undefined },
                ],
              },
              { type: 'divider' as const },
              {
                key: 'sort-by',
                type: 'group' as const,
                label: 'Sắp xếp',
                children: [
                  { key: 's-updated', label: 'Hoạt động gần đây', icon: <ClockCircleOutlined />,  onClick: () => setSortMode('updated'), extra: sort === 'updated' ? <CheckOutlined /> : undefined },
                  { key: 's-created', label: 'Ngày tạo',          icon: <PlusOutlined />,         onClick: () => setSortMode('created'), extra: sort === 'created' ? <CheckOutlined /> : undefined },
                  { key: 's-name',    label: 'Tên A–Z',           icon: <SortAscendingOutlined />, onClick: () => setSortMode('name'),   extra: sort === 'name'    ? <CheckOutlined /> : undefined },
                ],
              },
              { type: 'divider' as const },
              { key: 'expand-all',   icon: <DownOutlined />,   label: 'Mở rộng tất cả', onClick: () => setExpandSignal(n => n + 1) },
              { key: 'collapse-all', icon: <ShrinkOutlined />, label: 'Thu gọn tất cả', onClick: () => setCollapseSignal(n => n + 1) },
            ],
          }}
        >
          <Tooltip title="Nhóm & sắp xếp">
            <Button
              type="text" size="small" icon={<ControlOutlined />}
              style={{ width: 22, height: 22, padding: 0, color: token.colorTextTertiary }}
            />
          </Tooltip>
        </Dropdown>
      </div>

      <SessionGroups
        groups={unpinned}
        groupMode={groupMode}
        sort={sort}
        collapseSignal={collapseSignal}
        expandSignal={expandSignal}
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

/** Bucket key + display label for a session under each group mode. */
function bucketKey(g: GroupInfo, mode: GroupMode, ts: number): { key: string; label: string } {
  if (mode === 'project') {
    const f = g.folder || '(unknown)';
    return { key: f, label: f };
  }
  if (mode === 'none') return { key: 'all', label: 'Sessions' };

  // Date — bucket by recency relative to today.
  if (!ts) return { key: 'older', label: 'Older' };
  const d = new Date(ts);
  const now = new Date();
  const isSameDay = d.toDateString() === now.toDateString();
  const yest = new Date(now); yest.setDate(now.getDate() - 1);
  const isYesterday = d.toDateString() === yest.toDateString();
  if (isSameDay)   return { key: 'today',     label: 'Today' };
  if (isYesterday) return { key: 'yesterday', label: 'Yesterday' };
  const diffDays = Math.floor((now.getTime() - d.getTime()) / 86_400_000);
  if (diffDays <= 7)  return { key: 'past7',  label: 'Previous 7 days' };
  if (diffDays <= 30) return { key: 'past30', label: 'Previous 30 days' };
  return { key: 'older', label: 'Older' };
}

/** Canonical bucket ordering for the chronological view (matches Codex). */
const CHRONO_ORDER: Record<string, number> = {
  today: 0, yesterday: 1, past7: 2, past30: 3, older: 4,
};

function SessionGroups({
  groups, groupMode, sort, collapseSignal, expandSignal, renderItem, token,
}: {
  groups: GroupInfo[];
  groupMode: GroupMode;
  sort: SortMode;
  collapseSignal: number;
  expandSignal: number;
  renderItem: (g: GroupInfo) => React.ReactNode;
  token: ReturnType<typeof theme.useToken>['token'];
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(loadCollapsed);

  // Bucket sessions and compute each bucket's "freshest" timestamp so the
  // Recent-projects mode and within-bucket sort can both reuse it.
  type Bucket = { key: string; label: string; items: GroupInfo[]; maxTs: number };
  const buckets = new Map<string, Bucket>();
  for (const g of groups) {
    const ts = itemTimestamp(g, sort);
    const { key, label } = bucketKey(g, groupMode, ts);
    let b = buckets.get(key);
    if (!b) { b = { key, label, items: [], maxTs: 0 }; buckets.set(key, b); }
    b.items.push(g);
    if (ts > b.maxTs) b.maxTs = ts;
  }

  // Items inside each bucket follow the active sort: name A–Z, else newest-first.
  for (const b of buckets.values()) {
    if (sort === 'name') {
      b.items.sort((a, x) => (a.name || a.jid).localeCompare(x.name || x.jid));
    } else {
      b.items.sort((a, x) => itemTimestamp(x, sort) - itemTimestamp(a, sort));
    }
  }

  // Bucket order depends on group mode:
  //   date    → canonical today / yesterday / past7 / past30 / older
  //   none    → single bucket, no sort needed
  //   project → follows the sort mode: A–Z for name, else freshest bucket first
  const sorted = [...buckets.values()].sort((a, b) => {
    switch (groupMode) {
      case 'date': return (CHRONO_ORDER[a.key] ?? 99) - (CHRONO_ORDER[b.key] ?? 99);
      case 'none': return 0;
      case 'project':
      default:
        return sort === 'name' ? a.label.localeCompare(b.label) : b.maxTs - a.maxTs;
    }
  });

  // Collapse-all / Expand-all broadcasts from the parent menu.
  useEffect(() => {
    if (collapseSignal === 0) return;
    const all = new Set<string>(sorted.map(b => `${groupMode}:${b.key}`));
    setCollapsed(prev => {
      const merged = new Set(prev);
      for (const k of all) merged.add(k);
      saveCollapsed(merged);
      return merged;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [collapseSignal]);

  useEffect(() => {
    if (expandSignal === 0) return;
    setCollapsed(prev => {
      if (prev.size === 0) return prev;
      // Drop any key that belongs to the current group mode.
      const next = new Set<string>();
      for (const k of prev) if (!k.startsWith(`${groupMode}:`)) next.add(k);
      saveCollapsed(next);
      return next;
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [expandSignal]);

  if (sorted.length === 0) {
    return (
      <div className="px-4 py-1 text-xs" style={{ color: token.colorTextTertiary }}>
        No chats yet. Click + New Chat above.
      </div>
    );
  }

  const toggle = (key: string) => {
    setCollapsed(prev => {
      const next = new Set(prev);
      const k = `${groupMode}:${key}`;
      if (next.has(k)) next.delete(k); else next.add(k);
      saveCollapsed(next);
      return next;
    });
  };

  return (
    <>
      {sorted.map(b => {
        const collapsedKey = `${groupMode}:${b.key}`;
        const isCollapsed = collapsed.has(collapsedKey);
        const isProjectMode = groupMode === 'project';
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
              {isProjectMode && <FolderOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} />}
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
