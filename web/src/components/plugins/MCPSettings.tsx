import React, { useState, useEffect, useCallback } from 'react';
import {
  Typography, Button, Card, Space, Tag, Modal, Form, Input,
  Popconfirm, message, Switch, Divider, Tooltip, Checkbox,
  Radio, Tabs, theme, Flex, Spin, Drawer, Badge,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, SettingOutlined,
  PlayCircleOutlined, DisconnectOutlined, ExclamationCircleOutlined,
  ReloadOutlined, ApiOutlined, CheckCircleOutlined,
  CloseCircleOutlined, SyncOutlined, MinusCircleOutlined,
  ToolOutlined, CodeOutlined, BugOutlined,
  UnorderedListOutlined, ThunderboltOutlined, CloseOutlined,
  SafetyOutlined,
} from '@ant-design/icons';
import { useAppContext } from '../../contexts/AppContext';
import type { ToolAutoAcceptRule } from '../../types';

const { Text, Paragraph, Title } = Typography;
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

// ===== Auto-access rule helpers =====

/** Build a stable rule ID for a server+tool combination */
function ruleId(server: string, tool?: string) {
  return tool ? `mcp:${server}:${tool}` : `mcp:${server}:*`;
}

function useAutoAccess(serverName: string) {
  const { ws } = useAppContext();
  const { toolRules, addToolRule, removeToolRule } = ws;

  /** Is a specific tool (or whole server when tool=undefined) auto-accepted? */
  const isAutoAccepted = useCallback((tool?: string): boolean => {
    const id = ruleId(serverName, tool);
    const specific = toolRules.find(r => r.id === id);
    if (specific) return specific.enabled && specific.action === 'auto_accept';
    // Fall back to server-wide wildcard
    const wildcard = toolRules.find(r => r.id === ruleId(serverName));
    return !!(wildcard?.enabled && wildcard.action === 'auto_accept');
  }, [toolRules, serverName]);

  /** Count how many tools have explicit auto-accept rules */
  const autoCount = useCallback((tools: McpToolDef[]): number => {
    if (isAutoAccepted(undefined)) return tools.length;
    return tools.filter(t => isAutoAccepted(t.name)).length;
  }, [isAutoAccepted]);

  const setAutoAccept = useCallback((enabled: boolean, tool?: string) => {
    const id = ruleId(serverName, tool);
    if (enabled) {
      addToolRule({
        id,
        action: 'auto_accept',
        enabled: true,
        matcher: {
          type: 'mcp_server',
          server: serverName,
          tool: tool ?? null,
        },
        description: tool
          ? `Auto-accept ${serverName}:${tool}`
          : `Auto-accept all tools in ${serverName}`,
      } as ToolAutoAcceptRule);
    } else {
      removeToolRule(id);
      // Also remove wildcard if toggling individual tool and wildcard existed
      if (tool) {
        const wildId = ruleId(serverName);
        if (toolRules.find(r => r.id === wildId)) removeToolRule(wildId);
      }
    }
  }, [serverName, toolRules, addToolRule, removeToolRule]);

  /** Allow all tools of this server */
  const allowAll = useCallback((tools: McpToolDef[]) => {
    // Remove any per-tool rules first, replace with single wildcard
    tools.forEach(t => removeToolRule(ruleId(serverName, t.name)));
    removeToolRule(ruleId(serverName));
    addToolRule({
      id: ruleId(serverName),
      action: 'auto_accept',
      enabled: true,
      matcher: { type: 'mcp_server', server: serverName, tool: null },
      description: `Auto-accept all tools in ${serverName}`,
    } as ToolAutoAcceptRule);
  }, [serverName, addToolRule, removeToolRule]);

  /** Revoke all auto-accept rules for this server */
  const revokeAll = useCallback((tools: McpToolDef[]) => {
    tools.forEach(t => removeToolRule(ruleId(serverName, t.name)));
    removeToolRule(ruleId(serverName));
  }, [serverName, removeToolRule]);

  return { isAutoAccepted, autoCount, setAutoAccept, allowAll, revokeAll };
}

// ===== MCP Detail Drawer =====

interface DetailDrawerProps {
  server: McpServerItem | null;
  onClose: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onDelete: (scope: string) => void;
  onToggle: (enabled: boolean) => void;
  onToolDetail: (tool: McpToolDef) => void;
}

function McpDetailDrawer({
  server, onClose, onConnect, onDisconnect, onDelete, onToggle, onToolDetail,
}: DetailDrawerProps) {
  const { token } = theme.useToken();
  const aa = useAutoAccess(server?.name ?? '');
  const tools = server?.tools ?? [];
  const allAllowed = server ? aa.isAutoAccepted(undefined) : false;

  if (!server) return null;

  const autoCount = aa.autoCount(tools);

  return (
    <Drawer
      open={!!server}
      onClose={onClose}
      width={420}
      closable={false}
      styles={{ body: { padding: 0 } }}
    >
      {/* ── Header ── */}
      <div
        className="flex items-center justify-between px-5 py-4"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}`, background: token.colorBgContainer }}
      >
        <Flex align="center" gap={10} style={{ flex: 1, minWidth: 0 }}>
          <div style={{
            width: 36, height: 36, borderRadius: 10, flexShrink: 0,
            background: token.colorPrimaryBg,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
          }}>
            <ApiOutlined style={{ color: token.colorPrimary, fontSize: 16 }} />
          </div>
          <div style={{ minWidth: 0 }}>
            <Flex align="center" gap={6}>
              <Text strong ellipsis style={{ maxWidth: 180 }}>{server.name}</Text>
              {server.builtin && <Tag color="blue" style={{ fontSize: 10, margin: 0 }}>Built-in</Tag>}
            </Flex>
            <Flex align="center" gap={4}>
              <StatusIcon status={server.status} />
              <Text type="secondary" style={{ fontSize: 11 }}>{server.transport}</Text>
            </Flex>
          </div>
        </Flex>
        <Button type="text" size="small" icon={<CloseOutlined />} onClick={onClose} />
      </div>

      {/* ── Body ── */}
      <div style={{ overflowY: 'auto', height: 'calc(100% - 140px)' }}>
        {/* Description */}
        {server.description && (
          <div style={{ padding: '12px 20px', borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
            <Text type="secondary" style={{ fontSize: 12 }}>{server.description}</Text>
          </div>
        )}

        {/* Error */}
        {server.error && (
          <div style={{ padding: '8px 20px', background: token.colorErrorBg }}>
            <Flex align="center" gap={6}>
              <ExclamationCircleOutlined style={{ color: token.colorError }} />
              <Text type="danger" style={{ fontSize: 12 }} ellipsis>{server.error}</Text>
            </Flex>
          </div>
        )}

        {/* ── Auto-access section ── */}
        <div style={{ padding: '16px 20px', borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
          <Flex align="center" justify="space-between" style={{ marginBottom: 12 }}>
            <Flex align="center" gap={6}>
              <SafetyOutlined style={{ color: token.colorPrimary }} />
              <Text strong style={{ fontSize: 13 }}>Auto Access</Text>
              {autoCount > 0 && (
                <Badge
                  count={allAllowed ? 'ALL' : autoCount}
                  style={{ backgroundColor: token.colorSuccess, fontSize: 10 }}
                />
              )}
            </Flex>
            <Tooltip title={allAllowed ? 'Revoke all auto-access' : 'Allow all tools automatically'}>
              <Button
                size="small"
                type={allAllowed ? 'default' : 'primary'}
                danger={allAllowed}
                icon={<ThunderboltOutlined />}
                onClick={() => allAllowed ? aa.revokeAll(tools) : aa.allowAll(tools)}
              >
                {allAllowed ? 'Revoke All' : 'Allow All Tools'}
              </Button>
            </Tooltip>
          </Flex>
          <Text type="secondary" style={{ fontSize: 11 }}>
            Bật để agent tự động gọi tool mà không cần xác nhận mỗi lần.
          </Text>
        </div>

        {/* ── Tool list with per-tool switch ── */}
        <div style={{ padding: '8px 0' }}>
          <Flex align="center" justify="space-between" style={{ padding: '4px 20px 8px' }}>
            <Text type="secondary" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: 1 }}>
              Tools ({tools.length})
            </Text>
          </Flex>

          {tools.length === 0 && (
            <Flex align="center" justify="center" style={{ padding: '24px 0' }}>
              <Text type="secondary" style={{ fontSize: 12, fontStyle: 'italic' }}>No tools available</Text>
            </Flex>
          )}

          {tools.map(tool => {
            const auto = aa.isAutoAccepted(tool.name);
            return (
              <div
                key={tool.name}
                style={{
                  padding: '10px 20px',
                  borderBottom: `1px solid ${token.colorBorderSecondary}`,
                  cursor: 'pointer',
                  transition: 'background 0.15s',
                }}
                onMouseEnter={e => { e.currentTarget.style.background = token.colorFillAlter; }}
                onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; }}
              >
                <Flex align="flex-start" justify="space-between" gap={10}>
                  {/* Tool info — click for detail */}
                  <Flex
                    align="flex-start"
                    gap={8}
                    style={{ flex: 1, minWidth: 0 }}
                    onClick={() => onToolDetail(tool)}
                  >
                    <ToolOutlined style={{ color: token.colorPrimary, fontSize: 13, marginTop: 2, flexShrink: 0 }} />
                    <div style={{ minWidth: 0 }}>
                      <Flex align="center" gap={6}>
                        <Text strong style={{ fontSize: 12 }}>{tool.name}</Text>
                        {tool.inputSchema && (
                          <Tag color="green" style={{ fontSize: 9, margin: 0, padding: '0 4px' }}>schema</Tag>
                        )}
                        {auto && !allAllowed && (
                          <Tag color="blue" style={{ fontSize: 9, margin: 0, padding: '0 4px' }}>auto</Tag>
                        )}
                      </Flex>
                      {tool.description && (
                        <Text type="secondary" style={{ fontSize: 11, display: 'block', marginTop: 1 }} ellipsis>
                          {tool.description}
                        </Text>
                      )}
                    </div>
                  </Flex>

                  {/* Auto-access toggle — stop propagation */}
                  <Tooltip title={auto ? 'Tắt auto access' : 'Bật auto access cho tool này'}>
                    <Switch
                      size="small"
                      checked={auto}
                      onChange={(val, e) => { e.stopPropagation(); aa.setAutoAccept(val, tool.name); }}
                      disabled={allAllowed}
                    />
                  </Tooltip>
                </Flex>
              </div>
            );
          })}
        </div>
      </div>

      {/* ── Footer actions ── */}
      <div
        style={{
          position: 'absolute', bottom: 0, left: 0, right: 0,
          padding: '10px 16px',
          borderTop: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorBgContainer,
        }}
      >
        {server.builtin ? (
          <Flex align="center" justify="center">
            <Text type="secondary" style={{ fontSize: 12 }}>Built-in server — always active</Text>
          </Flex>
        ) : (
          <Flex gap={8}>
            {server.status === 'connected' ? (
              <Button size="small" icon={<DisconnectOutlined />} onClick={onDisconnect} style={{ flex: 1 }}>
                Disconnect
              </Button>
            ) : (
              <Button size="small" type="primary" icon={<PlayCircleOutlined />} onClick={onConnect} style={{ flex: 1 }}>
                Connect
              </Button>
            )}
            <Switch
              checked={server.enabled}
              onChange={onToggle}
              checkedChildren="ON"
              unCheckedChildren="OFF"
            />
            <Popconfirm
              title="Delete this MCP server?"
              onConfirm={() => { onDelete(server.scope); onClose(); }}
              okText="Delete"
              cancelText="Cancel"
              okButtonProps={{ danger: true }}
            >
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Flex>
        )}
      </div>
    </Drawer>
  );
}

// ===== Server Card (Browse) =====

function ServerCard({
  server, autoCount, onOpen, onToggle,
}: {
  server: McpServerItem;
  autoCount: number;
  onOpen: () => void;
  onToggle: (val: boolean) => void;
}) {
  const { token } = theme.useToken();
  const toolCount = server.tools?.length ?? 0;

  return (
    <Card
      size="small"
      hoverable
      onClick={onOpen}
      styles={{ body: { padding: '12px', display: 'flex', flexDirection: 'column', gap: 10 } }}
      style={{
        backgroundColor: token.colorBgContainer,
        borderColor: server.status === 'connected' ? token.colorSuccessBorder : token.colorBorderSecondary,
        transition: 'border-color 0.2s',
        cursor: 'pointer',
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
            onChange={(val, e) => { e.stopPropagation(); onToggle(val); }}
          />
        )}
      </Flex>

      {/* Description */}
      {server.description && (
        <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }} ellipsis={{ rows: 2 }}>
          {server.description}
        </Paragraph>
      )}

      {/* Tool count + auto badge */}
      <Flex align="center" gap={6}>
        {toolCount > 0 ? (
          <Tag color="default" style={{ fontSize: 10, margin: 0 }}>
            <UnorderedListOutlined /> {toolCount} tool{toolCount > 1 ? 's' : ''}
          </Tag>
        ) : (
          <Text type="secondary" style={{ fontSize: 11, fontStyle: 'italic' }}>No tools</Text>
        )}
        {autoCount > 0 && (
          <Tag color="green" style={{ fontSize: 10, margin: 0 }}>
            <ThunderboltOutlined /> {autoCount === toolCount ? 'All auto' : `${autoCount} auto`}
          </Tag>
        )}
      </Flex>

      {/* Error */}
      {server.error && (
        <Tooltip title={server.error}>
          <Flex align="center" gap={4}>
            <ExclamationCircleOutlined style={{ color: 'red', fontSize: 11 }} />
            <Text type="danger" style={{ fontSize: 11 }} ellipsis>{server.error}</Text>
          </Flex>
        </Tooltip>
      )}
    </Card>
  );
}

// ===== Manage Row =====

function ServerRow({
  server, idx, total, autoCount, onOpen, onConnect, onDisconnect, onToggle, onDelete,
}: {
  server: McpServerItem;
  idx: number;
  total: number;
  autoCount: number;
  onOpen: () => void;
  onConnect: () => void;
  onDisconnect: () => void;
  onToggle: (val: boolean) => void;
  onDelete: () => void;
}) {
  const { token } = theme.useToken();
  return (
    <div
      style={{
        display: 'flex', alignItems: 'center', gap: 12,
        padding: '12px 16px',
        borderBottom: idx < total - 1 ? `1px solid ${token.colorBorderSecondary}` : 'none',
        cursor: 'pointer',
      }}
      onClick={onOpen}
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

      <Flex align="center" gap={8} style={{ flexShrink: 0 }} onClick={e => e.stopPropagation()}>
        <StatusTag status={server.status} />
        {autoCount > 0 && (
          <Tag color="green" style={{ margin: 0, fontSize: 11 }}>
            <ThunderboltOutlined /> {autoCount} auto
          </Tag>
        )}
        {server.tools && server.tools.length > 0 && (
          <Tag color="blue" style={{ margin: 0, fontSize: 11 }}>
            <UnorderedListOutlined /> {server.tools.length}
          </Tag>
        )}
        {!server.builtin && (
          <>
            {server.status === 'connected' ? (
              <Button size="small" icon={<DisconnectOutlined />} onClick={onDisconnect}>Disconnect</Button>
            ) : (
              <Button size="small" icon={<PlayCircleOutlined />} onClick={onConnect} type="primary">Connect</Button>
            )}
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
            <Radio.Button value="user">User (~/.senclaw)</Radio.Button>
            <Radio.Button value="project">Project (.senclaw)</Radio.Button>
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

// ===== Tool Detail Modal =====

function ToolDetailModal({
  open, server, tool, onClose,
}: {
  open: boolean;
  server: McpServerItem | null;
  tool: McpToolDef | null;
  onClose: () => void;
}) {
  const { token } = theme.useToken();
  const [testArgs, setTestArgs] = useState('{}');
  const [testLoading, setTestLoading] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const aa = useAutoAccess(server?.name ?? '');

  useEffect(() => { if (open) { setTestArgs('{}'); setTestResult(null); } }, [open]);

  const handleTest = async () => {
    if (!server || !tool) return;
    setTestLoading(true);
    setTestResult(null);
    try {
      let args: any = {};
      try { args = JSON.parse(testArgs); } catch { args = {}; }
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(server.name)}/test`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tool: tool.name, args }),
      });
      const data = await r.json();
      if (data.ok) { setTestResult(JSON.stringify(data.result, null, 2)); message.success(`${tool.name} executed`); }
      else { setTestResult(`Error: ${data.error}`); message.error(data.error); }
    } catch (e: any) {
      setTestResult(`Error: ${e.message}`);
    } finally { setTestLoading(false); }
  };

  const auto = tool ? aa.isAutoAccepted(tool.name) : false;

  return (
    <Modal
      title={
        <Flex align="center" gap={8}>
          <ToolOutlined />
          <span>{tool?.name}</span>
          {server && <Tag color="blue" style={{ margin: 0 }}>{server.name}</Tag>}
        </Flex>
      }
      open={open}
      onCancel={onClose}
      footer={null}
      width={640}
      destroyOnClose
    >
      {tool && (
        <Flex vertical gap={16} style={{ marginTop: 12 }}>
          {/* Auto-access toggle */}
          <Flex
            align="center"
            justify="space-between"
            style={{
              padding: '10px 14px',
              borderRadius: 8,
              border: `1px solid ${auto ? token.colorSuccessBorder : token.colorBorderSecondary}`,
              background: auto ? token.colorSuccessBg : token.colorFillAlter,
            }}
          >
            <Flex align="center" gap={8}>
              <ThunderboltOutlined style={{ color: auto ? token.colorSuccess : token.colorTextSecondary }} />
              <div>
                <Text strong style={{ fontSize: 13 }}>Auto Access</Text>
                <Text type="secondary" style={{ fontSize: 11, display: 'block' }}>
                  Agent tự động gọi tool này không cần xác nhận
                </Text>
              </div>
            </Flex>
            <Switch
              checked={auto}
              onChange={val => aa.setAutoAccept(val, tool.name)}
              checkedChildren="ON"
              unCheckedChildren="OFF"
            />
          </Flex>

          {/* Description */}
          <div>
            <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>Description</Text>
            <Text style={{ fontSize: 13 }}>{tool.description || 'No description'}</Text>
          </div>

          {/* Input Schema */}
          {tool.inputSchema && (
            <div>
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 4 }}>
                <CodeOutlined /> Input Schema
              </Text>
              <pre style={{
                background: token.colorFillSecondary, padding: 12, borderRadius: 6,
                fontSize: 12, fontFamily: 'Menlo, Monaco, monospace',
                maxHeight: 200, overflow: 'auto', margin: 0,
                border: `1px solid ${token.colorBorderSecondary}`,
              }}>
                {JSON.stringify(tool.inputSchema, null, 2)}
              </pre>
            </div>
          )}

          {/* Test */}
          {server?.status === 'connected' && !server.builtin && (
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
              <Flex justify="flex-end" style={{ marginTop: 8 }}>
                <Button type="primary" size="small" icon={<BugOutlined />} onClick={handleTest} loading={testLoading}>
                  Run Test
                </Button>
              </Flex>
              {testResult !== null && (
                <pre style={{
                  background: testResult.startsWith('Error') ? '#fff2f0' : '#f6ffed',
                  padding: 12, borderRadius: 6, fontSize: 12,
                  fontFamily: 'Menlo, Monaco, monospace',
                  maxHeight: 300, overflow: 'auto', marginTop: 12,
                  border: `1px solid ${testResult.startsWith('Error') ? '#ffccc7' : '#b7eb8f'}`,
                }}>
                  {testResult}
                </pre>
              )}
            </div>
          )}
          {server?.builtin && (
            <Text type="secondary" style={{ fontSize: 12, fontStyle: 'italic' }}>
              Built-in server — invoke via Chat.
            </Text>
          )}
        </Flex>
      )}
    </Modal>
  );
}

// ===== Root =====

export const MCPSettings: React.FC = () => {
  const { token } = theme.useToken();
  const { ws } = useAppContext();
  const [servers, setServers] = useState<McpServerItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [tab, setTab] = useState<'browse' | 'manage'>('browse');
  const [addOpen, setAddOpen] = useState(false);

  // Detail drawer
  const [detailServer, setDetailServer] = useState<McpServerItem | null>(null);

  // Tool detail modal
  const [toolOpen, setToolOpen] = useState(false);
  const [toolServer, setToolServer] = useState<McpServerItem | null>(null);
  const [toolDetail, setToolDetail] = useState<McpToolDef | null>(null);

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

  // Keep detail drawer in sync when servers reload
  useEffect(() => {
    if (detailServer) {
      const updated = servers.find(s => s.name === detailServer.name);
      if (updated) setDetailServer(updated);
    }
  }, [servers]);

  const handleConnect = async (name: string) => {
    try {
      await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/connect`, { method: 'POST' });
      await load();
      message.success(`${name} connected`);
    } catch { message.error(`Failed to connect ${name}`); }
  };

  const handleDisconnect = async (name: string) => {
    try {
      await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/disconnect`, { method: 'POST' });
      await load();
      message.success(`${name} disconnected`);
    } catch { message.error(`Failed to disconnect ${name}`); }
  };

  const handleToggle = async (name: string, enabled: boolean, scope: string) => {
    try {
      await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/enabled`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled, scope }),
      });
      await load();
    } catch { message.error(`Failed to update ${name}`); }
  };

  const handleDelete = async (name: string, scope: string) => {
    try {
      await fetch(`/api/mcp-servers/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`, { method: 'DELETE' });
      await load();
      message.success(`${name} removed`);
    } catch { message.error(`Failed to remove ${name}`); }
  };

  const openDetail = (server: McpServerItem) => setDetailServer(server);

  const openToolDetail = (srv: McpServerItem, tool: McpToolDef) => {
    setToolServer(srv);
    setToolDetail(tool);
    setToolOpen(true);
  };

  const connected = servers.filter(s => s.status === 'connected').length;

  // Helper: get auto-count for a server using current rules
  const getAutoCount = (srv: McpServerItem): number => {
    const tools = srv.tools ?? [];
    if (!tools.length) return 0;
    const wildcardRule = ws.toolRules.find(r => r.id === ruleId(srv.name) && r.enabled && r.action === 'auto_accept');
    if (wildcardRule) return tools.length;
    return tools.filter(t => {
      const r = ws.toolRules.find(x => x.id === ruleId(srv.name, t.name));
      return r?.enabled && r.action === 'auto_accept';
    }).length;
  };

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
                      fontSize: '10px', padding: '1px 6px', borderRadius: 10,
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
          {connected > 0 && <Tag color="success" style={{ margin: 0 }}>{connected} connected</Tag>}
          <Button type="text" icon={<ReloadOutlined />} size="small" onClick={() => { setLoading(true); load(); }} />
          <Button type="primary" icon={<PlusOutlined />} size="small" onClick={() => setAddOpen(true)}>
            Add Server
          </Button>
        </Flex>
      </Flex>

      {/* Content */}
      {loading ? (
        <Flex align="center" justify="center" style={{ flex: 1 }}><Spin size="large" /></Flex>
      ) : tab === 'browse' ? (
        <div style={{ flex: 1, overflowY: 'auto', padding: 20 }}>
          {servers.length === 0 ? (
            <Flex vertical align="center" justify="center" style={{ padding: '80px 0' }}>
              <div style={{
                backgroundColor: token.colorPrimaryBg, width: 48, height: 48, borderRadius: 16,
                display: 'flex', alignItems: 'center', justifyContent: 'center', marginBottom: 16,
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
                  autoCount={getAutoCount(srv)}
                  onOpen={() => openDetail(srv)}
                  onToggle={val => handleToggle(srv.name, val, srv.scope)}
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
                  autoCount={getAutoCount(srv)}
                  onOpen={() => openDetail(srv)}
                  onConnect={() => handleConnect(srv.name)}
                  onDisconnect={() => handleDisconnect(srv.name)}
                  onToggle={val => handleToggle(srv.name, val, srv.scope)}
                  onDelete={() => handleDelete(srv.name, srv.scope)}
                />
              ))}
            </Card>
          )}
        </div>
      )}

      {/* Add Server Modal */}
      <AddServerModal open={addOpen} onClose={() => setAddOpen(false)} onSaved={load} />

      {/* MCP Detail Drawer */}
      <McpDetailDrawer
        server={detailServer}
        onClose={() => setDetailServer(null)}
        onConnect={() => detailServer && handleConnect(detailServer.name)}
        onDisconnect={() => detailServer && handleDisconnect(detailServer.name)}
        onToggle={val => detailServer && handleToggle(detailServer.name, val, detailServer.scope)}
        onDelete={scope => detailServer && handleDelete(detailServer.name, scope)}
        onToolDetail={tool => { if (detailServer) { setDetailServer(null); openToolDetail(detailServer, tool); } }}
      />

      {/* Tool Detail Modal */}
      <ToolDetailModal
        open={toolOpen}
        server={toolServer}
        tool={toolDetail}
        onClose={() => setToolOpen(false)}
      />
    </Flex>
  );
};
