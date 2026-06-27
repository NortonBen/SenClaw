import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Layout, theme, Empty, Button, Modal, Form, Input, Select, message,
  Card, Tag, Popconfirm, Typography, Tabs, Dropdown, AutoComplete, Tooltip,
} from 'antd';
import {
  TeamOutlined, UserOutlined, FolderOpenOutlined, ArrowLeftOutlined,
  PlusOutlined, DeleteOutlined, EditOutlined, MoreOutlined, MessageOutlined,
  FileTextOutlined,
} from '@ant-design/icons';
import { useNavigate, useParams } from 'react-router-dom';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { SessionList } from '../components/Sidebar';
import { MarkdownBody } from '../components/shared/MarkdownBody';
import {
  TriggerEditor, parseTriggerJson, stringifyTriggers, summarizeTriggers,
  type TriggerRule,
} from '../components/cowork/TriggerEditor';

const { Content } = Layout;
const { Title, Text } = Typography;

interface TeamMember {
  folder: string;
  role?: string;
  responsibilities?: string;
  triggers?: string;
  handoff_rules?: string;
  acceptance_criteria?: string;
  output_format?: string;
  sla?: string;
  limits?: string;
}

interface CoworkTeam {
  id: string;
  name: string;
  manager_folder: string;
  members: TeamMember[];
  workspace_dir: string | null;
  created_at: string;
  jid: string;
}

interface CoworkTeamTask {
  id: string;
  team_id: string;
  title: string;
  description?: string;
  status: string;
  assignee?: string;
  reviewer?: string;
  priority: string;
  depends_on: string[];
  result_output?: string;
  created_at: string;
  updated_at: string;
  due_at?: string;
  completed_at?: string;
}

const PINNED_KEY = 'senclaw:pinned-jids';
function loadPinned(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(PINNED_KEY) ?? '[]')); } catch { return new Set(); }
}

const KANBAN_COLS: CoworkTeamTask['status'][] = ['backlog', 'todo', 'in_progress', 'review', 'done', 'blocked'];
const STATUS_LABEL: Record<string, string> = {
  backlog: 'Backlog', todo: 'Todo', in_progress: 'In progress',
  review: 'Review', done: 'Done', blocked: 'Blocked',
};
const STATUS_COLOR: Record<string, string> = {
  backlog: 'default', todo: 'blue', in_progress: 'processing',
  review: 'purple', done: 'success', blocked: 'error',
};
const PRIORITY_COLOR: Record<string, string> = {
  low: 'default', medium: 'blue', high: 'orange', critical: 'red',
};

export function CoworkTeamDetailPage() {
  const { id: teamId } = useParams<{ id: string }>();
  const { ws } = useAppContext();
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [pinnedJids] = useState<Set<string>>(loadPinned);

  const [team, setTeam] = useState<CoworkTeam | null>(null);
  const [tasks, setTasks] = useState<CoworkTeamTask[]>([]);
  const [loading, setLoading] = useState(true);

  // Task modal state
  const [taskModalOpen, setTaskModalOpen] = useState(false);
  const [editingTask, setEditingTask] = useState<CoworkTeamTask | null>(null);
  const [taskForm] = Form.useForm();
  // Read-only "view result" popup — opened by the FileTextOutlined button
  // on cards that have a `result_output`. Separate from the edit modal so
  // the agent's reply gets a clean markdown render without form chrome.
  const [resultViewTask, setResultViewTask] = useState<CoworkTeamTask | null>(null);

  // Member modal state
  const [memberModalOpen, setMemberModalOpen] = useState(false);
  const [editingMember, setEditingMember] = useState<TeamMember | null>(null);
  const [memberForm] = Form.useForm();
  const [editingTriggers, setEditingTriggers] = useState<TriggerRule[]>([]);
  // Member's SOUL.md (persona file) — loaded on open, saved on submit.
  // Lives on disk at agents/<folder>/SOUL.md; persisted via the existing
  // /api/agents/:folder/files endpoint so the same edit flow as Profile
  // settings is reused here.
  const [editingSoul, setEditingSoul] = useState<string>('');
  const [soulLoading, setSoulLoading] = useState(false);
  const [soulOriginal, setSoulOriginal] = useState<string>('');
  const [personaPool, setPersonaPool] = useState<{ name: string; description: string }[]>([]);

  useEffect(() => {
    fetch('/api/cowork/personas')
      .then(r => r.ok ? r.json() : [])
      .then(setPersonaPool)
      .catch(() => setPersonaPool([]));
  }, []);

  const profiles = useMemo(
    () => ws.agents.filter(a => !a.folder.startsWith('schedule_')),
    [ws.agents]
  );
  const coworkGroups = ws.groups.filter(g => g.groupType === 'cowork');

  const loadTeam = useCallback(async () => {
    if (!teamId) return;
    setLoading(true);
    try {
      const teamsRes = await fetch('/api/cowork/teams');
      if (!teamsRes.ok) throw new Error(await teamsRes.text());
      const all: CoworkTeam[] = await teamsRes.json();
      const t = all.find(x => x.id === teamId);
      if (!t) { message.error('Team not found'); navigate('/cowork'); return; }
      setTeam(t);

      const tasksRes = await fetch(`/api/cowork/teams/${teamId}/tasks`);
      if (tasksRes.ok) setTasks(await tasksRes.json());
    } catch (e) {
      message.error(`Load failed: ${String((e as Error)?.message ?? e)}`);
    } finally {
      setLoading(false);
    }
  }, [teamId, navigate]);

  useEffect(() => { loadTeam(); }, [loadTeam]);

  // ─── Task CRUD ──────────────────────────────────────────

  const openCreateTask = () => {
    setEditingTask(null);
    taskForm.resetFields();
    taskForm.setFieldsValue({ status: 'todo', priority: 'medium' });
    setTaskModalOpen(true);
  };

  const openEditTask = (task: CoworkTeamTask) => {
    setEditingTask(task);
    taskForm.setFieldsValue({
      title: task.title,
      description: task.description ?? '',
      status: task.status,
      assignee: task.assignee ?? '',
      reviewer: task.reviewer ?? '',
      priority: task.priority,
      result_output: task.result_output ?? '',
    });
    setTaskModalOpen(true);
  };

  const handleSaveTask = async (values: any) => {
    if (!teamId) return;
    try {
      const body = {
        title: values.title,
        description: values.description || null,
        status: values.status,
        assignee: values.assignee || null,
        reviewer: values.reviewer || null,
        priority: values.priority,
        result_output: values.result_output || null,
      };
      if (editingTask) {
        const res = await fetch(`/api/cowork/teams/${teamId}/tasks/${editingTask.id}`, {
          method: 'PATCH',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        if (!res.ok) throw new Error(await res.text());
        message.success('Task updated');
      } else {
        const res = await fetch(`/api/cowork/teams/${teamId}/tasks`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        });
        if (!res.ok) throw new Error(await res.text());
        message.success('Task created');
      }
      setTaskModalOpen(false);
      await loadTeam();
    } catch (e) {
      message.error(`Save failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const updateTaskStatus = async (task: CoworkTeamTask, status: string) => {
    if (!teamId) return;
    try {
      await fetch(`/api/cowork/teams/${teamId}/tasks/${task.id}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status }),
      });
      await loadTeam();
    } catch {}
  };

  const handleDeleteTask = async (task: CoworkTeamTask) => {
    if (!teamId) return;
    try {
      await fetch(`/api/cowork/teams/${teamId}/tasks/${task.id}`, { method: 'DELETE' });
      message.success('Task deleted');
      await loadTeam();
    } catch (e) {
      message.error(`Delete failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const tasksByStatus = (col: string) =>
    tasks.filter(t => t.status === col).sort((a, b) => a.created_at.localeCompare(b.created_at));

  // ─── Member CRUD ────────────────────────────────────────

  // Derive a persona-file slug from a free-form display name. Match the
  // legacy cowork helper: lowercase, strip accents, hyphenate spaces, drop
  // anything else. Limits collisions without forcing the user to think
  // about file naming.
  const slugifyName = (name: string): string => {
    return name
      .normalize('NFKD')
      .replace(/[̀-ͯ]/g, '')   // strip combining marks
      .toLowerCase()
      .replace(/[^a-z0-9\s-]/g, '')
      .trim()
      .replace(/\s+/g, '-')
      .replace(/-+/g, '-')
      .slice(0, 48);
  };

  // Members live as standalone persona files under `virtual-agents/`, NOT
  // as Profile/agent rows. This keeps the cowork member pool separate from
  // user-facing Profiles. Reads/writes go through /api/cowork/personas/:name/file.
  const loadMemberSoul = async (slug: string) => {
    setSoulLoading(true);
    try {
      const res = await fetch(`/api/cowork/personas/${encodeURIComponent(slug)}/file`);
      if (!res.ok) throw new Error(await res.text());
      const data: { content: string; exists: boolean } = await res.json();
      setEditingSoul(data.content ?? '');
      setSoulOriginal(data.content ?? '');
    } catch {
      setEditingSoul('');
      setSoulOriginal('');
    } finally {
      setSoulLoading(false);
    }
  };

  const openCreateMember = () => {
    setEditingMember(null);
    memberForm.resetFields();
    setEditingTriggers([]);
    setEditingSoul('');
    setSoulOriginal('');
    setMemberModalOpen(true);
  };

  const openEditMember = (m: TeamMember) => {
    setEditingMember(m);
    setEditingTriggers(parseTriggerJson(m.triggers));
    memberForm.setFieldsValue({
      folder: m.folder,
      role: m.role ?? '',
      responsibilities: m.responsibilities ?? '',
      handoff_rules: m.handoff_rules ?? '',
      acceptance_criteria: m.acceptance_criteria ?? '',
      output_format: m.output_format ?? '',
      sla: m.sla ?? '',
      limits: m.limits ?? '',
    });
    setMemberModalOpen(true);
    // Fire SOUL.md fetch in parallel; don't block modal show.
    void loadMemberSoul(m.folder);
  };

  const handleSaveMember = async (values: any) => {
    if (!teamId) return;
    try {
      // On create we derive the slug from the display name; on edit we
      // keep the original folder so file paths stay stable.
      const folder = editingMember
        ? editingMember.folder
        : slugifyName(values.name ?? '');
      if (!folder) { message.error('name required'); return; }
      const res = await fetch(`/api/cowork/teams/${teamId}/members`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          folder,
          role: values.role || null,
          responsibilities: values.responsibilities || null,
          triggers: stringifyTriggers(editingTriggers),
          handoff_rules: values.handoff_rules || null,
          acceptance_criteria: values.acceptance_criteria || null,
          output_format: values.output_format || null,
          sla: values.sla || null,
          limits: values.limits || null,
        }),
      });
      if (!res.ok) throw new Error(await res.text());

      // Persist persona content if user edited it. Writes to the cowork
      // persona dir (independent of user Profiles — touches virtual-agents
      // only, never the agents DB table).
      if (editingSoul !== soulOriginal) {
        try {
          await fetch(`/api/cowork/personas/${encodeURIComponent(folder)}/file`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ content: editingSoul }),
          });
        } catch { /* non-fatal — team save already succeeded */ }
      }

      message.success(editingMember ? 'Member updated · persona saved if changed' : 'Member added');
      setMemberModalOpen(false);
      await loadTeam();
    } catch (e) {
      message.error(`Save failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const handleRemoveMember = async (m: TeamMember) => {
    if (!teamId) return;
    try {
      await fetch(
        `/api/cowork/teams/${teamId}/members/${encodeURIComponent(m.folder)}`,
        { method: 'DELETE' }
      );
      message.success(`Removed ${m.folder}`);
      await loadTeam();
    } catch (e) {
      message.error(`Remove failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  // ─── Render ─────────────────────────────────────────────

  if (!team && !loading) return null;

  return (
    <AppLayout
      sidebar={
        <SessionList
          groups={coworkGroups}
          selectedJid={null}
          agentStates={ws.agentStates}
          pinnedJids={pinnedJids}
          onSelect={(jid) => navigate(`/chat/${encodeURIComponent(jid)}`)}
          onNewChat={() => navigate('/cowork')}
          onPin={() => { /* no-op */ }}
          onRename={(jid, name) => ws.updateGroup(jid, { name })}
          onDelete={() => { /* delete via team detail's own UI */ }}
          onReload={ws.refreshGroups}
        />
      }
    >
      <Layout style={{ background: 'transparent', height: '100%' }}>
        <Content className="overflow-y-auto p-6">
          <div className="max-w-5xl mx-auto">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-3">
                <Button
                  type="text"
                  icon={<ArrowLeftOutlined />}
                  onClick={() => navigate('/cowork')}
                />
                <div>
                  <Title level={3} style={{ margin: 0 }}>
                    <TeamOutlined style={{ marginRight: 8, color: token.colorPrimary }} />
                    {team?.name ?? '…'}
                  </Title>
                  <div className="flex items-center gap-3 mt-1" style={{ color: token.colorTextSecondary, fontSize: 12 }}>
                    <span><UserOutlined /> Manager: <code>{team?.manager_folder}</code></span>
                    {team?.workspace_dir && (
                      <span><FolderOpenOutlined /> <code>{team.workspace_dir}</code></span>
                    )}
                  </div>
                </div>
              </div>
              <Button
                type="primary"
                icon={<MessageOutlined />}
                onClick={() => team && navigate(`/chat/${encodeURIComponent(team.jid)}`)}
              >
                Open team chat
              </Button>
            </div>

            <Tabs
              defaultActiveKey="tasks"
              items={[
                {
                  key: 'tasks',
                  label: <span>📋 Tasks ({tasks.length})</span>,
                  children: (
                    <div>
                      <div className="flex items-center justify-between mb-3">
                        <Text type="secondary">Manager-tracked work items. Drag-and-drop not yet wired — use status dropdown.</Text>
                        <Button type="primary" icon={<PlusOutlined />} onClick={openCreateTask}>Add task</Button>
                      </div>
                      <div className="grid grid-cols-1 md:grid-cols-3 lg:grid-cols-6 gap-2">
                        {KANBAN_COLS.map(col => (
                          <div
                            key={col}
                            className="rounded-md p-2"
                            style={{ background: token.colorFillAlter, minHeight: 200 }}
                          >
                            <div className="flex items-center justify-between mb-2 px-1">
                              <Tag color={STATUS_COLOR[col]}>{STATUS_LABEL[col]}</Tag>
                              <Text type="secondary" style={{ fontSize: 11 }}>{tasksByStatus(col).length}</Text>
                            </div>
                            <div className="space-y-2">
                              {tasksByStatus(col).map(task => (
                                <Card
                                  key={task.id}
                                  size="small"
                                  styles={{ body: { padding: 8 } }}
                                  hoverable
                                  onClick={() => openEditTask(task)}
                                  style={{ borderRadius: 6, borderColor: token.colorBorderSecondary }}
                                >
                                  <div className="flex items-start justify-between gap-1">
                                    <div className="flex-1 min-w-0">
                                      <div className="text-xs font-medium line-clamp-2" style={{ color: token.colorText }}>
                                        {task.title}
                                      </div>
                                      <div className="flex flex-wrap items-center gap-1 mt-1">
                                        <Tag color={PRIORITY_COLOR[task.priority]} style={{ fontSize: 9, padding: '0 4px', lineHeight: '14px' }}>
                                          {task.priority}
                                        </Tag>
                                        {task.assignee && (
                                          <Tag style={{ fontSize: 9, padding: '0 4px', lineHeight: '14px' }}>
                                            @{task.assignee}
                                          </Tag>
                                        )}
                                        {task.result_output && (
                                          <Tooltip title="View result markdown">
                                            <Button
                                              type="link"
                                              size="small"
                                              icon={<FileTextOutlined />}
                                              onClick={(e) => { e.stopPropagation(); setResultViewTask(task); }}
                                              style={{ padding: 0, fontSize: 11, height: 16, lineHeight: '14px' }}
                                            >
                                              View result
                                            </Button>
                                          </Tooltip>
                                        )}
                                      </div>
                                    </div>
                                    <Dropdown
                                      menu={{
                                        items: [
                                          ...KANBAN_COLS.filter(c => c !== task.status).map(c => ({
                                            key: c, label: `→ ${STATUS_LABEL[c]}`,
                                            onClick: () => updateTaskStatus(task, c),
                                          })),
                                          { type: 'divider' as const },
                                          {
                                            key: 'edit', icon: <EditOutlined />, label: 'Edit',
                                            onClick: () => openEditTask(task),
                                          },
                                          {
                                            key: 'delete', icon: <DeleteOutlined />, label: 'Delete', danger: true,
                                            onClick: () => handleDeleteTask(task),
                                          },
                                        ],
                                      }}
                                      trigger={['click']}
                                    >
                                      <Button
                                        type="text" size="small" icon={<MoreOutlined />}
                                        style={{ minWidth: 18, height: 18, padding: 0 }}
                                        onClick={(e) => e.stopPropagation()}
                                      />
                                    </Dropdown>
                                  </div>
                                </Card>
                              ))}
                              {tasksByStatus(col).length === 0 && (
                                <div className="text-[10px] text-center py-2" style={{ color: token.colorTextQuaternary }}>—</div>
                              )}
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  ),
                },
                {
                  key: 'members',
                  label: <span>👥 Members ({team?.members.length ?? 0})</span>,
                  children: (
                    <div>
                      <div className="flex items-center justify-between mb-3">
                        <Text type="secondary">Members + their triggers, role, and constraints. The manager delegates here.</Text>
                        <Button type="primary" icon={<PlusOutlined />} onClick={openCreateMember}>Add member</Button>
                      </div>
                      {(team?.members.length ?? 0) === 0 ? (
                        <Empty description="No members yet" />
                      ) : (
                        <div className="space-y-2">
                          {team?.members.map(m => {
                            // Cowork members are personas, not Profiles — match against
                            // the persona pool first, fall back to the slug.
                            const p = personaPool.find(p => p.name === m.folder);
                            const triggerCount = parseTriggerJson(m.triggers).length;
                            return (
                              <Card
                                key={m.folder}
                                hoverable
                                onClick={() => openEditMember(m)}
                                style={{ borderRadius: 8 }}
                                styles={{ body: { padding: 12 } }}
                              >
                                <div className="flex items-start justify-between gap-3">
                                  <div className="flex-1">
                                    <div className="flex items-center gap-2 mb-1">
                                      <Text strong>{p?.name ?? m.folder}</Text>
                                      <code style={{ fontSize: 10, color: token.colorTextTertiary }}>{m.folder}</code>
                                      {m.role && <Tag color="blue" style={{ fontSize: 10 }}>{m.role}</Tag>}
                                      {triggerCount > 0 && (
                                        <Tag color="gold" style={{ fontSize: 10 }}>
                                          ⚡ {triggerCount} trigger{triggerCount === 1 ? '' : 's'} · {summarizeTriggers(m.triggers)}
                                        </Tag>
                                      )}
                                    </div>
                                    {m.responsibilities && (
                                      <Text type="secondary" style={{ fontSize: 12, display: 'block' }}>
                                        {m.responsibilities}
                                      </Text>
                                    )}
                                  </div>
                                  <div className="flex items-center gap-1">
                                    <Button
                                      type="text" size="small" icon={<EditOutlined />}
                                      onClick={(e) => { e.stopPropagation(); openEditMember(m); }}
                                    />
                                    <Popconfirm
                                      title="Remove this member?"
                                      onConfirm={(e) => { e?.stopPropagation(); handleRemoveMember(m); }}
                                      onCancel={(e) => e?.stopPropagation()}
                                    >
                                      <Button
                                        type="text" size="small" danger icon={<DeleteOutlined />}
                                        onClick={(e) => e.stopPropagation()}
                                      />
                                    </Popconfirm>
                                  </div>
                                </div>
                              </Card>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  ),
                },
              ]}
            />
          </div>
        </Content>
      </Layout>

      {/* ─── Task modal ─── */}
      <Modal
        title={editingTask ? 'Edit task' : 'New task'}
        open={taskModalOpen}
        onCancel={() => setTaskModalOpen(false)}
        footer={null}
        destroyOnHidden
        width={560}
      >
        <Form form={taskForm} layout="vertical" onFinish={handleSaveTask}>
          <Form.Item name="title" label="Title" rules={[{ required: true, message: 'Required' }]}>
            <Input placeholder="Implement OAuth flow" autoFocus />
          </Form.Item>
          <Form.Item name="description" label="Description">
            <Input.TextArea rows={3} placeholder="What needs to be done…" />
          </Form.Item>
          <div className="grid grid-cols-3 gap-3">
            <Form.Item name="status" label="Status" rules={[{ required: true }]}>
              <Select
                options={KANBAN_COLS.map(c => ({ value: c, label: STATUS_LABEL[c] }))}
              />
            </Form.Item>
            <Form.Item name="priority" label="Priority" rules={[{ required: true }]}>
              <Select
                options={['low','medium','high','critical'].map(p => ({ value: p, label: p }))}
              />
            </Form.Item>
            <Form.Item name="assignee" label="Assignee">
              <Select
                allowClear
                placeholder="Pick a member"
                options={(team?.members ?? []).map(m => ({ value: m.folder, label: m.folder }))}
              />
            </Form.Item>
          </div>
          <Form.Item name="reviewer" label="Reviewer (optional)">
            <Select
              allowClear
              placeholder="Pick a member"
              options={(team?.members ?? []).map(m => ({ value: m.folder, label: m.folder }))}
            />
          </Form.Item>
          {editingTask && (
            <Form.Item name="result_output" label="Result output (manual)">
              <Input.TextArea rows={2} placeholder="What did this task produce?" />
            </Form.Item>
          )}
          <div className="flex items-center justify-end gap-2 pt-2">
            <Button onClick={() => setTaskModalOpen(false)}>Cancel</Button>
            <Button type="primary" htmlType="submit">Save</Button>
          </div>
        </Form>
      </Modal>

      {/* ─── Task result viewer (markdown) ─── */}
      <Modal
        title={
          resultViewTask
            ? <span>📄 Result · <span style={{ color: token.colorTextSecondary, fontSize: 13 }}>{resultViewTask.title}</span></span>
            : 'Result'
        }
        open={!!resultViewTask}
        onCancel={() => setResultViewTask(null)}
        footer={
          <div className="flex items-center justify-between">
            <Text type="secondary" style={{ fontSize: 11 }}>
              {resultViewTask?.assignee && <>Assigned to: <code>@{resultViewTask.assignee}</code></>}
            </Text>
            <div>
              <Button
                onClick={() => {
                  if (resultViewTask?.result_output) {
                    navigator.clipboard?.writeText(resultViewTask.result_output);
                    message.success('Copied to clipboard');
                  }
                }}
              >
                Copy
              </Button>
              <Button type="primary" onClick={() => setResultViewTask(null)} style={{ marginLeft: 8 }}>
                Close
              </Button>
            </div>
          </div>
        }
        width={760}
        destroyOnHidden
      >
        <div className="max-h-[60vh] overflow-y-auto">
          {resultViewTask?.result_output
            ? <MarkdownBody content={resultViewTask.result_output} />
            : <Empty description="No result yet" />}
        </div>
      </Modal>

      {/* ─── Member modal ─── */}
      <Modal
        title={editingMember ? <span>Edit member · <code>{editingMember.folder}</code></span> : 'Add member'}
        open={memberModalOpen}
        onCancel={() => setMemberModalOpen(false)}
        footer={null}
        destroyOnHidden
        width={620}
      >
        <Form form={memberForm} layout="vertical" onFinish={handleSaveMember}>
          {!editingMember && (
            <Form.Item
              name="name"
              label="Member name"
              rules={[{ required: true, message: 'Required' }]}
              extra={
                <Text type="secondary" style={{ fontSize: 11 }}>
                  A short label for this member. The filename slug is derived
                  automatically. Type a name that matches an existing persona
                  (e.g. <code>browser-agent</code>) to pre-load its SOUL.md.
                </Text>
              }
            >
              <AutoComplete
                placeholder="MVP reviewer / browser-agent / Security analyst…"
                options={personaPool.map(p => ({
                  value: p.name,
                  label: `${p.name} — ${p.description.slice(0, 60)}${p.description.length > 60 ? '…' : ''}`,
                }))}
                filterOption={(input, option) =>
                  (option?.value ?? '').toLowerCase().includes(input.toLowerCase())
                }
                onSelect={(picked: string) => {
                  // Picking a known persona pre-loads its SOUL.md so the user
                  // can tweak instead of starting from blank.
                  if (picked) void loadMemberSoul(picked);
                }}
                onChange={(value: string) => {
                  // Live preview of derived slug; cheap to recompute.
                  const slug = slugifyName(value ?? '');
                  if (slug) void loadMemberSoul(slug);
                }}
              />
            </Form.Item>
          )}
          <Form.Item name="role" label="Role">
            <Input placeholder="reviewer / researcher / etc." />
          </Form.Item>
          <Form.Item name="responsibilities" label="Responsibilities">
            <Input.TextArea rows={2} placeholder="- review PRs\n- verify migrations" />
          </Form.Item>
          <Form.Item label={<span>Triggers ⚡</span>} extra="Empty = manual-dispatch only.">
            <TriggerEditor value={editingTriggers} onChange={setEditingTriggers} />
          </Form.Item>

          <Form.Item
            label={<span>Persona content (virtual-agents/&lt;slug&gt;.md)</span>}
            tooltip="This member's persona file under ~/.senclaw/virtual-agents/<slug>.md. Lives independently from user Profiles — editing here NEVER touches the agent table."
            extra={
              soulOriginal !== editingSoul
                ? <Text type="warning" style={{ fontSize: 11 }}>Persona modified — saving the member will write it to virtual-agents/.</Text>
                : <Text type="secondary" style={{ fontSize: 11 }}>Loaded from virtual-agents/. Edit freely; saves with the member.</Text>
            }
          >
            <Input.TextArea
              rows={8}
              value={editingSoul}
              onChange={(e) => setEditingSoul(e.target.value)}
              disabled={soulLoading}
              placeholder={soulLoading ? 'Loading SOUL.md…' : 'You are a focused specialist that…'}
              style={{ fontFamily: 'ui-monospace, SFMono-Regular, monospace', fontSize: 12 }}
            />
          </Form.Item>

          <Form.Item name="handoff_rules" label="Handoff rules">
            <Input.TextArea rows={2} placeholder="if blocked → handoff to ops" />
          </Form.Item>
          <Form.Item name="acceptance_criteria" label="Acceptance criteria">
            <Input.TextArea rows={2} placeholder="all tests pass; no critical issues" />
          </Form.Item>
          <div className="grid grid-cols-2 gap-3">
            <Form.Item name="output_format" label="Output format">
              <Input placeholder="markdown with sections" />
            </Form.Item>
            <Form.Item name="sla" label="SLA">
              <Input placeholder="< 5 min, < 10k tokens" />
            </Form.Item>
          </div>
          <Form.Item name="limits" label="Limits">
            <Input placeholder="read-only; no Bash" />
          </Form.Item>
          <div className="flex items-center justify-end gap-2 pt-2">
            <Button onClick={() => setMemberModalOpen(false)}>Cancel</Button>
            <Button type="primary" htmlType="submit">Save</Button>
          </div>
        </Form>
      </Modal>
    </AppLayout>
  );
}
