import React, { useState, useEffect, useCallback } from 'react';
import {
  Typography, Button, Card, Space, Tag, Modal, Form, Input,
  Popconfirm, message, Switch, Divider, Tooltip, Checkbox,
  Radio, Tabs, theme, Flex, Spin,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, SettingOutlined,
  PlayCircleOutlined, DisconnectOutlined, ExclamationCircleOutlined,
  ReloadOutlined, ApiOutlined, CheckCircleOutlined,
  CloseCircleOutlined, SyncOutlined, MinusCircleOutlined,
  ToolOutlined, CodeOutlined, BugOutlined,
  UnorderedListOutlined,
} from '@ant-design/icons';

const { Text, Paragraph } = Typography;
const { TextArea } = Input;

// ===== Types =====

interface McpToolDef {
  name: string;
  description?: string | null;
  inputSchema?: Record<string, any> | null;
}

interface McpServerItem {
  name: string;
  transport: 'stdio' | 'sse' | 'http';
  description?: string | null;
  enabled: boolean;
  use_tools?: string[] | null;
  command?: string | null;
  args: string[];
  env: Record<string, string>;
  url?: string | null;
  headers: Record<string, string>;
  scope: 'user' | 'project';
  status: 'disconnected' | 'connecting' | 'connected' | 'error';
  tools?: McpToolDef[] | null;
  error?: string | null;
  builtin: boolean;
}

// ===== Helpers =====

function StatusIcon({ status }: { status: McpServerItem['status'] }) {
  const { token } = theme.useToken();
  switch (status) {
    case 'connected':    return <CheckCircleOutlined style={{ color: token.colorSuccess, fontSize: 13 }} />;
    case 'connecting':   return <SyncOutlined spin style={{ color: token.colorPrimary, fontSize: 13 }} />;
    case 'error':        return <CloseCircleOutlined style={{ color: token.colorError, fontSize: 13 }} />;
    default:             return <MinusCircleOutlined style={{ color: token.colorTextQuaternary, fontSize: 13 }} />;
  }
}

function StatusTag({ status }: { status: McpServerItem['status'] }) {
  switch (status) {
    case 'connected':  return <Tag color="success">Connected</Tag>;
    case 'connecting': return <Tag color="processing">Connecting</Tag>;
    case 'error':      return <Tag color="error">Error</Tag>;
    default:           return <Tag color="default">Disconnected</Tag>;
  }
}

// ===== Server Card (Browse) =====

function ServerCard({
  server, onConnect, onDisconnect, onFilter, onToggle, onDelete, onToolDetail, onToolList,
}: {
  server: McpServerItem;
  onConnect: () => void;
  onDisconnect: () => void;
  onFilter: () => void;
  onToggle: (val: boolean) => void;
  onDelete: () => void;
  onToolDetail: (tool: McpToolDef) => void;
  onToolList: () => void;
}) {
  const { token } = theme.useToken();

  return (
    <Card
      size="small"
      styles={{ body: { padding: '12px', display: 'flex', flexDirection: 'column', gap: 10 } }}
      style={{
        backgroundColor: token.colorBgContainer,
        borderColor: server.status === 'connected'
          ? token.colorSuccessBorder
          : token.colorBorderSecondary,
        transition: 'border-color 0.2s',
      }}
    >
      {/* Header */}
      <Flex align="flex-start" justify="space-between" gap={8}>
        <Flex align="center" gap={8} style={{ minWidth: 0, flex: 1 }}>
          <div style={{
            width: 32, height: 32, borderRadius: 8, flexShrink: 0,
            backgroundColor: token.colorPrimaryBg,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
          }}>
            <ApiOutlined style={{ color: token.colorPrimary, fontSize: 15 }} />
          </div>
          <div style={{ minWidth: 0 }}>
            <Flex align="center" gap={6}>
              <Text strong style={{ fontSize: 13 }} ellipsis={{ tooltip: server.name }}>
                {server.name}
              </Text>
              {server.builtin && <Tag color="blue" style={{ fontSize: 10, margin: 0 }}>Built-in</Tag>}
            </Flex>
            <Flex align="center" gap={4} style={{ marginTop: 2 }}>
              <StatusIcon status={server.status} />
              <Text type="secondary" style={{ fontSize: 11 }}>{server.transport}</Text>
            </Flex>
          </div>
        </Flex>
        {!server.builtin && (
          <Switch
            size="small"
            checked={server.enabled}
            onChange={onToggle}
          />
        )}
      </Flex>

      {/* Description */}
      {server.description && (
        <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }} ellipsis={{ rows: 2 }}>
          {server.description}
        </Paragraph>
      )}

      {/* Tools */}
      <Flex wrap="wrap" gap={4} align="center">
        {server.tools?.slice(0, 5).map(t => (
          <Tag
            key={t.name}
            style={{ fontSize: 10, margin: 0, cursor: 'pointer' }}
            color={t.inputSchema ? 'blue' : undefined}
            onClick={(e) => { e.stopPropagation(); onToolDetail(t); }}
          >
            {t.name}
          </Tag>
        ))}
        {server.tools && server.tools.length > 5 && (
          <Text
            type="secondary"
            style={{ fontSize: 10, cursor: 'pointer', textDecoration: 'underline' }}
            onClick={(e) => { e.stopPropagation(); onToolList(); }}
          >
            +{server.tools.length - 5} more
          </Text>
        )}
        {server.tools && server.tools.length > 0 && server.tools.length <= 5 && (
          <Text
            type="secondary"
            style={{ fontSize: 10, cursor: 'pointer', marginLeft: 'auto' }}
            onClick={(e) => { e.stopPropagation(); onToolList(); }}
          >
            <UnorderedListOutlined /> View all
          </Text>
        )}
        {!server.tools?.length && (
          <Text type="secondary" style={{ fontSize: 11, fontStyle: 'italic' }}>No tools</Text>
        )}
      </Flex>

      {/* Error */}
      {server.error && (
        <Tooltip title={server.error}>
          <Flex align="center" gap={4}>
            <ExclamationCircleOutlined style={{ color: 'red', fontSize: 11 }} />
            <Text type="danger" style={{ fontSize: 11 }} ellipsis>
              {server.error}
            </Text>
          </Flex>
        </Tooltip>
      )}

      {/* Actions */}
      {!server.builtin && (
        <Flex gap={6} style={{ marginTop: 2 }}>
          {server.status === 'connected' ? (
            <Button size="small" icon={<DisconnectOutlined />} onClick={onDisconnect} style={{ flex: 1, fontSize: 11 }}>
              Disconnect
            </Button>
          ) : (
            <Button size="small" icon={<PlayCircleOutlined />} onClick={onConnect} type="primary" style={{ flex: 1, fontSize: 11 }}>
              Connect
            </Button>
          )}
          <Tooltip title="Tool filters">
            <Button size="small" icon={<SettingOutlined />} onClick={onFilter} />
          </Tooltip>
          <Popconfirm
            title="Delete this MCP server?"
            onConfirm={onDelete}
            okText="Delete"
            cancelText="Cancel"
            okButtonProps={{ danger: true }}
          >
            <Button size="small" type="text" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Flex>
      )}
    </Card>
  );
}

// ===== Manage Row (Manage tab) =====

function ServerRow({
  server, idx, total, onConnect, onDisconnect, onFilter, onToggle, onDelete, onToolList,
}: {
  server: McpServerItem;
  idx: number;
  total: number;
  onConnect: () => void;
  onDisconnect: () => void;
  onFilter: () => void;
  onToggle: (val: boolean) => void;
  onDelete: () => void;
  onToolList: () => void;
}) {
  const { token } = theme.useToken();
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 12,
        padding: '12px 16px',
        borderBottom: idx < total - 1 ? `1px solid ${token.colorBorderSecondary}` : 'none',
      }}
      onMouseEnter={e => { e.currentTarget.style.backgroundColor = token.colorFillAlter; }}
      onMouseLeave={e => { e.currentTarget.style.backgroundColor = 'transparent'; }}
    >
      <Flex align="center" gap={6} style={{ flex: 1, minWidth: 0 }}>
        <StatusIcon status={server.status} />
        <div style={{ minWidth: 0 }}>
          <Flex align="center" gap={6}>
            <Text strong style={{ fontSize: 13 }}>{server.name}</Text>
            {server.builtin && <Tag color="blue" style={{ fontSize: 10, margin: 0 }}>Built-in</Tag>}
            <Tag style={{ fontSize: 10, margin: 0 }}>{server.transport}</Tag>
          </Flex>
          {server.description && (
            <Text type="secondary" style={{ fontSize: 11, display: 'block' }} ellipsis>
              {server.description}
            </Text>
          )}
        </div>
      </Flex>

      <Flex align="center" gap={8} style={{ flexShrink: 0 }}>
        <StatusTag status={server.status} />
        {server.tools && server.tools.length > 0 && (
          <Tag
            color="blue"
            style={{ cursor: 'pointer', margin: 0, fontSize: 11 }}
            onClick={(e) => { e.stopPropagation(); onToolList(); }}
          >
            <UnorderedListOutlined /> {server.tools.length} tool{server.tools.length > 1 ? 's' : ''}
          </Tag>
        )}
        {!server.builtin && (
          <>
            {server.status === 'connected' ? (
              <Button size="small" icon={<DisconnectOutlined />} onClick={onDisconnect}>Disconnect</Button>
            ) : (
              <Button size="small" icon={<PlayCircleOutlined />} onClick={onConnect} type="primary">Connect</Button>
            )}
            <Tooltip title="Tool filters">
              <Button size="small" icon={<SettingOutlined />} onClick={onFilter} />
            </Tooltip>
            <Switch size="small" checked={server.enabled} onChange={onToggle} />
            <Popconfirm
              title="Delete this MCP server?"
              onConfirm={onDelete}
              okText="Delete"
              cancelText="Cancel"
              okButtonProps={{ danger: true }}
            >
              <Button size="small" type="text" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </>
        )}
      </Flex>
    </div>
  );
}

// ===== Add Server Modal =====

function AddServerModal({ open, onClose, onSaved }: {
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);

  const onFinish = async (values: any) => {
    setSaving(true);
    const env: Record<string, string> = {};
    (values.envStr ?? '').split('\n').forEach((line: string) => {
      const idx = line.indexOf('=');
      if (idx > 0) env[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    });
    const headers: Record<string, string> = {};
    (values.headersStr ?? '').split('\n').forEach((line: string) => {
      const idx = line.indexOf(':');
      if (idx > 0) headers[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    });
    try {
      const r = await fetch('/api/mcp-servers', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: values.name.trim(),
          transport: values.transport,
          description: values.description?.trim() || null,
          enabled: values.enabled !== false,
          scope: values.scope,
          command: values.transport === 'stdio' ? values.command?.trim() : null,
          args: values.transport === 'stdio' ? (values.args ?? '').split(' ').filter(Boolean) : [],
          env,
          url: values.transport !== 'stdio' ? values.url?.trim() : null,
          headers,
        }),
      });
      if (!r.ok) {
        const d = await r.json().catch(() => ({}));
        throw new Error(d.error || `HTTP ${r.status}`);
      }
      message.success('MCP server added');
      onSaved();
      onClose();
      form.resetFields();
    } catch (e: any) {
      message.error(e.message || 'Failed to save');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal
      title="Add MCP Server"
      open={open}
      onCancel={onClose}
      footer={null}
      width={560}
      destroyOnClose
    >
      <Form
        form={form}
        layout="vertical"
        onFinish={onFinish}
        initialValues={{ transport: 'stdio', scope: 'user', enabled: true }}
        style={{ marginTop: 20 }}
      >
        <Form.Item name="name" label="Server Name" rules={[{ required: true }]}>
          <Input placeholder="e.g. filesystem-server" />
        </Form.Item>

        <Form.Item name="transport" label="Transport" rules={[{ required: true }]}>
          <Radio.Group optionType="button" buttonStyle="solid">
            <Radio.Button value="stdio">stdio</Radio.Button>
            <Radio.Button value="sse">sse</Radio.Button>
            <Radio.Button value="http">http</Radio.Button>
          </Radio.Group>
        </Form.Item>

        <Form.Item name="description" label="Description">
          <Input placeholder="Optional description" />
        </Form.Item>

        <Form.Item name="scope" label="Scope" rules={[{ required: true }]}>
          <Radio.Group optionType="button" buttonStyle="solid">
            <Radio.Button value="user">User (~/.semaclaw)</Radio.Button>
            <Radio.Button value="project">Project (.semaclaw)</Radio.Button>
          </Radio.Group>
        </Form.Item>

        <Form.Item name="enabled" valuePropName="checked">
          <Checkbox>Enabled on save</Checkbox>
        </Form.Item>

        <Form.Item noStyle shouldUpdate={(prev, curr) => prev.transport !== curr.transport}>
          {({ getFieldValue }) =>
            getFieldValue('transport') === 'stdio' ? (
              <>
                <Form.Item name="command" label="Command" rules={[{ required: true }]}>
                  <Input placeholder="npx -y @modelcontextprotocol/server-filesystem" />
                </Form.Item>
                <Form.Item name="args" label="Arguments (space-separated)">
                  <Input placeholder="/path/to/allowed" />
                </Form.Item>
                <Form.Item name="envStr" label="Environment Variables (KEY=VALUE per line)">
                  <TextArea rows={3} placeholder="API_KEY=xxx" />
                </Form.Item>
              </>
            ) : (
              <>
                <Form.Item name="url" label="URL" rules={[{ required: true }]}>
                  <Input placeholder="http://localhost:8080/sse" />
                </Form.Item>
                <Form.Item name="headersStr" label="Headers (Name: Value per line)">
                  <TextArea rows={3} placeholder="Authorization: Bearer xxx" />
                </Form.Item>
              </>
            )
          }
        </Form.Item>

        <Divider />
        <Flex justify="flex-end" gap={8}>
          <Button onClick={onClose}>Cancel</Button>
          <Button type="primary" htmlType="submit" loading={saving}>Add Server</Button>
        </Flex>
      </Form>
    </Modal>
  );
}

// ===== Tool List Modal =====

function ToolListModal({
  open, server, onClose, onToolDetail,
}: {
  open: boolean;
  server: McpServerItem | null;
  onClose: () => void;
  onToolDetail: (tool: McpToolDef) => void;
}) {
  const { token } = theme.useToken();

  if (!server) return null;

  return (
    <Modal
      title={
        <Space>
          <UnorderedListOutlined />
          <span>Tools</span>
          <Tag color="blue" style={{ margin: 0 }}>{server.name}</Tag>
          <Tag style={{ margin: 0, fontSize: 11 }}>{server.tools?.length ?? 0} total</Tag>
        </Space>
      }
      open={open}
      onCancel={onClose}
      footer={null}
      width={560}
      destroyOnClose
    >
      <div style={{ marginTop: 12 }}>
        {server.tools && server.tools.length > 0 ? (
          <Flex vertical gap={4}>
            {server.tools.map(t => (
              <div
                key={t.name}
                style={{
                  padding: '10px 12px',
                  borderRadius: 6,
                  cursor: 'pointer',
                  border: `1px solid ${token.colorBorderSecondary}`,
                  backgroundColor: token.colorBgContainer,
                  transition: 'background-color 0.2s',
                }}
                onMouseEnter={e => { e.currentTarget.style.backgroundColor = token.colorFillAlter; }}
                onMouseLeave={e => { e.currentTarget.style.backgroundColor = token.colorBgContainer; }}
                onClick={() => {
                  onToolDetail(t);
                  onClose();
                }}
              >
                <Flex align="center" justify="space-between" gap={8}>
                  <Flex align="center" gap={8} style={{ minWidth: 0 }}>
                    <ToolOutlined style={{ color: token.colorPrimary, fontSize: 14, flexShrink: 0 }} />
                    <Text strong style={{ fontSize: 13 }}>{t.name}</Text>
                    {t.inputSchema && (
                      <Tag color="green" style={{ fontSize: 10, margin: 0, flexShrink: 0 }}>schema</Tag>
                    )}
                  </Flex>
                  <Text type="secondary" style={{ fontSize: 11, flexShrink: 0 }}>
                    Click for details
                  </Text>
                </Flex>
                {t.description && (
                  <Text type="secondary" style={{ fontSize: 12, marginTop: 4 }} ellipsis>
                    {t.description}
                  </Text>
                )}
              </div>
            ))}
          </Flex>
        ) : (
          <Flex align="center" justify="center" style={{ padding: '40px 0' }}>
            <Text type="secondary">No tools available</Text>
          </Flex>
        )}
      </div>
    </Modal>
  );
}

// ===== Root =====

export const MCPSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [servers, setServers] = useState<McpServerItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [tab, setTab] = useState<'browse' | 'manage'>('browse');
  const [addOpen, setAddOpen] = useState(false);
  const [editingServer, setEditingServer] = useState<McpServerItem | null>(null);
  const [toolFilter, setToolFilter] = useState('');
  const [filterOpen, setFilterOpen] = useState(false);
  const [savingFilter, setSavingFilter] = useState(false);
  // Tool detail modal
  const [toolOpen, setToolOpen] = useState(false);
  const [toolServer, setToolServer] = useState<McpServerItem | null>(null);
  const [toolDetail, setToolDetail] = useState<McpToolDef | null>(null);
  const [testArgs, setTestArgs] = useState('{}');
  const [testLoading, setTestLoading] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  // Tool list modal
  const [toolListOpen, setToolListOpen] = useState(false);
  const [toolListServer, setToolListServer] = useState<McpServerItem | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/mcp-servers');
      const data = await r.json();
      setServers((data.servers || []) as McpServerItem[]);
    } catch {
      message.error('Failed to load MCP servers');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  const handleConnect = async (name: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/connect`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} connected`);
    } catch { message.error(`Failed to connect ${name}`); }
  };

  const handleDisconnect = async (name: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/disconnect`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} disconnected`);
    } catch { message.error(`Failed to disconnect ${name}`); }
  };

  const handleToggle = async (name: string, enabled: boolean, scope: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/enabled`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled, scope }),
      });
      if (!r.ok) throw new Error();
      await load();
    } catch { message.error(`Failed to update ${name}`); }
  };

  const handleDelete = async (name: string, scope: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`, { method: 'DELETE' });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} removed`);
    } catch { message.error(`Failed to remove ${name}`); }
  };

  const openFilter = (server: McpServerItem) => {
    setEditingServer(server);
    setToolFilter((server.use_tools ?? []).join('\n'));
    setFilterOpen(true);
  };

  const saveFilter = async () => {
    if (!editingServer) return;
    setSavingFilter(true);
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(editingServer.name)}/tools`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          toolNames: toolFilter.split('\n').map(l => l.trim()).filter(Boolean),
          scope: editingServer.scope,
        }),
      });
      if (!r.ok) throw new Error();
      await load();
      setFilterOpen(false);
      message.success('Tool filters saved');
    } catch { message.error('Failed to save filters'); }
    finally { setSavingFilter(false); }
  };

  const handleToolDetail = (server: McpServerItem, tool: McpToolDef) => {
    setToolServer(server);
    setToolDetail(tool);
    setTestArgs('{}');
    setTestResult(null);
    setToolOpen(true);
  };

  const handleToolList = (server: McpServerItem) => {
    setToolListServer(server);
    setToolListOpen(true);
  };

  const handleToolTest = async () => {
    if (!toolServer || !toolDetail) return;
    setTestLoading(true);
    setTestResult(null);
    try {
      let args: any = {};
      try { args = JSON.parse(testArgs); } catch { args = {}; }
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(toolServer.name)}/test`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tool: toolDetail.name, args }),
      });
      const data = await r.json();
      if (data.ok) {
        setTestResult(JSON.stringify(data.result, null, 2));
        message.success(`Tool ${toolDetail.name} executed`);
      } else {
        setTestResult(`Error: ${data.error}`);
        message.error(data.error);
      }
    } catch (e: any) {
      setTestResult(`Error: ${e.message}`);
      message.error(e.message);
    } finally { setTestLoading(false); }
  };

  const connected = servers.filter(s => s.status === 'connected').length;

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
                  {servers.length > 0 && (
                    <span style={{
                      backgroundColor: token.colorFillAlter,
                      color: token.colorTextSecondary,
                      fontSize: '10px',
                      padding: '1px 6px',
                      borderRadius: 10,
                    }}>
                      {servers.length}
                    </span>
                  )}
                </Space>
              ),
            },
          ]}
        />
        <Flex align="center" gap={8}>
          {connected > 0 && (
            <Tag color="success" style={{ margin: 0 }}>{connected} connected</Tag>
          )}
          <Button
            type="text"
            icon={<ReloadOutlined />}
            size="small"
            onClick={() => { setLoading(true); load(); }}
            title="Refresh"
          />
          <Button
            type="primary"
            icon={<PlusOutlined />}
            size="small"
            onClick={() => setAddOpen(true)}
          >
            Add Server
          </Button>
        </Flex>
      </Flex>

      {/* Content */}
      {loading ? (
        <Flex align="center" justify="center" style={{ flex: 1 }}>
          <Spin size="large" />
        </Flex>
      ) : tab === 'browse' ? (
        <div style={{ flex: 1, overflowY: 'auto', padding: 20 }}>
          {servers.length === 0 ? (
            <Flex vertical align="center" justify="center" style={{ padding: '80px 0' }}>
              <div style={{
                backgroundColor: token.colorPrimaryBg,
                width: 48, height: 48, borderRadius: 16,
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                marginBottom: 16,
              }}>
                <ApiOutlined style={{ color: token.colorPrimary, fontSize: 24 }} />
              </div>
              <Text type="secondary">No MCP servers configured.</Text>
              <Button type="primary" icon={<PlusOutlined />} style={{ marginTop: 16 }} onClick={() => setAddOpen(true)}>
                Add your first server
              </Button>
            </Flex>
          ) : (
            <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))' }}>
              {servers.map(srv => (
                <ServerCard
                  key={srv.name}
                  server={srv}
                  onConnect={() => handleConnect(srv.name)}
                  onDisconnect={() => handleDisconnect(srv.name)}
                  onFilter={() => openFilter(srv)}
                  onToggle={val => handleToggle(srv.name, val, srv.scope)}
                  onDelete={() => handleDelete(srv.name, srv.scope)}
                  onToolDetail={(tool) => handleToolDetail(srv, tool)}
                  onToolList={() => handleToolList(srv)}
                />
              ))}
            </div>
          )}
        </div>
      ) : (
        <div style={{ flex: 1, overflowY: 'auto', padding: 20 }}>
          {servers.length === 0 ? (
            <Flex vertical align="center" justify="center" style={{ padding: '80px 0' }}>
              <Text type="secondary">No servers yet.</Text>
            </Flex>
          ) : (
            <Card
              size="small"
              styles={{ body: { padding: 0 } }}
              style={{ backgroundColor: token.colorBgContainer, borderColor: token.colorBorderSecondary, overflow: 'hidden' }}
            >
              {servers.map((srv, idx) => (
                <ServerRow
                  key={srv.name}
                  server={srv}
                  idx={idx}
                  total={servers.length}
                  onConnect={() => handleConnect(srv.name)}
                  onDisconnect={() => handleDisconnect(srv.name)}
                  onFilter={() => openFilter(srv)}
                  onToggle={val => handleToggle(srv.name, val, srv.scope)}
                  onDelete={() => handleDelete(srv.name, srv.scope)}
                  onToolList={() => handleToolList(srv)}
                />
              ))}
            </Card>
          )}
        </div>
      )}

      {/* Add Server Modal */}
      <AddServerModal open={addOpen} onClose={() => setAddOpen(false)} onSaved={load} />

      {/* Tool Filter Modal */}
      <Modal
        title={`Tool Filters — ${editingServer?.name}`}
        open={filterOpen}
        onCancel={() => setFilterOpen(false)}
        onOk={saveFilter}
        okText="Save Filters"
        confirmLoading={savingFilter}
      >
        <Text type="secondary" style={{ fontSize: 13 }}>
          One tool name per line. Leave empty to allow all tools.
        </Text>
        <TextArea
          rows={7}
          value={toolFilter}
          onChange={e => setToolFilter(e.target.value)}
          style={{ marginTop: 12, fontFamily: 'monospace', fontSize: 12 }}
          placeholder={'tool_name_1\ntool_name_2'}
        />
        {editingServer?.tools && editingServer.tools.length > 0 && (
          <div style={{ marginTop: 12 }}>
            <Text type="secondary" style={{ fontSize: 11 }}>Available tools:</Text>
            <Flex wrap="wrap" gap={4} style={{ marginTop: 6 }}>
              {editingServer.tools.map(t => (
                <Tag
                  key={t.name}
                  style={{ cursor: 'pointer', fontSize: 11 }}
                  onClick={() => setToolFilter(prev =>
                    prev ? `${prev}\n${t.name}` : t.name
                  )}
                >
                  + {t.name}
                </Tag>
              ))}
            </Flex>
          </div>
        )}
      </Modal>

      {/* Tool List Modal */}
      <ToolListModal
        open={toolListOpen}
        server={toolListServer}
        onClose={() => setToolListOpen(false)}
        onToolDetail={(tool) => {
          if (toolListServer) handleToolDetail(toolListServer, tool);
        }}
      />

      {/* Tool Detail Modal */}
      <Modal
        title={
          <Space>
            <ToolOutlined />
            <span>{toolDetail?.name}</span>
            {toolServer && <Tag color="blue" style={{ margin: 0 }}>{toolServer.name}</Tag>}
          </Space>
        }
        open={toolOpen}
        onCancel={() => setToolOpen(false)}
        footer={null}
        width={640}
        destroyOnClose
      >
        {toolDetail && (
          <Flex vertical gap={16} style={{ marginTop: 12 }}>
            {/* Description */}
            <div>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Description</Text>
              <Text style={{ fontSize: 13 }}>{toolDetail.description || 'No description'}</Text>
            </div>

            {/* Input Schema */}
            {toolDetail.inputSchema && (
              <div>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>
                  <CodeOutlined /> Input Schema
                </Text>
                <pre style={{
                  background: '#f6f8fa', padding: 12, borderRadius: 6,
                  fontSize: 12, fontFamily: 'Menlo, Monaco, monospace',
                  maxHeight: 200, overflow: 'auto', margin: 0,
                  border: '1px solid #f0f0f0',
                }}>
                  {JSON.stringify(toolDetail.inputSchema, null, 2)}
                </pre>
              </div>
            )}

            {/* Test section */}
            {toolServer?.status === 'connected' && !toolServer.builtin && (
              <div>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 8 }}>
                  <BugOutlined /> Test Tool
                </Text>
                <TextArea
                  rows={5}
                  value={testArgs}
                  onChange={e => setTestArgs(e.target.value)}
                  style={{ fontFamily: 'Menlo, Monaco, monospace', fontSize: 12 }}
                  placeholder='{"key": "value"}'
                />
                <Flex justify="space-between" align="center" style={{ marginTop: 8 }}>
                  <Text type="secondary" style={{ fontSize: 11 }}>Enter arguments as JSON</Text>
                  <Button
                    type="primary"
                    size="small"
                    icon={<BugOutlined />}
                    onClick={handleToolTest}
                    loading={testLoading}
                  >
                    Run Test
                  </Button>
                </Flex>
                {testResult !== null && (
                  <pre style={{
                    background: testResult.startsWith('Error') ? '#fff2f0' : '#f6ffed',
                    padding: 12, borderRadius: 6,
                    fontSize: 12, fontFamily: 'Menlo, Monaco, monospace',
                    maxHeight: 300, overflow: 'auto', marginTop: 12,
                    border: `1px solid ${testResult.startsWith('Error') ? '#ffccc7' : '#b7eb8f'}`,
                  }}>
                    {testResult}
                  </pre>
                )}
              </div>
            )}
            {(!toolServer || toolServer.status !== 'connected') && (
              <Text type="secondary" style={{ fontSize: 12, fontStyle: 'italic' }}>
                Connect the server to test this tool.
              </Text>
            )}
            {toolServer?.builtin && (
              <Text type="secondary" style={{ fontSize: 12, fontStyle: 'italic' }}>
                Built-in server tools execute via the agent. Use the Chat to invoke them.
              </Text>
            )}
          </Flex>
        )}
      </Modal>
    </Flex>
  );
};
