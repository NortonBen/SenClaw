import React, { useState, useEffect, useCallback } from 'react';
import {
  Typography, Button, Card, Space, Tag, Modal, Form, Input, Select,
  Breadcrumb, Layout, Flex, Tabs, Badge, Avatar, Tooltip, Popconfirm,
  Empty, Dropdown, List, Divider, message, Spin, Segmented, Upload, Statistic,
  Row, Col, Progress
} from 'antd';
import {
  PlusOutlined, TeamOutlined, ProjectOutlined, MessageOutlined,
  AppstoreOutlined, DeleteOutlined, EditOutlined, HomeOutlined,
  CheckCircleOutlined, ClockCircleOutlined, ExclamationCircleOutlined,
  PauseCircleOutlined, ThunderboltOutlined, UserOutlined,
  RobotOutlined, SendOutlined, ReloadOutlined, MoreOutlined,
  BugOutlined, ArrowRightOutlined, InboxOutlined,
  FileOutlined, FileTextOutlined, FolderOutlined, DownloadOutlined, UploadOutlined, PaperClipOutlined
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { CoworkSidebar } from '../components/CoworkSidebar';
import type {
  CoworkWorkspace, CoworkBoardEntry,
  CoworkTask, CoworkMessage, CoworkTemplate, CoworkMember
} from '../types';

const { Title, Text, Paragraph } = Typography;
const { Content } = Layout;
const { TextArea } = Input;

const STATUS_COLORS: Record<string, string> = {
  backlog: '#8c8c8c', todo: '#1890ff', in_progress: '#faad14',
  review: '#722ed1', done: '#52c41a', blocked: '#ff4d4f',
};

const STATUS_LABELS: Record<string, string> = {
  backlog: 'Backlog', todo: 'To Do', in_progress: 'In Progress',
  review: 'Review', done: 'Done', blocked: 'Blocked',
};

const PRIORITY_COLORS: Record<string, string> = {
  low: '#8c8c8c', medium: '#1890ff', high: '#fa8c16', critical: '#ff4d4f',
};

const BOARD_SECTIONS = ['brief', 'guidelines', 'progress', 'reference', 'decisions'];

const KANBAN_COLUMNS: CoworkTask['status'][] = ['backlog', 'todo', 'in_progress', 'review', 'done'];

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function renderMarkdown(md: string): string {
  let html = md
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    // Headers
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    // Bold and italic
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    // Inline code
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    // Code blocks
    .replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code>$2</code></pre>')
    // Links
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2">$1</a>')
    // Line breaks
    .replace(/\n/g, '<br/>');
  return html;
}

export function CoworkPage() {
  const { ws } = useAppContext();
  const navigate = useNavigate();

  // State
  const [workspaces, setWorkspaces] = useState<CoworkWorkspace[]>([]);
  const [selectedWs, setSelectedWs] = useState<CoworkWorkspace | null>(null);
  const [tasks, setTasks] = useState<CoworkTask[]>([]);
  const [board, setBoard] = useState<CoworkBoardEntry[]>([]);
  const [messages, setMessages] = useState<CoworkMessage[]>([]);
  const [templates, setTemplates] = useState<CoworkTemplate[]>([]);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<string>('tasks');
  const [wsFiles, setWsFiles] = useState<any[]>([]);
  const [selectedFilePath, setSelectedFilePath] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [fileLoading, setFileLoading] = useState(false);
  const [members, setMembers] = useState<CoworkMember[]>([]);

  // Modal states
  const [wsModalOpen, setWsModalOpen] = useState(false);
  const [taskModalOpen, setTaskModalOpen] = useState(false);
  const [editingTask, setEditingTask] = useState<CoworkTask | null>(null);
  const [memberModalOpen, setMemberModalOpen] = useState(false);
  const [editingMember, setEditingMember] = useState<CoworkMember | null>(null);
  const [msgText, setMsgText] = useState('');
  const [wsForm] = Form.useForm();
  const [taskForm] = Form.useForm();
  const [memberForm] = Form.useForm();

  // API helpers
  const api = useCallback(async (path: string, opts?: RequestInit) => {
    const resp = await fetch(path, opts);
    if (!resp.ok) throw new Error(await resp.text());
    return resp.json();
  }, []);

  const loadWorkspaces = useCallback(async () => {
    try {
      const data = await api('/api/cowork/workspaces');
      setWorkspaces(data.workspaces || []);
    } catch { /* ignore */ }
  }, [api]);

  const loadTemplates = useCallback(async () => {
    try {
      const data = await api('/api/cowork/templates');
      setTemplates(data.templates || []);
    } catch { /* ignore */ }
  }, [api]);

  const loadTasks = useCallback(async (wsId: string) => {
    try {
      const data = await api(`/api/cowork/workspaces/${wsId}/tasks`);
      setTasks(data.tasks || []);
    } catch { /* ignore */ }
  }, [api]);

  const loadBoard = useCallback(async (wsId: string) => {
    try {
      const data = await api(`/api/cowork/workspaces/${wsId}/board`);
      setBoard(data.entries || []);
    } catch { /* ignore */ }
  }, [api]);

  const loadMessages = useCallback(async (wsId: string) => {
    try {
      const data = await api(`/api/cowork/workspaces/${wsId}/messages?limit=100`);
      setMessages((data.messages || []).reverse());
    } catch { /* ignore */ }
  }, [api]);

  const loadFiles = useCallback(async (wsId: string) => {
    try {
      const data = await api(`/api/cowork/workspaces/${wsId}/files`);
      setWsFiles(data.files || []);
    } catch { /* ignore */ }
  }, [api]);

  const loadFileContent = useCallback(async (wsId: string, filePath: string) => {
    setFileLoading(true);
    try {
      const data = await api(`/api/cowork/workspaces/${wsId}/files?path=${encodeURIComponent(filePath)}`);
      setSelectedFilePath(filePath);
      setFileContent(data.content || '');
    } catch { /* ignore */ }
    finally { setFileLoading(false); }
  }, [api]);

  const loadMembers = useCallback(async (wsId: string) => {
    try {
      const data = await api(`/api/cowork/workspaces/${wsId}/members`);
      setMembers(data.members || []);
    } catch { /* ignore */ }
  }, [api]);

  useEffect(() => { loadWorkspaces(); loadTemplates(); }, [loadWorkspaces, loadTemplates]);

  const selectWorkspace = useCallback((ws: CoworkWorkspace) => {
    setSelectedWs(ws);
    setLoading(true);
    setSelectedFilePath(null);
    setFileContent(null);
    Promise.all([
      loadTasks(ws.id), loadBoard(ws.id), loadMessages(ws.id), loadFiles(ws.id), loadMembers(ws.id)
    ]).finally(() => setLoading(false));
  }, [loadTasks, loadBoard, loadMessages, loadFiles, loadMembers]);

  // Create workspace
  const [uploadFiles, setUploadFiles] = useState<File[]>([]);
  const [creating, setCreating] = useState(false);

  const handleCreateWorkspace = async (values: any) => {
    try {
      setCreating(true);
      const resp = await api('/api/cowork/workspaces', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: values.name, description: values.description, workingDir: values.workingDir || null, template: values.template || null }),
      });

      // If files selected, upload to the new workspace
      if (uploadFiles.length > 0) {
        const formData = new FormData();
        uploadFiles.forEach(f => formData.append('file', f));
        await fetch(`/api/cowork/workspaces/${resp.id}/documents`, {
          method: 'POST',
          body: formData,
        });
      }

      setWsModalOpen(false);
      wsForm.resetFields();
      setUploadFiles([]);
      await loadWorkspaces();
      message.success(uploadFiles.length > 0
        ? 'Workspace created with documents'
        : 'Workspace created');
    } catch (e: any) { message.error(e.message); } finally { setCreating(false); }
  };

  // Delete workspace
  const handleDeleteWorkspace = async (id: string) => {
    try {
      await api(`/api/cowork/workspaces/${id}`, { method: 'DELETE' });
      if (selectedWs?.id === id) setSelectedWs(null);
      await loadWorkspaces();
      message.success('Workspace deleted');
    } catch (e: any) { message.error(e.message); }
  };

  // Create task
  const handleCreateTask = async (values: any) => {
    if (!selectedWs) return;
    try {
      await api(`/api/cowork/workspaces/${selectedWs.id}/tasks`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(values),
      });
      setTaskModalOpen(false);
      taskForm.resetFields();
      await loadTasks(selectedWs.id);
      message.success('Task created');
    } catch (e: any) { message.error(e.message); }
  };

  // Update task status
  const handleUpdateTaskStatus = async (task: CoworkTask, newStatus: string) => {
    try {
      await api(`/api/cowork/workspaces/${task.workspaceId}/tasks/${task.id}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ status: newStatus }),
      });
      await loadTasks(task.workspaceId);
    } catch (e: any) { message.error(e.message); }
  };

  // Delete task
  const handleDeleteTask = async (task: CoworkTask) => {
    try {
      await api(`/api/cowork/workspaces/${task.workspaceId}/tasks/${task.id}`, { method: 'DELETE' });
      await loadTasks(task.workspaceId);
    } catch (e: any) { message.error(e.message); }
  };

  // Update board
  const handleUpdateBoard = async (section: string, content: string) => {
    if (!selectedWs) return;
    try {
      await api(`/api/cowork/workspaces/${selectedWs.id}/board/${section}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content, author: 'user' }),
      });
      await loadBoard(selectedWs.id);
    } catch (e: any) { message.error(e.message); }
  };

  // Send message
  const handleSendMessage = async () => {
    if (!selectedWs || !msgText.trim()) return;
    try {
      await fetch('/api/cowork/workspaces/' + selectedWs.id + '/messages', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          from_member: 'user',
          content: msgText,
          message_type: 'status',
        }),
      });
      setMsgText('');
      await Promise.all([loadMessages(selectedWs.id), loadTasks(selectedWs.id)]);
    } catch { /* ignore */ }
  };

  // Add member
  const handleAddMember = async (values: any) => {
    if (!selectedWs) return;
    try {
      await api(`/api/cowork/workspaces/${selectedWs.id}/members`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(values),
      });
      setMemberModalOpen(false);
      memberForm.resetFields();
      await loadMembers(selectedWs.id);
      message.success('Member added');
    } catch (e: any) { message.error(e.message); }
  };

  // Edit member
  const handleEditMember = async (values: any) => {
    if (!selectedWs || !editingMember) return;
    try {
      const payload: any = {};
      if (values.role) payload.role = values.role;
      if (values.persona) payload.persona = values.persona;

      // Responsibilities: newline-separated → JSON array string
      if (values.responsibilities) {
        const lines = values.responsibilities.split('\n').map((l: string) => l.trim()).filter(Boolean);
        payload.responsibilities = JSON.stringify(lines);
      }
      // Acceptance criteria: newline-separated → JSON array string
      if (values.acceptanceCriteria) {
        const lines = values.acceptanceCriteria.split('\n').map((l: string) => l.trim()).filter(Boolean);
        payload.acceptanceCriteria = JSON.stringify(lines);
      }
      // Triggers: JSON string as-is
      if (values.triggers) payload.triggers = values.triggers;
      // Handoff rules: JSON string as-is
      if (values.handoffRules) payload.handoffRules = values.handoffRules;

      // Output: structured → JSON object string
      const outputParts: string[] = [];
      if (values.outputFormat) outputParts.push(`"format":"${values.outputFormat}"`);
      if (values.outputAttachDiff && values.outputAttachDiff !== '') outputParts.push(`"attachDiff":${values.outputAttachDiff === 'true'}`);
      if (values.outputRequiredSections) {
        const secs = values.outputRequiredSections.split('\n').map((l: string) => l.trim()).filter(Boolean);
        outputParts.push(`"requiredSections":${JSON.stringify(secs)}`);
      }
      if (outputParts.length > 0) payload.outputFormat = `{${outputParts.join(',')}}`;

      // SLA: structured → JSON object string
      const slaParts: string[] = [];
      if (values.slaMaxDuration) slaParts.push(`"maxDurationPerTaskMinutes":${values.slaMaxDuration}`);
      if (values.slaMaxTokens) slaParts.push(`"maxTokenPerTask":${values.slaMaxTokens}`);
      if (values.slaEscalateAfter) slaParts.push(`"escalateAfterBlockedMinutes":${values.slaEscalateAfter}`);
      if (slaParts.length > 0) payload.sla = `{${slaParts.join(',')}}`;

      // Limits: structured → JSON object string
      const limitParts: string[] = [];
      if (values.limitsMaxFileSize) limitParts.push(`"maxFileSizeWriteKb":${values.limitsMaxFileSize}`);
      if (values.limitsAllowedBash) {
        const cmds = values.limitsAllowedBash.split('\n').map((l: string) => l.trim()).filter(Boolean);
        limitParts.push(`"allowedBashCommands":${JSON.stringify(cmds)}`);
      }
      if (values.limitsDeniedTools) {
        const tools = values.limitsDeniedTools.split('\n').map((l: string) => l.trim()).filter(Boolean);
        limitParts.push(`"deniedTools":${JSON.stringify(tools)}`);
      }
      if (limitParts.length > 0) payload.limits = `{${limitParts.join(',')}}`;

      await api(`/api/cowork/workspaces/${selectedWs.id}/members/${editingMember.memberId}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      setMemberModalOpen(false);
      setEditingMember(null);
      memberForm.resetFields();
      await loadMembers(selectedWs.id);
      message.success('Member updated');
    } catch (e: any) { message.error(e.message); }
  };

  // Remove member
  const handleRemoveMember = async (memberId: string) => {
    if (!selectedWs) return;
    try {
      await api(`/api/cowork/workspaces/${selectedWs.id}/members/${memberId}`, { method: 'DELETE' });
      await loadMembers(selectedWs.id);
      message.success('Member removed');
    } catch (e: any) { message.error(e.message); }
  };

  const tasksByStatus = (status: string) => tasks.filter(t => t.status === status);

  // Stats
  const activeWSCount = workspaces.filter(w => w.status === 'active').length;
  const totalTasks = tasks.length;
  const doneTasks = tasks.filter(t => t.status === 'done').length;
  const taskProgress = totalTasks > 0 ? Math.round((doneTasks / totalTasks) * 100) : 0;

  // ---- Dashboard (home) ----
  if (!selectedWs) {
    return (
      <AppLayout sidebar={
        <CoworkSidebar
          workspaces={workspaces}
          selectedWs={null}
          onSelectWorkspace={selectWorkspace}
          onCreateWorkspace={() => setWsModalOpen(true)}
          onDeleteWorkspace={handleDeleteWorkspace}
          onRefresh={loadWorkspaces}
          loading={loading}
        />
      }>
        <Layout style={{ background: 'transparent', height: '100%', display: 'flex', flexDirection: 'column' }}>
          <header style={{ padding: '0 24px', height: 56, display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid #f0f0f0', background: '#fff', flexShrink: 0 }}>
            <Breadcrumb items={[
              { title: <Space onClick={() => navigate('/chats')} style={{ cursor: 'pointer' }}><HomeOutlined /><span>Home</span></Space> },
              { title: <Text strong>Cowork Space</Text> }
            ]} />
            <Button type="primary" icon={<PlusOutlined />} onClick={() => setWsModalOpen(true)}>
              New Workspace
            </Button>
          </header>
          <Content style={{ flex: 1, overflowY: 'auto', padding: 24 }}>
            <div style={{ maxWidth: 900, margin: '0 auto' }}>
              {/* Dashboard header */}
              <Flex justify="space-between" align="center" style={{ marginBottom: 24 }}>
                <div>
                  <Title level={3} style={{ margin: 0 }}>Cowork Dashboard</Title>
                  <Text type="secondary">Multi-agent collaborative workspaces</Text>
                </div>
              </Flex>

              {/* Stats row */}
              <Row gutter={16} style={{ marginBottom: 24 }}>
                <Col span={6}>
                  <Card size="small" style={{ borderRadius: 10 }}>
                    <Statistic title="Workspaces" value={workspaces.length} suffix={<Text type="secondary" style={{ fontSize: 12 }}>/ {activeWSCount} active</Text>} />
                  </Card>
                </Col>
                <Col span={6}>
                  <Card size="small" style={{ borderRadius: 10 }}>
                    <Statistic title="Total Tasks" value={totalTasks} />
                  </Card>
                </Col>
                <Col span={6}>
                  <Card size="small" style={{ borderRadius: 10 }}>
                    <Statistic title="Done" value={doneTasks} suffix={totalTasks > 0 ? <Progress percent={taskProgress} size="small" style={{ width: 80 }} /> : undefined} />
                  </Card>
                </Col>
                <Col span={6}>
                  <Card size="small" style={{ borderRadius: 10 }}>
                    <Statistic title="Templates" value={templates.length} suffix={<Text type="secondary" style={{ fontSize: 12 }}>available</Text>} />
                  </Card>
                </Col>
              </Row>

              {workspaces.length === 0 ? (
                <Card style={{ textAlign: 'center', padding: 48, borderRadius: 12 }}>
                  <ProjectOutlined style={{ fontSize: 48, color: '#d9d9d9', marginBottom: 16 }} />
                  <Title level={4} type="secondary">No workspaces yet</Title>
                  <Text type="secondary">Create a shared workspace for multi-agent collaboration</Text>
                  <br />
                  <Button type="primary" icon={<PlusOutlined />} onClick={() => setWsModalOpen(true)} style={{ marginTop: 16 }}>
                    Create Workspace
                  </Button>
                </Card>
              ) : (
                <>
                  <Flex justify="space-between" align="center" style={{ marginBottom: 12 }}>
                    <Title level={5} style={{ margin: 0 }}>Recent Workspaces</Title>
                  </Flex>
                  <List
                    grid={{ gutter: 16, column: 2 }}
                    dataSource={workspaces}
                    renderItem={ws => (
                      <List.Item>
                        <Card
                          hoverable
                          style={{ borderRadius: 12 }}
                          onClick={() => selectWorkspace(ws)}
                        >
                          <Flex justify="space-between" align="center">
                            <Space>
                              <Avatar
                                icon={<ProjectOutlined />}
                                style={{ backgroundColor: ws.status === 'active' ? '#52c41a' : '#d9d9d9' }}
                              />
                              <div>
                                <Text strong style={{ fontSize: 15 }}>{ws.name}</Text>
                                <br />
                                <Text type="secondary" style={{ fontSize: 12 }}>
                                  {ws.description || 'No description'}
                                </Text>
                              </div>
                            </Space>
                            <Space>
                              <Tag color={ws.status === 'active' ? 'green' : 'default'}>{ws.status}</Tag>
                              <Popconfirm
                                title="Delete this workspace?"
                                onConfirm={(e) => { e?.stopPropagation(); handleDeleteWorkspace(ws.id); }}
                                onCancel={e => e?.stopPropagation()}
                              >
                                <Button type="text" danger icon={<DeleteOutlined />} onClick={e => e.stopPropagation()} />
                              </Popconfirm>
                            </Space>
                          </Flex>
                          <Divider style={{ margin: '8px 0' }} />
                          <Flex gap={16} vertical style={{ marginTop: 4 }}>
                            <Text type="secondary" style={{ fontSize: 11 }}>Created: {new Date(ws.createdAt).toLocaleDateString()}</Text>
                            {ws.workingDir && <Text type="secondary" style={{ fontSize: 11 }}>Project: {ws.workingDir}</Text>}
                          </Flex>
                        </Card>
                      </List.Item>
                    )}
                  />
                </>
              )}

              {/* Templates quick reference */}
              {templates.length > 0 && workspaces.length > 0 && (
                <>
                  <Title level={5} style={{ marginTop: 24, marginBottom: 12 }}>Available Templates</Title>
                  <Row gutter={12}>
                    {templates.slice(0, 4).map(t => (
                      <Col span={6} key={t.name}>
                        <Card
                          size="small"
                          hoverable
                          style={{ borderRadius: 10, textAlign: 'center' }}
                          onClick={() => { setWsModalOpen(true); wsForm.setFieldsValue({ template: t.name }); }}
                        >
                          <Text strong style={{ fontSize: 13 }}>{t.name}</Text>
                          <br />
                          <Text type="secondary" style={{ fontSize: 11 }}>{t.members.length} members</Text>
                        </Card>
                      </Col>
                    ))}
                  </Row>
                </>
              )}
            </div>
          </Content>

          {/* Create Workspace Modal */}
          <Modal title="Create Workspace" open={wsModalOpen} onCancel={() => { setWsModalOpen(false); setUploadFiles([]); }} footer={null} destroyOnClose width={580}>
            <Form form={wsForm} layout="vertical" onFinish={handleCreateWorkspace} style={{ marginTop: 16 }}>
              <Form.Item name="name" label="Name" rules={[{ required: true }]}>
                <Input placeholder="project-alpha" />
              </Form.Item>
              <Form.Item name="description" label="Description">
                <TextArea rows={3} placeholder="What is this workspace for?" />
              </Form.Item>
              <Form.Item name="workingDir" label="Working Directory (project folder)">
                <Input placeholder="/path/to/project — agent work dir" />
              </Form.Item>
              <Form.Item name="template" label="Template (optional)">
                <Select
                  allowClear
                  placeholder="No template — start from scratch"
                  options={templates.map(t => ({ value: t.name, label: t.name }))}
                  optionRender={opt => (
                    <Flex vertical gap={2}>
                      <Text strong>{opt.label}</Text>
                      <Text type="secondary" style={{ fontSize: 11 }}>{templates.find(t => t.name === opt.value)?.description}</Text>
                    </Flex>
                  )}
                />
              </Form.Item>
              <Form.Item label="Reference Documents">
                <Upload.Dragger
                  multiple
                  beforeUpload={file => { setUploadFiles(prev => [...prev, file]); return false; }}
                  onRemove={file => { setUploadFiles(prev => prev.filter(f => f.name !== file.name && f.size !== file.size)); }}
                  fileList={uploadFiles.map(f => ({ uid: `${f.name}-${f.size}`, name: f.name, status: 'done' as const }))}
                  accept=".md,.txt,.pdf,.json,.yaml,.yml,.toml,.csv,.ts,.js,.py,.rs,.html,.css"
                >
                  <p className="ant-upload-drag-icon">
                    <InboxOutlined />
                  </p>
                  <p className="ant-upload-text" style={{ fontSize: 13 }}>Drop files here or click to browse</p>
                  <p className="ant-upload-hint" style={{ fontSize: 11 }}>
                    Upload workspace reference docs (.md, .pdf, .txt, etc.)
                  </p>
                </Upload.Dragger>
              </Form.Item>
              <Form.Item style={{ textAlign: 'right', marginBottom: 0 }}>
                <Space>
                  <Button onClick={() => { setWsModalOpen(false); setUploadFiles([]); }}>Cancel</Button>
                  <Button type="primary" htmlType="submit" loading={creating}>Create</Button>
                </Space>
              </Form.Item>
            </Form>
          </Modal>
        </Layout>
      </AppLayout>
    );
  }

  // ---- Workspace view ----
  return (
    <AppLayout sidebar={
      <CoworkSidebar
        workspaces={workspaces}
        selectedWs={selectedWs}
        onSelectWorkspace={selectWorkspace}
        onCreateWorkspace={() => setWsModalOpen(true)}
        onDeleteWorkspace={handleDeleteWorkspace}
        onRefresh={loadWorkspaces}
        loading={loading}
      />
    }>
      <Layout style={{ background: 'transparent', height: '100%', display: 'flex', flexDirection: 'column' }}>
        {/* Header */}
        <header style={{ padding: '0 24px', height: 56, display: 'flex', alignItems: 'center', justifyContent: 'space-between', borderBottom: '1px solid #f0f0f0', background: '#fff', flexShrink: 0 }}>
          <Flex align="center" gap={12}>
            <Breadcrumb items={[
              { title: <Space onClick={() => navigate('/chats')} style={{ cursor: 'pointer' }}><HomeOutlined /><span>Home</span></Space> },
              { title: <Space onClick={() => { setSelectedWs(null); setTasks([]); setBoard([]); }} style={{ cursor: 'pointer' }}>Cowork</Space> },
              { title: <Text strong>{selectedWs.name}</Text> }
            ]} />
            <Tag color={selectedWs.status === 'active' ? 'green' : 'default'}>{selectedWs.status}</Tag>
          </Flex>
          <Space>
            <Button size="small" icon={<ReloadOutlined />} onClick={() => selectWorkspace(selectedWs)}>Refresh</Button>
            <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => { setEditingTask(null); taskForm.resetFields(); setTaskModalOpen(true); }}>New Task</Button>
            <Popconfirm
              title="Delete this workspace? All data will be lost."
              onConfirm={() => { handleDeleteWorkspace(selectedWs.id); setSelectedWs(null); }}
            >
              <Button size="small" danger icon={<DeleteOutlined />}>Delete</Button>
            </Popconfirm>
          </Space>
        </header>

        <Content style={{ flex: 1, overflow: 'hidden', display: 'flex' }}>
          {/* Main content area */}
          <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
            <Tabs
              activeKey={tab}
              onChange={setTab}
              style={{ padding: '0 16px', margin: 0 }}
              tabBarStyle={{ marginBottom: 0 }}
              items={[
                {
                  key: 'tasks',
                  label: <Space><ProjectOutlined />Tasks ({tasks.length})</Space>,
                  children: (
                    <div style={{ flex: 1, overflowX: 'auto', overflowY: 'auto', padding: '12px 4px', height: 'calc(100vh - 180px)' }}>
                      {loading ? <Spin style={{ display: 'block', marginTop: 64 }} /> : (
                        <Flex gap={12} style={{ minWidth: 900 }}>
                          {KANBAN_COLUMNS.map(col => (
                            <div key={col} style={{ flex: 1, minWidth: 160, background: '#f5f5f5', borderRadius: 8, padding: 8 }}>
                              <Flex justify="space-between" align="center" style={{ marginBottom: 8 }}>
                                <Tag color={STATUS_COLORS[col]}>{STATUS_LABELS[col]}</Tag>
                                <Text type="secondary" style={{ fontSize: 12 }}>{tasksByStatus(col).length}</Text>
                              </Flex>
                              {tasksByStatus(col).map(task => (
                                <Card
                                  key={task.id}
                                  size="small"
                                  style={{ marginBottom: 8, borderRadius: 8, cursor: 'pointer' }}
                                  onClick={() => {
                                    setEditingTask(task);
                                    // Parse attachments JSON string to array for Select
                                    let attachments: string[] = [];
                                    if (task.attachments) {
                                      try { attachments = JSON.parse(task.attachments); } catch { /* ignore */ }
                                    }
                                    taskForm.setFieldsValue({ ...task, attachments });
                                    setTaskModalOpen(true);
                                  }}
                                >
                                  <Text style={{ fontSize: 13 }}>{task.title}</Text>
                                  <Flex justify="space-between" align="center" style={{ marginTop: 4 }}>
                                    <Space size={4}>
                                      {task.priority !== 'medium' && (
                                        <Tag style={{ fontSize: 10, lineHeight: '16px' }} color={PRIORITY_COLORS[task.priority]}>{task.priority}</Tag>
                                      )}
                                      {task.assignee && (
                                        <Tag style={{ fontSize: 10, lineHeight: '16px', padding: '0 4px' }}>{task.assignee}</Tag>
                                      )}
                                    </Space>
                                    <Dropdown menu={{ items: KANBAN_COLUMNS.filter(c => c !== task.status).map(c => ({ key: c, label: STATUS_LABELS[c], onClick: () => handleUpdateTaskStatus(task, c) })) }}>
                                      <Button type="text" size="small" icon={<MoreOutlined />} onClick={e => e.stopPropagation()} />
                                    </Dropdown>
                                  </Flex>
                                </Card>
                              ))}
                            </div>
                          ))}
                        </Flex>
                      )}
                    </div>
                  ),
                },
                {
                  key: 'board',
                  label: <Space><AppstoreOutlined />Board</Space>,
                  children: (
                    <div style={{ overflowY: 'auto', padding: 16, height: 'calc(100vh - 180px)' }}>
                      {loading ? <Spin style={{ display: 'block', marginTop: 64 }} /> : (
                        <>
                          {BOARD_SECTIONS.map(section => {
                            const entry = board.find(e => e.section === section);
                            return (
                              <Card
                                key={section}
                                size="small"
                                title={<Text strong style={{ textTransform: 'capitalize' }}>{section}</Text>}
                                style={{ marginBottom: 12, borderRadius: 8 }}
                                extra={
                                  <Button type="link" size="small" onClick={() => {
                                    const c = entry?.content || '';
                                    const newContent = prompt(`Edit ${section}:`, c);
                                    if (newContent !== null && newContent !== c) handleUpdateBoard(section, newContent);
                                  }}>Edit</Button>
                                }
                              >
                                <Text style={{ whiteSpace: 'pre-wrap', fontSize: 13 }}>
                                  {entry?.content || <Text type="secondary" italic>Not set yet</Text>}
                                </Text>
                                {entry && <Text type="secondary" style={{ fontSize: 11, display: 'block', marginTop: 8 }}>Updated by {entry.author} at {new Date(entry.updatedAt).toLocaleString()}</Text>}
                              </Card>
                            );
                          })}
                        </>
                      )}
                    </div>
                  ),
                },
                {
                  key: 'messages',
                  label: <Space><MessageOutlined />Messages</Space>,
                  children: (
                    <div style={{ display: 'flex', flexDirection: 'column', height: 'calc(100vh - 180px)' }}>
                      <div style={{ flex: 1, overflowY: 'auto', padding: 16 }}>
                        {loading ? <Spin style={{ display: 'block', marginTop: 64 }} /> : (
                          messages.length === 0 ? <Empty description="No messages yet" /> : (
                            messages.map(msg => (
                              <div key={msg.id} style={{ marginBottom: 12, padding: '8px 12px', borderRadius: 8, background: msg.fromMember === 'user' ? '#e6f7ff' : '#f6ffed', border: '1px solid #f0f0f0' }}>
                                <Flex justify="space-between" align="center">
                                  <Space size={4}>
                                    <Text strong style={{ fontSize: 12 }}>{msg.fromMember}</Text>
                                    {msg.toMember && <><ArrowRightOutlined style={{ fontSize: 10 }} /><Text style={{ fontSize: 12 }}>{msg.toMember}</Text></>}
                                    <Tag style={{ fontSize: 10, lineHeight: '16px' }}>{msg.messageType}</Tag>
                                  </Space>
                                  <Text type="secondary" style={{ fontSize: 11 }}>{new Date(msg.createdAt).toLocaleTimeString()}</Text>
                                </Flex>
                                <Text style={{ fontSize: 13, marginTop: 4, display: 'block' }}>{msg.content}</Text>
                                {msg.taskId && <Text type="secondary" style={{ fontSize: 11 }}>Task: {msg.taskId}</Text>}
                              </div>
                            ))
                          )
                        )}
                      </div>
                      <div style={{ padding: '8px 16px', borderTop: '1px solid #f0f0f0', display: 'flex', gap: 8 }}>
                        <Input
                          placeholder="Type a status update..."
                          value={msgText}
                          onChange={e => setMsgText(e.target.value)}
                          onPressEnter={handleSendMessage}
                        />
                        <Button icon={<SendOutlined />} onClick={handleSendMessage} disabled={!msgText.trim()}>Send</Button>
                      </div>
                    </div>
                  ),
                },
                {
                  key: 'agents',
                  label: <Space><RobotOutlined />Agents ({members.length})</Space>,
                  children: (
                    <div style={{ overflowY: 'auto', padding: 16, height: 'calc(100vh - 180px)' }}>
                      <Flex justify="space-between" align="center" style={{ marginBottom: 16 }}>
                        <Text type="secondary" style={{ fontSize: 13 }}>{members.length} agent{members.length !== 1 ? 's' : ''} in workspace</Text>
                        <Button
                          type="primary"
                          size="small"
                          icon={<PlusOutlined />}
                          onClick={() => { setEditingMember(null); memberForm.resetFields(); setMemberModalOpen(true); }}
                        >
                          Add Agent
                        </Button>
                      </Flex>
                      {loading ? <Spin style={{ display: 'block', marginTop: 64 }} /> : (
                        members.length === 0 ? <Empty description="No agent members" /> : (
                          <List
                            grid={{ gutter: 16, column: 2 }}
                            dataSource={members}
                            renderItem={(m: CoworkMember) => (
                              <List.Item>
                                <Card
                                  size="small"
                                  title={
                                    <Space>
                                      <Avatar icon={<RobotOutlined />} size={28} style={{ backgroundColor: '#1890ff' }} />
                                      <span>{m.memberId}</span>
                                      {m.subdir && <Tag style={{ fontSize: 10, fontFamily: 'monospace' }}>{m.subdir}/</Tag>}
                                      <Tag color={m.role === 'reviewer' ? 'purple' : 'blue'} style={{ fontSize: 10 }}>{m.role}</Tag>
                                    </Space>
                                  }
                                  extra={
                                    <Space size={0}>
                                      <Button
                                        type="text"
                                        size="small"
                                        icon={<EditOutlined style={{ fontSize: 11 }} />}
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          setEditingMember(m);
                                          const vals: any = { role: m.role, subdir: m.subdir || '' };
                                          if (m.persona) vals.persona = m.persona;
                                          // Parse array fields → newline-separated
                                          if (m.responsibilities) {
                                            try { vals.responsibilities = (JSON.parse(m.responsibilities) as string[]).join('\n'); } catch { vals.responsibilities = m.responsibilities; }
                                          }
                                          if (m.acceptanceCriteria) {
                                            try { vals.acceptanceCriteria = (JSON.parse(m.acceptanceCriteria) as string[]).join('\n'); } catch { vals.acceptanceCriteria = m.acceptanceCriteria; }
                                          }
                                          // Triggers & handoff → pretty-print JSON for editing
                                          if (m.triggers) {
                                            try { vals.triggers = JSON.stringify(JSON.parse(m.triggers), null, 2); } catch { vals.triggers = m.triggers; }
                                          }
                                          if (m.handoffRules) {
                                            try { vals.handoffRules = JSON.stringify(JSON.parse(m.handoffRules), null, 2); } catch { vals.handoffRules = m.handoffRules; }
                                          }
                                          // Output → structured fields
                                          if (m.outputFormat) {
                                            try {
                                              const o = JSON.parse(m.outputFormat);
                                              if (o.format) vals.outputFormat = o.format;
                                              if (o.requiredSections) vals.outputRequiredSections = (o.requiredSections as string[]).join('\n');
                                              if (o.attachDiff !== undefined) vals.outputAttachDiff = o.attachDiff ? 'true' : 'false';
                                            } catch { vals.outputFormat = m.outputFormat; }
                                          }
                                          // SLA → structured fields
                                          if (m.sla) {
                                            try {
                                              const s = JSON.parse(m.sla);
                                              if (s.maxDurationPerTaskMinutes) vals.slaMaxDuration = s.maxDurationPerTaskMinutes;
                                              if (s.maxTokenPerTask) vals.slaMaxTokens = s.maxTokenPerTask;
                                              if (s.escalateAfterBlockedMinutes) vals.slaEscalateAfter = s.escalateAfterBlockedMinutes;
                                            } catch { /* ignore */ }
                                          }
                                          // Limits → structured fields
                                          if (m.limits) {
                                            try {
                                              const l = JSON.parse(m.limits);
                                              if (l.maxFileSizeWriteKb) vals.limitsMaxFileSize = l.maxFileSizeWriteKb;
                                              if (l.allowedBashCommands) vals.limitsAllowedBash = (l.allowedBashCommands as string[]).join('\n');
                                              if (l.deniedTools) vals.limitsDeniedTools = (l.deniedTools as string[]).join('\n');
                                            } catch { /* ignore */ }
                                          }
                                          memberForm.setFieldsValue(vals);
                                          setMemberModalOpen(true);
                                        }}
                                      />
                                      <Popconfirm
                                        title="Remove this agent from workspace?"
                                        onConfirm={(e) => { e?.stopPropagation(); handleRemoveMember(m.memberId); }}
                                        onCancel={e => e?.stopPropagation()}
                                      >
                                        <Button type="text" size="small" danger icon={<DeleteOutlined style={{ fontSize: 11 }} />} onClick={e => e.stopPropagation()} />
                                      </Popconfirm>
                                    </Space>
                                  }
                                  style={{ borderRadius: 10, height: '100%' }}
                                >
                                  {m.persona && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Persona</Text>
                                      <br />
                                      <Text style={{ fontSize: 12, fontStyle: 'italic' }}>{m.persona}</Text>
                                    </div>
                                  )}
                                  {m.responsibilities && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Responsibilities</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => { try { return JSON.parse(m.responsibilities); } catch { return [m.responsibilities]; } })().map((r: string, i: number) => (
                                          <Tag key={i} style={{ fontSize: 11, marginBottom: 2 }}>{r}</Tag>
                                        ))}
                                      </div>
                                    </div>
                                  )}
                                  {m.triggers && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Triggers</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => { try { return JSON.parse(m.triggers); } catch { return []; } })().map((t: any, i: number) => (
                                          <Tag key={i} style={{ fontSize: 11, marginBottom: 2 }} color="purple">{t.type}{t.condition ? `: ${t.condition}` : ''}{t.from ? ` from ${t.from}` : ''}</Tag>
                                        ))}
                                      </div>
                                    </div>
                                  )}
                                  {m.handoffRules && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Handoff Rules</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => { try { return JSON.parse(m.handoffRules); } catch { return []; } })().map((h: any, i: number) => (
                                          <div key={i} style={{ fontSize: 11, marginBottom: 2 }}>
                                            <Tag color="orange">{h.when}</Tag>
                                            <ArrowRightOutlined style={{ fontSize: 10, margin: '0 2px' }} />
                                            <Text style={{ fontSize: 11 }}>{h.to} ({h.type})</Text>
                                          </div>
                                        ))}
                                      </div>
                                    </div>
                                  )}
                                  {m.acceptanceCriteria && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Acceptance Criteria</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => { try { return JSON.parse(m.acceptanceCriteria); } catch { return [m.acceptanceCriteria]; } })().map((c: string, i: number) => (
                                          <div key={i} style={{ fontSize: 11, marginBottom: 1 }}>
                                            <CheckCircleOutlined style={{ color: '#52c41a', marginRight: 4, fontSize: 10 }} />{c}
                                          </div>
                                        ))}
                                      </div>
                                    </div>
                                  )}
                                  {m.outputFormat && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Output</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => {
                                          try {
                                            const o = JSON.parse(m.outputFormat);
                                            return (
                                              <Space size={4} wrap>
                                                {o.format && <Tag color="geekblue" style={{ fontSize: 10 }}>{o.format}</Tag>}
                                                {o.attachDiff && <Tag color="green" style={{ fontSize: 10 }}>+diff</Tag>}
                                                {o.requiredSections && (o.requiredSections as string[]).map((s: string, i: number) => (
                                                  <Tag key={i} style={{ fontSize: 10 }}>{s}</Tag>
                                                ))}
                                              </Space>
                                            );
                                          } catch { return <Text style={{ fontSize: 11 }}>{m.outputFormat}</Text>; }
                                        })()}
                                      </div>
                                    </div>
                                  )}
                                  {m.sla && (
                                    <div style={{ marginBottom: 8 }}>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>SLA</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => {
                                          try {
                                            const s = JSON.parse(m.sla);
                                            return (
                                              <Space size={4} wrap>
                                                {s.maxDurationPerTaskMinutes && <Tag color="orange" style={{ fontSize: 10 }}>{s.maxDurationPerTaskMinutes}min</Tag>}
                                                {s.maxTokenPerTask && <Tag color="orange" style={{ fontSize: 10 }}>{s.maxTokenPerTask} tokens</Tag>}
                                                {s.escalateAfterBlockedMinutes && <Tag color="red" style={{ fontSize: 10 }}>escalate {s.escalateAfterBlockedMinutes}min</Tag>}
                                              </Space>
                                            );
                                          } catch { return <Text style={{ fontSize: 11 }}>{m.sla}</Text>; }
                                        })()}
                                      </div>
                                    </div>
                                  )}
                                  {m.limits && (
                                    <div>
                                      <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: 1 }}>Limits</Text>
                                      <div style={{ marginTop: 2 }}>
                                        {(() => {
                                          try {
                                            const l = JSON.parse(m.limits);
                                            return (
                                              <Space size={4} wrap>
                                                {l.maxFileSizeWriteKb && <Tag color="gold" style={{ fontSize: 10 }}>max {l.maxFileSizeWriteKb}KB write</Tag>}
                                                {l.allowedBashCommands && (l.allowedBashCommands as string[]).map((c: string, i: number) => (
                                                  <Tag key={`a${i}`} color="green" style={{ fontSize: 10 }}>{c}</Tag>
                                                ))}
                                                {l.deniedTools && (l.deniedTools as string[]).map((t: string, i: number) => (
                                                  <Tag key={`d${i}`} color="red" style={{ fontSize: 10 }}>no {t}</Tag>
                                                ))}
                                              </Space>
                                            );
                                          } catch { return <Text style={{ fontSize: 11 }}>{m.limits}</Text>; }
                                        })()}
                                      </div>
                                    </div>
                                  )}
                                </Card>
                              </List.Item>
                            )}
                          />
                        )
                      )}
                    </div>
                  ),
                },
                {
                  key: 'files',
                  label: <Space><FileOutlined />Files ({wsFiles.length})</Space>,
                  children: (
                    <div style={{ display: 'flex', height: 'calc(100vh - 180px)' }}>
                      {/* File tree */}
                      <div style={{ width: 260, borderRight: '1px solid #f0f0f0', overflowY: 'auto', padding: 8, flexShrink: 0, display: 'flex', flexDirection: 'column' }}>
                        {/* Upload button */}
                        <Upload
                          multiple
                          showUploadList={false}
                          beforeUpload={file => {
                            const fd = new FormData();
                            fd.append('file', file);
                            fetch(`/api/cowork/workspaces/${selectedWs!.id}/documents`, { method: 'POST', body: fd })
                              .then(() => { loadFiles(selectedWs!.id); message.success(`Uploaded ${file.name}`); })
                              .catch(() => message.error(`Failed to upload ${file.name}`));
                            return false;
                          }}
                          accept=".md,.txt,.pdf,.json,.yaml,.yml,.toml,.csv,.ts,.js,.py,.rs,.html,.css"
                        >
                          <Button block size="small" icon={<UploadOutlined />} style={{ marginBottom: 8, fontSize: 12 }}>
                            Upload files
                          </Button>
                        </Upload>
                        {loading ? <Spin size="small" style={{ display: 'block', marginTop: 32, textAlign: 'center' }} /> : (
                          wsFiles.length === 0 ? <Empty description="No files" image={Empty.PRESENTED_IMAGE_SIMPLE} /> : (
                            <List
                              size="small"
                              dataSource={wsFiles}
                              renderItem={(f: any) => (
                                <div
                                  key={f.path}
                                  onClick={() => !f.isDir && loadFileContent(selectedWs!.id, f.path)}
                                  style={{
                                    padding: '4px 8px',
                                    cursor: f.isDir ? 'default' : 'pointer',
                                    borderRadius: 4,
                                    background: selectedFilePath === f.path ? '#e6f7ff' : 'transparent',
                                    fontSize: 12,
                                    display: 'flex',
                                    alignItems: 'center',
                                    gap: 6,
                                  }}
                                >
                                  {f.isDir ? <FolderOutlined style={{ color: '#faad14' }} /> : <FileTextOutlined style={{ color: '#1890ff' }} />}
                                  <Text style={{ fontSize: 12, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>{f.name}</Text>
                                  {!f.isDir && <Text type="secondary" style={{ fontSize: 10 }}>{formatSize(f.size)}</Text>}
                                </div>
                              )}
                            />
                          )
                        )}
                      </div>
                      {/* File content viewer */}
                      <div style={{ flex: 1, overflowY: 'auto', padding: 16 }}>
                        {fileLoading ? <Spin style={{ display: 'block', marginTop: 64 }} /> : (
                          selectedFilePath && fileContent !== null ? (
                            <div>
                              <Flex justify="space-between" align="center" style={{ marginBottom: 12 }}>
                                <Text strong>{selectedFilePath.split('/').pop()}</Text>
                                <Space>
                                  <Text type="secondary" style={{ fontSize: 11 }}>{selectedFilePath}</Text>
                                  <a
                                    href={`/api/cowork/workspaces/${selectedWs!.id}/files/download?path=${encodeURIComponent(selectedFilePath)}`}
                                    download
                                  >
                                    <Button size="small" icon={<DownloadOutlined />} type="link">Download</Button>
                                  </a>
                                </Space>
                              </Flex>
                              <Card style={{ borderRadius: 8 }}>
                                {selectedFilePath.endsWith('.md') || selectedFilePath.endsWith('.markdown') ? (
                                  <div
                                    style={{ whiteSpace: 'pre-wrap', fontFamily: 'monospace', fontSize: 13, lineHeight: 1.6 }}
                                    dangerouslySetInnerHTML={{ __html: renderMarkdown(fileContent) }}
                                  />
                                ) : (
                                  <pre style={{
                                    whiteSpace: 'pre-wrap',
                                    fontFamily: 'Menlo, Monaco, monospace',
                                    fontSize: 12,
                                    lineHeight: 1.5,
                                    margin: 0,
                                    padding: 12,
                                    background: '#f6f8fa',
                                    borderRadius: 6,
                                    maxHeight: 'calc(100vh - 320px)',
                                    overflow: 'auto',
                                  }}>
                                    {fileContent}
                                  </pre>
                                )}
                              </Card>
                            </div>
                          ) : (
                            <div style={{ textAlign: 'center', marginTop: 64, color: '#8c8c8c' }}>
                              <FileTextOutlined style={{ fontSize: 48, marginBottom: 16 }} />
                              <br />
                              <Text type="secondary">Select a file to preview</Text>
                            </div>
                          )
                        )}
                      </div>
                    </div>
                  ),
                },
              ]}
            />
          </div>
        </Content>

        {/* Task Create/Edit Modal */}
        <Modal
          title={editingTask ? `Edit: ${editingTask.title}` : 'New Task'}
          open={taskModalOpen}
          onCancel={() => { setTaskModalOpen(false); setEditingTask(null); }}
          footer={null}
          destroyOnClose
          width={560}
        >
          <Form form={taskForm} layout="vertical" onFinish={editingTask ? (values) => {
            const payload = { ...values };
            // Serialize attachments array to JSON string for update
            if (Array.isArray(values.attachments)) {
              payload.attachments = JSON.stringify(values.attachments);
            }
            api(`/api/cowork/workspaces/${selectedWs.id}/tasks/${editingTask.id}`, {
              method: 'PATCH',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify(payload),
            }).then(() => { setTaskModalOpen(false); setEditingTask(null); loadTasks(selectedWs.id); });
          } : handleCreateTask} style={{ marginTop: 16 }}>
            <Form.Item name="title" label="Title" rules={[{ required: true }]}>
              <Input placeholder="Implement auth module" />
            </Form.Item>
            <Form.Item name="description" label="Description">
              <TextArea rows={3} placeholder="Describe the task..." />
            </Form.Item>
            <Form.Item name="attachments" label={<Space><PaperClipOutlined />Attach workspace files</Space>}>
              <Select
                mode="multiple"
                allowClear
                placeholder="Select files to attach..."
                options={wsFiles.filter((f: any) => !f.isDir).map((f: any) => ({ value: f.path, label: f.name }))}
                optionRender={opt => (
                  <Flex gap={8} align="center">
                    <FileTextOutlined style={{ color: '#1890ff', fontSize: 12 }} />
                    <Text style={{ fontSize: 12 }}>{opt.label}</Text>
                  </Flex>
                )}
              />
            </Form.Item>
            <Flex gap={12}>
              <Form.Item name="assignee" label="Assignee" style={{ flex: 1 }}>
                <Select
                  allowClear
                  placeholder="Select agent..."
                  options={members.map(m => ({ value: m.memberId, label: m.memberId }))}
                  optionRender={opt => (
                    <Space>
                      <RobotOutlined style={{ fontSize: 11, color: '#1890ff' }} />
                      <span>{opt.label}</span>
                      {members.find(m => m.memberId === opt.value)?.role && (
                        <Tag style={{ fontSize: 10, lineHeight: '16px' }}>{members.find(m => m.memberId === opt.value)?.role}</Tag>
                      )}
                    </Space>
                  )}
                />
              </Form.Item>
              <Form.Item name="priority" label="Priority" style={{ flex: 1 }} initialValue="medium">
                <Select options={['low', 'medium', 'high', 'critical'].map(p => ({ value: p, label: p }))} />
              </Form.Item>
            </Flex>
            {editingTask && (
              <Form.Item name="status" label="Status">
                <Select options={KANBAN_COLUMNS.map(s => ({ value: s, label: STATUS_LABELS[s] }))} />
              </Form.Item>
            )}
            <Form.Item style={{ textAlign: 'right', marginBottom: 0 }}>
              <Space>
                {editingTask && (
                  <Popconfirm title="Delete this task?" onConfirm={() => { handleDeleteTask(editingTask); setTaskModalOpen(false); }}>
                    <Button danger>Delete</Button>
                  </Popconfirm>
                )}
                <Button onClick={() => { setTaskModalOpen(false); setEditingTask(null); }}>Cancel</Button>
                <Button type="primary" htmlType="submit">{editingTask ? 'Save' : 'Create'}</Button>
              </Space>
            </Form.Item>
          </Form>
        </Modal>

        {/* Member Add/Edit Modal */}
        <Modal
          title={editingMember ? `Edit Agent: ${editingMember.memberId}` : 'Add Agent'}
          open={memberModalOpen}
          onCancel={() => { setMemberModalOpen(false); setEditingMember(null); memberForm.resetFields(); }}
          footer={null}
          destroyOnClose
          width={640}
        >
          <Form
            form={memberForm}
            layout="vertical"
            onFinish={editingMember ? handleEditMember : handleAddMember}
            style={{ marginTop: 16 }}
          >
            {!editingMember ? (
              <>
                <Form.Item name="memberId" label="Agent Folder" rules={[{ required: true, message: 'Required' }]}>
                  <Input placeholder="e.g. code-agent, review-agent, test-agent" />
                </Form.Item>
                <Flex gap={12}>
                  <Form.Item name="role" label="Role" style={{ flex: 1 }} initialValue="worker">
                    <Select options={[
                      { value: 'worker', label: 'Worker — executes implementation' },
                      { value: 'reviewer', label: 'Reviewer — reviews and approves' },
                    ]} />
                  </Form.Item>
                  <Form.Item name="subdir" label="Subdir" style={{ flex: 1 }}>
                    <Input placeholder="e.g. impl, review, tests" />
                  </Form.Item>
                </Flex>
              </>
            ) : (
              <Tabs
                defaultActiveKey="core"
                style={{ minHeight: 360 }}
                items={[
                  {
                    key: 'core',
                    label: 'Core',
                    children: (
                      <>
                        <Flex gap={12}>
                          <Form.Item name="role" label="Role" style={{ flex: 1 }}>
                            <Select options={[
                              { value: 'lead', label: 'Lead — orchestrates, assigns tasks' },
                              { value: 'worker', label: 'Worker — executes implementation' },
                              { value: 'reviewer', label: 'Reviewer — reviews and approves' },
                            ]} />
                          </Form.Item>
                          <Form.Item name="subdir" label="Subdir" style={{ flex: 1 }}>
                            <Input placeholder="e.g. impl" />
                          </Form.Item>
                        </Flex>
                        <Form.Item name="persona" label="Persona">
                          <TextArea rows={3} placeholder="Senior backend engineer. Prioritize correctness and performance. Always write unit tests for public functions." />
                        </Form.Item>
                        <Form.Item name="responsibilities" label="Responsibilities" help="One per line">
                          <TextArea rows={3} placeholder={'Implement tasks tagged "backend" or "feature"\nWrite unit tests and integration tests for your code\nFix bugs assigned after review cycle'} />
                        </Form.Item>
                        <Form.Item name="acceptanceCriteria" label="Acceptance Criteria" help="One per line">
                          <TextArea rows={3} placeholder={'cargo test passes\ncargo clippy has no warnings\nNo new unwrap() in production paths'} />
                        </Form.Item>
                      </>
                    ),
                  },
                  {
                    key: 'triggers',
                    label: 'Triggers',
                    children: (
                      <>
                        <Form.Item name="triggers" label="Triggers" help='JSON array of trigger objects. Types: task_assigned, message_received, task_status_changed, on_mention, cron'>
                          <TextArea
                            rows={6}
                            placeholder={`[\n  {\n    "type": "task_assigned",\n    "condition": "assignee == me",\n    "from": null,\n    "messageType": null,\n    "status": null,\n    "assignee": null,\n    "cron": null\n  }\n]`}
                            style={{ fontFamily: 'Menlo, Monaco, monospace', fontSize: 12 }}
                          />
                        </Form.Item>
                      </>
                    ),
                  },
                  {
                    key: 'handoff',
                    label: 'Handoff',
                    children: (
                      <>
                        <Form.Item name="handoffRules" label="Handoff Rules" help='JSON array. Types: review_request, result, status, alert'>
                          <TextArea
                            rows={5}
                            placeholder={`[\n  {\n    "when": "task_complete",\n    "to": "review-agent",\n    "type": "review_request",\n    "messageTemplate": null\n  }\n]`}
                            style={{ fontFamily: 'Menlo, Monaco, monospace', fontSize: 12 }}
                          />
                        </Form.Item>
                      </>
                    ),
                  },
                  {
                    key: 'output',
                    label: 'Output',
                    children: (
                      <>
                        <Flex gap={12}>
                          <Form.Item name="outputFormat" label="Format" style={{ flex: 1 }}>
                            <Select
                              allowClear
                              placeholder="None"
                              options={[
                                { value: 'markdown', label: 'Markdown' },
                                { value: 'text', label: 'Plain Text' },
                                { value: 'json', label: 'JSON' },
                              ]}
                            />
                          </Form.Item>
                          <Form.Item name="outputAttachDiff" label="Attach Diff" style={{ flex: 1 }} valuePropName="checked">
                            <Select
                              allowClear
                              placeholder="Default"
                              options={[
                                { value: 'true', label: 'Yes' },
                                { value: 'false', label: 'No' },
                              ]}
                            />
                          </Form.Item>
                        </Flex>
                        <Form.Item name="outputRequiredSections" label="Required Sections" help="One per line">
                          <TextArea rows={3} placeholder={'Summary\nFiles Changed\nTest Results\nNotes'} />
                        </Form.Item>
                      </>
                    ),
                  },
                  {
                    key: 'sla',
                    label: 'SLA',
                    children: (
                      <>
                        <Flex gap={12}>
                          <Form.Item name="slaMaxDuration" label="Max Duration (min)" style={{ flex: 1 }}>
                            <Input type="number" placeholder="60" />
                          </Form.Item>
                          <Form.Item name="slaMaxTokens" label="Max Tokens" style={{ flex: 1 }}>
                            <Input type="number" placeholder="50000" />
                          </Form.Item>
                          <Form.Item name="slaEscalateAfter" label="Escalate After Blocked (min)" style={{ flex: 1 }}>
                            <Input type="number" placeholder="30" />
                          </Form.Item>
                        </Flex>
                      </>
                    ),
                  },
                  {
                    key: 'limits',
                    label: 'Limits',
                    children: (
                      <>
                        <Form.Item name="limitsMaxFileSize" label="Max File Write (KB)">
                          <Input type="number" placeholder="500" />
                        </Form.Item>
                        <Form.Item name="limitsAllowedBash" label="Allowed Bash Commands" help="One per line. Empty = allow all safe commands">
                          <TextArea rows={3} placeholder={'cargo build\ncargo test\ncargo clippy\ngit diff'} />
                        </Form.Item>
                        <Form.Item name="limitsDeniedTools" label="Denied Tools" help="One per line">
                          <TextArea rows={2} placeholder={'Write\nEdit\nBash'} />
                        </Form.Item>
                      </>
                    ),
                  },
                ]}
              />
            )}
            <Divider style={{ margin: '12px 0' }} />
            <Form.Item style={{ textAlign: 'right', marginBottom: 0 }}>
              <Space>
                <Button onClick={() => { setMemberModalOpen(false); setEditingMember(null); memberForm.resetFields(); }}>Cancel</Button>
                <Button type="primary" htmlType="submit">{editingMember ? 'Save' : 'Add'}</Button>
              </Space>
            </Form.Item>
          </Form>
        </Modal>

      </Layout>
    </AppLayout>
  );
}
