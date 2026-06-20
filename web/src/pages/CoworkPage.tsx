import { useEffect, useState } from 'react';
import {
  TriggerEditor, parseTriggerJson, stringifyTriggers, summarizeTriggers,
  type TriggerRule,
} from '../components/cowork/TriggerEditor';
import {
  Layout, theme, Empty, Button, Modal, Form, Input, Select, message, Card, Tag, Popconfirm, Typography,
} from 'antd';
import {
  CoffeeOutlined, PlusOutlined, DeleteOutlined, TeamOutlined, UserOutlined, FolderOpenOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { SessionList } from '../components/Sidebar';

const { Content } = Layout;
const { Title, Text, Paragraph } = Typography;

const PINNED_KEY = 'senclaw:pinned-jids';
function loadPinned(): Set<string> {
  try { return new Set(JSON.parse(localStorage.getItem(PINNED_KEY) ?? '[]')); } catch { return new Set(); }
}

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

interface TemplateMemberSpec {
  folder: string;
  role: string;
  responsibilities: string;
  triggers_json: string;
}

interface TeamTemplate {
  id: string;
  name: string;
  description: string;
  manager: string;
  manager_role: string;
  members: TemplateMemberSpec[];
  icon: string;
}

export function CoworkPage() {
  const { ws } = useAppContext();
  const navigate = useNavigate();
  const { token } = theme.useToken();
  const [pinnedJids] = useState<Set<string>>(loadPinned);
  const [teams, setTeams] = useState<CoworkTeam[]>([]);
  const [templates, setTemplates] = useState<TeamTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [modalOpen, setModalOpen] = useState(false);
  const [form] = Form.useForm();
  // Member-trigger edit modal state
  const [memberEditTeam, setMemberEditTeam] = useState<CoworkTeam | null>(null);
  const [memberEditTarget, setMemberEditTarget] = useState<TeamMember | null>(null);
  const [memberForm] = Form.useForm();
  const [editingTriggers, setEditingTriggers] = useState<TriggerRule[]>([]);

  const openEditMember = (team: CoworkTeam, member: TeamMember) => {
    setMemberEditTeam(team);
    setMemberEditTarget(member);
    setEditingTriggers(parseTriggerJson(member.triggers));
    memberForm.setFieldsValue({
      folder: member.folder,
      role: member.role ?? '',
      responsibilities: member.responsibilities ?? '',
      handoff_rules: member.handoff_rules ?? '',
      acceptance_criteria: member.acceptance_criteria ?? '',
      output_format: member.output_format ?? '',
      sla: member.sla ?? '',
      limits: member.limits ?? '',
    });
  };

  const handleMemberSave = async (values: any) => {
    if (!memberEditTeam || !memberEditTarget) return;
    try {
      const res = await fetch(`/api/cowork/teams/${memberEditTeam.id}/members`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          folder: memberEditTarget.folder,
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
      message.success(`Member "${memberEditTarget.folder}" updated`);
      setMemberEditTeam(null);
      setMemberEditTarget(null);
      await loadTeams();
    } catch (e) {
      message.error(`Save failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const handleMemberRemove = async () => {
    if (!memberEditTeam || !memberEditTarget) return;
    try {
      const res = await fetch(
        `/api/cowork/teams/${memberEditTeam.id}/members/${encodeURIComponent(memberEditTarget.folder)}`,
        { method: 'DELETE' }
      );
      if (!res.ok) throw new Error(await res.text());
      message.success(`Removed ${memberEditTarget.folder} from team`);
      setMemberEditTeam(null);
      setMemberEditTarget(null);
      await loadTeams();
    } catch (e) {
      message.error(`Remove failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  // Filter sidebar to cowork-type groups so users can return to active team chats.
  const coworkGroups = ws.groups.filter(g => g.groupType === 'cowork');

  // Profile choices (skip schedule_* — those aren't user-facing).
  const profiles = ws.agents.filter(a => !a.folder.startsWith('schedule_'));

  const loadTeams = async () => {
    setLoading(true);
    try {
      const res = await fetch('/api/cowork/teams');
      if (!res.ok) throw new Error(await res.text());
      setTeams(await res.json());
    } catch (e) {
      message.error(`Failed to load teams: ${String((e as Error)?.message ?? e)}`);
    } finally {
      setLoading(false);
    }
  };

  const loadTemplates = async () => {
    try {
      const res = await fetch('/api/cowork/templates');
      if (res.ok) setTemplates(await res.json());
    } catch {}
  };

  useEffect(() => { loadTeams(); loadTemplates(); }, []);

  const instantiateTemplate = async (tmpl: TeamTemplate) => {
    try {
      const res = await fetch('/api/cowork/teams/from-template', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ template_id: tmpl.id }),
      });
      if (!res.ok) throw new Error(await res.text());
      const team: CoworkTeam = await res.json();
      message.success(`Spun up "${team.name}" — opening chat`);
      await loadTeams();
      navigate(`/chat/${encodeURIComponent(team.jid)}`);
    } catch (e) {
      message.error(`Template failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const handleCreate = async (values: any) => {
    try {
      const res = await fetch('/api/cowork/teams', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: values.name,
          manager_folder: values.manager_folder,
          members: values.members ?? [],
          workspace_dir: values.workspace_dir || null,
        }),
      });
      if (!res.ok) throw new Error(await res.text());
      message.success('Team created — chat group materialised');
      setModalOpen(false);
      form.resetFields();
      await loadTeams();
    } catch (e) {
      message.error(`Create failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      const res = await fetch(`/api/cowork/teams/${id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      message.success('Team deleted');
      await loadTeams();
    } catch (e) {
      message.error(`Delete failed: ${String((e as Error)?.message ?? e)}`);
    }
  };

  const openTeamChat = (team: CoworkTeam) => {
    navigate(`/chat/${encodeURIComponent(team.jid)}`);
  };

  return (
    <AppLayout
      sidebar={
        <SessionList
          groups={coworkGroups}
          selectedJid={null}
          agentStates={ws.agentStates}
          pinnedJids={pinnedJids}
          onSelect={(jid) => navigate(`/chat/${encodeURIComponent(jid)}`)}
          onNewChat={() => setModalOpen(true)}
          onPin={() => { /* no-op */ }}
          onRename={(jid, name) => ws.updateGroup(jid, { name })}
          onDelete={(jid) => {
            const team = teams.find(t => t.jid === jid);
            if (team) handleDelete(team.id); else ws.unregisterGroup(jid);
          }}
        />
      }
    >
      <Layout style={{ background: 'transparent', height: '100%' }}>
        <Content className="overflow-y-auto p-8">
          <div className="max-w-4xl mx-auto">
            <div className="flex items-center justify-between mb-6">
              <div>
                <Title level={3} style={{ margin: 0 }}>
                  <TeamOutlined style={{ marginRight: 10, color: token.colorPrimary }} />
                  Cowork Teams
                </Title>
                <Text type="secondary">
                  A manager profile leads specialist members to collaborate on a shared workspace.
                  Open a team to chat with the manager — it delegates to members via dispatch tools.
                </Text>
              </div>
              <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>
                New Team
              </Button>
            </div>

            {/* Pre-defined templates — one-click team spinup using built-in personas */}
            {templates.length > 0 && (
              <div className="mb-8">
                <div className="mb-3 flex items-center gap-2">
                  <Text strong style={{ fontSize: 13 }}>Quick start templates</Text>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    Pre-defined squads using built-in personas. Click to spin up + open chat.
                  </Text>
                </div>
                <div className="grid grid-cols-2 md:grid-cols-3 gap-2">
                  {templates.map(tmpl => (
                    <Card
                      key={tmpl.id}
                      hoverable
                      onClick={() => instantiateTemplate(tmpl)}
                      style={{
                        borderRadius: 10,
                        borderColor: token.colorBorderSecondary,
                        background: token.colorBgContainer,
                      }}
                      styles={{ body: { padding: 12 } }}
                    >
                      <div className="flex items-start gap-2">
                        <div style={{ fontSize: 22 }}>{tmpl.icon}</div>
                        <div className="flex-1 min-w-0">
                          <div className="font-medium text-sm" style={{ color: token.colorText }}>
                            {tmpl.name}
                          </div>
                          <div className="text-[11px] mt-0.5 line-clamp-2" style={{ color: token.colorTextSecondary }}>
                            {tmpl.description}
                          </div>
                          <div className="flex flex-wrap gap-1 mt-1.5">
                            <Tag color="blue" style={{ fontSize: 10, padding: '0 4px', lineHeight: '16px' }}>
                              {tmpl.manager}
                            </Tag>
                            {tmpl.members.map(m => (
                              <Tag key={m.folder} style={{ fontSize: 10, padding: '0 4px', lineHeight: '16px' }}>
                                {m.folder}
                              </Tag>
                            ))}
                          </div>
                        </div>
                      </div>
                    </Card>
                  ))}
                </div>
              </div>
            )}

            {teams.length === 0 && !loading ? (
              <Empty
                image={<CoffeeOutlined style={{ fontSize: 56, color: token.colorPrimary, opacity: 0.6 }} />}
                description={
                  <div className="space-y-1 mt-3">
                    <div style={{ color: token.colorText, fontSize: 16, fontWeight: 500 }}>
                      No teams yet
                    </div>
                    <div style={{ color: token.colorTextSecondary, fontSize: 12 }}>
                      Create your first team — pick a manager profile and a few specialists.
                    </div>
                  </div>
                }
              >
                <Button type="primary" icon={<PlusOutlined />} onClick={() => setModalOpen(true)}>
                  Create Team
                </Button>
              </Empty>
            ) : (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                {teams.map(t => {
                  const managerProfile = profiles.find(p => p.folder === t.manager_folder);
                  return (
                    <Card
                      key={t.id}
                      hoverable
                      onClick={() => navigate(`/cowork/${t.id}`)}
                      style={{ borderRadius: 12, borderColor: token.colorBorderSecondary }}
                      title={
                        <div className="flex items-center gap-2">
                          <TeamOutlined style={{ color: token.colorPrimary }} />
                          <span>{t.name}</span>
                        </div>
                      }
                      extra={
                        <Popconfirm
                          title="Delete this team?"
                          description="Chat history is preserved in the DB."
                          onConfirm={(e) => { e?.stopPropagation(); handleDelete(t.id); }}
                          onCancel={(e) => e?.stopPropagation()}
                          okText="Delete"
                          cancelText="Cancel"
                        >
                          <Button
                            type="text" danger size="small" icon={<DeleteOutlined />}
                            onClick={e => e.stopPropagation()}
                          />
                        </Popconfirm>
                      }
                      styles={{ body: { padding: 14 } }}
                    >
                      <div className="flex items-center gap-2 mb-2">
                        <UserOutlined style={{ color: token.colorTextTertiary, fontSize: 12 }} />
                        <Text type="secondary" style={{ fontSize: 12 }}>Manager:</Text>
                        <Tag color="blue">{managerProfile?.name ?? t.manager_folder}</Tag>
                      </div>
                      <div className="flex items-start gap-2 mb-2">
                        <TeamOutlined style={{ color: token.colorTextTertiary, fontSize: 12, marginTop: 4 }} />
                        <Text type="secondary" style={{ fontSize: 12, marginTop: 2 }}>Members:</Text>
                        <div className="flex flex-wrap gap-1">
                          {t.members.length === 0
                            ? <Text type="secondary" style={{ fontSize: 11 }}>(solo manager)</Text>
                            : t.members.map(m => {
                              const p = profiles.find(p => p.folder === m.folder);
                              const label = p?.name ?? m.folder;
                              const triggerCount = parseTriggerJson(m.triggers).length;
                              const summary = summarizeTriggers(m.triggers);
                              return (
                                <Tag
                                  key={m.folder}
                                  color={triggerCount > 0 ? 'gold' : undefined}
                                  style={{ cursor: 'pointer' }}
                                  onClick={(e) => { e.stopPropagation(); openEditMember(t, m); }}
                                  title={triggerCount > 0 ? `Triggers (${triggerCount}): ${summary}` : 'Click to edit triggers'}
                                >
                                  {label}{triggerCount > 0 ? ` ⚡${triggerCount}` : ''}
                                </Tag>
                              );
                            })}
                        </div>
                      </div>
                      {t.workspace_dir && (
                        <div className="flex items-center gap-2">
                          <FolderOpenOutlined style={{ color: token.colorTextTertiary, fontSize: 12 }} />
                          <Text type="secondary" style={{ fontSize: 11, fontFamily: 'monospace' }}>
                            {t.workspace_dir}
                          </Text>
                        </div>
                      )}
                      <div className="flex items-center gap-2 mt-3 pt-2" style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}>
                        <Button
                          size="small"
                          icon={<TeamOutlined />}
                          onClick={(e) => { e.stopPropagation(); navigate(`/cowork/${t.id}`); }}
                          block
                        >
                          Manage tasks & members
                        </Button>
                        <Button
                          size="small"
                          type="primary"
                          onClick={(e) => { e.stopPropagation(); openTeamChat(t); }}
                          block
                        >
                          Open chat
                        </Button>
                      </div>
                    </Card>
                  );
                })}
              </div>
            )}
          </div>
        </Content>
      </Layout>

      <Modal
        title="Create Cowork Team"
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        footer={null}
        destroyOnHidden
        width={560}
      >
        <Form form={form} layout="vertical" onFinish={handleCreate} style={{ marginTop: 16 }}>
          {/* Template picker — autofill name/manager/members from a built-in template */}
          <Form.Item
            name="template_id"
            label="Start from template (optional)"
            extra="Pick a template to auto-fill the form. You can still edit any field below."
          >
            <Select
              allowClear
              placeholder="No template — fill in manually"
              options={templates.map(t => ({
                value: t.id,
                label: `${t.icon} ${t.name}`,
              }))}
              optionRender={(opt) => {
                const tmpl = templates.find(x => x.id === opt.value);
                if (!tmpl) return opt.label;
                return (
                  <div>
                    <div style={{ fontWeight: 500 }}>{tmpl.icon} {tmpl.name}</div>
                    <div style={{ fontSize: 11, color: token.colorTextSecondary }}>
                      {tmpl.description}
                    </div>
                    <div style={{ fontSize: 10, color: token.colorTextTertiary, marginTop: 2 }}>
                      Manager: <code>{tmpl.manager}</code>
                      {tmpl.members.length > 0 && (
                        <> · Members: {tmpl.members.map(m => <code key={m.folder} style={{ marginLeft: 4 }}>{m.folder}</code>)}</>
                      )}
                    </div>
                  </div>
                );
              }}
              onChange={(templateId: string | undefined) => {
                if (!templateId) return;
                const tmpl = templates.find(t => t.id === templateId);
                if (!tmpl) return;
                // Auto-fill from template; user can still override.
                form.setFieldsValue({
                  name: tmpl.name,
                  manager_folder: tmpl.manager,
                  members: tmpl.members.map(m => m.folder),
                });
              }}
            />
          </Form.Item>

          <Form.Item
            name="name"
            label="Team name"
            rules={[{ required: true, message: 'Required' }]}
          >
            <Input placeholder="Frontend squad" autoFocus />
          </Form.Item>

          <Form.Item
            name="manager_folder"
            label="Manager"
            rules={[{ required: true, message: 'Required' }]}
            extra={
              <span>
                The lead agent you'll chat with. It delegates to members via dispatch tools.{' '}
                <Text type="secondary" style={{ fontSize: 11 }}>
                  You can pick any existing agent — not limited to user profiles.
                </Text>
              </span>
            }
          >
            <Select
              placeholder="Pick an agent (or template will auto-fill)"
              showSearch
              optionFilterProp="label"
              options={ws.agents.map(a => ({
                value: a.folder,
                label: `${a.name} (${a.folder})`,
              }))}
            />
          </Form.Item>

          <Form.Item
            name="members"
            label="Members"
            extra={
              <span>
                Agents the manager can delegate sub-tasks to.{' '}
                <Text type="secondary" style={{ fontSize: 11 }}>
                  Type a folder slug + Enter to add a custom agent not in the list.
                </Text>
              </span>
            }
          >
            <Select
              mode="tags"
              placeholder="Pick agents — or type a custom folder slug"
              showSearch
              optionFilterProp="label"
              options={ws.agents.map(a => ({
                value: a.folder,
                label: `${a.name} (${a.folder})`,
              }))}
              tokenSeparators={[',']}
            />
          </Form.Item>

          <Form.Item
            name="workspace_dir"
            label="Shared workspace (optional)"
            extra="Absolute path. All members run in this working directory."
          >
            <Input placeholder="/Users/you/code/my-project" style={{ fontFamily: 'monospace' }} />
          </Form.Item>

          <Form.Item style={{ marginBottom: 0, textAlign: 'right', marginTop: 8 }}>
            <Button onClick={() => setModalOpen(false)} style={{ marginRight: 8 }}>Cancel</Button>
            <Button type="primary" htmlType="submit">Create Team</Button>
          </Form.Item>
        </Form>
      </Modal>

      {/* Member trigger editor — pops up when user clicks a member tag */}
      <Modal
        title={
          memberEditTarget
            ? <span>Edit member · <code>{memberEditTarget.folder}</code></span>
            : 'Edit member'
        }
        open={!!memberEditTarget}
        onCancel={() => { setMemberEditTeam(null); setMemberEditTarget(null); }}
        footer={null}
        destroyOnHidden
        width={600}
      >
        <Form form={memberForm} layout="vertical" onFinish={handleMemberSave} style={{ marginTop: 12 }}>
          <Form.Item name="role" label="Role" tooltip="Short label (e.g. 'reviewer', 'researcher').">
            <Input placeholder="reviewer" />
          </Form.Item>
          <Form.Item
            name="responsibilities"
            label="Responsibilities"
            tooltip="What this member is expected to do when activated. Free text — bullet list or paragraph."
          >
            <Input.TextArea rows={2} placeholder="- review PRs and flag risks\n- verify migrations are safe" />
          </Form.Item>
          <Form.Item
            label={<span>Triggers ⚡</span>}
            tooltip="Structured rules — when this member auto-activates. Stored as a JSON array of typed trigger objects, matching the legacy CoworkManager schema."
            extra="Add one or more rules. Empty = manual-dispatch only."
          >
            <TriggerEditor value={editingTriggers} onChange={setEditingTriggers} />
          </Form.Item>
          <Form.Item
            name="handoff_rules"
            label="Handoff rules"
            tooltip="When this member should pass control to someone else."
          >
            <Input.TextArea rows={2} placeholder="if blocked by infra → handoff to ops" />
          </Form.Item>
          <Form.Item
            name="acceptance_criteria"
            label="Acceptance criteria"
            tooltip="How to know the work is done."
          >
            <Input.TextArea rows={2} placeholder="all tests pass; no critical issues" />
          </Form.Item>
          <div className="grid grid-cols-2 gap-3">
            <Form.Item name="output_format" label="Output format" tooltip="Markdown / JSON / table / etc.">
              <Input placeholder="markdown with sections: summary, findings, next" />
            </Form.Item>
            <Form.Item name="sla" label="SLA" tooltip="Time/cost budget.">
              <Input placeholder="< 5 min, < 10k tokens" />
            </Form.Item>
          </div>
          <Form.Item name="limits" label="Limits" tooltip="Tool / file / scope restrictions.">
            <Input placeholder="read-only; no Bash" />
          </Form.Item>

          <div className="flex items-center justify-between pt-2">
            <Popconfirm
              title="Remove this member from the team?"
              description="The agent profile and its history stay; only this team membership is removed."
              onConfirm={handleMemberRemove}
              okText="Remove"
              cancelText="Cancel"
            >
              <Button danger icon={<DeleteOutlined />}>Remove from team</Button>
            </Popconfirm>
            <div>
              <Button onClick={() => { setMemberEditTeam(null); setMemberEditTarget(null); }} style={{ marginRight: 8 }}>Cancel</Button>
              <Button type="primary" htmlType="submit">Save</Button>
            </div>
          </div>
        </Form>
      </Modal>
    </AppLayout>
  );
}
