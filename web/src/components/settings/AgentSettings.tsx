import React, { useState, useEffect, useRef } from 'react';
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
  Switch, 
  Popconfirm, 
  message,
  Tooltip,
  Avatar,
  Divider,
  Select,
  Checkbox
} from 'antd';
import { 
  PlusOutlined, 
  DeleteOutlined, 
  EditOutlined, 
  RobotOutlined,
  LinkOutlined,
  InfoCircleOutlined
} from '@ant-design/icons';
import type { 
  AgentInfo, 
  ChannelInfo, 
  BindingWithRelationsInfo, 
  RegisterAgentPayload, 
  UpdateAgentPayload,
  RegisterBindingPayload
} from '../../types';

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;
const { Option } = Select;

interface AgentSettingsProps {
  agents: AgentInfo[];
  channels: ChannelInfo[];
  bindings: BindingWithRelationsInfo[];
  onRegister: (data: RegisterAgentPayload) => void;
  onUnregister: (id: number) => void;
  onUpdate: (id: number, updates: UpdateAgentPayload) => void;
  onRegisterBinding: (data: RegisterBindingPayload) => void;
  onUnregisterBinding: (id: number) => void;
}

const slugify = (s: string): string => {
  return s.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '').slice(0, 32);
};

export const AgentSettings: React.FC<AgentSettingsProps> = ({
  agents,
  channels,
  bindings,
  onRegister,
  onUnregister,
  onUpdate,
  onRegisterBinding,
  onUnregisterBinding
}) => {
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingAgent, setEditingAgent] = useState<AgentInfo | null>(null);
  const [form] = Form.useForm();
  
  // For binding creation in the same form
  const [bindToChannel, setBindToChannel] = useState<number | null>(null);
  
  const pendingBinding = useRef<{ folder: string; channelId: number; jid: string; isAdmin: boolean; botToken: string } | null>(null);

  // When a new agent appears that matches a pending binding, create the binding
  useEffect(() => {
    if (!pendingBinding.current) return;
    const pb = pendingBinding.current;
    const newAgent = agents.find(a => a.folder === pb.folder);
    if (newAgent) {
      onRegisterBinding({ 
        agentId: newAgent.id, 
        channelId: pb.channelId, 
        ...(pb.jid ? { jid: pb.jid } : {}), 
        ...(pb.isAdmin ? { isAdmin: true } : {}), 
        ...(pb.botToken ? { botTokenOverride: pb.botToken } : {}) 
      });
      pendingBinding.current = null;
    }
  }, [agents, onRegisterBinding]);

  const handleAdd = () => {
    setEditingAgent(null);
    setBindToChannel(null);
    form.resetFields();
    setIsModalOpen(true);
  };

  const handleEdit = (agent: AgentInfo) => {
    setEditingAgent(agent);
    setBindToChannel(null);
    form.setFieldsValue({
      name: agent.name,
      folder: agent.folder,
      requiresTrigger: agent.requiresTrigger,
      workDirs: agent.allowedWorkDirs?.join('\n') ?? ''
    });
    setIsModalOpen(true);
  };

  const onFinish = (values: any) => {
    const workDirs = values.workDirs?.trim() ? values.workDirs.split('\n').map((s: string) => s.trim()).filter(Boolean) : null;
    
    if (editingAgent) {
      onUpdate(editingAgent.id, {
        name: values.name,
        requiresTrigger: values.requiresTrigger,
        allowedWorkDirs: workDirs
      });
      message.success('Agent updated');
    } else {
      onRegister({
        name: values.name,
        folder: values.folder,
        requiresTrigger: values.requiresTrigger,
        allowedWorkDirs: workDirs
      });
      
      // If channel selected, queue binding creation
      if (values.bindChannelId) {
        pendingBinding.current = { 
          folder: values.folder.trim(), 
          channelId: Number(values.bindChannelId), 
          jid: (values.bindJid || '').trim(), 
          isAdmin: !!values.bindIsAdmin, 
          botToken: (values.bindBotToken || '').trim() 
        };
      }
      message.success('Agent created');
    }
    setIsModalOpen(false);
  };

  const agentBindings = (agentId: number) => bindings.filter(b => b.agent.id === agentId);

  const columns = [
    {
      title: 'Agent',
      key: 'agent',
      render: (_: any, record: AgentInfo) => {
        const ab = agentBindings(record.id);
        const isAdmin = ab.some(b => b.isAdmin);
        return (
          <Space>
            <Avatar 
              style={{ backgroundColor: isAdmin ? '#faad14' : '#5BBFE8' }} 
              icon={<RobotOutlined />} 
            />
            <div>
              <Text strong>{record.name}</Text>
              <br />
              <Text type="secondary" style={{ fontSize: 12 }}>ID: {record.folder}</Text>
            </div>
            {isAdmin && <Tag color="gold" style={{ borderRadius: 4 }}>Main</Tag>}
          </Space>
        );
      },
    },
    {
      title: 'Bindings',
      key: 'bindings',
      render: (_: any, record: AgentInfo) => {
        const ab = agentBindings(record.id);
        if (ab.length === 0) return <Text type="secondary" italic>Web only</Text>;
        return (
          <Space wrap size={[0, 4]}>
            {ab.map(b => (
              <Tag 
                key={b.id} 
                color={b.isAdmin ? 'orange' : 'blue'}
                style={{ borderRadius: 6 }}
                closable={!b.isAdmin}
                onClose={() => onUnregisterBinding(b.id)}
              >
                <Space size={4}>
                  <LinkOutlined />
                  {b.channel.name}
                  {b.jid && <Text style={{ fontSize: 10, opacity: 0.7 }}>({b.jid})</Text>}
                </Space>
              </Tag>
            ))}
          </Space>
        );
      },
    },
    {
      title: 'Config',
      key: 'config',
      render: (_: any, record: AgentInfo) => (
        <Space direction="vertical" size={0}>
          {record.requiresTrigger && <Tag style={{ borderRadius: 4 }}>@mention</Tag>}
          {record.allowedWorkDirs && <Tag color="processing" style={{ borderRadius: 4 }}>Workdir limits</Tag>}
        </Space>
      ),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: any, record: AgentInfo) => {
        const ab = agentBindings(record.id);
        const isAdmin = ab.some(b => b.isAdmin);
        return (
          <Space size="middle">
            <Tooltip title="Edit">
              <Button 
                type="text" 
                icon={<EditOutlined />} 
                onClick={() => handleEdit(record)}
              />
            </Tooltip>
            {!isAdmin && (
              <Popconfirm
                title="Delete agent?"
                description="Are you sure? This will remove the agent and all its data."
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
            )}
          </Space>
        );
      },
    },
  ];

  return (
    <div style={{ maxWidth: 1000 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 32 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Agents</Title>
          <Text type="secondary">Define and configure your AI agents and their access.</Text>
        </div>
        <Button 
          type="primary" 
          icon={<PlusOutlined />} 
          onClick={handleAdd}
          style={{ borderRadius: 8, height: 40 }}
        >
          New Agent
        </Button>
      </div>

      <Card 
        style={{ borderRadius: 12, border: '1px solid #f0f0f0' }}
        styles={{ body: { padding: 0 } }}
      >
        <Table 
          columns={columns} 
          dataSource={agents} 
          rowKey="id"
          pagination={false}
          locale={{ emptyText: 'No agents created yet.' }}
        />
      </Card>

      <Modal
        title={editingAgent ? 'Edit Agent' : 'Create New Agent'}
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
          initialValues={{ requiresTrigger: true }}
          style={{ marginTop: 24 }}
        >
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
            <Form.Item
              name="name"
              label="Display Name"
              rules={[{ required: true, message: 'Required' }]}
            >
              <Input 
                placeholder="Team Assistant" 
                onChange={e => {
                  if (!editingAgent) {
                    const currentFolder = form.getFieldValue('folder');
                    if (!currentFolder || currentFolder === slugify(form.getFieldValue('name') || '')) {
                      form.setFieldsValue({ folder: slugify(e.target.value) });
                    }
                  }
                }}
              />
            </Form.Item>

            <Form.Item
              name="folder"
              label="Agent ID (Folder)"
              rules={[{ required: true, message: 'Required' }]}
            >
              <Input 
                placeholder="team-assistant" 
                disabled={!!editingAgent}
                style={{ fontFamily: 'monospace' }}
              />
            </Form.Item>
          </div>

          <Form.Item
            name="requiresTrigger"
            label="Require @mention to respond"
            valuePropName="checked"
          >
            <Switch />
          </Form.Item>

          <Form.Item
            name="workDirs"
            label="Allowed Working Directories"
            tooltip="Restrict agent to these paths (one per line). Leave empty for no restriction."
          >
            <TextArea 
              rows={3} 
              placeholder="/Users/name/projects/my-app" 
              style={{ fontFamily: 'monospace' }}
            />
          </Form.Item>

          {!editingAgent && (
            <>
              <Divider plain><Text type="secondary" style={{ fontSize: 12 }}>Optional Channel Binding</Text></Divider>
              
              <Form.Item
                name="bindChannelId"
                label="Bind to Channel"
              >
                <Select 
                  placeholder="Select a channel to bind immediately" 
                  allowClear
                  onChange={val => setBindToChannel(val)}
                >
                  {channels.map(c => (
                    <Option key={c.id} value={c.id}>{c.name} ({c.platformType})</Option>
                  ))}
                </Select>
              </Form.Item>

              {bindToChannel && (
                <div style={{ backgroundColor: '#f9f9f9', padding: '16px', borderRadius: '8px', marginBottom: '24px' }}>
                  <Form.Item
                    name="bindJid"
                    label="Chat ID (JID)"
                    extra="Leave empty for auto-binding on first message"
                  >
                    <Input placeholder="e.g. tg:group:-100123" style={{ fontFamily: 'monospace' }} />
                  </Form.Item>

                  <Form.Item
                    name="bindIsAdmin"
                    valuePropName="checked"
                    style={{ marginBottom: 12 }}
                  >
                    <Checkbox>Set as Main Agent for this channel</Checkbox>
                  </Form.Item>

                </div>
              )}
            </>
          )}

          <Form.Item style={{ marginBottom: 0, marginTop: 24, textAlign: 'right' }}>
            <Space>
              <Button onClick={() => setIsModalOpen(false)}>Cancel</Button>
              <Button type="primary" htmlType="submit">
                {editingAgent ? 'Save Changes' : 'Create Agent'}
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};
