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
  Select
} from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  EditOutlined,
  RobotOutlined,
  LinkOutlined,
  InfoCircleOutlined,
  ThunderboltOutlined
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
  
  const pendingBindings = useRef<{ folder: string; channelIds: number[] } | null>(null);

  // When a new agent appears that matches pending bindings, create them
  useEffect(() => {
    if (!pendingBindings.current) return;
    const pb = pendingBindings.current;
    const newAgent = agents.find(a => a.folder === pb.folder);
    if (newAgent) {
      pb.channelIds.forEach(channelId => {
        onRegisterBinding({ agentId: newAgent.id, channelId });
      });
      pendingBindings.current = null;
    }
  }, [agents, onRegisterBinding]);

  const handleAdd = () => {
    setEditingAgent(null);
    form.resetFields();
    setIsModalOpen(true);
  };

  const handleEdit = (agent: AgentInfo) => {
    setEditingAgent(agent);
    const currentChannelIds = bindings
      .filter(b => (b.agent?.id ?? b.agentId) === agent.id)
      .map(b => b.channel?.id ?? b.channelId);
    form.setFieldsValue({
      name: agent.name,
      folder: agent.folder,
      requiresTrigger: agent.requiresTrigger,
      workDirsList: agent.allowedWorkDirs ?? [],
      corePrompt: agent.corePrompt ?? '',
      memory: '',  // populated by fetch below
      modelId: agent.modelId ?? undefined,
      bindChannelIds: currentChannelIds,
    });
    setIsModalOpen(true);

    // Pull live SOUL.md + MEMORY.md from disk. The agent record's
    // `corePrompt` is the DB copy; SOUL.md may have drifted via direct edits
    // or the persona_update tool, so re-fetch and trust the file.
    fetch(`/api/agents/${encodeURIComponent(agent.folder)}/files`)
      .then(r => r.json())
      .then((data: { soul: string; memory: string }) => {
        form.setFieldsValue({
          corePrompt: data.soul || agent.corePrompt || '',
          memory: data.memory ?? '',
        });
      })
      .catch(() => { /* file-fetch failure is non-fatal — keep DB defaults */ });
  };

  const writeFiles = async (folder: string, soul?: string, memory?: string) => {
    try {
      await fetch(`/api/agents/${encodeURIComponent(folder)}/files`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ soul, memory }),
      });
    } catch { /* non-fatal */ }
  };

  const onFinish = (values: any) => {
    const rawList = Array.isArray(values.workDirsList) ? values.workDirsList : [];
    const workDirs = rawList.map((s: string) => (s ?? '').trim()).filter(Boolean);
    const workDirsPayload: string[] | null = workDirs.length > 0 ? workDirs : null;
    const memoryContent: string = values.memory ?? '';

    if (editingAgent) {
      onUpdate(editingAgent.id, {
        name: values.name,
        requiresTrigger: values.requiresTrigger,
        allowedWorkDirs: workDirsPayload,
        corePrompt: values.corePrompt ?? '',
        modelId: values.modelId || null,
      });

      // MEMORY.md isn't part of the WS update payload (it lives only on
      // disk), so persist it separately. SOUL.md was already written by the
      // backend's agent update via corePrompt; we don't need to PUT soul.
      writeFiles(editingAgent.folder, undefined, memoryContent);

      // Sync channel bindings: add new, remove removed
      const currentBindings = bindings.filter(b => (b.agent?.id ?? b.agentId) === editingAgent.id);
      const currentChannelIds = new Set(currentBindings.map(b => b.channel?.id ?? b.channelId));
      const newChannelIds = new Set<number>((values.bindChannelIds ?? []).map(Number));

      currentBindings.forEach(b => {
        if (!newChannelIds.has(b.channel?.id ?? b.channelId)) onUnregisterBinding(b.id);
      });
      newChannelIds.forEach(channelId => {
        if (!currentChannelIds.has(channelId)) onRegisterBinding({ agentId: editingAgent.id, channelId });
      });

      message.success('Profile updated · SOUL.md & MEMORY.md saved, memory re-ingested');
    } else {
      onRegister({
        name: values.name,
        folder: values.folder,
        requiresTrigger: values.requiresTrigger,
        allowedWorkDirs: workDirsPayload,
        corePrompt: values.corePrompt ?? '',
        modelId: values.modelId || null,
      });

      // After backend scaffolds the directory, write the initial MEMORY.md
      // if the user provided any. Folder is known at create time so we can
      // PUT directly without waiting for an agent:registered event.
      if (memoryContent.trim()) {
        writeFiles(values.folder.trim(), undefined, memoryContent);
      }

      if (values.bindChannelIds?.length) {
        pendingBindings.current = {
          folder: values.folder.trim(),
          channelIds: values.bindChannelIds.map(Number),
        };
      }
      message.success('Profile created · SOUL.md + MEMORY.md scaffolded, cognitive memory ingested');
    }
    setIsModalOpen(false);
  };

  const agentBindings = (agentId: number) =>
    bindings.filter(b => (b.agent?.id ?? b.agentId) === agentId);

  // For a given channel, return the agent id it's already exclusively bound to
  // (undefined if channel supports multi-bind or has no binding).
  const exclusivelyBoundTo = (channelId: number): number | undefined => {
    const ch = channels.find(c => c.id === channelId);
    if (!ch || ch.platformType === 'senclaw') return undefined;
    const existing = bindings.find(b => (b.channel?.id ?? b.channelId) === channelId);
    return existing ? (existing.agent?.id ?? existing.agentId) : undefined;
  };

  const columns = [
    {
      title: 'Profile',
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
                  {b.channel?.name ?? `ch#${b.channelId}`}
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
                title="Delete profile?"
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
          <Title level={4} style={{ margin: 0 }}>Profile</Title>
          <Text type="secondary">
            Each profile owns its own SOUL.md (persona) and MEMORY.md (aggregated memory).
            Editing this form rewrites SOUL.md and re-ingests it into cognitive memory.
          </Text>
        </div>
        <Button 
          type="primary" 
          icon={<PlusOutlined />} 
          onClick={handleAdd}
          style={{ borderRadius: 8, height: 40 }}
        >
          New Profile
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
          locale={{ emptyText: 'No profiles created yet.' }}
        />
      </Card>

      <Modal
        title={editingAgent ? 'Edit Profile' : 'Create New Profile'}
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
                placeholder="My Profile"
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
              label="Profile ID (Folder)"
              rules={[{ required: true, message: 'Required' }]}
            >
              <Input 
                placeholder="my-profile"
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
            label="Allowed Working Directories"
            tooltip="Restrict the profile to these paths. Leave empty for no restriction (full filesystem access)."
          >
            <Form.List name="workDirsList">
              {(fields, { add, remove }) => (
                <div className="space-y-2">
                  {fields.map(({ key, name, ...rest }) => (
                    <div key={key} className="flex items-center gap-2">
                      <Form.Item
                        {...rest}
                        name={name}
                        rules={[
                          { required: true, message: 'Path required' },
                          { whitespace: true, message: 'Path required' },
                        ]}
                        style={{ marginBottom: 0, flex: 1 }}
                      >
                        <Input
                          placeholder="/Users/name/projects/my-app or ~/code/project"
                          style={{ fontFamily: 'ui-monospace, SFMono-Regular, monospace', fontSize: 12 }}
                        />
                      </Form.Item>
                      <Button
                        type="text"
                        danger
                        size="small"
                        icon={<DeleteOutlined />}
                        onClick={() => remove(name)}
                        aria-label="Remove path"
                      />
                    </div>
                  ))}
                  <Button
                    type="dashed"
                    size="small"
                    icon={<PlusOutlined />}
                    onClick={() => add('')}
                    block
                  >
                    Add directory
                  </Button>
                </div>
              )}
            </Form.List>
          </Form.Item>

          <Form.Item
            name="corePrompt"
            label="Persona (SOUL.md)"
            tooltip="The profile's persona — written to agents/<folder>/SOUL.md on save and re-ingested into cognitive memory. Describe identity, expertise, tone, and any behaviour-shaping rules."
            extra="Saved to SOUL.md; cognitive memory will re-aggregate this content automatically."
          >
            <TextArea
              rows={6}
              placeholder="You are a helpful assistant that specializes in..."
              style={{ fontFamily: 'monospace' }}
            />
          </Form.Item>

          <Form.Item
            name="memory"
            label="Aggregated Memory (MEMORY.md)"
            tooltip="Free-form long-term memory for this profile — facts, references, conventions, prior conclusions. Written to agents/<folder>/MEMORY.md and indexed by the memory manager."
            extra="Saved to MEMORY.md. The agent reads from this file across sessions; edit freely between conversations."
          >
            <TextArea
              rows={6}
              placeholder={'# Memory\n\n## Facts\n- ...\n\n## Conventions\n- ...'}
              style={{ fontFamily: 'monospace' }}
            />
          </Form.Item>

          <Divider plain><Text type="secondary" style={{ fontSize: 12 }}>Channel Binding</Text></Divider>

          <Form.Item
            name="bindChannelIds"
            label="Bound Channels"
            rules={[{
              validator: (_, selectedIds: number[] = []) => {
                const conflicts = selectedIds.filter(id => {
                  const boundTo = exclusivelyBoundTo(id);
                  return boundTo !== undefined && boundTo !== editingAgent?.id;
                });
                if (conflicts.length > 0) {
                  const names = conflicts.map(id => channels.find(c => c.id === id)?.name ?? `#${id}`).join(', ');
                  return Promise.reject(`${names} already bound to another agent. Only Senclaw Connector supports multiple bindings.`);
                }
                return Promise.resolve();
              },
            }]}
          >
            <Select
              mode="multiple"
              placeholder="Select one or more channels"
              allowClear
              optionFilterProp="label"
              options={channels.map(c => {
                const boundTo = exclusivelyBoundTo(c.id);
                const takenByOther = boundTo !== undefined && boundTo !== editingAgent?.id;
                const takenAgent = takenByOther ? agents.find(a => a.id === boundTo) : undefined;
                return {
                  value: c.id,
                  label: `${c.name} (${c.platformType})`,
                  disabled: takenByOther,
                  title: takenByOther ? `Already bound to agent "${takenAgent?.name ?? boundTo}"` : undefined,
                };
              })}
            />
          </Form.Item>

          <Form.Item style={{ marginBottom: 0, marginTop: 24, textAlign: 'right' }}>
            <Space>
              <Button onClick={() => setIsModalOpen(false)}>Cancel</Button>
              <Button type="primary" htmlType="submit">
                {editingAgent ? 'Save Profile' : 'Create Profile'}
              </Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
};
