import React, { useEffect, useState } from 'react';
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
  Select,
  Popconfirm,
  message,
  Tooltip,
  Avatar,
  Descriptions,
  Divider
} from 'antd';
import {
  DeleteOutlined,
  EditOutlined,
  TeamOutlined,
  CrownOutlined,
  PlusOutlined,
  InfoCircleOutlined
} from '@ant-design/icons';
import type { AgentInfo, ChannelInfo, GroupInfo, RegisterGroupPayload, UpdateGroupPayload } from '../../types';

const { Title, Text } = Typography;
const { TextArea } = Input;

interface GroupSettingsProps {
  groups: GroupInfo[];
  agents: AgentInfo[];
  channels: ChannelInfo[];
  onRegisterGroup: (data: RegisterGroupPayload) => void;
  onUpdateGroup: (jid: string, updates: UpdateGroupPayload) => void;
  onUnregisterGroup: (jid: string) => void;
}

const PLATFORM_COLORS: Record<string, string> = {
  telegram: '#2AABEE',
  feishu: '#3370FF',
  qq: '#12B7F5',
  wechat: '#07C160',
  web: '#5BBFE8',
};

const GROUP_TYPE_OPTIONS = [
  { value: 'chat', label: 'Chat' },
  { value: 'cowork', label: 'Cowork' },
  { value: 'code', label: 'Code' },
];

const GROUP_TYPE_COLORS: Record<string, string> = {
  chat: '#5BBFE8',
  cowork: '#722ED1',
  code: '#52C41A',
};

interface LlmConfigLite {
  id: string;
  label: string;
  modelName?: string;
}

export const GroupSettings: React.FC<GroupSettingsProps> = ({
  groups,
  agents,
  channels,
  onRegisterGroup,
  onUpdateGroup,
  onUnregisterGroup,
}) => {
  const [editingGroup, setEditingGroup] = useState<GroupInfo | null>(null);
  const [isCreateOpen, setIsCreateOpen] = useState(false);
  const [form] = Form.useForm();
  const [createForm] = Form.useForm();
  const [llmConfigs, setLlmConfigs] = useState<LlmConfigLite[]>([]);
  const [activeLlmId, setActiveLlmId] = useState<string | null>(null);

  // Load the global LLM config list so groups can pick a per-group model.
  useEffect(() => {
    (async () => {
      try {
        const r = await fetch('/api/llm-config');
        const d = await r.json();
        const list: LlmConfigLite[] = Array.isArray(d) ? d : (d.configs ?? d.data ?? []);
        setLlmConfigs(list);
        if (!Array.isArray(d)) setActiveLlmId(d.activeId ?? null);
      } catch {
        // Non-fatal: the dropdown just shows "Default" only.
      }
    })();
  }, []);

  // Option list for the model picker: a "Default" entry (clears the override)
  // followed by every configured LLM. Label the active one so users know the
  // fallback target.
  const modelOptions = [
    {
      value: '',
      label:
        'Default (global active' +
        (activeLlmId
          ? `: ${llmConfigs.find(c => c.id === activeLlmId)?.label ?? activeLlmId}`
          : '') +
        ')',
    },
    ...llmConfigs.map(c => ({
      value: c.id,
      label: c.modelName ? `${c.label} — ${c.modelName}` : c.label,
    })),
  ];

  const modelLabel = (id: string | null | undefined) => {
    if (!id) return null;
    const c = llmConfigs.find(x => x.id === id);
    return c ? (c.label || c.modelName || id) : id;
  };

  // ---- Edit handlers ----

  const handleEdit = (group: GroupInfo) => {
    setEditingGroup(group);
    form.setFieldsValue({
      name: group.name,
      groupType: group.groupType || 'chat',
      isAdmin: group.isAdmin,
      modelId: group.modelId ?? '',
      allowedTools: group.allowedTools?.join('\n') ?? '',
      allowedPaths: group.allowedPaths?.join('\n') ?? '',
      allowedWorkDirs: group.allowedWorkDirs?.join('\n') ?? '',
    });
  };

  const handleSave = () => {
    const values = form.getFieldsValue();
    if (!editingGroup) return;

    const toArray = (s: string | undefined) =>
      s?.trim() ? s.split('\n').map((l: string) => l.trim()).filter(Boolean) : null;

    onUpdateGroup(editingGroup.jid, {
      name: values.name,
      groupType: values.groupType,
      isAdmin: values.isAdmin,
      modelId: values.modelId ? values.modelId : null,
      allowedTools: toArray(values.allowedTools),
      allowedPaths: toArray(values.allowedPaths),
      allowedWorkDirs: toArray(values.allowedWorkDirs),
    });
    message.success('Group updated');
    setEditingGroup(null);
  };

  // ---- Create handlers ----

  const handleCreate = () => {
    createForm.resetFields();
    createForm.setFieldsValue({ groupType: 'chat', channel: 'telegram', requiresTrigger: true });
    setIsCreateOpen(true);
  };

  const handleCreateFinish = () => {
    const values = createForm.getFieldsValue();
    if (!values.folder || !values.name) {
      message.error('Agent folder and name are required');
      return;
    }

    const toArray = (s: string | undefined) =>
      s?.trim() ? s.split('\n').map((l: string) => l.trim()).filter(Boolean) : null;

    onRegisterGroup({
      jid: values.jid || undefined,
      folder: values.folder,
      name: values.name,
      channel: values.channel,
      groupType: values.groupType || 'chat',
      requiresTrigger: values.requiresTrigger,
      modelId: values.modelId ? values.modelId : null,
      allowedTools: toArray(values.allowedTools),
      allowedPaths: toArray(values.allowedPaths),
      allowedWorkDirs: toArray(values.allowedWorkDirs),
    });
    message.success('Group created');
    setIsCreateOpen(false);
  };

  const adminCount = groups.filter(g => g.isAdmin).length;

  const columns = [
    {
      title: 'Group',
      key: 'group',
      render: (_: unknown, record: GroupInfo) => (
        <Space>
          <Avatar
            style={{ backgroundColor: record.isAdmin ? '#faad14' : PLATFORM_COLORS[record.channel] ?? '#5BBFE8' }}
            icon={record.isAdmin ? <CrownOutlined /> : <TeamOutlined />}
          />
          <div>
            <Text strong>{record.name || 'Unnamed'}</Text>
            {record.isAdmin && <Tag color="gold" style={{ marginLeft: 8, borderRadius: 4 }}>Admin</Tag>}
            <br />
            <Text type="secondary" style={{ fontSize: 12 }}>
              {record.jid}
            </Text>
          </div>
        </Space>
      ),
    },
    {
      title: 'Type',
      key: 'groupType',
      width: 100,
      render: (_: unknown, record: GroupInfo) => (
        <Tag color={GROUP_TYPE_COLORS[record.groupType] ?? 'default'} style={{ borderRadius: 4 }}>
          {record.groupType || 'chat'}
        </Tag>
      ),
    },
    {
      title: 'Channel',
      key: 'channel',
      width: 120,
      render: (_: unknown, record: GroupInfo) => (
        <Tag color={PLATFORM_COLORS[record.channel] ?? 'default'} style={{ borderRadius: 4 }}>
          {record.channel}
        </Tag>
      ),
    },
    {
      title: 'Agent',
      key: 'agent',
      width: 140,
      render: (_: unknown, record: GroupInfo) => (
        <Text code style={{ fontSize: 12 }}>{record.folder || '—'}</Text>
      ),
    },
    {
      title: 'Model',
      key: 'model',
      width: 140,
      render: (_: unknown, record: GroupInfo) => {
        const label = modelLabel(record.modelId);
        return label ? (
          <Tag color="geekblue" style={{ borderRadius: 4 }}>{label}</Tag>
        ) : (
          <Text type="secondary" style={{ fontSize: 12 }}>Default</Text>
        );
      },
    },
    {
      title: 'Messages',
      key: 'messages',
      width: 100,
      render: (_: unknown, record: GroupInfo) => (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {record.maxMessages ?? '—'}
        </Text>
      ),
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 120,
      render: (_: unknown, record: GroupInfo) => (
        <Space size="middle">
          <Tooltip title="Edit">
            <Button
              type="text"
              icon={<EditOutlined />}
              onClick={() => handleEdit(record)}
            />
          </Tooltip>
          <Popconfirm
            title="Remove group?"
            description="This unregisters the group. The agent folder and data are preserved."
            onConfirm={() => {
              onUnregisterGroup(record.jid);
              message.success('Group removed');
            }}
            okText="Yes"
            cancelText="No"
            okButtonProps={{ danger: true }}
          >
            <Tooltip title="Remove">
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

  const renderToolsPathsWorkDirsFields = () => (
    <>
      <Form.Item
        name="allowedTools"
        label="Allowed Tools"
        tooltip="Restrict tools the agent can use (one per line). Leave empty for no restriction."
      >
        <TextArea
          rows={3}
          placeholder="Read&#10;Write&#10;Grep"
          style={{ fontFamily: 'monospace' }}
        />
      </Form.Item>

      <Form.Item
        name="allowedPaths"
        label="Allowed Paths"
        tooltip="Restrict accessible filesystem paths (one per line). Leave empty for no restriction."
      >
        <TextArea
          rows={3}
          placeholder="/Users/name/projects&#10;/tmp"
          style={{ fontFamily: 'monospace' }}
        />
      </Form.Item>

      <Form.Item
        name="allowedWorkDirs"
        label="Allowed Working Directories"
        tooltip="Restrict working directories (one per line). Leave empty for no restriction."
      >
        <TextArea
          rows={3}
          placeholder="/Users/name/projects/my-app"
          style={{ fontFamily: 'monospace' }}
        />
      </Form.Item>
    </>
  );

  return (
    <div style={{ maxWidth: 1000 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 32 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Groups</Title>
          <Text type="secondary">
            Manage chat groups — create, rename, set admin privileges, or remove.
            {adminCount > 0 && <span> · <Tag color="gold" style={{ borderRadius: 4 }}>{adminCount} admin{adminCount > 1 ? 's' : ''}</Tag></span>}
          </Text>
        </div>
        <Button
          type="primary"
          icon={<PlusOutlined />}
          onClick={handleCreate}
          style={{ borderRadius: 8, height: 40 }}
        >
          New Group
        </Button>
      </div>

      <Card
        style={{ borderRadius: 12, border: '1px solid #f0f0f0' }}
        styles={{ body: { padding: 0 } }}
      >
        <Table
          columns={columns}
          dataSource={groups}
          rowKey="jid"
          pagination={false}
          locale={{ emptyText: 'No groups registered yet.' }}
        />
      </Card>

      {/* Edit Modal */}
      <Modal
        title="Edit Group"
        open={!!editingGroup}
        onCancel={() => setEditingGroup(null)}
        onOk={handleSave}
        okText="Save"
        destroyOnClose
        width={600}
      >
        {editingGroup && (
          <>
            <Descriptions
              column={1}
              size="small"
              style={{ marginBottom: 20 }}
              labelStyle={{ fontWeight: 500, color: '#8c8c8c' }}
            >
              <Descriptions.Item label="JID">
                <Text code style={{ fontSize: 12 }}>{editingGroup.jid}</Text>
              </Descriptions.Item>
              <Descriptions.Item label="Agent Folder">
                <Text code style={{ fontSize: 12 }}>{editingGroup.folder || '—'}</Text>
              </Descriptions.Item>
              <Descriptions.Item label="Channel">
                <Tag color={PLATFORM_COLORS[editingGroup.channel] ?? 'default'}>{editingGroup.channel}</Tag>
              </Descriptions.Item>
            </Descriptions>

            <Form form={form} layout="vertical">
              <Form.Item
                name="name"
                label="Group Name"
                rules={[{ required: true, message: 'Name is required' }]}
              >
                <Input placeholder="My Group" />
              </Form.Item>

              <Form.Item
                name="groupType"
                label="Group Type"
                tooltip="Chat = conversation, Cowork = multi-agent collaboration, Code = code-focused agent"
              >
                <Select options={GROUP_TYPE_OPTIONS} />
              </Form.Item>

              <Form.Item
                name="modelId"
                label="LLM Model"
                tooltip="Which LLM this group uses. 'Default' falls back to the globally active model set in the LLM settings."
              >
                <Select options={modelOptions} />
              </Form.Item>

              <Form.Item
                name="isAdmin"
                label="Admin Group"
                valuePropName="checked"
                tooltip="Admin groups can execute commands (/help, /reset, /history) and access settings. Only one admin group is recommended."
              >
                <Switch />
              </Form.Item>

              <Divider plain><Text type="secondary" style={{ fontSize: 12 }}>Permissions</Text></Divider>

              {renderToolsPathsWorkDirsFields()}
            </Form>
          </>
        )}
      </Modal>

      {/* Create Modal */}
      <Modal
        title="Create New Group"
        open={isCreateOpen}
        onCancel={() => setIsCreateOpen(false)}
        onOk={handleCreateFinish}
        okText="Create"
        destroyOnClose
        width={600}
      >
        <Form
          form={createForm}
          layout="vertical"
          initialValues={{ groupType: 'chat', requiresTrigger: true }}
          style={{ marginTop: 24 }}
        >
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
            <Form.Item
              name="folder"
              label="Agent"
              rules={[{ required: true, message: 'Required' }]}
              tooltip="Select the agent this group will use."
            >
              <Select
                showSearch
                placeholder="Select an agent"
                optionFilterProp="label"
                options={agents.map(a => ({
                  value: a.folder,
                  label: `${a.name} (${a.folder})`,
                }))}
              />
            </Form.Item>
            <Form.Item
              name="name"
              label="Group Name"
              rules={[{ required: true, message: 'Required' }]}
            >
              <Input placeholder="My Group" />
            </Form.Item>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
            <Form.Item
              name="jid"
              label="JID (Group ID)"
              tooltip="Unique group identifier. Auto-generated if left empty for feishu/qq."
            >
              <Input placeholder="tg:group:123456" style={{ fontFamily: 'monospace' }} />
            </Form.Item>
            <Form.Item
              name="channel"
              label="Channel"
              rules={[{ required: true, message: 'Required' }]}
              tooltip="Select the platform channel this group connects through."
            >
              <Select
                showSearch
                placeholder="Select a channel"
                optionFilterProp="label"
                options={channels.map(c => ({
                  value: c.platformType,
                  label: `${c.name} (${c.platformType})`,
                }))}
              />
            </Form.Item>
          </div>

          <Form.Item
            name="groupType"
            label="Group Type"
            tooltip="Chat = conversation, Cowork = multi-agent collaboration, Code = code-focused agent"
          >
            <Select options={GROUP_TYPE_OPTIONS} />
          </Form.Item>

          <Form.Item
            name="requiresTrigger"
            label="Require @mention to respond"
            valuePropName="checked"
          >
            <Switch />
          </Form.Item>

          <Form.Item
            name="modelId"
            label="LLM Model"
            tooltip="Which LLM this group uses. 'Default' falls back to the globally active model set in the LLM settings."
          >
            <Select options={modelOptions} />
          </Form.Item>

          <Divider plain><Text type="secondary" style={{ fontSize: 12 }}>Permissions</Text></Divider>

          {renderToolsPathsWorkDirsFields()}
        </Form>
      </Modal>
    </div>
  );
};
