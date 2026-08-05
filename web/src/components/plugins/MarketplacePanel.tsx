import { useState, useEffect } from 'react';
import { Typography, Card, Button, Table, Space, Tag, message, Modal, Form, Input, Select, Spin, Switch, Empty, Tooltip } from 'antd';
import { PlusOutlined, CloudDownloadOutlined, DeleteOutlined, ReloadOutlined, SearchOutlined, DownloadOutlined, ShopOutlined } from '@ant-design/icons';
import { ClawHubSearchDialog } from './ClawHubSearchDialog';
import ScanReportDialog, { readScanError, type ScanReport } from '../security/ScanReportDialog';

const { Title, Text } = Typography;

type SourceType = 'hub' | 'git' | 'local';

interface MarketplaceSource {
  id: string;
  name: string;
  type: SourceType;
  url?: string;
  branch?: string;
  localPath: string;
  priority: number;
  enabled: boolean;
  lastSynced?: string;
  syncError?: string;
}

interface MarketplacePlugin {
  name: string;
  description: string;
  version?: string;
  author?: string;
  category?: string;
  license?: string;
  repository?: string;
  sourceId: string;
  enabled: boolean;
  /** Hub plugins are catalog entries until installed; git/local are always true. */
  installed: boolean;
  skillCount: number;
  subagentCount: number;
  mcpServerCount: number;
  hasHooks: boolean;
}

const TYPE_COLOR: Record<SourceType, string> = { hub: 'purple', git: 'blue', local: 'green' };

export default function MarketplacePanel() {
  const [sources, setSources] = useState<MarketplaceSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [addModalVisible, setAddModalVisible] = useState(false);
  const [clawhubOpen, setClawhubOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [form] = Form.useForm();

  useEffect(() => {
    fetchSources();
  }, []);

  const fetchSources = async () => {
    try {
      setLoading(true);
      const res = await fetch('/api/marketplace/sources');
      if (!res.ok) throw new Error('Failed to fetch sources');
      const data = await res.json();
      setSources(data.sources || []);
    } catch (error) {
      message.error('Failed to load marketplace sources');
      console.error(error);
    } finally {
      setLoading(false);
    }
  };

  const handleAddSource = async (values: any) => {
    try {
      const res = await fetch('/api/marketplace/sources', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: values.name,
          type: values.type,
          url: values.type === 'local' ? undefined : values.url,
          branch: values.type === 'git' ? values.branch : undefined,
          localPath: values.type === 'local' ? values.localPath : undefined,
          priority: values.priority ? Number(values.priority) : undefined,
          enabled: values.enabled,
        }),
      });
      if (!res.ok) throw new Error(await res.text());
      message.success('Source added');
      setAddModalVisible(false);
      form.resetFields();
      fetchSources();
    } catch (error: any) {
      message.error(`Failed to add source: ${error?.message ?? error}`);
      console.error(error);
    }
  };

  const handleSync = async (id: string) => {
    try {
      setBusy(id);
      const res = await fetch(`/api/marketplace/sources/${id}/sync`, { method: 'POST' });
      if (!res.ok) throw new Error(await res.text());
      message.success('Source synced');
      fetchSources();
    } catch (error: any) {
      message.error(`Sync failed: ${error?.message ?? error}`);
    } finally {
      setBusy(null);
    }
  };

  const handleDelete = async (id: string) => {
    Modal.confirm({
      title: 'Delete Source',
      content: 'Remove this source and everything installed from it?',
      onOk: async () => {
        try {
          const res = await fetch(`/api/marketplace/sources/${id}`, { method: 'DELETE' });
          if (!res.ok) throw new Error('Failed to delete source');
          message.success('Source deleted');
          fetchSources();
        } catch (error) {
          message.error('Failed to delete source');
          console.error(error);
        }
      },
    });
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, record: MarketplaceSource) => (
        <Space>
          {record.type === 'hub' && <ShopOutlined />}
          <span>{name}</span>
        </Space>
      ),
    },
    {
      title: 'Type',
      dataIndex: 'type',
      key: 'type',
      width: 90,
      render: (type: SourceType) => <Tag color={TYPE_COLOR[type]}>{type.toUpperCase()}</Tag>,
    },
    {
      title: 'URL/Path',
      key: 'origin',
      ellipsis: true,
      render: (_: any, record: MarketplaceSource) => (
        <Text type="secondary" style={{ fontSize: 12 }}>{record.url || record.localPath}</Text>
      ),
    },
    {
      title: 'Priority',
      dataIndex: 'priority',
      key: 'priority',
      width: 80,
    },
    {
      title: 'Enabled',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 80,
      render: (enabled: boolean) => (
        <Tag color={enabled ? 'green' : 'default'}>{enabled ? 'Yes' : 'No'}</Tag>
      ),
    },
    {
      title: 'Last Synced',
      dataIndex: 'lastSynced',
      key: 'lastSynced',
      width: 170,
      render: (date: string, record: MarketplaceSource) =>
        record.syncError ? (
          <Tooltip title={record.syncError}>
            <Tag color="red">sync error</Tag>
          </Tooltip>
        ) : (
          date ? new Date(date).toLocaleString() : 'Never'
        ),
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 110,
      render: (_: any, record: MarketplaceSource) => (
        <Space size="small">
          {record.type !== 'local' && (
            <Tooltip title={record.type === 'hub' ? 'Refresh catalog' : 'Pull latest'}>
              <Button
                type="text"
                icon={<CloudDownloadOutlined />}
                loading={busy === record.id}
                onClick={() => handleSync(record.id)}
                size="small"
              />
            </Tooltip>
          )}
          <Button
            type="text"
            danger
            icon={<DeleteOutlined />}
            onClick={() => handleDelete(record.id)}
            size="small"
          />
        </Space>
      ),
    },
  ];

  return (
    <div style={{ padding: '24px' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '16px' }}>
        <Title level={4} style={{ margin: 0 }}>
          Marketplace Sources
        </Title>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={fetchSources}>
            Refresh
          </Button>
          <Button
            icon={<SearchOutlined />}
            onClick={() => setClawhubOpen(true)}
          >
            Search ClaWHub
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setAddModalVisible(true)}>
            Add Source
          </Button>
        </Space>
      </div>

      <Card>
        <Spin spinning={loading}>
          <Table
            dataSource={sources}
            columns={columns}
            rowKey="id"
            pagination={false}
            size="small"
            expandable={{
              expandedRowRender: (record: MarketplaceSource) => <SourcePlugins source={record} />,
              rowExpandable: () => true,
            }}
          />
        </Spin>
      </Card>

      <Modal
        title="Add Marketplace Source"
        open={addModalVisible}
        onCancel={() => setAddModalVisible(false)}
        onOk={() => form.submit()}
        width={600}
      >
        <Form form={form} layout="vertical" onFinish={handleAddSource}>
          <Form.Item label="Name" name="name" extra="Optional — defaults to the host or repo">
            <Input placeholder="My Skills Repository" />
          </Form.Item>
          <Form.Item
            label="Type"
            name="type"
            initialValue="hub"
            rules={[{ required: true }]}
          >
            <Select>
              <Select.Option value="hub">Hub store (marketplace.json)</Select.Option>
              <Select.Option value="git">Git Repository</Select.Option>
              <Select.Option value="local">Local Directory</Select.Option>
            </Select>
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(prev, curr) => prev.type !== curr.type}>
            {({ getFieldValue }) => {
              const type = getFieldValue('type');
              if (type === 'local') {
                return (
                  <Form.Item
                    label="Local Path"
                    name="localPath"
                    rules={[{ required: true, message: 'Please enter a local path' }]}
                  >
                    <Input placeholder="/path/to/local/directory" />
                  </Form.Item>
                );
              }
              if (type === 'git') {
                return (
                  <>
                    <Form.Item
                      label="Git URL"
                      name="url"
                      rules={[{ required: true, message: 'Please enter a Git URL' }]}
                    >
                      <Input placeholder="https://github.com/user/repo" />
                    </Form.Item>
                    <Form.Item label="Branch" name="branch" initialValue="main">
                      <Input placeholder="main" />
                    </Form.Item>
                  </>
                );
              }
              return (
                <Form.Item
                  label="Hub URL"
                  name="url"
                  rules={[{ required: true, message: 'Please enter a hub URL' }]}
                  extra="A site root gets /marketplace.json appended automatically"
                >
                  <Input placeholder="https://senclaw.bacnd.com" />
                </Form.Item>
              );
            }}
          </Form.Item>
          <Form.Item label="Priority" name="priority" initialValue={10}>
            <Input type="number" />
          </Form.Item>
          <Form.Item label="Enabled" name="enabled" initialValue={true}>
            <Select>
              <Select.Option value={true}>Yes</Select.Option>
              <Select.Option value={false}>No</Select.Option>
            </Select>
          </Form.Item>
        </Form>
      </Modal>

      <ClawHubSearchDialog
        open={clawhubOpen}
        onClose={() => setClawhubOpen(false)}
        onInstalled={() => fetchSources()}
      />
    </div>
  );
}

/** Plugins of one source. For a hub these are catalog entries, installable one by one. */
function SourcePlugins({ source }: { source: MarketplaceSource }) {
  const [plugins, setPlugins] = useState<MarketplacePlugin[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [scanState, setScanState] = useState<{
    report: ScanReport;
    plugin: MarketplacePlugin;
    blocked: boolean;
  } | null>(null);

  const load = async () => {
    try {
      setLoading(true);
      const res = await fetch(`/api/marketplace/sources/${source.id}`);
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      setPlugins(data.plugins || []);
    } catch (error: any) {
      message.error(`Failed to load plugins: ${error?.message ?? error}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source.id]);

  const call = async (name: string, path: string, method: string) => {
    try {
      setBusy(name);
      const res = await fetch(path, { method });
      if (!res.ok) throw new Error(await res.text());
      await load();
    } catch (error: any) {
      message.error(`${method} ${path} failed: ${error?.message ?? error}`);
    } finally {
      setBusy(null);
    }
  };

  /**
   * Install, routing a scan verdict to the review dialog. A blocked install
   * (422) opens the dialog with an override; a successful install that still
   * produced findings opens it read-only.
   */
  const install = async (p: MarketplacePlugin, force = false) => {
    const path =
      `/api/marketplace/sources/${source.id}/plugins/${encodeURIComponent(p.name)}/install` +
      (force ? '?force=true' : '');
    try {
      setBusy(p.name);
      const res = await fetch(path, { method: 'POST' });
      if (!res.ok) {
        const { blocked, error, scan } = await readScanError(res);
        if (blocked && scan) {
          setScanState({ report: scan, plugin: p, blocked: true });
          return;
        }
        throw new Error(error);
      }
      const body = await res.json();
      if (body?.scan?.findings?.length) {
        setScanState({ report: body.scan, plugin: p, blocked: false });
      } else {
        message.success(`Installed ${p.name}`);
      }
      await load();
    } catch (error: any) {
      message.error(`Install ${p.name} failed: ${error?.message ?? error}`);
    } finally {
      setBusy(null);
    }
  };
  const uninstall = (p: MarketplacePlugin) =>
    call(p.name, `/api/marketplace/sources/${source.id}/plugins/${encodeURIComponent(p.name)}`, 'DELETE');
  const toggle = (p: MarketplacePlugin) =>
    call(p.name, `/api/marketplace/sources/${source.id}/plugins/${encodeURIComponent(p.name)}/toggle`, 'POST');

  if (loading) return <Spin size="small" />;
  if (!plugins.length) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description={source.type === 'hub' ? 'Catalog is empty — try syncing the hub' : 'No plugins found in this source'}
      />
    );
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={4}>
      {plugins.map((p) => (
        <div
          key={p.name}
          style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '4px 0' }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <Space size={6} wrap>
              <Text strong>{p.name}</Text>
              {p.version && <Tag>{p.version}</Tag>}
              {p.category && <Tag color="geekblue">{p.category}</Tag>}
              {!p.installed && <Tag color="default">not installed</Tag>}
            </Space>
            <div>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {p.description}
              </Text>
            </div>
            {p.installed && (
              <Text type="secondary" style={{ fontSize: 11 }}>
                {p.skillCount} skills · {p.subagentCount} subagents · {p.mcpServerCount} MCP
                {p.hasHooks ? ' · hooks' : ''}
              </Text>
            )}
          </div>
          <Space size="small">
            {p.installed ? (
              <>
                <Switch
                  size="small"
                  checked={p.enabled}
                  loading={busy === p.name}
                  onChange={() => toggle(p)}
                />
                {source.type === 'hub' && (
                  <Button size="small" danger type="text" loading={busy === p.name} onClick={() => uninstall(p)}>
                    Remove
                  </Button>
                )}
              </>
            ) : (
              <Button
                size="small"
                type="primary"
                icon={<DownloadOutlined />}
                loading={busy === p.name}
                onClick={() => install(p)}
              >
                Install
              </Button>
            )}
          </Space>
        </div>
      ))}

      <ScanReportDialog
        open={!!scanState}
        report={scanState?.report}
        target={scanState?.plugin.name}
        blocked={!!scanState?.blocked}
        busy={busy === scanState?.plugin.name}
        onCancel={() => setScanState(null)}
        onForceInstall={() => {
          const p = scanState?.plugin;
          setScanState(null);
          if (p) void install(p, true);
        }}
      />
    </Space>
  );
}
