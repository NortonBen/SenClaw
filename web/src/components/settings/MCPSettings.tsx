import React, { useState, useEffect, useCallback } from 'react';
import {
  Typography,
  Button,
  Card,
  Space,
  Tag,
  Modal,
  Form,
  Input,
  Select,
  Popconfirm,
  message,
  Switch,
  Divider,
  Tooltip,
  Checkbox,
  Radio,
  Row,
  Col,
  Tabs,
  theme,
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  SettingOutlined,
  PlayCircleOutlined,
  DisconnectOutlined,
  ExclamationCircleOutlined,
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;
const { Option } = Select;
const { TextArea } = Input;

// ===== Types =====

interface McpToolDef {
  name: string;
  description?: string | null;
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

type Transport = 'stdio' | 'sse' | 'http';

export const MCPSettings: React.FC = () => {
  const [servers, setServers] = useState<McpServerItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [form] = Form.useForm();
  const [saving, setSaving] = useState(false);
  const [editingServer, setEditingServer] = useState<McpServerItem | null>(null);
  const [isFilterModalOpen, setIsFilterModalOpen] = useState(false);
  const [toolFilter, setToolFilter] = useState('');
  const [activeTab, setActiveTab] = useState('browse');

  const { token } = theme.useToken();

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/mcp-servers');
      const data = await r.json();
      setServers((data.servers || []) as McpServerItem[]);
    } catch (e) {
      message.error('Failed to load MCP servers');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const handleConnect = async (name: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/connect`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} connected`);
    } catch {
      message.error(`Failed to connect ${name}`);
    }
  };

  const handleDisconnect = async (name: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/disconnect`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} disconnected`);
    } catch {
      message.error(`Failed to disconnect ${name}`);
    }
  };

  const toggleEnabled = async (name: string, enabled: boolean, scope: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/enabled`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled, scope }),
      });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} ${enabled ? 'enabled' : 'disabled'}`);
    } catch {
      message.error(`Failed to update ${name}`);
    }
  };

  const handleDelete = async (name: string, scope: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`, { method: 'DELETE' });
      if (!r.ok) throw new Error();
      await load();
      message.success(`${name} removed`);
    } catch {
      message.error(`Failed to remove ${name}`);
    }
  };

  const saveTools = async (name: string, scope: string, toolNames: string[]) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/tools`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ toolNames, scope }),
      });
      if (!r.ok) throw new Error();
      await load();
      setIsFilterModalOpen(false);
      message.success(`Tool filter updated for ${name}`);
    } catch {
      message.error(`Failed to update tools for ${name}`);
    }
  };

  const onFinish = async (values: any) => {
    setSaving(true);
    const env: Record<string, string> = {};
    if (values.envStr) {
      values.envStr.split('\n').forEach((line: string) => {
        const idx = line.indexOf('=');
        if (idx > 0) env[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
      });
    }
    const headers: Record<string, string> = {};
    if (values.headersStr) {
      values.headersStr.split('\n').forEach((line: string) => {
        const idx = line.indexOf(':');
        if (idx > 0) headers[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
      });
    }
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
          args: values.transport === 'stdio' ? values.args?.split(' ').filter(Boolean) : [],
          env,
          url: values.transport !== 'stdio' ? values.url?.trim() : null,
          headers,
        }),
      });
      if (!r.ok) {
        const data = await r.json().catch(() => ({}));
        throw new Error(data.error || `HTTP ${r.status}`);
      }
      setIsModalOpen(false);
      load();
      message.success('MCP server added');
    } catch (e: any) {
      message.error(e.message || 'Failed to save');
    } finally {
      setSaving(false);
    }
  };

  const getStatusTag = (status: string) => {
    switch (status) {
      case 'connected':
        return <Tag color="success">Connected</Tag>;
      case 'connecting':
        return <Tag color="processing">Connecting</Tag>;
      case 'error':
        return <Tag color="error">Error</Tag>;
      case 'disconnected':
        return <Tag color="default">Disconnected</Tag>;
      default:
        return <Tag>{status}</Tag>;
    }
  };

  // Card component for a single MCP server
  const ServerCard: React.FC<{ server: McpServerItem }> = ({ server }) => {
    const actions = [];
    if (!server.builtin) {
      actions.push(
        server.status === 'connected' ? (
          <Button
            size="small"
            icon={<DisconnectOutlined />}
            onClick={() => handleDisconnect(server.name)}
          >
            Disconnect
          </Button>
        ) : (
          <Button
            size="small"
            icon={<PlayCircleOutlined />}
            onClick={() => handleConnect(server.name)}
          >
            Connect
          </Button>
        ),
      );
      actions.push(
        <Button
          size="small"
          icon={<SettingOutlined />}
          onClick={() => {
            setEditingServer(server);
            setToolFilter((server.use_tools || []).join('\n'));
            setIsFilterModalOpen(true);
          }}
        >
          Filters
        </Button>,
      );
      actions.push(
        <Switch
          size="small"
          checked={server.enabled}
          onChange={(val) => toggleEnabled(server.name, val, server.scope)}
          aria-label={`Toggle ${server.name} enabled`}
        />,
      );
      actions.push(
        <Popconfirm
          title="Delete this MCP server?"
          onConfirm={() => handleDelete(server.name, server.scope)}
          okText="Yes"
          cancelText="No"
          okButtonProps={{ danger: true }}
        >
          <Button size="small" type="text" danger icon={<DeleteOutlined />} aria-label="Delete" />
        </Popconfirm>,
      );
    }

    return (
      <Card
        hoverable
        style={{
          borderRadius: token.borderRadiusLG,
          border: `1px solid ${token.colorBorder}`,
          background: token.colorBgContainer,
        }}
        title={
          <Space direction="vertical" size={0}>
            <Space>
              <Text strong>{server.name}</Text>
              {server.builtin && <Tag color="blue">Built-in</Tag>}
              <Tag>{server.transport}</Tag>
            </Space>
            {server.description && (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {server.description}
              </Text>
            )}
          </Space>
        }
        extra={getStatusTag(server.status)}
        actions={actions}
        tabIndex={0}
        role="listitem"
      >
        <Space wrap size={[4, 4]}>
          {server.tools?.slice(0, 5).map((t) => (
            <Tag key={t.name} style={{ fontSize: 10, margin: 0 }}>{t.name}</Tag>
          ))}
          {server.tools && server.tools.length > 5 && (
            <Text type="secondary" style={{ fontSize: 10 }}>+{server.tools.length - 5} more</Text>
          )}
          {!server.tools?.length && (
            <Text type="secondary" style={{ fontSize: 11 }}>No tools</Text>
          )}
        </Space>
        {server.error && (
          <Tooltip title={server.error}>
            <Text type="danger" style={{ fontSize: 11 }}>
              <ExclamationCircleOutlined /> Error info
            </Text>
          </Tooltip>
        )}
      </Card>
    );
  };

  return (
    <div style={{ maxWidth: 1200, margin: '0 auto', padding: 24 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 32 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>MCP Servers</Title>
          <Text type="secondary">Manage Model Context Protocol servers to extend your agent's capabilities.</Text>
        </div>
        <Button
          type="primary"
          icon={<PlusOutlined />}
          onClick={() => { form.resetFields(); setIsModalOpen(true); }}
          style={{ borderRadius: 8, height: 40 }}
        >
          Add Server
        </Button>
      </div>

      <Tabs
        activeKey={activeTab}
        onChange={(key) => setActiveTab(key)}
        items={[
          {
            key: 'browse',
            label: 'Browse',
            children: (
              <Row gutter={[16, 16]} role="list" aria-label="MCP server list">
                {servers.map((srv) => (
                  <Col key={srv.name} xs={24} sm={12} md={8} lg={6}>
                    <ServerCard server={srv} />
                  </Col>
                ))}
                {loading && <Text>Loading...</Text>}
              </Row>
            ),
          },
          {
            key: 'manage',
            label: 'Manage',
            children: (
              <div>
                {/* Add Server Modal */}
                <Modal
                  title="Add MCP Server"
                  open={isModalOpen}
                  onCancel={() => setIsModalOpen(false)}
                  footer={null}
                  width={600}
                  destroyOnClose
                >
                  <Form
                    form={form}
                    layout="vertical"
                    onFinish={onFinish}
                    initialValues={{ transport: 'stdio', scope: 'user', enabled: true }}
                    style={{ marginTop: 24 }}
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
                      <Checkbox>Enabled</Checkbox>
                    </Form.Item>

                    <Form.Item noStyle shouldUpdate={(prev, curr) => prev.transport !== curr.transport}>
                      {({ getFieldValue }) => {
                        const transport = getFieldValue('transport');
                        if (transport === 'stdio') {
                          return (
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
                          );
                        }
                        return (
                          <>
                            <Form.Item name="url" label="URL" rules={[{ required: true }]}>
                              <Input placeholder="http://localhost:8080/sse" />
                            </Form.Item>
                            <Form.Item name="headersStr" label="Headers (Name: Value per line)">
                              <TextArea rows={3} placeholder="Authorization: Bearer xxx" />
                            </Form.Item>
                          </>
                        );
                      }}
                    </Form.Item>

                    <Divider />
                    <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
                      <Button onClick={() => setIsModalOpen(false)}>Cancel</Button>
                      <Button type="primary" htmlType="submit" loading={saving}>
                        Add Server
                      </Button>
                    </div>
                  </Form>
                </Modal>
              </div>
            ),
          },
        ]}
      />

      {/* Tool Filter Modal */}
      <Modal
        title={`Tool Filters: ${editingServer?.name}`}
        open={isFilterModalOpen}
        onCancel={() => setIsFilterModalOpen(false)}
        onOk={() => {
          if (editingServer) {
            const names = toolFilter.split('\n').map((l) => l.trim()).filter(Boolean);
            saveTools(editingServer.name, editingServer.scope, names);
          }
        }}
        okText="Save Filters"
      >
        <div style={{ marginTop: 16 }}>
          <Text type="secondary" style={{ fontSize: 13 }}>
            Enter one tool name per line to restrict which tools are exposed.
            Leave empty to allow all tools from this server.
          </Text>
          <TextArea
            rows={6}
            value={toolFilter}
            onChange={(e) => setToolFilter(e.target.value)}
            style={{ marginTop: 12, fontFamily: 'monospace' }}
            placeholder="tool_name_1\ntool_name_2"
          />
        </div>
      </Modal>
    </div>
  );
};
