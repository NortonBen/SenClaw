import { useState, useEffect, useRef, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import {
  theme,
  Typography,
  Button,
  Input,
  Switch,
  Tag,
  Card,
  Space,
  Avatar,
  Empty,
  Spin,
  Modal,
  message,
  Tooltip,
  Tabs,
  Flex
} from 'antd';
import {
  UserOutlined,
  SearchOutlined,
  PlusOutlined,
  ArrowLeftOutlined,
  EditOutlined,
  ReloadOutlined,
  ExclamationCircleOutlined
} from '@ant-design/icons';

const { Text, Title, Paragraph } = Typography;
const { TextArea } = Input;

// ─── Types ───────────────────────────────────────────────────────────────────

interface Subagent {
  name: string;
  description: string;
  tools: string[] | null;
  model: string | null;
  maxConcurrent: number;
  filePath: string;
  disabled: boolean;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(text || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

// ─── Card grid ────────────────────────────────────────────────────────────────

function SubagentCard({ agent, onClick }: { agent: Subagent; onClick: () => void }) {
  const { token } = theme.useToken();

  return (
    <Card
      hoverable
      size="small"
      onClick={onClick}
      style={{
        height: '100%',
        backgroundColor: token.colorBgContainer,
        borderColor: token.colorBorderSecondary,
      }}
      styles={{ body: { padding: '12px', height: '100%', display: 'flex', flexDirection: 'column' } }}
    >
      <Flex vertical gap={8} style={{ height: '100%' }}>
        {/* Header: icon + name */}
        <Flex align="center" gap={10}>
          <div style={{
            backgroundColor: token.colorPrimaryBg,
            width: 32,
            height: 32,
            borderRadius: 8,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            flexShrink: 0
          }}>
            <UserOutlined style={{ color: token.colorPrimary, fontSize: 16 }} />
          </div>
          <Text strong style={{ fontSize: token.fontSizeSM, flex: 1 }} ellipsis={{ tooltip: agent.name }}>
            {agent.name}
          </Text>
        </Flex>

        {/* Description */}
        <div style={{ flex: 1 }}>
          <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }} ellipsis={{ rows: 2 }}>
            {agent.description || <span style={{ fontStyle: 'italic', opacity: 0.5 }}>No description</span>}
          </Paragraph>
        </div>

        {/* Footer */}
        <Flex align="center" gap={6} wrap="wrap">
          <Tag color="purple" style={{ margin: 0, fontSize: '10px' }}>
            max {agent.maxConcurrent}
          </Tag>
          {agent.disabled && (
            <Tag color="error" style={{ margin: 0, fontSize: '10px' }}>off</Tag>
          )}
        </Flex>
      </Flex>
    </Card>
  );
}

// ─── Detail view ──────────────────────────────────────────────────────────────

function SubagentDetail({ agent, onBack, onToggleDisabled }: {
  agent: Subagent;
  onBack: () => void;
  onToggleDisabled: (name: string, disabled: boolean) => void;
}) {
  const { token } = theme.useToken();
  const [editing, setEditing] = useState(false);
  const [readme, setReadme] = useState<string | null>(null);
  const [draftContent, setDraftContent] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fetch(`/api/subagents/${encodeURIComponent(agent.name)}/readme`)
      .then(r => r.ok ? r.text() : '')
      .then(setReadme)
      .catch(() => setReadme(''));
  }, [agent.name]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await fetch(`/api/subagents/${encodeURIComponent(agent.name)}/readme`, {
        method: 'PUT', headers: { 'Content-Type': 'text/plain' }, body: draftContent,
      });
      setReadme(draftContent);
      setEditing(false);
      message.success('Persona saved');
    } catch (e) {
      message.error('Failed to save persona');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Flex vertical style={{ height: '100%', background: token.colorBgLayout }}>
      {/* Top bar */}
      <Flex
        align="center"
        gap="middle"
        style={{
          padding: '12px 20px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          backgroundColor: token.colorBgContainer,
          flexShrink: 0
        }}
      >
        <Button
          type="text"
          icon={<ArrowLeftOutlined />}
          onClick={onBack}
          size="small"
        >
          Back
        </Button>
        <div style={{ width: '1px', height: '16px', backgroundColor: token.colorBorderSecondary }} />

        {/* Name + meta */}
        <Flex align="center" gap="small" style={{ flex: 1, minWidth: 0 }}>
          <Text strong ellipsis={{ tooltip: agent.name }}>{agent.name}</Text>
          <Tag color="purple" style={{ margin: 0, fontSize: '10px' }}>max {agent.maxConcurrent}</Tag>
          {agent.model && (
            <Text type="secondary" style={{ fontSize: 12 }}>{agent.model}</Text>
          )}
          {agent.disabled && (
            <Tag color="error" style={{ margin: 0, fontSize: '10px' }}>disabled</Tag>
          )}
        </Flex>

        {/* Actions */}
        <Space size="small">
          {editing ? (
            <>
              <Button size="small" onClick={() => setEditing(false)}>Cancel</Button>
              <Button size="small" type="primary" onClick={handleSave} loading={saving}>
                Save
              </Button>
            </>
          ) : (
            <Button
              size="small"
              icon={<EditOutlined />}
              onClick={() => { setDraftContent(readme ?? ''); setEditing(true); }}
            >
              Edit
            </Button>
          )}
          <Switch
            checked={!agent.disabled}
            onChange={() => onToggleDisabled(agent.name, agent.disabled)}
            size="small"
          />
        </Space>
      </Flex>

      {/* Path */}
      <div
        style={{
          backgroundColor: token.colorFillAlter,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          padding: '6px 20px',
          flexShrink: 0
        }}
      >
        <Text type="secondary" style={{ fontSize: '10px', fontFamily: 'monospace' }}>
          {agent.filePath}
        </Text>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflowY: 'auto', backgroundColor: token.colorBgContainer }}>
        {readme === null ? (
          <Flex align="center" justify="center" style={{ height: '300px' }}>
            <Spin tip="Loading persona..." />
          </Flex>
        ) : editing ? (
          <TextArea
            style={{
              height: '100%',
              padding: '20px',
              fontFamily: 'monospace',
              fontSize: token.fontSizeSM,
              resize: 'none',
              border: 'none',
              backgroundColor: 'transparent'
            }}
            value={draftContent}
            onChange={e => setDraftContent(e.target.value)}
            spellCheck={false}
          />
        ) : readme ? (
          <div style={{ padding: '20px 24px' }}>
            <div style={{
              color: token.colorText,
              fontSize: token.fontSizeSM,
              lineHeight: 1.6
            }}>
              <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>{readme}</ReactMarkdown>
            </div>
          </div>
        ) : (
          <Empty description="No persona content" style={{ marginTop: '64px' }} />
        )}
      </div>
    </Flex>
  );
}

// ─── Create editor ────────────────────────────────────────────────────────────

const NEW_TEMPLATE = `---
name:
description:
max_concurrent: 3
---

**Calibrate your effort to the task.** For straightforward, well-defined requests, respond directly and efficiently — avoid over-research, over-plan, or over-elaborate. For complex or ambiguous tasks, engage your full methodology. Always strike the right balance between efficiency and output quality, guided by the intrinsic nature and complexity of the task.

`;

function CreateSubagentEditor({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const { token } = theme.useToken();
  const [content, setContent] = useState(NEW_TEMPLATE);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  const isDirty = content !== NEW_TEMPLATE;

  const handleClose = () => {
    if (isDirty) {
      Modal.confirm({
        title: 'Discard changes?',
        content: 'You have unsaved changes. Are you sure you want to close?',
        okText: 'Discard',
        okType: 'danger',
        cancelText: 'Keep editing',
        onOk: onClose
      });
    } else {
      onClose();
    }
  };

  const extractName = (text: string): string => {
    const match = text.match(/^name:\s*(.+)$/m);
    return match ? match[1].trim() : '';
  };

  const handleSave = async () => {
    const name = extractName(content);
    if (!name) {
      setError('Please fill in the "name" field in the frontmatter.');
      return;
    }
    setSaving(true);
    setError('');
    try {
      const res = await fetch('/api/subagents/create', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name, content }),
      });
      if (!res.ok) {
        const data = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
        setError(data.error || `HTTP ${res.status}`);
        return;
      }
      message.success('Virtual agent created');
      onCreated();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Flex vertical style={{ height: '100%' }}>
      {/* Top bar */}
      <Flex
        align="center"
        gap="middle"
        style={{
          padding: '12px 20px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          backgroundColor: token.colorBgContainer,
          flexShrink: 0
        }}
      >
        <Flex align="center" gap="small" style={{ flex: 1 }}>
          <Avatar
            size={24}
            icon={<UserOutlined style={{ fontSize: '12px' }} />}
            style={{ backgroundColor: token.colorPrimaryBg, color: token.colorPrimary }}
          />
          <Text strong style={{ fontSize: '14px' }}>New Virtual Agent</Text>
        </Flex>
        <Button type="text" size="small" onClick={handleClose}>Cancel</Button>
      </Flex>

      {/* Editor */}
      <div style={{ flex: 1, overflow: 'hidden', position: 'relative' }}>
        <TextArea
          style={{
            height: '100%',
            padding: '20px',
            fontFamily: token.fontFamilyCode,
            fontSize: '12px',
            resize: 'none',
            border: 'none',
            backgroundColor: token.colorBgContainer
          }}
          value={content}
          onChange={e => { setContent(e.target.value); setError(''); }}
          spellCheck={false}
          placeholder="Edit your persona file here..."
        />

        {/* Save button — bottom right */}
        <Flex vertical align="end" gap="middle" style={{ position: 'absolute', bottom: '24px', right: '24px' }}>
          {error && (
            <div
              style={{
                backgroundColor: token.colorErrorBg,
                border: `1px solid ${token.colorErrorBorder}`,
                borderRadius: '8px',
                padding: '8px 12px',
                maxWidth: '300px'
              }}
            >
              <Space size={6}>
                <ExclamationCircleOutlined style={{ color: token.colorError }} />
                <Text type="danger" style={{ fontSize: '12px' }}>{error}</Text>
              </Space>
            </div>
          )}
          <Button
            type="primary"
            size="large"
            onClick={handleSave}
            loading={saving}
            style={{
              borderRadius: '12px',
              height: '44px',
              padding: '0 24px',
              boxShadow: token.boxShadow
            }}
          >
            {saving ? 'Saving...' : 'Save Agent'}
          </Button>
        </Flex>
      </div>
    </Flex>
  );
}

// ─── Browse tab ───────────────────────────────────────────────────────────────

function BrowseTab({ agents, onRefreshAgents, onReloadSuccess }: { agents: Subagent[]; onRefreshAgents: () => void; onReloadSuccess: () => void }) {
  const { token } = theme.useToken();
  const [query, setQuery] = useState('');
  const [selectedAgent, setSelectedAgent] = useState<Subagent | null>(null);
  const [creating, setCreating] = useState(false);

  const handleToggleDisabled = async (name: string, currentlyDisabled: boolean) => {
    const action = currentlyDisabled ? 'enable' : 'disable';
    try {
      await apiFetch(`/api/subagents/${encodeURIComponent(name)}/${action}`, { method: 'POST' });
      onRefreshAgents();
      onReloadSuccess();
      if (selectedAgent?.name === name) {
        setSelectedAgent(prev => prev ? { ...prev, disabled: !currentlyDisabled } : null);
      }
    } catch { /* ignore */ }
  };

  const localMatched = query.trim()
    ? agents.filter(a =>
      a.name.toLowerCase().includes(query.toLowerCase()) ||
      a.description.toLowerCase().includes(query.toLowerCase())
    )
    : agents;

  if (creating) {
    return (
      <CreateSubagentEditor
        onClose={() => setCreating(false)}
        onCreated={() => { setCreating(false); onRefreshAgents(); onReloadSuccess(); }}
      />
    );
  }

  if (selectedAgent) {
    return (
      <SubagentDetail
        agent={selectedAgent}
        onBack={() => setSelectedAgent(null)}
        onToggleDisabled={handleToggleDisabled}
      />
    );
  }

  return (
    <Flex vertical style={{ height: '100%' }}>
      {/* Search bar + Add button */}
      <Flex
        align="center"
        gap="middle"
        style={{
          padding: '12px 20px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          backgroundColor: token.colorBgContainer,
          flexShrink: 0
        }}
      >
        <Input
          placeholder="Search persona name or description..."
          prefix={<SearchOutlined style={{ color: token.colorTextPlaceholder }} />}
          value={query}
          onChange={e => setQuery(e.target.value)}
          style={{
            borderRadius: '8px',
            backgroundColor: token.colorBgLayout,
            flex: 1
          }}
          allowClear
        />
        <Button
          type="primary"
          icon={<PlusOutlined />}
          onClick={() => setCreating(true)}
          style={{ borderRadius: '8px' }}
        >
          New
        </Button>
      </Flex>

      <div style={{ flex: 1, overflowY: 'auto', padding: '20px' }}>
        {localMatched.length === 0 ? (
          <Flex vertical align="center" justify="center" style={{ height: '300px' }}>
            <Empty
              image={<UserOutlined style={{ fontSize: '48px', color: token.colorTextDisabled }} />}
              description={
                <Space direction="vertical" size={2}>
                  <Text type="secondary">{query.trim() ? `No agents match "${query}"` : 'No virtual agents found'}</Text>
                  {!query.trim() && (
                    <Text type="secondary" style={{ fontSize: '11px' }}>
                      Add .md persona files to ~/semaclaw/virtual-agents/
                    </Text>
                  )}
                </Space>
              }
            />
          </Flex>
        ) : (
          <div
            style={{
              display: 'grid',
              gap: '16px',
              gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))'
            }}
          >
            {localMatched.map(a => (
              <SubagentCard key={a.name} agent={a} onClick={() => setSelectedAgent(a)} />
            ))}
          </div>
        )}
      </div>
    </Flex>
  );
}

// ─── Manage tab ───────────────────────────────────────────────────────────────

function ManageTab({ agents, onRefreshAgents, onReloadSuccess }: { agents: Subagent[]; onRefreshAgents: () => void; onReloadSuccess: () => void }) {
  const { token } = theme.useToken();
  const [toggling, setToggling] = useState<string | null>(null);

  const handleToggle = async (name: string, currentlyDisabled: boolean) => {
    setToggling(name);
    const action = currentlyDisabled ? 'enable' : 'disable';
    try {
      await apiFetch(`/api/subagents/${encodeURIComponent(name)}/${action}`, { method: 'POST' });
      onRefreshAgents();
      onReloadSuccess();
    } catch { /* ignore */ } finally {
      setToggling(null);
    }
  };

  if (agents.length === 0) {
    return (
      <Flex vertical align="center" justify="center" style={{ height: '300px' }}>
        <Empty description="No virtual agents found" />
      </Flex>
    );
  }

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '20px' }}>
      <Card
        size="small"
        styles={{ body: { padding: 0 } }}
        style={{
          backgroundColor: token.colorBgContainer,
          borderColor: token.colorBorderSecondary,
          overflow: 'hidden',
          borderRadius: '12px'
        }}
      >
        {agents.map((agent, i) => (
          <Flex
            key={agent.name}
            align="center"
            gap="middle"
            style={{
              padding: '12px 16px',
              borderBottom: i === agents.length - 1 ? 'none' : `1px solid ${token.colorBorderSecondary}`,
              transition: 'background-color 0.2s',
              cursor: 'default'
            }}
            onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = token.colorFillAlter; }}
            onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent'; }}
          >
            <div style={{
              backgroundColor: token.colorPrimaryBg,
              width: 32,
              height: 32,
              borderRadius: 8,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0
            }}>
              <UserOutlined style={{ color: token.colorPrimary, fontSize: 16 }} />
            </div>
            <Flex vertical style={{ flex: 1, minWidth: 0 }}>
              <Space size={8}>
                <Text strong style={{
                  color: agent.disabled ? token.colorTextDisabled : token.colorText,
                  fontSize: token.fontSizeSM
                }}>
                  {agent.name}
                </Text>
                <Tag color="purple" style={{ margin: 0, fontSize: '10px' }}>
                  max {agent.maxConcurrent}
                </Tag>
              </Space>
              {agent.description && (
                <Text type="secondary" style={{ fontSize: 12 }} ellipsis>
                  {agent.description}
                </Text>
              )}
            </Flex>
            <Switch
              checked={!agent.disabled}
              onChange={() => handleToggle(agent.name, agent.disabled)}
              loading={toggling === agent.name}
              size="small"
            />
          </Flex>
        ))}
      </Card>
    </div>
  );
}

// ─── Root ─────────────────────────────────────────────────────────────────────

export function SubagentsPanel() {
  const { token } = theme.useToken();
  const [tab, setTab] = useState<'browse' | 'manage'>('browse');
  const [agents, setAgents] = useState<Subagent[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchAgents = useCallback(async () => {
    try {
      const data = await apiFetch<{ subagents: Subagent[] }>('/api/subagents');
      setAgents(data.subagents);
    } catch { /* ignore */ } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchAgents(); }, [fetchAgents]);

  return (
    <Flex vertical style={{ height: '100%', overflow: 'hidden' }}>
      {/* Tab bar */}
      <Flex
        align="center"
        justify="space-between"
        style={{
          padding: '0 20px',
          backgroundColor: token.colorBgContainer,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          flexShrink: 0,
        }}
      >
        <Tabs
          activeKey={tab}
          onChange={k => setTab(k as 'browse' | 'manage')}
          style={{ marginBottom: -1 }}
          items={[
            { key: 'browse', label: 'Browse' },
            {
              key: 'manage',
              label: (
                <Space size={6}>
                  Manage
                  {agents.length > 0 && (
                    <span style={{
                      backgroundColor: token.colorFillAlter,
                      color: token.colorTextSecondary,
                      fontSize: '10px',
                      padding: '1px 6px',
                      borderRadius: 10,
                    }}>
                      {agents.length}
                    </span>
                  )}
                </Space>
              ),
            },
          ]}
        />
        <Tooltip title="Refresh list">
          <Button
            type="text"
            icon={<ReloadOutlined />}
            size="small"
            onClick={() => { setLoading(true); fetchAgents(); }}
          />
        </Tooltip>
      </Flex>

      {/* Content */}
      {loading ? (
        <Flex align="center" justify="center" style={{ flex: 1 }}>
          <Spin size="large" />
        </Flex>
      ) : tab === 'browse' ? (
        <BrowseTab agents={agents} onRefreshAgents={fetchAgents} onReloadSuccess={() => {}} />
      ) : (
        <ManageTab agents={agents} onRefreshAgents={fetchAgents} onReloadSuccess={() => {}} />
      )}
    </Flex>
  );
}
