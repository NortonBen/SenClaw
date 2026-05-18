import { useState, useEffect } from 'react';
import { Typography, Card, Button, Table, Space, Tag, message, Modal, Form, Input, Select, Spin } from 'antd';
import { PlusOutlined, CloudDownloadOutlined, DeleteOutlined, ReloadOutlined, SettingOutlined, SearchOutlined } from '@ant-design/icons';
import { ClawHubSearchDialog } from './ClawHubSearchDialog';

const { Title, Text } = Typography;

interface MarketplaceSource {
  id: string;
  name: string;
  type: 'git' | 'local';
  url?: string;
  branch?: string;
  local_path: string;
  priority: number;
  enabled: boolean;
  last_synced?: string;
}

interface MarketplacePlugin {
  name: string;
  source_id: string;
  enabled: boolean;
  type: 'skill' | 'subagent' | 'mcp';
}

export default function MarketplacePanel() {
  const [sources, setSources] = useState<MarketplaceSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [addModalVisible, setAddModalVisible] = useState(false);
  const [clawhubOpen, setClawhubOpen] = useState(false);
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
          url: values.url,
          branch: values.branch,
          local_path: values.local_path,
          priority: values.priority,
          enabled: values.enabled,
        }),
      });
      if (!res.ok) throw new Error('Failed to add source');
      message.success('Source added successfully');
      setAddModalVisible(false);
      form.resetFields();
      fetchSources();
    } catch (error) {
      message.error('Failed to add source');
      console.error(error);
    }
  };

  const handleSync = async (id: string) => {
    try {
      const res = await fetch(`/api/marketplace/sources/${id}/sync`, {
        method: 'POST',
      });
      if (!res.ok) throw new Error('Failed to sync source');
      message.success('Source synced successfully');
      fetchSources();
    } catch (error) {
      message.error('Failed to sync source');
      console.error(error);
    }
  };

  const handleDelete = async (id: string) => {
    Modal.confirm({
      title: 'Delete Source',
      content: 'Are you sure you want to delete this source?',
      onOk: async () => {
        try {
          const res = await fetch(`/api/marketplace/sources/${id}`, {
            method: 'DELETE',
          });
          if (!res.ok) throw new Error('Failed to delete source');
          message.success('Source deleted successfully');
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
    },
    {
      title: 'Type',
      dataIndex: 'type',
      key: 'type',
      render: (type: string) => (
        <Tag color={type === 'git' ? 'blue' : 'green'}>{type.toUpperCase()}</Tag>
      ),
    },
    {
      title: 'URL/Path',
      dataIndex: 'local_path',
      key: 'local_path',
      ellipsis: true,
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
      dataIndex: 'last_synced',
      key: 'last_synced',
      width: 150,
      render: (date: string) => (date ? new Date(date).toLocaleString() : 'Never'),
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 150,
      render: (_: any, record: MarketplaceSource) => (
        <Space size="small">
          {record.type === 'git' && (
            <Button
              type="text"
              icon={<CloudDownloadOutlined />}
              onClick={() => handleSync(record.id)}
              size="small"
            />
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
          <Form.Item
            label="Name"
            name="name"
            rules={[{ required: true, message: 'Please enter a name' }]}
          >
            <Input placeholder="My Skills Repository" />
          </Form.Item>
          <Form.Item
            label="Type"
            name="type"
            initialValue="git"
            rules={[{ required: true }]}
          >
            <Select>
              <Select.Option value="git">Git Repository</Select.Option>
              <Select.Option value="local">Local Directory</Select.Option>
            </Select>
          </Form.Item>
          <Form.Item noStyle shouldUpdate={(prev, curr) => prev.type !== curr.type}>
            {({ getFieldValue }) =>
              getFieldValue('type') === 'git' ? (
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
              ) : (
                <Form.Item
                  label="Local Path"
                  name="local_path"
                  rules={[{ required: true, message: 'Please enter a local path' }]}
                >
                  <Input placeholder="/path/to/local/directory" />
                </Form.Item>
              )
            }
          </Form.Item>
          <Form.Item label="Priority" name="priority" initialValue={10}>
            <Input type="number" />
          </Form.Item>
          <Form.Item label="Enabled" name="enabled" valuePropName="checked" initialValue={true}>
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
