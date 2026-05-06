import React, { useState } from 'react';
import {
  Layout, Typography, Button, Space, Tooltip, Tag, Divider,
  Modal, Form, Input, Select, Switch, theme, message as antMessage, Dropdown,
} from 'antd';
import {
  FolderOutlined, BranchesOutlined, ReloadOutlined, CodeOutlined, MoreOutlined,
  FolderOpenOutlined, CloseOutlined, ColumnWidthOutlined,
} from '@ant-design/icons';
import { FileTree } from './FileTree';
import { GitLog } from './GitLog';
import { FolderPicker } from './FolderPicker';
import { AgentCommandInput } from './AgentCommandInput';
import type { CodeSession, FileNode, GitCommit, CodeChatGroup, CodeChatMessage } from '../../../hooks/useCode';
import type { PermissionMessage } from '../../../types';
import { useAppContext } from '../../../contexts/AppContext';
import { CommonChatInput, CommonPermissionRequestCard } from '../../chat-common';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

const { Content } = Layout;
const { Title, Text } = Typography;
const { Option } = Select;

const LANGUAGES = ['rust', 'typescript', 'javascript', 'python', 'go', 'java', 'other'];
const AGENT_COMMANDS = [
  { key: 'analyze', desc: 'Phan tich codebase, tim van de va de xuat huong xu ly.' },
  { key: 'architecture', desc: 'Danh gia kien truc va goi y refactor theo module.' },
  { key: 'batch', desc: 'Chay nhieu tac vu code theo lo trong cung session.' },
  { key: 'build-dashboard', desc: 'Tao dashboard tong hop trang thai, metrics, va logs.' },
  { key: 'code-review', desc: 'Review thay doi code theo uu tien bug, risk, regression.' },
  { key: 'compact', desc: 'Nen gon context chat de tiep tuc lam viec dai hoi thoai.' },
  { key: 'context', desc: 'Tong hop ngu canh file/session truoc khi thuc thi task.' },
];

export interface CreateSessionParams {
  name: string;
  workspace: string;
  language?: string;
  init_git?: boolean;
}

interface Props {
  activeSession: CodeSession | null;
  onCreate: (params: CreateSessionParams) => Promise<void>;
  onGetFiles: (id: string) => Promise<{ tree: FileNode[] } | null>;
  onGetFileContent: (id: string, path: string) => Promise<string | null>;
  onGetGitLog: (id: string) => Promise<GitCommit[]>;
  onRollback: (id: string, steps: number) => Promise<boolean>;
  onSendChat: (id: string, groupId: string, prompt: string) => Promise<{
    reply: string;
    parsed?: { refs?: string[]; skills?: string[]; command?: string | null; plain_text?: string };
    resolved_refs?: string[];
    dag_plan?: string;
    messages?: CodeChatMessage[];
    queued_preview?: CodeChatMessage[];
  } | null>;
  onListChatGroups: (projectId: string) => Promise<CodeChatGroup[]>;
  onCreateChatGroup: (projectId: string, name: string) => Promise<CodeChatGroup | null>;
  onListGroupMessages: (groupId: string) => Promise<CodeChatMessage[]>;
  onStopCurrentTask: (groupId: string) => Promise<{ ok: boolean; action: 'stopped' | 'removed' | 'noop'; target_id?: string | null } | null>;
  error: string | null;
  createTrigger?: number;
}

interface LocalChatMessage {
  id: string;
  role: 'user' | 'agent' | 'permission';
  text: string;
  createdAt: number;
  status?: 'queued' | 'processing' | 'done' | 'failed';
  dagPlan?: string | null;
  requestId?: string;
  toolName?: string;
  options?: Array<{ key: string; label: string }>;
  resolved?: { key: string; label: string };
}

export function CodeView({
  activeSession,
  onCreate,
  onGetFiles,
  onGetFileContent,
  onGetGitLog,
  onRollback,
  onSendChat,
  onListChatGroups,
  onCreateChatGroup,
  onListGroupMessages,
  onStopCurrentTask,
  error,
  createTrigger,
}: Props) {
  const { token } = theme.useToken();
  const { ws } = useAppContext();

  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [treeLoading, setTreeLoading] = useState(false);
  const [gitLog, setGitLog] = useState<GitCommit[]>([]);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [panelTab, setPanelTab] = useState<'files' | 'git'>('files');
  const [panelOpen, setPanelOpen] = useState(false);
  const [panelLayout, setPanelLayout] = useState<'single' | 'split'>('single');
  const [chatInput, setChatInput] = useState('');
  const [projectGroups, setProjectGroups] = useState<CodeChatGroup[]>([]);
  const [activeGroupId, setActiveGroupId] = useState<string | null>(null);
  const [groupQuery, setGroupQuery] = useState('');
  const [creatingGroup, setCreatingGroup] = useState(false);
  const [stoppingTask, setStoppingTask] = useState(false);
  const [chatByGroup, setChatByGroup] = useState<Record<string, LocalChatMessage[]>>({});
  const [queuePreview, setQueuePreview] = useState<CodeChatMessage[]>([]);
  const [agentTyping, setAgentTyping] = useState(false);
  const [chatSending, setChatSending] = useState(false);
  const chatScrollRef = React.useRef<HTMLDivElement | null>(null);
  const sendLockRef = React.useRef(false);
  const lastSendRef = React.useRef<{ groupId: string; text: string; at: number } | null>(null);
  const [showScrollToBottom, setShowScrollToBottom] = useState(false);
  const [previewContent, setPreviewContent] = useState<string>('');
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewCopied, setPreviewCopied] = useState(false);

  const [createModalOpen, setCreateModalOpen] = useState(false);

  React.useEffect(() => {
    if (createTrigger) setCreateModalOpen(true);
  }, [createTrigger]);
  const [creating, setCreating] = useState(false);
  const [folderPickerOpen, setFolderPickerOpen] = useState(false);
  const [form] = Form.useForm();

  // Reload tree + git log when active session changes
  const prevSessionId = React.useRef<string | null>(null);
  React.useEffect(() => {
    if (!activeSession) {
      setFileTree([]);
      setGitLog([]);
      setSelectedFile(null);
      prevSessionId.current = null;
      setChatInput('');
      setAgentTyping(false);
      setPreviewContent('');
      setPreviewLoading(false);
      setProjectGroups([]);
      setActiveGroupId(null);
      setQueuePreview([]);
      return;
    }
    if (activeSession.id === prevSessionId.current) return;
    prevSessionId.current = activeSession.id;
    setSelectedFile(null);
    setChatInput('');
    setAgentTyping(false);
    setPreviewContent('');
    setPreviewLoading(false);
    setQueuePreview([]);
    setTreeLoading(true);
    onGetFiles(activeSession.id).then(r => {
      setFileTree(r?.tree ?? []);
      setTreeLoading(false);
    });
    if (activeSession.git_enabled) {
      onGetGitLog(activeSession.id).then(setGitLog);
    } else {
      setGitLog([]);
    }
    onListChatGroups(activeSession.id).then(async groups => {
      if (groups.length === 0) {
        const created = await onCreateChatGroup(activeSession.id, 'Main');
        const next = created ? [created] : [];
        setProjectGroups(next);
        setActiveGroupId(created?.id ?? null);
      } else {
        setProjectGroups(groups);
        setActiveGroupId(groups[0].id);
      }
    });
  }, [activeSession?.id, onCreateChatGroup, onListChatGroups]);

  const refreshGroupMessages = React.useCallback(async (groupId: string) => {
    const messages = await onListGroupMessages(groupId);
    setChatByGroup(prev => ({
      ...prev,
      [groupId]: messages.map(m => ({
        id: m.id,
        role: m.role,
        text: m.content,
        createdAt: m.created_at,
        status: m.status,
        dagPlan: m.dag_plan ?? null,
      })),
    }));
    setQueuePreview(messages.filter(m => m.status === 'queued' || m.status === 'processing').slice(0, 5));
    setAgentTyping(messages.some(m => m.status === 'processing'));
    return messages;
  }, [onListGroupMessages]);

  React.useEffect(() => {
    if (!activeGroupId) return;
    refreshGroupMessages(activeGroupId);
  }, [activeGroupId, refreshGroupMessages]);

  React.useEffect(() => {
    if (!activeGroupId) return;
    const protocol = window.location.protocol === 'https:' ? 'wss' : 'ws';
    const ws = new WebSocket(`${protocol}://${window.location.host}/api/code/ws?group_id=${encodeURIComponent(activeGroupId)}`);
    ws.onmessage = (evt) => {
      try {
        const msg = JSON.parse(String(evt.data));
        if (msg.type !== 'code:chat:update' || msg.group_id !== activeGroupId) return;
        const messages = (msg.messages ?? []) as CodeChatMessage[];
        setChatByGroup(prev => ({
          ...prev,
          [activeGroupId]: messages.map(m => ({
            id: m.id,
            role: m.role,
            text: m.content,
            createdAt: m.created_at,
            status: m.status,
            dagPlan: m.dag_plan ?? null,
          })),
        }));
        setQueuePreview((msg.queued_preview ?? []).slice(0, 5));
        setAgentTyping(messages.some(m => m.status === 'processing'));
      } catch {
        // ignore bad payload
      }
    };
    return () => {
      ws.close();
    };
  }, [activeGroupId]);

  React.useEffect(() => {
    if (!activeSession || !selectedFile) {
      setPreviewContent('');
      setPreviewLoading(false);
      return;
    }
    setPreviewLoading(true);
    onGetFileContent(activeSession.id, selectedFile)
      .then(content => setPreviewContent(content ?? '// Unable to load preview'))
      .finally(() => setPreviewLoading(false));
  }, [activeSession?.id, selectedFile, onGetFileContent]);

  const codeAgentJid = activeGroupId ? `code-chat:${activeGroupId}` : null;

  React.useEffect(() => {
    if (!codeAgentJid) return;
    ws.subscribe(codeAgentJid);
  }, [codeAgentJid, ws]);

  const permissionMessages = React.useMemo<LocalChatMessage[]>(() => {
    if (!codeAgentJid) return [];
    const messages = ws.messages[codeAgentJid] ?? [];
    return messages
      .filter((m): m is PermissionMessage => m.role === 'permission')
      .map((m) => ({
        id: `perm-${m.requestId}`,
        role: 'permission',
        text: m.content,
        createdAt: new Date(m.timestamp).getTime(),
        requestId: m.requestId,
        toolName: m.toolName,
        options: m.options,
        resolved: m.resolved,
      }));
  }, [codeAgentJid, ws.messages]);

  const sessionMessages = React.useMemo(() => {
    const base = activeGroupId ? (chatByGroup[activeGroupId] ?? []) : [];
    const map = new Map<string, LocalChatMessage>();
    for (const m of base) map.set(m.id, m);
    for (const m of permissionMessages) map.set(m.id, m);
    return Array.from(map.values()).sort((a, b) => a.createdAt - b.createdAt);
  }, [activeGroupId, chatByGroup, permissionMessages]);

  React.useEffect(() => {
    const el = chatScrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [activeSession?.id, sessionMessages.length, agentTyping]);

  const handleChatScroll = () => {
    const el = chatScrollRef.current;
    if (!el) return;
    const gap = el.scrollHeight - el.scrollTop - el.clientHeight;
    setShowScrollToBottom(gap > 120);
  };

  const handleCreate = async (values: any) => {
    setCreating(true);
    await onCreate({
      name: values.name,
      workspace: values.workspace,
      language: values.language,
      init_git: values.init_git ?? true,
    });
    setCreating(false);
    setCreateModalOpen(false);
    form.resetFields();
  };

  const handleRollback = async (steps: number) => {
    if (!activeSession) return;
    const ok = await onRollback(activeSession.id, steps);
    if (ok) {
      antMessage.success(`Rolled back ${steps} commit${steps > 1 ? 's' : ''}`);
      const log = await onGetGitLog(activeSession.id);
      setGitLog(log);
      const result = await onGetFiles(activeSession.id);
      setFileTree(result?.tree ?? []);
    } else {
      antMessage.error(error ?? 'Rollback failed');
    }
  };

  const refreshTree = async () => {
    if (!activeSession) return;
    setTreeLoading(true);
    const result = await onGetFiles(activeSession.id);
    setFileTree(result?.tree ?? []);
    setTreeLoading(false);
  };

  const ensureActiveGroupId = async (): Promise<string | null> => {
    if (!activeSession) return null;
    if (activeGroupId) return activeGroupId;
    if (projectGroups.length > 0) {
      setActiveGroupId(projectGroups[0].id);
      return projectGroups[0].id;
    }
    setCreatingGroup(true);
    const created = await onCreateChatGroup(activeSession.id, 'Main');
    setCreatingGroup(false);
    if (!created) return null;
    setProjectGroups(prev => [created, ...prev]);
    setActiveGroupId(created.id);
    return created.id;
  };

  const sendChat = async () => {
    if (!activeSession) {
      antMessage.warning('Vui long chon project truoc khi chat');
      return;
    }
    const groupId = await ensureActiveGroupId();
    if (!groupId) {
      antMessage.error('Chua khoi tao duoc group chat. Thu lai sau.');
      return;
    }
    const text = chatInput.trim();
    if (!text) return;
    if (sendLockRef.current) return;

    const now = Date.now();
    const lastSend = lastSendRef.current;
    if (
      lastSend &&
      lastSend.groupId === groupId &&
      lastSend.text === text &&
      now - lastSend.at < 900
    ) {
      return;
    }

    sendLockRef.current = true;
    setChatSending(true);
    lastSendRef.current = { groupId, text, at: now };
    await new Promise(resolve => window.setTimeout(resolve, 120));

    try {
      const userMsg: LocalChatMessage = {
        id: `u-${Date.now()}`,
        role: 'user',
        text,
        createdAt: Date.now(),
      };
      setChatByGroup(prev => ({
        ...prev,
        [groupId]: [...(prev[groupId] ?? []), { ...userMsg, status: 'queued' }],
      }));
      setChatInput('');
      setAgentTyping(true);
      const result = await onSendChat(activeSession.id, groupId, text);
      const parsedHints = result?.parsed
        ? [
            result.parsed.command ? `command: /${result.parsed.command}` : null,
            result.parsed.refs && result.parsed.refs.length ? `refs: ${result.parsed.refs.join(', ')}` : null,
            result.parsed.skills && result.parsed.skills.length ? `skills: ${result.parsed.skills.join(', ')}` : null,
            result.parsed.plain_text ? `plain: ${result.parsed.plain_text}` : null,
            result.resolved_refs && result.resolved_refs.length ? `resolved: ${result.resolved_refs.join(', ')}` : null,
          ].filter(Boolean).join('\n\n')
        : '';
      if (!result) {
        antMessage.error('Chat backend khong phan hoi');
        const failMsg: LocalChatMessage = {
          id: `a-${Date.now()}`,
          role: 'agent',
          text: `Khong goi duoc backend chat. Vui long kiem tra API /api/code/sessions/${activeSession.id}/chat`,
          createdAt: Date.now(),
          status: 'failed',
        };
        setChatByGroup(prev => ({
          ...prev,
          [groupId]: [...(prev[groupId] ?? []), failMsg],
        }));
        setAgentTyping(false);
        return;
      }
      if (result.messages?.length) {
        setChatByGroup(prev => ({
          ...prev,
          [groupId]: result.messages!.map(m => ({
            id: m.id,
            role: m.role,
            text: m.content,
            createdAt: m.created_at,
            status: m.status,
            dagPlan: m.dag_plan ?? null,
          })),
        }));
        setQueuePreview((result.queued_preview ?? []).slice(0, 5));
        setAgentTyping(result.messages.some(m => m.status === 'processing'));
      } else {
        const agentMsg: LocalChatMessage = {
          id: `a-${Date.now()}`,
          role: 'agent',
          text: `${result.reply}${parsedHints ? `\n\n${parsedHints}` : ''}`,
          createdAt: Date.now(),
          status: 'done',
          dagPlan: result.dag_plan ?? null,
        };
        setChatByGroup(prev => ({
          ...prev,
          [groupId]: [...(prev[groupId] ?? []), agentMsg],
        }));
      }
      setAgentTyping(false);
    } finally {
      sendLockRef.current = false;
      setChatSending(false);
    }
  };

  const stopOrRemoveCurrentTask = async () => {
    if (!activeGroupId) return;
    setStoppingTask(true);
    const result = await onStopCurrentTask(activeGroupId);
    setStoppingTask(false);
    if (!result) {
      antMessage.error('Khong the stop/remove task hien tai');
      return;
    }
    if (result.action === 'stopped') {
      antMessage.success('Da stop task dang xu ly');
      return;
    }
    if (result.action === 'removed') {
      antMessage.success('Da remove task dau queue');
      return;
    }
    antMessage.info('Khong co task dang xu ly hoac trong queue');
  };

  const quickPrompts = [
    'Phân tích cấu trúc project này',
    'Tìm bug và đề xuất fix',
    'Viết test cho module đang mở',
  ];

  const formatTime = (ts: number) =>
    new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

  const previewLines = previewContent.split('\n');
  const mentionItems = React.useMemo(() => {
    const out: Array<{ key: string; desc?: string }> = [];
    const walk = (nodes: FileNode[]) => {
      for (const node of nodes) {
        out.push({
          key: node.path,
          desc: node.type === 'dir' ? 'Folder trong workspace' : 'File trong workspace',
        });
        if (node.children?.length) walk(node.children);
      }
    };
    walk(fileTree);
    return out.slice(0, 400);
  }, [fileTree]);

  return (
    <Layout style={{ height: '100%', background: 'transparent' }}>
      {/* Main content */}
      <Content style={{ display: 'flex', flexDirection: 'column', overflow: 'hidden', position: 'relative' }}>
        {!activeSession ? (
          <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', flexDirection: 'column', gap: 12 }}>
            <div style={{
              width: 58, height: 58, borderRadius: 14,
              background: token.colorFillTertiary,
              display: 'flex', alignItems: 'center', justifyContent: 'center',
            }}>
              <CodeOutlined style={{ fontSize: 24, color: token.colorTextTertiary }} />
            </div>
            <Text style={{ color: token.colorTextTertiary, fontSize: 14 }}>
              Chọn một project ở thanh bên trái
            </Text>
          </div>
        ) : (
          <div style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
            {/* Project header */}
            <div style={{
              padding: '10px 18px',
              borderBottom: `1px solid ${token.colorBorderSecondary}`,
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              background: token.colorBgContainer,
            }}>
              <div style={{
                width: 28, height: 28, borderRadius: 8,
                background: `${token.colorPrimary}18`,
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                color: token.colorPrimary, fontSize: 14, flexShrink: 0,
              }}>
                <FolderOutlined />
              </div>
              <Text strong style={{ fontSize: 14 }}>{activeSession.name}</Text>
              {activeSession.language && <Tag color="blue" style={{ margin: 0, borderRadius: 6 }}>{activeSession.language}</Tag>}
              {activeSession.git_enabled && <Tag color="green" style={{ margin: 0, borderRadius: 6 }}>git</Tag>}
              <Text type="secondary" style={{ fontSize: 11, fontFamily: 'monospace', opacity: 0.7 }}>
                {activeSession.workspace.replace(/^\/Users\/[^/]+/, '~')}
              </Text>
              <div style={{ marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 6 }}>
                <Tooltip title="Refresh files">
                  <Button size="small" type="text" icon={<ReloadOutlined />} onClick={refreshTree} style={{ borderRadius: 8 }} />
                </Tooltip>
                <Dropdown
                  trigger={['click']}
                  menu={{
                    items: [
                      { key: 'files', icon: <FolderOutlined />, label: 'Files' },
                      ...(activeSession.git_enabled ? [{ key: 'git', icon: <BranchesOutlined />, label: 'Git' }] : []),
                    ],
                    onClick: ({ key }) => {
                      setPanelTab(key as 'files' | 'git');
                      setPanelOpen(true);
                      setPanelLayout('single');
                    },
                  }}
                >
                  <Button size="small" type="text" icon={<MoreOutlined />} style={{ borderRadius: 8 }}>
                    Workspace
                  </Button>
                </Dropdown>
              </div>
             
            </div>

            {/* Body - Agent chat first */}
            <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
              <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
                <div style={{ padding: '10px 16px', borderBottom: `1px solid ${token.colorBorderSecondary}`, display: 'flex', gap: 8, alignItems: 'center' }}>
                  <Text style={{ fontSize: 12, color: token.colorTextSecondary }}>Group chat</Text>
                  <Select
                    showSearch
                    size="small"
                    placeholder="Chon group chat"
                    value={activeGroupId ?? undefined}
                    style={{ minWidth: 260 }}
                    options={projectGroups
                      .filter(g => g.name.toLowerCase().includes(groupQuery.toLowerCase()))
                      .map(g => ({ label: g.name, value: g.id }))}
                    onSearch={setGroupQuery}
                    onChange={(v) => setActiveGroupId(v)}
                    filterOption={false}
                  />
                  <Button
                    size="small"
                    loading={creatingGroup}
                    onClick={async () => {
                      if (!activeSession) return;
                      setCreatingGroup(true);
                      const created = await onCreateChatGroup(activeSession.id, `Group ${projectGroups.length + 1}`);
                      setCreatingGroup(false);
                      if (created) {
                        setProjectGroups(prev => [created, ...prev]);
                        setActiveGroupId(created.id);
                      }
                    }}
                  >
                    New Group
                  </Button>
                  <Button
                    size="small"
                    danger
                    loading={stoppingTask}
                    disabled={!activeGroupId}
                    onClick={stopOrRemoveCurrentTask}
                  >
                    Stop/Remove Task
                  </Button>
                  {selectedFile && (
                    <Tag style={{ marginLeft: 'auto', borderRadius: 999, fontFamily: 'monospace' }}>
                      {selectedFile}
                    </Tag>
                  )}
                </div>

                <div
                  ref={chatScrollRef}
                  onScroll={handleChatScroll}
                  style={{ flex: 1, overflowY: 'auto', padding: '16px 18px', display: 'flex', flexDirection: 'column', gap: 10, position: 'relative' }}
                >
                {sessionMessages.length === 0 && (
                  <div style={{ margin: 'auto', textAlign: 'center', maxWidth: 560 }}>
                    <Title level={5} style={{ marginBottom: 8 }}>Bắt đầu chat với Agent</Title>
                    <Text type="secondary" style={{ fontSize: 13 }}>
                      Mô tả task code bạn muốn làm. Dùng menu <code>Workspace</code> để mở Files/Git khi cần ngữ cảnh.
                    </Text>
                    <Divider />
                    <Text type="secondary" style={{ fontFamily: 'monospace', fontSize: 12 }}>
                      {activeSession.workspace}
                    </Text>
                    <div style={{ marginTop: 14, display: 'flex', gap: 8, justifyContent: 'center', flexWrap: 'wrap' }}>
                      {quickPrompts.map(prompt => (
                        <Button
                          key={prompt}
                          size="small"
                          style={{ borderRadius: 999 }}
                          onClick={() => setChatInput(prompt)}
                        >
                          {prompt}
                        </Button>
                      ))}
                    </div>
                  </div>
                )}

                {sessionMessages.map(msg => (
                  <div key={msg.id} style={{ display: 'flex', justifyContent: msg.role === 'user' ? 'flex-end' : 'flex-start' }}>
                    <div style={{ maxWidth: '78%' }}>
                      {msg.role === 'permission' && msg.requestId ? (
                        <CommonPermissionRequestCard
                          toolName={msg.toolName ?? 'tool'}
                          content={msg.text}
                          requestId={msg.requestId}
                          options={msg.options ?? []}
                          resolved={msg.resolved}
                          onResolve={(requestId, optionKey) => ws.resolvePermission(requestId, optionKey)}
                        />
                      ) : (
                      <div
                        style={{
                          padding: '10px 12px',
                          borderRadius: 12,
                          background: msg.role === 'user' ? token.colorPrimary : token.colorFillAlter,
                          color: msg.role === 'user' ? '#fff' : token.colorText,
                          border: msg.role === 'agent' ? `1px solid ${token.colorBorderSecondary}` : 'none',
                          whiteSpace: 'pre-wrap',
                          fontSize: 13,
                        }}
                      >
                        {msg.role === 'agent' ? (
                          <div className="code-chat-markdown">
                            <ReactMarkdown
                              remarkPlugins={[remarkGfm]}
                              components={{
                                p: ({ children }) => <p style={{ margin: '0 0 8px 0' }}>{children}</p>,
                                code: ({ children }) => (
                                  <code style={{ background: token.colorFillSecondary, padding: '1px 5px', borderRadius: 6, fontSize: 12 }}>
                                    {children}
                                  </code>
                                ),
                                pre: ({ children }) => (
                                  <pre style={{ background: token.colorBgContainer, border: `1px solid ${token.colorBorderSecondary}`, borderRadius: 8, padding: 10, overflowX: 'auto', margin: '6px 0' }}>
                                    {children}
                                  </pre>
                                ),
                              }}
                            >
                              {msg.text}
                            </ReactMarkdown>
                          </div>
                        ) : (
                          msg.text
                        )}
                      </div>
                      )}
                      <Text
                        type="secondary"
                        style={{
                          display: 'block',
                          fontSize: 10,
                          marginTop: 4,
                          textAlign: msg.role === 'user' ? 'right' : 'left',
                          padding: '0 4px',
                        }}
                      >
                        {formatTime(msg.createdAt)}
                      </Text>
                    </div>
                  </div>
                ))}

                {agentTyping && (
                  <div style={{ display: 'flex', justifyContent: 'flex-start' }}>
                    <div style={{ padding: '10px 12px', borderRadius: 12, background: token.colorFillAlter, border: `1px solid ${token.colorBorderSecondary}` }}>
                      <div style={{ display: 'flex', gap: 5, alignItems: 'center', minHeight: 14 }}>
                        {[0, 150, 300].map(delay => (
                          <span
                            key={delay}
                            style={{
                              width: 6,
                              height: 6,
                              borderRadius: '50%',
                              background: token.colorPrimary,
                              opacity: 0.75,
                              animation: `codeChatBounce 900ms ease-in-out infinite`,
                              animationDelay: `${delay}ms`,
                            }}
                          />
                        ))}
                        <Text type="secondary" style={{ fontSize: 12, marginLeft: 2 }}>
                          Agent đang suy nghĩ...
                        </Text>
                      </div>
                    </div>
                  </div>
                )}
                {showScrollToBottom && (
                  <Button
                    size="small"
                    type="primary"
                    style={{
                      position: 'sticky',
                      bottom: 8,
                      margin: '8px auto 0',
                      borderRadius: 999,
                      zIndex: 2,
                    }}
                    onClick={() => {
                      const el = chatScrollRef.current;
                      if (!el) return;
                      el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
                    }}
                  >
                    Xuống cuối
                  </Button>
                )}
                {queuePreview.length > 0 && (
                  <div style={{ marginTop: 8, padding: 10, border: `1px dashed ${token.colorBorderSecondary}`, borderRadius: 10 }}>
                    <Text style={{ fontSize: 12, color: token.colorTextSecondary }}>Queue preview ({queuePreview.length})</Text>
                    <div style={{ marginTop: 6, display: 'flex', flexDirection: 'column', gap: 4 }}>
                      {queuePreview.map(q => (
                        <Text key={q.id} style={{ fontSize: 12, color: token.colorTextTertiary }}>
                          - {q.content.slice(0, 80)}
                        </Text>
                      ))}
                    </div>
                  </div>
                )}
                </div>

                <div style={{ padding: 12, borderTop: `1px solid ${token.colorBorderSecondary}`, background: token.colorBgContainer }}>
                  <CommonChatInput helperText="Enter để gửi · Shift+Enter xuống dòng · / @ # gợi ý">
                    <AgentCommandInput
                      value={chatInput}
                      onChange={setChatInput}
                      onSubmit={sendChat}
                      disabled={!activeSession}
                      sending={agentTyping || chatSending}
                      commands={AGENT_COMMANDS}
                      mentionItems={mentionItems}
                    />
                  </CommonChatInput>
                </div>
              </div>

              {panelOpen && (
                <div
                  style={{
                    width: 380,
                    padding: 8,
                    background: token.colorBgLayout,
                    display: 'flex',
                    flexDirection: 'column',
                    minHeight: 0,
                  }}
                >
                  <div
                    style={{
                      border: `1px solid ${token.colorBorderSecondary}`,
                      borderRadius: 12,
                      background: token.colorBgContainer,
                      display: 'flex',
                      flexDirection: 'column',
                      minHeight: 0,
                      height: '100%',
                      boxShadow: '0 8px 24px rgba(0,0,0,0.18)',
                    }}
                  >
                    <div
                      style={{
                        padding: '10px 12px',
                        borderBottom: `1px solid ${token.colorBorderSecondary}`,
                        display: 'flex',
                        alignItems: 'center',
                        gap: 8,
                      }}
                    >
                      <Text strong style={{ fontSize: 13 }}>{panelTab === 'files' ? 'Files' : 'Git'}</Text>
                      <div style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
                        <Dropdown
                          trigger={['click']}
                          menu={{
                            items: [
                              { key: 'single', label: 'Single view' },
                              { key: 'split', label: 'Split view' },
                            ],
                            onClick: ({ key }) => setPanelLayout(key as 'single' | 'split'),
                          }}
                        >
                          <Button
                            size="small"
                            type="text"
                            icon={<ColumnWidthOutlined />}
                            style={{ borderRadius: 8 }}
                          />
                        </Dropdown>
                        <Button
                          size="small"
                          type={panelTab === 'files' ? 'primary' : 'text'}
                          icon={<FolderOutlined />}
                          style={{ borderRadius: 8 }}
                          onClick={() => setPanelTab('files')}
                        />
                        {activeSession.git_enabled && (
                          <Button
                            size="small"
                            type={panelTab === 'git' ? 'primary' : 'text'}
                            icon={<BranchesOutlined />}
                            style={{ borderRadius: 8 }}
                            onClick={() => setPanelTab('git')}
                          />
                        )}
                        <Button
                          size="small"
                          type="text"
                          icon={<CloseOutlined />}
                          style={{ borderRadius: 8 }}
                          onClick={() => setPanelOpen(false)}
                        />
                      </div>
                    </div>
                    {panelLayout === 'single' ? (
                      <div style={{ flex: 1, overflowY: 'auto', padding: 10 }}>
                        {panelTab === 'files' && (
                          <FileTree
                            tree={fileTree}
                            loading={treeLoading}
                            selectedPath={selectedFile}
                            onSelect={setSelectedFile}
                          />
                        )}
                        {panelTab === 'git' && <GitLog log={gitLog} onRollback={handleRollback} />}
                      </div>
                    ) : (
                      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
                        <div style={{ flex: 1, overflowY: 'auto', padding: 10, borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
                          {panelTab === 'files' && (
                            <FileTree
                              tree={fileTree}
                              loading={treeLoading}
                              selectedPath={selectedFile}
                              onSelect={setSelectedFile}
                            />
                          )}
                          {panelTab === 'git' && <GitLog log={gitLog} onRollback={handleRollback} />}
                        </div>
                        <div style={{ flex: 1, minHeight: 120, display: 'flex', flexDirection: 'column' }}>
                          <div style={{ padding: '8px 10px', borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
                            <Text style={{ fontSize: 12, color: token.colorTextSecondary }}>Preview</Text>
                          </div>
                          {previewLoading ? (
                            <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 12 }}>
                              <Text type="secondary" style={{ fontSize: 12 }}>Loading preview...</Text>
                            </div>
                          ) : selectedFile ? (
                            <div style={{ flex: 1, overflow: 'auto', padding: 10 }}>
                              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                                <Text style={{ fontSize: 11, fontFamily: 'monospace', color: token.colorTextTertiary }}>
                                  {selectedFile}
                                </Text>
                                <div style={{ marginLeft: 'auto' }}>
                                  <Button
                                    size="small"
                                    onClick={async () => {
                                      await navigator.clipboard.writeText(previewContent || '');
                                      setPreviewCopied(true);
                                      window.setTimeout(() => setPreviewCopied(false), 1200);
                                    }}
                                    style={{ borderRadius: 8 }}
                                  >
                                    {previewCopied ? 'Copied' : 'Copy'}
                                  </Button>
                                </div>
                              </div>
                              <div
                                style={{
                                  marginTop: 8,
                                  borderRadius: 8,
                                  border: `1px solid ${token.colorBorderSecondary}`,
                                  background: token.colorBgLayout,
                                  overflow: 'auto',
                                  fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
                                  fontSize: 12,
                                  lineHeight: 1.5,
                                }}
                              >
                                {previewLines.length === 0 || (previewLines.length === 1 && previewLines[0] === '') ? (
                                  <div style={{ padding: 10, color: token.colorTextTertiary }}>{'// Empty file'}</div>
                                ) : (
                                  previewLines.map((line, idx) => (
                                    <div key={`${idx}-${line.slice(0, 12)}`} style={{ display: 'grid', gridTemplateColumns: '44px 1fr' }}>
                                      <div
                                        style={{
                                          textAlign: 'right',
                                          padding: '0 10px 0 6px',
                                          color: token.colorTextQuaternary,
                                          borderRight: `1px solid ${token.colorBorderSecondary}`,
                                          userSelect: 'none',
                                        }}
                                      >
                                        {idx + 1}
                                      </div>
                                      <div style={{ padding: '0 10px', color: token.colorText, whiteSpace: 'pre' }}>
                                        {line || ' '}
                                      </div>
                                    </div>
                                  ))
                                )}
                              </div>
                            </div>
                          ) : (
                            <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 12 }}>
                              <Text type="secondary" style={{ fontSize: 12, textAlign: 'center' }}>
                                Chọn file để xem preview nhanh tại đây.
                              </Text>
                            </div>
                          )}
                        </div>
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </Content>
      <style>{`
        @keyframes codeChatBounce {
          0%, 80%, 100% { transform: translateY(0); opacity: 0.45; }
          40% { transform: translateY(-3px); opacity: 1; }
        }
      `}</style>

      {/* Folder picker */}
      <FolderPicker
        open={folderPickerOpen}
        value={form.getFieldValue('workspace') || undefined}
        onSelect={path => {
          form.setFieldValue('workspace', path);
          setFolderPickerOpen(false);
        }}
        onCancel={() => setFolderPickerOpen(false)}
      />

      {/* Create session modal */}
      <Modal
        title="New Project"
        open={createModalOpen}
        onCancel={() => { setCreateModalOpen(false); form.resetFields(); }}
        footer={null}
        width={480}
      >
        <Form form={form} layout="vertical" onFinish={handleCreate} style={{ marginTop: 16 }}>
          <Form.Item name="name" label="Project name" rules={[{ required: true }]}>
            <Input placeholder="My project" />
          </Form.Item>
          <Form.Item name="workspace" label="Workspace path" rules={[{ required: true, message: 'Please select a workspace folder' }]}>
            <Input
              placeholder="Click Browse to choose a folder…"
              readOnly
              addonAfter={
                <Button
                  size="small"
                  type="text"
                  icon={<FolderOpenOutlined />}
                  onClick={() => setFolderPickerOpen(true)}
                  style={{ margin: -4 }}
                >
                  Browse
                </Button>
              }
              style={{ cursor: 'default' }}
            />
          </Form.Item>
          <Form.Item name="language" label="Primary language">
            <Select placeholder="Select language" allowClear>
              {LANGUAGES.map(l => <Option key={l} value={l}>{l}</Option>)}
            </Select>
          </Form.Item>
          <Form.Item name="init_git" label="Enable git (for rollback)" valuePropName="checked" initialValue={true}>
            <Switch />
          </Form.Item>
          <Form.Item style={{ marginBottom: 0, textAlign: 'right' }}>
            <Space>
              <Button onClick={() => { setCreateModalOpen(false); form.resetFields(); }}>Cancel</Button>
              <Button type="primary" htmlType="submit" loading={creating}>Create</Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </Layout>
  );
}
