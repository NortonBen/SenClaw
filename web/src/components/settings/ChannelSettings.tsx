import React, { useState } from 'react';
import { 
  Typography, 
  Button, 
  Card, 
  Space, 
  Table, 
  Tag, 
  Modal, 
  Form, 
  Input, 
  Select, 
  Popconfirm, 
  message,
  Tooltip
} from 'antd';
import { 
  PlusOutlined, 
  DeleteOutlined, 
  EditOutlined, 
  CloudServerOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined
} from '@ant-design/icons';
import type { ChannelInfo, RegisterChannelPayload, UpdateChannelPayload } from '../../types';

const { Title, Text } = Typography;
const { Option } = Select;
const { TextArea } = Input;

interface ChannelSettingsProps {
  channels: ChannelInfo[];
  onRegister: (data: RegisterChannelPayload) => void;
  onUnregister: (id: number) => void;
  onUpdate: (id: number, updates: UpdateChannelPayload) => void;
}

const PLATFORM_LABELS: Record<string, string> = {
  telegram: 'Telegram',
  feishu: 'Feishu',
  qq: 'QQ',
  wechat: 'WeChat',
  web: 'Web',
};

export const ChannelSettings: React.FC<ChannelSettingsProps> = ({
  channels,
  onRegister,
  onUnregister,
  onUpdate
}) => {
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingChannel, setEditingChannel] = useState<ChannelInfo | null>(null);
  const [form] = Form.useForm();

  const handleAdd = () => {
    setEditingChannel(null);
    form.resetFields();
    setIsModalOpen(true);
  };

  const handleEdit = (channel: ChannelInfo) => {
    setEditingChannel(channel);
    form.setFieldsValue({
      name: channel.name,
      platformType: channel.platformType,
      credentialsJson: channel.credentialsJson
    });
    setIsModalOpen(true);
  };

  const handleCancel = () => {
    setIsModalOpen(false);
    form.resetFields();
  };

  const onFinish = (values: any) => {
    let credentials = {};
    try {
      credentials = JSON.parse(values.credentialsJson);
    } catch (e) {
      message.error('Invalid JSON in credentials');
      return;
    }

    if (editingChannel) {
      onUpdate(editingChannel.id, {
        name: values.name,
        credentials
      });
      message.success('Channel updated');
    } else {
      onRegister({
        name: values.name,
        platformType: values.platformType,
        credentials
      });
      message.success('Channel registered');
    }
    setIsModalOpen(false);
    form.resetFields();
  };

  const columns = [
    {
      title: 'Platform',
      dataIndex: 'platformType',
      key: 'platformType',
      render: (platform: string) => (
        <Tag color="blue" style={{ borderRadius: 6 }}>
          {PLATFORM_LABELS[platform] || platform}
        </Tag>
      ),
    },
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (text: string) => <Text strong>{text}</Text>,
    },
    {
      title: 'Status',
      dataIndex: 'connectionState',
      key: 'connectionState',
      render: (state: string) => {
        const isConnected = state === 'connected';
        return (
          <Space>
            {isConnected ? (
              <CheckCircleOutlined style={{ color: '#52c41a' }} />
            ) : (
              <CloseCircleOutlined style={{ color: '#bfbfbf' }} />
            )}
            <Text type={isConnected ? 'success' : 'secondary'}>{state}</Text>
          </Space>
        );
      },
    },
    {
      title: 'Created At',
      dataIndex: 'createdAt',
      key: 'createdAt',
      render: (date: string) => new Date(date).toLocaleDateString(),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: any, record: ChannelInfo) => (
        <Space size="middle">
          <Tooltip title="Edit">
            <Button 
              type="text" 
              icon={<EditOutlined />} 
              onClick={() => handleEdit(record)}
            />
          </Tooltip>
          <Popconfirm
            title="Unregister channel?"
            description="Are you sure you want to unregister this channel? This will affect all bound agents."
            onConfirm={() => onUnregister(record.id)}
            okText="Yes"
            cancelText="No"
            okButtonProps={{ danger: true }}
          >
            <Tooltip title="Delete">
              <Button 
                type="text" 
                danger 
                icon={<DeleteOutlined />} 
              />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ maxWidth: 1000 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 32 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Channels</Title>
          <Text type="secondary">Manage your communication platforms and credentials.</Text>
        </div>
        <Button 
          type="primary" 
          icon={<PlusOutlined />} 
          onClick={handleAdd}
          style={{ borderRadius: 8, height: 40 }}
        >
          Add Channel
        </Button>
      </div>

      <Card 
        style={{ borderRadius: 12, border: '1px solid #f0f0f0' }}
        styles={{ body: { padding: 0 } }}
      >
        <Table 
          columns={columns} 
          dataSource={channels} 
          rowKey="id"
          pagination={false}
          locale={{ emptyText: 'No channels configured yet.' }}
        />
      </Card>

      <Modal
        title={editingChannel ? 'Edit Channel' : 'Add New Channel'}
        open={isModalOpen}
        onCancel={handleCancel}
        footer={null}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={onFinish}
          initialValues={{ platformType: 'telegram', credentialsJson: '{}' }}
          style={{ marginTop: 24 }}
        >
          <Form.Item
            name="platformType"
            label="Platform"
            rules={[{ required: true }]}
          >
            <Select placeholder="Select a platform">
              {Object.entries(PLATFORM_LABELS).map(([key, label]) => (
                <Option key={key} value={key}>{label}</Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item
            name="name"
            label="Channel Name"
            rules={[{ required: true, message: 'Please input channel name' }]}
          >
            <Input placeholder="e.g. My Telegram Bot" />
          </Form.Item>

          <Form.Item
            name="credentialsJson"
            label="Credentials (JSON)"
            rules={[{ required: true, message: 'Please input credentials' }]}
          >
            <TextArea 
              rows={6} 
              placeholder='{"botToken": "your-token-here"}' 
              style={{ fontFamily: 'monospace' }}
            />
          </Form.Item>

          <Form.Item style={{ marginBottom: 0, marginTop: 24, textAlign: 'right' }}>
            <Space>
              <Button onClick={handleCancel}>Cancel</Button>
              <Button type="primary" htmlType="submit">
                {editingChannel ? 'Save Changes' : 'Register Channel'}
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};
