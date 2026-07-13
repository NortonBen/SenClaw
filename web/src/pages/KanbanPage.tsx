import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Layout, Card, Button, Modal, Form, Input, Select, message, Tag, Badge,
  Drawer, Tooltip, Typography, Space, Divider, Avatar, Spin, Empty, Dropdown,
  theme,
} from 'antd';
import {
  PlusOutlined, ArrowLeftOutlined, ReloadOutlined, DeleteOutlined,
  CheckCircleOutlined, StopOutlined, ThunderboltOutlined, EditOutlined,
  CommentOutlined, LinkOutlined, ExclamationCircleOutlined, MenuOutlined,
  AppstoreOutlined, FilterOutlined, ColumnWidthOutlined, LoadingOutlined,
  RobotOutlined, FolderOpenOutlined, ClockCircleOutlined, BlockOutlined,
  UnlockOutlined, MoreOutlined, SyncOutlined,
} from '@ant-design/icons';
import { AppLayout } from '../components/AppLayout';

const { Content } = Layout;
const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface BoardSummary {
  id: number;
  title: string;
  description: string;
  workspace_dir: string;
  column_count: number;
  card_count: number;
}

interface CardData {
  id: number;
  column_id: number;
  title: string;
  description: string;
  priority: string;
  assignee: string;
  labels: string;
  done: boolean;
  comment_count: number;
  open_deps: number;
  child_total: number;
  child_done: number;
}

interface ColumnData {
  id: number;
  title: string;
  role: string;
  color: string;
  wip_limit: number;
  cards: CardData[];
}

interface BoardMeta {
  id: number;
  title: string;
  description: string;
  workspace_dir: string;
}

interface BoardFull {
  meta: BoardMeta;
  columns: ColumnData[];
}

interface CommentData {
  id: number;
  author: string;
  body: string;
  kind: string;
  created_at: string;
}

interface LinkData {
  parent_id: number;
  child_id: number;
  parent_title: string;
  child_title: string;
  parent_done: boolean;
  child_done: boolean;
}

interface CardDetail {
  card: CardData & { board_id?: number };
  comments: CommentData[];
  links: LinkData[];
}

interface TemplateData {
  id: string;
  name: string;
  description: string;
  builtin: boolean;
  columns: { title: string; role: string; color: string; wip_limit: number }[];
}

interface Persona {
  name: string;
  description: string;
}

interface ActivityItem {
  card_id: number;
  card_title: string;
  author: string;
  body: string;
  kind: string;
  created_at: string;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const ROLE_COLORS: Record<string, string> = {
  triage: '#a855f7',
  todo: '#64748b',
  ready: '#0ea5e9',
  in_progress: '#3b82f6',
  blocked: '#ef4444',
  done: '#22c55e',
};

const PRIORITY_COLORS: Record<string, string> = {
  urgent: '#ef4444',
  high: '#f97316',
  medium: '#3b82f6',
  low: '#9ca3af',
};

const ROLE_OPTIONS = [
  { label: 'Triage', value: 'triage' },
  { label: 'To Do', value: 'todo' },
  { label: 'Ready', value: 'ready' },
  { label: 'In Progress', value: 'in_progress' },
  { label: 'Blocked', value: 'blocked' },
  { label: 'Done', value: 'done' },
];

const PRIORITY_OPTIONS = [
  { label: 'Urgent', value: 'urgent' },
  { label: 'High', value: 'high' },
  { label: 'Medium', value: 'medium' },
  { label: 'Low', value: 'low' },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function avatarColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  const hue = ((hash % 360) + 360) % 360;
  return `hsl(${hue}, 55%, 50%)`;
}

function parseLabels(labels: string): string[] {
  if (!labels) return [];
  try {
    const parsed = JSON.parse(labels);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return labels.split(',').map(l => l.trim()).filter(Boolean);
  }
}

async function api<T = unknown>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(text || `HTTP ${res.status}`);
  }
  return res.json();
}

function relativeTime(iso: string): string {
  const d = new Date(iso);
  const diff = Date.now() - d.getTime();
  if (diff < 60_000) return 'just now';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

function kindIcon(kind: string) {
  switch (kind) {
    case 'completion': return <CheckCircleOutlined style={{ color: ROLE_COLORS.done }} />;
    case 'block': return <StopOutlined style={{ color: ROLE_COLORS.blocked }} />;
    case 'unblock': return <UnlockOutlined style={{ color: ROLE_COLORS.ready }} />;
    case 'breakdown': return <ThunderboltOutlined style={{ color: '#f59e0b' }} />;
    default: return <CommentOutlined />;
  }
}

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

function AvatarChip({ name }: { name: string }) {
  if (!name) return null;
  return (
    <Tooltip title={name}>
      <Avatar size={20} style={{ backgroundColor: avatarColor(name), fontSize: 10 }}>
        {name.charAt(0).toUpperCase()}
      </Avatar>
    </Tooltip>
  );
}

function PriorityBadge({ priority }: { priority: string }) {
  if (!priority) return null;
  const color = PRIORITY_COLORS[priority] ?? '#9ca3af';
  return (
    <Tag color={color} style={{ margin: 0, fontSize: 11, lineHeight: '18px', padding: '0 4px' }}>
      {priority.charAt(0).toUpperCase() + priority.slice(1)}
    </Tag>
  );
}

// ---------------------------------------------------------------------------
// Board list view
// ---------------------------------------------------------------------------

function BoardListView({
  onSelect,
}: {
  onSelect: (id: number) => void;
}) {
  const { token } = theme.useToken();
  const [boards, setBoards] = useState<BoardSummary[]>([]);
  const [templates, setTemplates] = useState<TemplateData[]>([]);
  const [loading, setLoading] = useState(true);
  const [newOpen, setNewOpen] = useState(false);
  const [aiOpen, setAiOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [newForm] = Form.useForm();
  const [aiForm] = Form.useForm();

  const fetchBoards = useCallback(async () => {
    setLoading(true);
    try {
      const [b, t] = await Promise.all([
        api<BoardSummary[]>('/api/kanban/boards'),
        api<TemplateData[]>('/api/kanban/templates'),
      ]);
      setBoards(b);
      setTemplates(t);
    } catch (e: any) {
      message.error('Failed to load boards: ' + e.message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchBoards(); }, [fetchBoards]);

  const handleNewBoard = async () => {
    try {
      const vals = await newForm.validateFields();
      setCreating(true);
      const { id } = await api<{ id: number }>('/api/kanban/boards', {
        method: 'POST',
        body: JSON.stringify({
          title: vals.title,
          template_id: vals.template_id || undefined,
          workspace_dir: vals.workspace_dir || undefined,
          with_defaults: !vals.template_id,
        }),
      });
      message.success('Board created');
      setNewOpen(false);
      newForm.resetFields();
      onSelect(id);
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setCreating(false);
    }
  };

  const handleAiBoard = async () => {
    try {
      const vals = await aiForm.validateFields();
      setCreating(true);
      const { boardId } = await api<{ boardId: number }>('/api/kanban/generate', {
        method: 'POST',
        body: JSON.stringify({
          goal: vals.goal,
          template_id: vals.template_id || undefined,
          workspace_dir: vals.workspace_dir || undefined,
        }),
      });
      message.success('AI board generated');
      setAiOpen(false);
      aiForm.resetFields();
      onSelect(boardId);
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async (id: number, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await api('/api/kanban/board/delete', {
        method: 'POST',
        body: JSON.stringify({ id }),
      });
      message.success('Board deleted');
      fetchBoards();
    } catch (err: any) {
      message.error(err.message);
    }
  };

  return (
    <div className="p-6" style={{ maxWidth: 1200, margin: '0 auto' }}>
      <div className="flex items-center justify-between mb-6">
        <Title level={3} style={{ margin: 0 }}>
          <AppstoreOutlined className="mr-2" />
          Kanban Boards
        </Title>
        <Space>
          <Button icon={<PlusOutlined />} onClick={() => setNewOpen(true)}>
            New Board
          </Button>
          <Button
            type="primary"
            icon={<ThunderboltOutlined />}
            onClick={() => setAiOpen(true)}
          >
            AI Board
          </Button>
        </Space>
      </div>

      {loading ? (
        <div className="flex justify-center py-20"><Spin size="large" /></div>
      ) : boards.length === 0 ? (
        <Empty description="No boards yet" />
      ) : (
        <div className="grid gap-4" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))' }}>
          {boards.map(b => (
            <Card
              key={b.id}
              hoverable
              onClick={() => onSelect(b.id)}
              style={{ borderColor: token.colorBorderSecondary, cursor: 'pointer' }}
              styles={{ body: { padding: 16 } }}
            >
              <div className="flex items-start justify-between">
                <div className="flex-1 min-w-0">
                  <Text strong className="block truncate">{b.title}</Text>
                  {b.description && (
                    <Text type="secondary" className="block truncate text-xs mt-1">
                      {b.description}
                    </Text>
                  )}
                </div>
                <Tooltip title="Delete board">
                  <Button
                    type="text"
                    danger
                    size="small"
                    icon={<DeleteOutlined />}
                    onClick={(e) => handleDelete(b.id, e)}
                  />
                </Tooltip>
              </div>
              <div className="flex items-center gap-3 mt-3">
                <Tag>{b.column_count} columns</Tag>
                <Tag>{b.card_count} cards</Tag>
                {b.workspace_dir && (
                  <Tooltip title={b.workspace_dir}>
                    <FolderOpenOutlined style={{ color: token.colorTextSecondary }} />
                  </Tooltip>
                )}
              </div>
            </Card>
          ))}
        </div>
      )}

      {/* New Board Modal */}
      <Modal
        title="New Board"
        open={newOpen}
        onCancel={() => setNewOpen(false)}
        onOk={handleNewBoard}
        confirmLoading={creating}
        destroyOnClose
      >
        <Form form={newForm} layout="vertical" preserve={false}>
          <Form.Item name="title" label="Title" rules={[{ required: true, message: 'Enter a title' }]}>
            <Input placeholder="Board title" />
          </Form.Item>
          <Form.Item name="template_id" label="Template">
            <Select
              allowClear
              placeholder="Default columns"
              options={templates.map(t => ({ label: t.name, value: t.id }))}
            />
          </Form.Item>
          <Form.Item name="workspace_dir" label="Workspace directory">
            <Input placeholder="Optional path" />
          </Form.Item>
        </Form>
      </Modal>

      {/* AI Board Modal */}
      <Modal
        title={<><ThunderboltOutlined className="mr-2" />AI-Generated Board</>}
        open={aiOpen}
        onCancel={() => setAiOpen(false)}
        onOk={handleAiBoard}
        confirmLoading={creating}
        destroyOnClose
      >
        <Form form={aiForm} layout="vertical" preserve={false}>
          <Form.Item name="goal" label="Goal" rules={[{ required: true, message: 'Describe the goal' }]}>
            <TextArea rows={3} placeholder="Describe what you want to accomplish..." />
          </Form.Item>
          <Form.Item name="template_id" label="Template">
            <Select
              allowClear
              placeholder="AI generates columns"
              options={[
                { label: 'AI generates columns', value: '' },
                ...templates.map(t => ({ label: t.name, value: t.id })),
              ]}
            />
          </Form.Item>
          <Form.Item name="workspace_dir" label="Workspace directory">
            <Input placeholder="Optional path" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Board view (columns + cards)
// ---------------------------------------------------------------------------

function BoardView({
  boardId,
  onBack,
}: {
  boardId: number;
  onBack: () => void;
}) {
  const { token } = theme.useToken();
  const [board, setBoard] = useState<BoardFull | null>(null);
  const [loading, setLoading] = useState(true);
  const [personas, setPersonas] = useState<Persona[]>([]);
  const [templates, setTemplates] = useState<TemplateData[]>([]);
  const [activityOpen, setActivityOpen] = useState(false);
  const [activityRunning, setActivityRunning] = useState<CardData[]>([]);
  const [activityRecent, setActivityRecent] = useState<ActivityItem[]>([]);
  const [workerLanes, setWorkerLanes] = useState(false);
  const [assigneeFilter, setAssigneeFilter] = useState<string | null>(null);
  const [addColOpen, setAddColOpen] = useState(false);
  const [addColForm] = Form.useForm();
  const [cardDetailId, setCardDetailId] = useState<number | null>(null);
  const [addCardCol, setAddCardCol] = useState<number | null>(null);
  const [addCardTitle, setAddCardTitle] = useState('');
  const addCardRef = useRef<HTMLInputElement>(null);

  // Drag state
  const [dragCardId, setDragCardId] = useState<number | null>(null);
  const [dragOverCol, setDragOverCol] = useState<number | null>(null);

  const fetchBoard = useCallback(async () => {
    try {
      const data = await api<BoardFull>(`/api/kanban/board?id=${boardId}`);
      setBoard(data);
    } catch (e: any) {
      message.error('Failed to load board: ' + e.message);
    } finally {
      setLoading(false);
    }
  }, [boardId]);

  const fetchPersonas = useCallback(async () => {
    try {
      const data = await api<Persona[]>('/api/cowork/personas');
      setPersonas(data);
    } catch { /* personas optional */ }
  }, []);

  const fetchActivity = useCallback(async () => {
    try {
      const data = await api<{ running: CardData[]; recent: ActivityItem[] }>(
        `/api/kanban/activity?board_id=${boardId}`,
      );
      setActivityRunning(data.running ?? []);
      setActivityRecent(data.recent ?? []);
    } catch { /* optional */ }
  }, [boardId]);

  useEffect(() => {
    fetchBoard();
    fetchPersonas();
    api<TemplateData[]>('/api/kanban/templates').then(setTemplates).catch(() => {});
  }, [fetchBoard, fetchPersonas]);

  // WS live updates
  useEffect(() => {
    const port = window.location.port === '5173' ? 18789 : parseInt(window.location.port);
    const ws = new WebSocket(`ws://${window.location.hostname}:${port}/ws`);
    ws.onmessage = (e) => {
      try {
        const msg = JSON.parse(e.data);
        if (msg.type === 'kanban:update') {
          fetchBoard();
          if (activityOpen) fetchActivity();
        }
      } catch { /* ignore parse errors */ }
    };
    return () => ws.close();
  }, [boardId, fetchBoard, activityOpen, fetchActivity]);

  useEffect(() => {
    if (activityOpen) fetchActivity();
  }, [activityOpen, fetchActivity]);

  // All assignees for filter
  const allAssignees = useMemo(() => {
    if (!board) return [];
    const set = new Set<string>();
    board.columns.forEach(c => c.cards.forEach(card => {
      if (card.assignee) set.add(card.assignee);
    }));
    return Array.from(set).sort();
  }, [board]);

  // Filtered + optionally grouped columns
  const displayColumns = useMemo(() => {
    if (!board) return [];
    return board.columns.map(col => ({
      ...col,
      cards: col.cards.filter(card => {
        if (assigneeFilter && card.assignee !== assigneeFilter) return false;
        return true;
      }),
    }));
  }, [board, assigneeFilter]);

  // Worker-lanes view: group cards by assignee
  const workerLaneData = useMemo(() => {
    if (!board || !workerLanes) return new Map<string, Map<number, CardData[]>>();
    const lanes = new Map<string, Map<number, CardData[]>>();
    board.columns.forEach(col => {
      col.cards.forEach(card => {
        const who = card.assignee || '(unassigned)';
        if (!lanes.has(who)) lanes.set(who, new Map());
        const colMap = lanes.get(who)!;
        if (!colMap.has(col.id)) colMap.set(col.id, []);
        colMap.get(col.id)!.push(card);
      });
    });
    return lanes;
  }, [board, workerLanes]);

  // -- Actions --

  const handleAddColumn = async () => {
    try {
      const vals = await addColForm.validateFields();
      await api('/api/kanban/column/add', {
        method: 'POST',
        body: JSON.stringify({ board_id: boardId, title: vals.title, role: vals.role || undefined }),
      });
      message.success('Column added');
      setAddColOpen(false);
      addColForm.resetFields();
      fetchBoard();
    } catch (e: any) {
      if (e.message) message.error(e.message);
    }
  };

  const handleDeleteColumn = async (colId: number) => {
    try {
      await api('/api/kanban/column/delete', {
        method: 'POST',
        body: JSON.stringify({ id: colId }),
      });
      message.success('Column deleted');
      fetchBoard();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const handleAddCard = async (columnId: number) => {
    if (!addCardTitle.trim()) return;
    try {
      await api('/api/kanban/card/add', {
        method: 'POST',
        body: JSON.stringify({ column_id: columnId, title: addCardTitle.trim() }),
      });
      setAddCardTitle('');
      setAddCardCol(null);
      fetchBoard();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const handleDrop = async (targetColId: number) => {
    if (dragCardId === null) return;
    setDragOverCol(null);
    setDragCardId(null);
    try {
      await api('/api/kanban/card/move', {
        method: 'POST',
        body: JSON.stringify({ id: dragCardId, column_id: targetColId }),
      });
      fetchBoard();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const handleRenameBoard = async () => {
    if (!board) return;
    const title = prompt('Board title:', board.meta.title);
    if (title === null) return;
    const description = prompt('Description:', board.meta.description || '');
    try {
      await api('/api/kanban/board/rename', {
        method: 'POST',
        body: JSON.stringify({ id: boardId, title, description: description || '' }),
      });
      fetchBoard();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  if (loading) {
    return <div className="flex justify-center items-center h-full"><Spin size="large" /></div>;
  }

  if (!board) {
    return <Empty description="Board not found" />;
  }

  const renderCard = (card: CardData) => {
    const labels = parseLabels(card.labels);
    return (
      <div
        key={card.id}
        draggable
        onDragStart={(e) => {
          setDragCardId(card.id);
          e.dataTransfer.effectAllowed = 'move';
        }}
        onDragEnd={() => { setDragCardId(null); setDragOverCol(null); }}
        onClick={() => setCardDetailId(card.id)}
        className="rounded-lg p-3 mb-2 cursor-pointer transition-shadow"
        style={{
          background: token.colorBgContainer,
          border: `1px solid ${token.colorBorderSecondary}`,
          opacity: dragCardId === card.id ? 0.5 : 1,
          boxShadow: token.boxShadowTertiary,
        }}
      >
        <div className="flex items-start justify-between gap-1">
          <Text
            className="flex-1 text-sm leading-snug"
            style={{ textDecoration: card.done ? 'line-through' : undefined }}
          >
            {card.done && <CheckCircleOutlined style={{ color: ROLE_COLORS.done, marginRight: 4 }} />}
            {card.title}
          </Text>
          {card.priority && <PriorityBadge priority={card.priority} />}
        </div>

        {labels.length > 0 && (
          <div className="flex flex-wrap gap-1 mt-1.5">
            {labels.map((l, i) => (
              <Tag key={i} style={{ margin: 0, fontSize: 10, lineHeight: '16px', padding: '0 4px' }}>{l}</Tag>
            ))}
          </div>
        )}

        <div className="flex items-center gap-2 mt-2">
          {card.assignee && <AvatarChip name={card.assignee} />}
          {card.comment_count > 0 && (
            <Tooltip title={`${card.comment_count} comments`}>
              <span className="text-xs" style={{ color: token.colorTextSecondary }}>
                <CommentOutlined /> {card.comment_count}
              </span>
            </Tooltip>
          )}
          {card.open_deps > 0 && (
            <Tooltip title={`${card.open_deps} open dependencies`}>
              <span className="text-xs" style={{ color: ROLE_COLORS.blocked }}>
                <LinkOutlined /> {card.open_deps}
              </span>
            </Tooltip>
          )}
          {card.child_total > 0 && (
            <Tooltip title={`Subtasks: ${card.child_done}/${card.child_total}`}>
              <span className="text-xs" style={{ color: token.colorTextSecondary }}>
                {card.child_done}/{card.child_total}
              </span>
            </Tooltip>
          )}
        </div>
      </div>
    );
  };

  const renderColumn = (col: ColumnData) => {
    const roleColor = ROLE_COLORS[col.role] || token.colorPrimary;
    const isOver = dragOverCol === col.id;
    return (
      <div
        key={col.id}
        className="flex flex-col rounded-xl shrink-0"
        style={{
          width: 300,
          minHeight: 200,
          background: isOver
            ? token.colorBgTextHover
            : token.colorBgLayout,
          border: isOver
            ? `2px dashed ${roleColor}`
            : `1px solid ${token.colorBorderSecondary}`,
          transition: 'background 0.15s, border 0.15s',
        }}
        onDragOver={(e) => { e.preventDefault(); setDragOverCol(col.id); }}
        onDragLeave={() => setDragOverCol(null)}
        onDrop={(e) => { e.preventDefault(); handleDrop(col.id); }}
      >
        {/* Column header */}
        <div className="flex items-center justify-between px-3 py-2">
          <div className="flex items-center gap-2">
            <div
              className="rounded-full"
              style={{ width: 8, height: 8, background: roleColor }}
            />
            <Text strong className="text-sm">{col.title}</Text>
            <Badge
              count={col.cards.length}
              showZero
              style={{ backgroundColor: token.colorTextQuaternary }}
              size="small"
            />
            {col.wip_limit > 0 && col.cards.length > col.wip_limit && (
              <Tooltip title={`WIP limit: ${col.wip_limit}`}>
                <ExclamationCircleOutlined style={{ color: ROLE_COLORS.blocked, fontSize: 12 }} />
              </Tooltip>
            )}
          </div>
          <Dropdown
            menu={{
              items: [
                { key: 'delete', label: 'Delete column', danger: true, icon: <DeleteOutlined /> },
              ],
              onClick: ({ key }) => {
                if (key === 'delete') handleDeleteColumn(col.id);
              },
            }}
            trigger={['click']}
          >
            <Button type="text" size="small" icon={<MoreOutlined />} />
          </Dropdown>
        </div>

        <Divider style={{ margin: 0 }} />

        {/* Cards */}
        <div className="flex-1 p-2 overflow-y-auto" style={{ maxHeight: 'calc(100vh - 240px)' }}>
          {col.cards.map(renderCard)}

          {/* Add card inline */}
          {addCardCol === col.id ? (
            <div className="mt-1">
              <Input
                ref={addCardRef as any}
                size="small"
                placeholder="Card title"
                value={addCardTitle}
                onChange={e => setAddCardTitle(e.target.value)}
                onPressEnter={() => handleAddCard(col.id)}
                onBlur={() => { if (!addCardTitle.trim()) setAddCardCol(null); }}
                autoFocus
              />
              <div className="flex gap-1 mt-1">
                <Button size="small" type="primary" onClick={() => handleAddCard(col.id)}>
                  Add
                </Button>
                <Button size="small" onClick={() => { setAddCardCol(null); setAddCardTitle(''); }}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <Button
              type="dashed"
              size="small"
              block
              icon={<PlusOutlined />}
              onClick={() => { setAddCardCol(col.id); setAddCardTitle(''); }}
              className="mt-1"
            >
              Add card
            </Button>
          )}
        </div>
      </div>
    );
  };

  return (
    <div className="flex flex-col h-full">
      {/* Board header */}
      <div
        className="flex items-center justify-between px-4 py-2 shrink-0"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}
      >
        <div className="flex items-center gap-3">
          <Button type="text" icon={<ArrowLeftOutlined />} onClick={onBack} />
          <div>
            <div className="flex items-center gap-2">
              <Title level={4} style={{ margin: 0 }}>{board.meta.title}</Title>
              <Button type="text" size="small" icon={<EditOutlined />} onClick={handleRenameBoard} />
            </div>
            {board.meta.description && (
              <Text type="secondary" className="text-xs">{board.meta.description}</Text>
            )}
          </div>
          {board.meta.workspace_dir && (
            <Tooltip title={board.meta.workspace_dir}>
              <Tag icon={<FolderOpenOutlined />} className="ml-2">{board.meta.workspace_dir.split('/').pop()}</Tag>
            </Tooltip>
          )}
        </div>

        <Space>
          <Tooltip title="Worker lanes view">
            <Button
              type={workerLanes ? 'primary' : 'default'}
              size="small"
              icon={<ColumnWidthOutlined />}
              onClick={() => setWorkerLanes(v => !v)}
            />
          </Tooltip>
          <Select
            allowClear
            placeholder="Filter assignee"
            size="small"
            style={{ minWidth: 140 }}
            value={assigneeFilter}
            onChange={v => setAssigneeFilter(v ?? null)}
            options={allAssignees.map(a => ({ label: a, value: a }))}
            suffixIcon={<FilterOutlined />}
          />
          <Button size="small" icon={<PlusOutlined />} onClick={() => setAddColOpen(true)}>
            Add column
          </Button>
          <Button
            size="small"
            icon={<MenuOutlined />}
            onClick={() => setActivityOpen(true)}
          >
            Activity
          </Button>
          <Button size="small" icon={<ReloadOutlined />} onClick={() => { setLoading(true); fetchBoard(); }} />
        </Space>
      </div>

      {/* Columns area */}
      {workerLanes ? (
        <div className="flex-1 overflow-auto p-4">
          {workerLaneData.size === 0 ? (
            <Empty description="No cards" />
          ) : (
            Array.from(workerLaneData.entries()).map(([who, colMap]) => (
              <div key={who} className="mb-4">
                <div className="flex items-center gap-2 mb-2">
                  <AvatarChip name={who} />
                  <Text strong>{who}</Text>
                </div>
                <div className="flex gap-3 overflow-x-auto pb-2">
                  {board.columns.map(col => {
                    const cards = colMap.get(col.id) ?? [];
                    if (cards.length === 0) return null;
                    const roleColor = ROLE_COLORS[col.role] || token.colorPrimary;
                    return (
                      <div key={col.id} className="shrink-0" style={{ width: 260 }}>
                        <div className="flex items-center gap-1 mb-1">
                          <div className="rounded-full" style={{ width: 6, height: 6, background: roleColor }} />
                          <Text type="secondary" className="text-xs">{col.title}</Text>
                        </div>
                        {cards.map(renderCard)}
                      </div>
                    );
                  })}
                </div>
                <Divider style={{ margin: '8px 0' }} />
              </div>
            ))
          )}
        </div>
      ) : (
        <div className="flex-1 overflow-x-auto p-4">
          <div className="flex gap-3" style={{ minWidth: 'max-content' }}>
            {displayColumns.map(renderColumn)}
          </div>
        </div>
      )}

      {/* Add column modal */}
      <Modal
        title="Add Column"
        open={addColOpen}
        onCancel={() => setAddColOpen(false)}
        onOk={handleAddColumn}
        destroyOnClose
      >
        <Form form={addColForm} layout="vertical" preserve={false}>
          <Form.Item name="title" label="Title" rules={[{ required: true }]}>
            <Input placeholder="Column title" />
          </Form.Item>
          <Form.Item name="role" label="Role">
            <Select allowClear placeholder="Optional" options={ROLE_OPTIONS} />
          </Form.Item>
        </Form>
      </Modal>

      {/* Card detail modal */}
      {cardDetailId !== null && (
        <CardDetailModal
          cardId={cardDetailId}
          personas={personas}
          onClose={() => { setCardDetailId(null); fetchBoard(); }}
        />
      )}

      {/* Activity drawer */}
      <Drawer
        title="Activity"
        open={activityOpen}
        onClose={() => setActivityOpen(false)}
        width={380}
      >
        {activityRunning.length > 0 && (
          <>
            <Text strong className="block mb-2">
              <SyncOutlined spin className="mr-1" /> Running
            </Text>
            {activityRunning.map(card => (
              <div
                key={card.id}
                className="flex items-center gap-2 mb-2 p-2 rounded"
                style={{ background: token.colorBgLayout }}
              >
                <LoadingOutlined style={{ color: ROLE_COLORS.in_progress }} />
                <Text className="text-sm flex-1 truncate">{card.title}</Text>
                {card.assignee && <AvatarChip name={card.assignee} />}
              </div>
            ))}
            <Divider />
          </>
        )}

        <Text strong className="block mb-2">
          <ClockCircleOutlined className="mr-1" /> Recent
        </Text>
        {activityRecent.length === 0 ? (
          <Empty description="No activity yet" />
        ) : (
          activityRecent.map((item, i) => (
            <div key={i} className="flex gap-2 mb-3">
              <div className="mt-0.5">{kindIcon(item.kind)}</div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-1">
                  <Text strong className="text-xs">{item.author || 'System'}</Text>
                  <Text type="secondary" className="text-xs">{relativeTime(item.created_at)}</Text>
                </div>
                <Text className="text-sm block truncate">{item.card_title}</Text>
                <Text type="secondary" className="text-xs block" ellipsis>{item.body}</Text>
              </div>
            </div>
          ))
        )}
      </Drawer>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Card detail modal
// ---------------------------------------------------------------------------

function CardDetailModal({
  cardId,
  personas,
  onClose,
}: {
  cardId: number;
  personas: Persona[];
  onClose: () => void;
}) {
  const { token } = theme.useToken();
  const [detail, setDetail] = useState<CardDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState('');
  const [assignee, setAssignee] = useState('');
  const [labelsStr, setLabelsStr] = useState('');
  const [editingTitle, setEditingTitle] = useState(false);
  const [editingDesc, setEditingDesc] = useState(false);
  const [commentBody, setCommentBody] = useState('');
  const [actionLoading, setActionLoading] = useState(false);

  const fetchDetail = useCallback(async () => {
    try {
      const data = await api<CardDetail>(`/api/kanban/card?id=${cardId}`);
      setDetail(data);
      setTitle(data.card.title);
      setDescription(data.card.description || '');
      setPriority(data.card.priority || '');
      setAssignee(data.card.assignee || '');
      setLabelsStr(
        (() => {
          const labels = parseLabels(data.card.labels);
          return labels.join(', ');
        })(),
      );
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setLoading(false);
    }
  }, [cardId]);

  useEffect(() => { fetchDetail(); }, [fetchDetail]);

  const saveField = async (fields: Record<string, unknown>) => {
    setSaving(true);
    try {
      await api('/api/kanban/card/update', {
        method: 'POST',
        body: JSON.stringify({ id: cardId, ...fields }),
      });
      fetchDetail();
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setSaving(false);
    }
  };

  const handleComplete = async () => {
    const summary = prompt('Completion summary:');
    if (summary === null) return;
    setActionLoading(true);
    try {
      await api('/api/kanban/card/complete', {
        method: 'POST',
        body: JSON.stringify({ card_id: cardId, summary }),
      });
      message.success('Card completed');
      fetchDetail();
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setActionLoading(false);
    }
  };

  const handleBlock = async () => {
    const reason = prompt('Block reason:');
    if (reason === null) return;
    setActionLoading(true);
    try {
      await api('/api/kanban/card/block', {
        method: 'POST',
        body: JSON.stringify({ card_id: cardId, reason }),
      });
      message.success('Card blocked');
      fetchDetail();
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setActionLoading(false);
    }
  };

  const handleUnblock = async () => {
    setActionLoading(true);
    try {
      await api('/api/kanban/card/unblock', {
        method: 'POST',
        body: JSON.stringify({ card_id: cardId }),
      });
      message.success('Card unblocked');
      fetchDetail();
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setActionLoading(false);
    }
  };

  const handleBreakdown = async () => {
    setActionLoading(true);
    try {
      await api('/api/kanban/breakdown', {
        method: 'POST',
        body: JSON.stringify({ card_id: cardId }),
      });
      message.success('AI breakdown started');
      fetchDetail();
    } catch (e: any) {
      message.error(e.message);
    } finally {
      setActionLoading(false);
    }
  };

  const handleDelete = async () => {
    try {
      await api('/api/kanban/card/delete', {
        method: 'POST',
        body: JSON.stringify({ id: cardId }),
      });
      message.success('Card deleted');
      onClose();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  const handleAddComment = async () => {
    if (!commentBody.trim()) return;
    try {
      await api('/api/kanban/card/comment', {
        method: 'POST',
        body: JSON.stringify({ card_id: cardId, body: commentBody, author: 'user' }),
      });
      setCommentBody('');
      fetchDetail();
    } catch (e: any) {
      message.error(e.message);
    }
  };

  if (loading) {
    return (
      <Modal open onCancel={onClose} footer={null} width={640}>
        <div className="flex justify-center py-10"><Spin /></div>
      </Modal>
    );
  }

  if (!detail) {
    return (
      <Modal open onCancel={onClose} footer={null} width={640}>
        <Empty description="Card not found" />
      </Modal>
    );
  }

  const card = detail.card;
  const isBlocked = card.description?.includes('[BLOCKED]') || false;

  return (
    <Modal
      open
      onCancel={onClose}
      footer={null}
      width={680}
      styles={{ body: { maxHeight: '75vh', overflowY: 'auto' } }}
    >
      {/* Title */}
      <div className="mb-4">
        {editingTitle ? (
          <Input
            value={title}
            onChange={e => setTitle(e.target.value)}
            onBlur={() => { setEditingTitle(false); if (title !== card.title) saveField({ title }); }}
            onPressEnter={() => { setEditingTitle(false); if (title !== card.title) saveField({ title }); }}
            autoFocus
            size="large"
          />
        ) : (
          <div
            className="flex items-center gap-2 cursor-pointer"
            onClick={() => setEditingTitle(true)}
          >
            <Title level={4} style={{ margin: 0, textDecoration: card.done ? 'line-through' : undefined }}>
              {card.done && <CheckCircleOutlined style={{ color: ROLE_COLORS.done, marginRight: 8 }} />}
              {title}
            </Title>
            <EditOutlined style={{ color: token.colorTextSecondary }} />
          </div>
        )}
      </div>

      {/* Fields */}
      <div className="grid grid-cols-2 gap-3 mb-4">
        <div>
          <Text type="secondary" className="text-xs block mb-1">Priority</Text>
          <Select
            value={priority || undefined}
            onChange={v => { setPriority(v); saveField({ priority: v }); }}
            allowClear
            placeholder="None"
            options={PRIORITY_OPTIONS}
            className="w-full"
            size="small"
          />
        </div>
        <div>
          <Text type="secondary" className="text-xs block mb-1">Assignee</Text>
          <Select
            value={assignee || undefined}
            onChange={v => { setAssignee(v || ''); saveField({ assignee: v || '' }); }}
            allowClear
            placeholder="Unassigned"
            className="w-full"
            size="small"
            options={[
              ...personas.map(p => ({ label: p.name, value: p.name })),
            ]}
          />
        </div>
        <div className="col-span-2">
          <Text type="secondary" className="text-xs block mb-1">Labels (comma-separated)</Text>
          <Input
            value={labelsStr}
            onChange={e => setLabelsStr(e.target.value)}
            onBlur={() => {
              const arr = labelsStr.split(',').map(l => l.trim()).filter(Boolean);
              saveField({ labels: JSON.stringify(arr) });
            }}
            size="small"
            placeholder="e.g. frontend, urgent"
          />
        </div>
      </div>

      {/* Description */}
      <div className="mb-4">
        <Text type="secondary" className="text-xs block mb-1">Description</Text>
        {editingDesc ? (
          <div>
            <TextArea
              value={description}
              onChange={e => setDescription(e.target.value)}
              rows={4}
              autoFocus
            />
            <div className="flex gap-1 mt-1">
              <Button
                size="small"
                type="primary"
                loading={saving}
                onClick={() => { setEditingDesc(false); saveField({ description }); }}
              >
                Save
              </Button>
              <Button size="small" onClick={() => { setEditingDesc(false); setDescription(card.description || ''); }}>
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <div
            className="p-2 rounded cursor-pointer min-h-[40px]"
            style={{ background: token.colorBgLayout, border: `1px solid ${token.colorBorderSecondary}` }}
            onClick={() => setEditingDesc(true)}
          >
            {description ? (
              <Text className="text-sm whitespace-pre-wrap">{description}</Text>
            ) : (
              <Text type="secondary" className="text-sm">Click to add description...</Text>
            )}
          </div>
        )}
      </div>

      <Divider style={{ margin: '12px 0' }} />

      {/* Actions */}
      <div className="flex flex-wrap gap-2 mb-4">
        <Button
          icon={<CheckCircleOutlined />}
          onClick={handleComplete}
          loading={actionLoading}
          disabled={card.done}
        >
          Complete
        </Button>
        {isBlocked ? (
          <Button icon={<UnlockOutlined />} onClick={handleUnblock} loading={actionLoading}>
            Unblock
          </Button>
        ) : (
          <Button icon={<StopOutlined />} onClick={handleBlock} loading={actionLoading} danger>
            Block
          </Button>
        )}
        <Button icon={<ThunderboltOutlined />} onClick={handleBreakdown} loading={actionLoading}>
          AI Breakdown
        </Button>
        <Button icon={<DeleteOutlined />} onClick={handleDelete} danger>
          Delete
        </Button>
      </div>

      {/* Dependencies */}
      {detail.links.length > 0 && (
        <div className="mb-4">
          <Text strong className="text-sm block mb-2">
            <LinkOutlined className="mr-1" /> Dependencies
          </Text>
          {detail.links.map((link, i) => (
            <div key={i} className="flex items-center gap-2 mb-1 text-sm">
              {link.parent_done ? (
                <CheckCircleOutlined style={{ color: ROLE_COLORS.done }} />
              ) : (
                <BlockOutlined style={{ color: token.colorTextSecondary }} />
              )}
              <Text>{link.parent_id === cardId ? link.child_title : link.parent_title}</Text>
              <Tag className="text-xs">
                {link.parent_id === cardId ? 'child' : 'parent'}
              </Tag>
            </div>
          ))}
        </div>
      )}

      <Divider style={{ margin: '12px 0' }} />

      {/* Comments */}
      <Text strong className="text-sm block mb-2">
        <CommentOutlined className="mr-1" /> Comments ({detail.comments.length})
      </Text>

      {detail.comments.map(c => (
        <div key={c.id} className="flex gap-2 mb-3">
          <AvatarChip name={c.author || 'system'} />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2">
              <Text strong className="text-xs">{c.author || 'system'}</Text>
              {c.kind && c.kind !== 'comment' && (
                <Tag style={{ margin: 0, fontSize: 10, lineHeight: '16px', padding: '0 3px' }}>
                  {c.kind}
                </Tag>
              )}
              <Text type="secondary" className="text-xs">{relativeTime(c.created_at)}</Text>
            </div>
            <Text className="text-sm whitespace-pre-wrap">{c.body}</Text>
          </div>
        </div>
      ))}

      <div className="flex gap-2 mt-2">
        <Input
          placeholder="Add a comment..."
          value={commentBody}
          onChange={e => setCommentBody(e.target.value)}
          onPressEnter={handleAddComment}
        />
        <Button type="primary" onClick={handleAddComment} disabled={!commentBody.trim()}>
          Send
        </Button>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Main export
// ---------------------------------------------------------------------------

export function KanbanPage() {
  const [boardId, setBoardId] = useState<number | null>(null);

  return (
    <AppLayout sidebar={null}>
      <Content className="h-full overflow-hidden">
        {boardId === null ? (
          <BoardListView onSelect={setBoardId} />
        ) : (
          <BoardView boardId={boardId} onBack={() => setBoardId(null)} />
        )}
      </Content>
    </AppLayout>
  );
}
