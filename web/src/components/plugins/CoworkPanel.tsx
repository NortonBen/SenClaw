import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  Flex, Typography, theme, Card, Space, Tag, Tabs, Button, Modal, Input, Select,
  Switch, message, Popconfirm, Empty, Tooltip, Divider,
} from 'antd';
import {
  CoffeeOutlined, PlusOutlined, EditOutlined, DeleteOutlined, TeamOutlined,
  ThunderboltOutlined, SettingOutlined, SaveOutlined, FolderOpenOutlined, RobotOutlined,
  DownloadOutlined, UploadOutlined,
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import {
  TriggerEditor, parseTriggerJson, stringifyTriggers, type TriggerRule,
} from '../cowork/TriggerEditor';

const { Title, Text, Paragraph } = Typography;

// ─── Types (mirror the Rust cowork API) ──────────────────────────────────────

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

interface CoworkTeamSettings {
  manager_preamble?: string | null;
  manager_tools?: string[] | null;
  auto_create_tasks?: boolean | null;
}

interface TemplateView {
  id: string;
  name: string;
  description: string;
  icon: string;
  manager: string;
  manager_role: string;
  members: TeamMember[];
  settings: CoworkTeamSettings;
  builtin: boolean;
}

interface CoworkTeam {
  id: string;
  name: string;
  manager_folder: string;
  members: TeamMember[];
  workspace_dir: string | null;
  created_at: string;
  jid: string;
  settings: CoworkTeamSettings;
}

interface PersonaView { name: string; description: string; }

const DEFAULT_TOOLS = ['Task', 'TodoWrite'];

async function api<T>(url: string, init?: RequestInit): Promise<T> {
  const res = await fetch(url, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers ?? {}) },
  });
  if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
  return res.status === 204 ? (undefined as T) : res.json();
}

// ─── Portable import/export ───────────────────────────────────────────────────

const EXPORT_KIND = 'cowork-template';
const EXPORT_VERSION = 1;

/** The body the create/update endpoints accept — also the portable file shape. */
interface PortableTemplate {
  name: string;
  description?: string;
  icon?: string;
  manager_folder: string;
  manager_role?: string;
  members: TeamMember[];
  settings?: CoworkTeamSettings;
}

function toPortable(t: TemplateView): PortableTemplate {
  return {
    name: t.name, description: t.description, icon: t.icon,
    manager_folder: t.manager, manager_role: t.manager_role,
    members: t.members, settings: t.settings,
  };
}

function downloadJson(filename: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url; a.download = filename;
  document.body.appendChild(a); a.click(); a.remove();
  URL.revokeObjectURL(url);
}

function slugify(s: string): string {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '') || 'template';
}

/** Coerce a parsed JSON value into a list of PortableTemplate. Accepts a bare
 *  template object, an array of them, or a `{kind, templates|template}` wrapper. */
function normalizeImport(raw: unknown): PortableTemplate[] {
  const pick = (o: any): PortableTemplate | null =>
    o && typeof o === 'object' && o.name && (o.manager_folder || o.manager)
      ? {
          name: String(o.name),
          description: o.description ?? '',
          icon: o.icon ?? '🧩',
          manager_folder: String(o.manager_folder ?? o.manager),
          manager_role: o.manager_role ?? 'lead',
          members: Array.isArray(o.members) ? o.members : [],
          settings: o.settings ?? {},
        }
      : null;

  let candidates: any[] = [];
  if (Array.isArray(raw)) candidates = raw;
  else if (raw && typeof raw === 'object') {
    const r = raw as any;
    if (Array.isArray(r.templates)) candidates = r.templates;
    else if (r.template) candidates = [r.template];
    else candidates = [r];
  }
  return candidates.map(pick).filter((x): x is PortableTemplate => x !== null);
}

// ─── Member editor (used inside the template editor) ─────────────────────────

function MemberRows({ value, onChange }: { value: TeamMember[]; onChange: (m: TeamMember[]) => void }) {
  const { token } = theme.useToken();
  const update = (i: number, patch: Partial<TeamMember>) =>
    onChange(value.map((m, idx) => (idx === i ? { ...m, ...patch } : m)));
  const remove = (i: number) => onChange(value.filter((_, idx) => idx !== i));
  return (
    <Space direction="vertical" style={{ width: '100%' }} size={10}>
      {value.map((m, i) => (
        <Card key={i} size="small" style={{ background: token.colorFillQuaternary }}
          styles={{ body: { padding: 12 } }}>
          <Flex gap={8} align="center" style={{ marginBottom: 8 }}>
            <Input placeholder="folder slug (persona)" value={m.folder}
              onChange={e => update(i, { folder: e.target.value })} style={{ flex: 1 }} />
            <Input placeholder="role" value={m.role ?? ''} style={{ width: 130 }}
              onChange={e => update(i, { role: e.target.value })} />
            <Button danger type="text" icon={<DeleteOutlined />} onClick={() => remove(i)} />
          </Flex>
          <Input.TextArea placeholder="responsibilities" autoSize={{ minRows: 1, maxRows: 3 }}
            value={m.responsibilities ?? ''} style={{ marginBottom: 8 }}
            onChange={e => update(i, { responsibilities: e.target.value })} />
          <Text type="secondary" style={{ fontSize: 11 }}>Triggers</Text>
          <TriggerEditor value={parseTriggerJson(m.triggers)}
            onChange={(rules: TriggerRule[]) => update(i, { triggers: stringifyTriggers(rules) ?? undefined })} />
        </Card>
      ))}
      <Button icon={<PlusOutlined />} block type="dashed"
        onClick={() => onChange([...value, { folder: '', role: '', triggers: '[{"type":"task_assigned"}]' }])}>
        Add member
      </Button>
    </Space>
  );
}

// ─── Behaviour settings fields (shared by template + team editors) ───────────

function BehaviourFields({ value, onChange }: { value: CoworkTeamSettings; onChange: (s: CoworkTeamSettings) => void }) {
  return (
    <Space direction="vertical" style={{ width: '100%' }} size={10}>
      <div>
        <Text strong style={{ fontSize: 12 }}>Auto-create tasks on each user message</Text>
        <div><Switch checked={value.auto_create_tasks !== false}
          onChange={v => onChange({ ...value, auto_create_tasks: v })} /></div>
      </div>
      <div>
        <Text strong style={{ fontSize: 12 }}>Manager tools</Text>
        <Select mode="tags" style={{ width: '100%' }} placeholder={DEFAULT_TOOLS.join(', ')}
          value={value.manager_tools ?? undefined}
          onChange={(v: string[]) => onChange({ ...value, manager_tools: v.length ? v : null })}
          options={DEFAULT_TOOLS.map(t => ({ value: t }))} />
        <Text type="secondary" style={{ fontSize: 11 }}>Empty = default ({DEFAULT_TOOLS.join(' + ')}).</Text>
      </div>
      <div>
        <Text strong style={{ fontSize: 12 }}>Manager preamble override</Text>
        <Input.TextArea autoSize={{ minRows: 2, maxRows: 8 }}
          placeholder="Leave empty to use the default PLAN→DELEGATE→SYNTHESIZE preamble."
          value={value.manager_preamble ?? ''}
          onChange={e => onChange({ ...value, manager_preamble: e.target.value || null })} />
      </div>
    </Space>
  );
}

// ─── Main panel ──────────────────────────────────────────────────────────────

const emptyTemplate = (): TemplateView => ({
  id: '', name: '', description: '', icon: '🧩', manager: '', manager_role: 'lead',
  members: [], settings: {}, builtin: false,
});

const CoworkPanel: React.FC = () => {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [tab, setTab] = useState('templates');
  const [templates, setTemplates] = useState<TemplateView[]>([]);
  const [teams, setTeams] = useState<CoworkTeam[]>([]);
  const [personas, setPersonas] = useState<PersonaView[]>([]);

  const [editing, setEditing] = useState<TemplateView | null>(null);
  const [teamSettings, setTeamSettings] = useState<CoworkTeam | null>(null);

  const reload = useCallback(async () => {
    try {
      const [t, tm, p] = await Promise.all([
        api<TemplateView[]>('/api/cowork/templates'),
        api<CoworkTeam[]>('/api/cowork/teams'),
        api<PersonaView[]>('/api/cowork/personas').catch(() => [] as PersonaView[]),
      ]);
      setTemplates(t); setTeams(tm); setPersonas(p);
    } catch (e) { message.error(`Load failed: ${e}`); }
  }, []);

  useEffect(() => { reload(); }, [reload]);

  const personaOptions = personas.map(p => ({ value: p.name, label: p.name }));

  // ── template actions ──
  const useTemplate = async (t: TemplateView) => {
    try {
      const team = await api<CoworkTeam>('/api/cowork/teams/from-template', {
        method: 'POST', body: JSON.stringify({ template_id: t.id }),
      });
      message.success(`Team "${team.name}" created`);
      await reload(); setTab('teams');
    } catch (e) { message.error(`${e}`); }
  };

  const saveTemplate = async () => {
    if (!editing) return;
    if (!editing.name.trim() || !editing.manager.trim()) { message.warning('Name and manager are required'); return; }
    const body = {
      name: editing.name, description: editing.description, icon: editing.icon,
      manager_folder: editing.manager, manager_role: editing.manager_role,
      members: editing.members.filter(m => m.folder.trim()), settings: editing.settings,
    };
    try {
      if (editing.id) await api(`/api/cowork/templates/${editing.id}`, { method: 'PUT', body: JSON.stringify(body) });
      else await api('/api/cowork/templates', { method: 'POST', body: JSON.stringify(body) });
      message.success('Template saved'); setEditing(null); await reload();
    } catch (e) { message.error(`${e}`); }
  };

  const deleteTemplate = async (id: string) => {
    try { await api(`/api/cowork/templates/${id}`, { method: 'DELETE' }); message.success('Deleted'); await reload(); }
    catch (e) { message.error(`${e}`); }
  };

  // ── import / export ──
  const fileRef = useRef<HTMLInputElement>(null);

  const exportTemplate = (t: TemplateView) => {
    downloadJson(`${slugify(t.name)}.cowork-template.json`, {
      kind: EXPORT_KIND, version: EXPORT_VERSION, template: toPortable(t),
    });
  };

  const exportAll = () => {
    if (templates.length === 0) { message.info('Nothing to export'); return; }
    downloadJson('cowork-templates.json', {
      kind: EXPORT_KIND, version: EXPORT_VERSION, templates: templates.map(toPortable),
    });
  };

  const onImportFile = async (file: File) => {
    try {
      const parsed = JSON.parse(await file.text());
      const items = normalizeImport(parsed);
      if (items.length === 0) { message.error('No valid template found in file'); return; }
      let ok = 0;
      for (const it of items) {
        try { await api('/api/cowork/templates', { method: 'POST', body: JSON.stringify(it) }); ok++; }
        catch (e) { message.error(`Import "${it.name}" failed: ${e}`); }
      }
      if (ok > 0) { message.success(`Imported ${ok} template${ok === 1 ? '' : 's'}`); await reload(); }
    } catch (e) { message.error(`Invalid JSON: ${e}`); }
  };

  // ── team actions ──
  const saveTeamSettings = async () => {
    if (!teamSettings) return;
    try {
      await api(`/api/cowork/teams/${teamSettings.id}`, {
        method: 'PATCH',
        body: JSON.stringify({
          name: teamSettings.name, manager_folder: teamSettings.manager_folder,
          workspace_dir: teamSettings.workspace_dir ?? '', settings: teamSettings.settings,
        }),
      });
      message.success('Team settings saved'); setTeamSettings(null); await reload();
    } catch (e) { message.error(`${e}`); }
  };

  const saveTeamAsTemplate = async (t: CoworkTeam) => {
    try {
      await api(`/api/cowork/teams/${t.id}/save-as-template`, { method: 'POST', body: JSON.stringify({}) });
      message.success('Saved as template'); await reload();
    } catch (e) { message.error(`${e}`); }
  };

  const deleteTeam = async (id: string) => {
    try { await api(`/api/cowork/teams/${id}`, { method: 'DELETE' }); message.success('Team deleted'); await reload(); }
    catch (e) { message.error(`${e}`); }
  };

  // ── render ──
  const headerStyle: React.CSSProperties = {
    background: `linear-gradient(135deg, ${token.colorPrimaryBg}, transparent)`,
    padding: '20px 24px', borderBottom: `1px solid ${token.colorBorderSecondary}`,
  };

  return (
    <Flex vertical style={{ height: '100%', overflow: 'auto' }}>
      <div style={headerStyle}>
        <Flex align="center" gap={14}>
          <div style={{
            width: 44, height: 44, borderRadius: 12, background: token.colorPrimary,
            display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 22,
          }}><CoffeeOutlined style={{ color: '#fff' }} /></div>
          <div>
            <Title level={4} style={{ margin: 0 }}>Cowork Space</Title>
            <Text type="secondary">Manage team templates & settings for multi-agent collaboration</Text>
          </div>
        </Flex>
      </div>

      <div style={{ padding: 24 }}>
        <Tabs activeKey={tab} onChange={setTab} items={[
          {
            key: 'templates',
            label: <span><ThunderboltOutlined /> Templates</span>,
            children: (
              <>
                <Flex justify="space-between" align="center" gap={8} style={{ marginBottom: 16 }} wrap>
                  <Text type="secondary">Built-in blueprints + your custom templates. Click <b>Use</b> to spin up a team.</Text>
                  <Space size={6}>
                    <Button icon={<UploadOutlined />} onClick={() => fileRef.current?.click()}>Import</Button>
                    <Tooltip title="Export all templates to one JSON file">
                      <Button icon={<DownloadOutlined />} onClick={exportAll}>Export all</Button>
                    </Tooltip>
                    <Button type="primary" icon={<PlusOutlined />} onClick={() => setEditing(emptyTemplate())}>New template</Button>
                  </Space>
                </Flex>
                <input ref={fileRef} type="file" accept="application/json,.json" style={{ display: 'none' }}
                  onChange={e => { const f = e.target.files?.[0]; if (f) onImportFile(f); e.target.value = ''; }} />
                <Flex wrap gap={14}>
                  {templates.map(t => (
                    <Card key={t.id} size="small" style={{ width: 300 }} hoverable styles={{ body: { padding: 16 } }}>
                      <Flex align="center" gap={10} style={{ marginBottom: 8 }}>
                        <span style={{ fontSize: 24 }}>{t.icon}</span>
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <Text strong ellipsis>{t.name}</Text>
                          <div>{t.builtin
                            ? <Tag style={{ fontSize: 10 }}>Built-in</Tag>
                            : <Tag color="blue" style={{ fontSize: 10 }}>Custom</Tag>}</div>
                        </div>
                      </Flex>
                      <Paragraph type="secondary" ellipsis={{ rows: 2 }} style={{ fontSize: 12, minHeight: 36 }}>
                        {t.description || '—'}
                      </Paragraph>
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        <RobotOutlined /> {t.manager} · {t.members.length} member{t.members.length === 1 ? '' : 's'}
                      </Text>
                      <Divider style={{ margin: '10px 0' }} />
                      <Flex gap={6} justify="space-between">
                        <Button size="small" type="primary" ghost icon={<ThunderboltOutlined />}
                          onClick={() => useTemplate(t)}>Use</Button>
                        <Space size={4}>
                          <Tooltip title="Export to JSON">
                            <Button size="small" icon={<DownloadOutlined />} onClick={() => exportTemplate(t)} />
                          </Tooltip>
                          <Tooltip title={t.builtin ? 'Clone to edit' : 'Edit'}>
                            <Button size="small" icon={<EditOutlined />}
                              onClick={() => setEditing(t.builtin
                                ? { ...t, id: '', name: `${t.name} (copy)`, builtin: false }
                                : { ...t })} />
                          </Tooltip>
                          {!t.builtin && (
                            <Popconfirm title="Delete template?" onConfirm={() => deleteTemplate(t.id)}>
                              <Button size="small" danger icon={<DeleteOutlined />} />
                            </Popconfirm>
                          )}
                        </Space>
                      </Flex>
                    </Card>
                  ))}
                </Flex>
              </>
            ),
          },
          {
            key: 'teams',
            label: <span><TeamOutlined /> Teams ({teams.length})</span>,
            children: teams.length === 0 ? (
              <Empty description="No teams yet — create one from a template" />
            ) : (
              <Flex wrap gap={14}>
                {teams.map(t => (
                  <Card key={t.id} size="small" style={{ width: 320 }} styles={{ body: { padding: 16 } }}>
                    <Flex justify="space-between" align="center" style={{ marginBottom: 6 }}>
                      <Text strong ellipsis>{t.name}</Text>
                      {t.settings?.auto_create_tasks === false &&
                        <Tag color="orange" style={{ fontSize: 10 }}>auto-task off</Tag>}
                    </Flex>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      <RobotOutlined /> {t.manager_folder} · {t.members.length} member{t.members.length === 1 ? '' : 's'}
                    </Text>
                    {t.workspace_dir && <div><Text type="secondary" style={{ fontSize: 11 }}>
                      <FolderOpenOutlined /> {t.workspace_dir}</Text></div>}
                    <Divider style={{ margin: '10px 0' }} />
                    <Flex gap={6} wrap>
                      <Button size="small" icon={<SettingOutlined />}
                        onClick={() => setTeamSettings({ ...t, settings: t.settings ?? {} })}>Settings</Button>
                      <Button size="small" icon={<FolderOpenOutlined />} onClick={() => navigate(`/cowork/${t.id}`)}>Open</Button>
                      <Tooltip title="Save this team as a reusable template">
                        <Button size="small" icon={<SaveOutlined />} onClick={() => saveTeamAsTemplate(t)} />
                      </Tooltip>
                      <Popconfirm title="Delete team?" onConfirm={() => deleteTeam(t.id)}>
                        <Button size="small" danger icon={<DeleteOutlined />} />
                      </Popconfirm>
                    </Flex>
                  </Card>
                ))}
              </Flex>
            ),
          },
        ]} />
      </div>

      {/* Template editor */}
      <Modal open={!!editing} title={editing?.id ? 'Edit template' : 'New template'}
        onCancel={() => setEditing(null)} onOk={saveTemplate} okText="Save" width={640} destroyOnHidden>
        {editing && (
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            <Flex gap={8}>
              <Input style={{ width: 70 }} value={editing.icon} onChange={e => setEditing({ ...editing, icon: e.target.value })} />
              <Input placeholder="Template name" value={editing.name} onChange={e => setEditing({ ...editing, name: e.target.value })} />
            </Flex>
            <Input.TextArea placeholder="Description" autoSize={{ minRows: 1, maxRows: 3 }}
              value={editing.description} onChange={e => setEditing({ ...editing, description: e.target.value })} />
            <Flex gap={8}>
              <Select showSearch style={{ flex: 1 }} placeholder="Manager folder" value={editing.manager || undefined}
                options={personaOptions} onChange={v => setEditing({ ...editing, manager: v })}
                filterOption={(i, o) => ((o?.label as string) ?? '').toLowerCase().includes(i.toLowerCase())} />
              <Input style={{ width: 140 }} placeholder="manager role" value={editing.manager_role}
                onChange={e => setEditing({ ...editing, manager_role: e.target.value })} />
            </Flex>
            <Divider style={{ margin: '4px 0' }}>Members</Divider>
            <MemberRows value={editing.members} onChange={m => setEditing({ ...editing, members: m })} />
            <Divider style={{ margin: '4px 0' }}>Behaviour</Divider>
            <BehaviourFields value={editing.settings} onChange={s => setEditing({ ...editing, settings: s })} />
          </Space>
        )}
      </Modal>

      {/* Team settings */}
      <Modal open={!!teamSettings} title="Team settings"
        onCancel={() => setTeamSettings(null)} onOk={saveTeamSettings} okText="Save" width={560} destroyOnHidden>
        {teamSettings && (
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            <div>
              <Text strong style={{ fontSize: 12 }}>Name</Text>
              <Input value={teamSettings.name} onChange={e => setTeamSettings({ ...teamSettings, name: e.target.value })} />
            </div>
            <div>
              <Text strong style={{ fontSize: 12 }}>Manager folder</Text>
              <Select showSearch style={{ width: '100%' }} value={teamSettings.manager_folder || undefined}
                options={personaOptions} onChange={v => setTeamSettings({ ...teamSettings, manager_folder: v })}
                filterOption={(i, o) => ((o?.label as string) ?? '').toLowerCase().includes(i.toLowerCase())} />
            </div>
            <div>
              <Text strong style={{ fontSize: 12 }}>Workspace dir</Text>
              <Input placeholder="/abs/path (optional)" value={teamSettings.workspace_dir ?? ''}
                onChange={e => setTeamSettings({ ...teamSettings, workspace_dir: e.target.value || null })} />
            </div>
            <Text type="secondary" style={{ fontSize: 11 }}>Tip: edit members & tasks from <b>Open</b> → team detail.</Text>
            <Divider style={{ margin: '4px 0' }}>Behaviour</Divider>
            <BehaviourFields value={teamSettings.settings} onChange={s => setTeamSettings({ ...teamSettings, settings: s })} />
          </Space>
        )}
      </Modal>
    </Flex>
  );
};

export default CoworkPanel;
