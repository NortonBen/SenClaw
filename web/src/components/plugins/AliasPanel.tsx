import { useState, useEffect, useCallback, useMemo } from 'react';
import {
  Typography,
  Table,
  Tag,
  Switch,
  Button,
  Space,
  Modal,
  Form,
  Input,
  Popconfirm,
  message,
  Alert,
  AutoComplete,
  Spin,
  theme,
} from 'antd';
import {
  PlusOutlined,
  ReloadOutlined,
  TagsOutlined,
  EditOutlined,
  DeleteOutlined,
} from '@ant-design/icons';

const { Title, Text } = Typography;

// ─── Types ────────────────────────────────────────────────────────────────────

interface ToolAlias {
  alias: string;
  target: string;
  description?: string | null;
  enabled: boolean;
  source: string;
  createdAt: number;
  updatedAt: number;
}

interface McpServerEntry {
  name: string;
  tools?: unknown[];
}

// ─── API helper ───────────────────────────────────────────────────────────────

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    let detail = '';
    try {
      detail = (await res.json())?.error ?? '';
    } catch {
      /* not json */
    }
    throw new Error(detail || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

function fullToolNames(servers: McpServerEntry[]): string[] {
  const out: string[] = [];
  for (const srv of servers) {
    if (!srv?.name) continue;
    for (const t of srv.tools ?? []) {
      const toolName =
        typeof t === 'string' ? t : (t as { name?: string } | null)?.name;
      if (toolName) out.push(`mcp__${srv.name}__${toolName}`);
    }
  }
  return Array.from(new Set(out)).sort();
}

// The daemon's stripped "bridge" spelling of an MCP tool name:
// `mcp__senclaw-<srv>__<srv>_<tool>` → `mcp__<srv>__<tool>`.
function normalizeMcpToolName(name: string): string {
  const prefix = 'mcp__senclaw-';
  if (!name.startsWith(prefix)) return name;
  const rest = name.slice(prefix.length);
  const split = rest.indexOf('__');
  if (split <= 0) return name;
  const server = rest.slice(0, split);
  let tool = rest.slice(split + 2);
  if (tool.startsWith(`${server}_`)) tool = tool.slice(server.length + 1);
  return `mcp__${server}__${tool}`;
}

// Every spelling the daemon's resolver accepts for `name`: as written, the
// stripped bridge form, and their hyphen/underscore-folded variants.
function toolNameVariants(name: string): string[] {
  const normalized = normalizeMcpToolName(name);
  return Array.from(
    new Set([
      name,
      name.replace(/-/g, '_'),
      normalized,
      normalized.replace(/-/g, '_'),
    ]),
  );
}

// Whether `name` matches a known MCP tool under any accepted spelling. Kept
// as lenient as the daemon's resolution so the panel never warns about a
// name that would in fact resolve.
function mcpToolExists(known: string[], name: string): boolean {
  const variants = new Set(toolNameVariants(name));
  return known.some(k => toolNameVariants(k).some(v => variants.has(v)));
}

// ─── Panel ────────────────────────────────────────────────────────────────────

export default function AliasPanel() {
  const { token } = theme.useToken();
  const [aliases, setAliases] = useState<ToolAlias[]>([]);
  const [knownTools, setKnownTools] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [togglingKey, setTogglingKey] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<ToolAlias | null>(null);
  const [saving, setSaving] = useState(false);
  const [form] = Form.useForm();

  const fetchAll = useCallback(async () => {
    setLoading(true);
    try {
      const data = await apiFetch<{ aliases: ToolAlias[] }>('/api/tool-aliases');
      setAliases(data.aliases ?? []);
    } catch (e) {
      message.error(`Không tải được danh sách alias: ${(e as Error).message}`);
    } finally {
      setLoading(false);
    }
    // Tool suggestions are best-effort — the panel works without them.
    try {
      const data = await apiFetch<{ servers: McpServerEntry[] }>('/api/mcp-servers');
      setKnownTools(fullToolNames(data.servers ?? []));
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    fetchAll();
  }, [fetchAll]);

  const knownSet = useMemo(() => new Set(knownTools), [knownTools]);
  const toolOptions = useMemo(
    () => knownTools.map(v => ({ value: v })),
    [knownTools],
  );

  // ── Mutations ──────────────────────────────────────────────────────────────

  const onToggle = async (row: ToolAlias, enabled: boolean) => {
    setTogglingKey(row.alias);
    try {
      await apiFetch(`/api/tool-aliases/${encodeURIComponent(row.alias)}/enabled`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled }),
      });
      message.success(enabled ? `Đã bật alias ${row.alias}` : `Đã tắt alias ${row.alias}`);
      await fetchAll();
    } catch (e) {
      message.error(`Không đổi được trạng thái: ${(e as Error).message}`);
    } finally {
      setTogglingKey(null);
    }
  };

  const onDelete = async (row: ToolAlias) => {
    try {
      await apiFetch(`/api/tool-aliases/${encodeURIComponent(row.alias)}`, {
        method: 'DELETE',
      });
      message.success(`Đã xoá alias ${row.alias}`);
      await fetchAll();
    } catch (e) {
      message.error(`Không xoá được: ${(e as Error).message}`);
    }
  };

  const openCreate = () => {
    setEditing(null);
    form.resetFields();
    setModalOpen(true);
  };

  const openEdit = (row: ToolAlias) => {
    setEditing(row);
    form.setFieldsValue({
      alias: row.alias,
      target: row.target,
      description: row.description ?? '',
    });
    setModalOpen(true);
  };

  const onSubmit = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      if (editing) {
        await apiFetch(`/api/tool-aliases/${encodeURIComponent(editing.alias)}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            target: values.target.trim(),
            description: values.description?.trim() || null,
          }),
        });
        message.success('Đã cập nhật alias');
      } else {
        await apiFetch('/api/tool-aliases', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            alias: values.alias.trim(),
            target: values.target.trim(),
            description: values.description?.trim() || null,
          }),
        });
        message.success('Đã tạo alias');
      }
      setModalOpen(false);
      await fetchAll();
    } catch (e) {
      message.error(`Không lưu được: ${(e as Error).message}`);
    } finally {
      setSaving(false);
    }
  };

  // ── Columns ────────────────────────────────────────────────────────────────

  const columns = [
    {
      title: 'Alias (tên agent gọi)',
      dataIndex: 'alias',
      key: 'alias',
      render: (v: string) => (
        <Space size={6}>
          <Text code style={{ fontSize: 12 }}>{v}</Text>
          {knownSet.has(v) ? (
            <Tag color="volcano" style={{ marginInlineEnd: 0 }}>ghi đè</Tag>
          ) : (
            <Tag color="geekblue" style={{ marginInlineEnd: 0 }}>định danh mới</Tag>
          )}
        </Space>
      ),
    },
    {
      title: 'Tool đích (thực thi)',
      dataIndex: 'target',
      key: 'target',
      render: (v: string) => <Text code style={{ fontSize: 12 }}>{v}</Text>,
    },
    {
      title: 'Nguồn',
      dataIndex: 'source',
      key: 'source',
      width: 140,
      render: (v: string) =>
        v === 'user' ? (
          <Tag color="blue">Người dùng</Tag>
        ) : (
          <Tag color="purple">App: {v.replace(/^app:/, '')}</Tag>
        ),
    },
    {
      title: 'Mô tả',
      dataIndex: 'description',
      key: 'description',
      ellipsis: true,
      render: (v: string | null) => (
        <Text type="secondary" style={{ fontSize: 12 }}>{v || '—'}</Text>
      ),
    },
    {
      title: 'Kích hoạt',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 90,
      render: (_: boolean, row: ToolAlias) => (
        <Switch
          size="small"
          checked={row.enabled}
          loading={togglingKey === row.alias}
          onChange={checked => onToggle(row, checked)}
        />
      ),
    },
    {
      title: '',
      key: 'actions',
      width: 90,
      render: (_: unknown, row: ToolAlias) => (
        <Space size={4}>
          {row.source === 'user' && (
            <Button
              type="text"
              size="small"
              icon={<EditOutlined />}
              onClick={() => openEdit(row)}
            />
          )}
          <Popconfirm
            title={`Xoá alias "${row.alias}"?`}
            description={
              row.source !== 'user'
                ? 'Alias của App sẽ được nhập lại (ở trạng thái tắt) khi app khởi động lại.'
                : undefined
            }
            okText="Xoá"
            cancelText="Huỷ"
            onConfirm={() => onDelete(row)}
          >
            <Button type="text" size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  // ── Render ─────────────────────────────────────────────────────────────────

  return (
    <div style={{ padding: 24, maxWidth: 1100, width: '100%', margin: '0 auto' }}>
      <Space
        style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}
        align="center"
      >
        <Space align="center" size={10}>
          <TagsOutlined style={{ fontSize: 20, color: token.colorPrimary }} />
          <Title level={4} style={{ margin: 0 }}>
            Alias MCP Tool
          </Title>
        </Space>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={fetchAll}>
            Tải lại
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={openCreate}>
            Thêm alias
          </Button>
        </Space>
      </Space>

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Định danh lại hoặc ghi đè MCP tool"
        description={
          <div style={{ fontSize: 12.5 }}>
            <div>
              • <b>Định danh mới</b>: alias là tên chưa tồn tại — tool đích sẽ hiện ra với
              tên mới cho agent (tên gốc vẫn gọi được).
            </div>
            <div>
              • <b>Ghi đè</b>: alias trùng tên một tool đang có — mọi lệnh gọi tên đó sẽ
              thực thi tool đích thay cho tool gốc.
            </div>
            <div>
              • Alias do <b>Space App</b> khai báo trong <Text code>senclaw-manifest.json</Text>{' '}
              (<Text code>mcp.toolAliases</Text>) được nhập ở trạng thái <b>tắt</b> — bạn phải
              bật tại đây thì mới có hiệu lực.
            </div>
          </div>
        }
      />

      <Spin spinning={loading}>
        <Table<ToolAlias>
          rowKey="alias"
          size="small"
          columns={columns}
          dataSource={aliases}
          pagination={false}
          locale={{
            emptyText: (
              <Text type="secondary">
                Chưa có alias nào. Bấm “Thêm alias” để định danh lại hoặc ghi đè một MCP tool.
              </Text>
            ),
          }}
        />
      </Spin>

      <Modal
        title={editing ? `Sửa alias: ${editing.alias}` : 'Thêm alias'}
        open={modalOpen}
        onOk={onSubmit}
        onCancel={() => setModalOpen(false)}
        okText={editing ? 'Lưu' : 'Tạo'}
        cancelText="Huỷ"
        confirmLoading={saving}
        destroyOnHidden
      >
        <Form form={form} layout="vertical" style={{ marginTop: 8 }}>
          <Form.Item
            name="alias"
            label="Alias (tên agent sẽ gọi)"
            tooltip="Tên mới để định danh lại, hoặc đúng tên một tool đang có để ghi đè nó."
            rules={[
              { required: true, message: 'Nhập tên alias' },
              {
                validator: (_, v: string) =>
                  v && /\s/.test(v)
                    ? Promise.reject(new Error('Tên không được chứa khoảng trắng'))
                    : Promise.resolve(),
              },
            ]}
          >
            <AutoComplete
              options={toolOptions}
              disabled={!!editing}
              filterOption={(input, opt) =>
                (opt?.value as string).toLowerCase().includes(input.toLowerCase())
              }
            >
              <Input placeholder="vd: mcp__browser__navigate" style={{ fontFamily: 'monospace' }} />
            </AutoComplete>
          </Form.Item>
          <Form.Item
            name="target"
            label="Tool đích (thực thi thật)"
            dependencies={['alias']}
            rules={[
              { required: true, message: 'Nhập tool đích' },
              {
                validator: (_, v: string) => {
                  if (v && /\s/.test(v)) {
                    return Promise.reject(new Error('Tên không được chứa khoảng trắng'));
                  }
                  if (v && v === form.getFieldValue('alias')) {
                    return Promise.reject(new Error('Tool đích phải khác alias'));
                  }
                  return Promise.resolve();
                },
              },
              {
                // Advisory only: the server may simply be off right now, and
                // the daemon falls back to the original name when an alias
                // target is missing — so a miss must never block saving.
                warningOnly: true,
                validator: (_, v: string) => {
                  const t = (v ?? '').trim();
                  if (
                    !t.startsWith('mcp__') ||
                    knownTools.length === 0 ||
                    mcpToolExists(knownTools, t)
                  ) {
                    return Promise.resolve();
                  }
                  return Promise.reject(
                    new Error(
                      'Chưa thấy tool này trên MCP server nào đang kết nối — kiểm tra tên hoặc bật server (vẫn lưu được).',
                    ),
                  );
                },
              },
            ]}
          >
            <AutoComplete
              options={toolOptions}
              filterOption={(input, opt) =>
                (opt?.value as string).toLowerCase().includes(input.toLowerCase())
              }
            >
              <Input
                placeholder="vd: mcp__senclaw-browser__browser_navigate"
                style={{ fontFamily: 'monospace' }}
              />
            </AutoComplete>
          </Form.Item>
          <Form.Item name="description" label="Mô tả (tuỳ chọn)">
            <Input placeholder="Ghi chú ngắn — hiển thị thay mô tả tool gốc khi định danh mới" />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
