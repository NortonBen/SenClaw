import React, { useState, useEffect, useRef, useCallback } from 'react';
import {
  Avatar, Typography, Space, Tag, Spin, Empty, Input, Button, Badge,
} from 'antd';
import {
  RobotOutlined, UserOutlined, SendOutlined, AlertOutlined,
  CheckCircleOutlined, ArrowRightOutlined, LoadingOutlined,
} from '@ant-design/icons';
import type { CoworkMessage, CoworkTask, TaskResultEvent } from '../../types';
import { TaskResultCard } from './TaskResultCard';

const { Text, Paragraph } = Typography;

const MSG_TYPE_COLOR: Record<string, string> = {
  handoff:        '#1890ff',
  review_request: '#722ed1',
  clarification:  '#fa8c16',
  result:         '#52c41a',
  status:         '#8c8c8c',
  alert:          '#ff4d4f',
};

const MSG_TYPE_ICON: Record<string, React.ReactNode> = {
  result:  <CheckCircleOutlined />,
  alert:   <AlertOutlined />,
  handoff: <ArrowRightOutlined />,
};

interface WorkspaceChatPanelProps {
  workspaceId: string;
  tasks: CoworkTask[];
  lastTaskResult: TaskResultEvent | null;
  onSend: (content: string) => void;
  sending?: boolean;
}

// Fake optimistic message inserted immediately on submit
function makeOptimistic(content: string): CoworkMessage {
  return {
    id: `optimistic-${Date.now()}`,
    workspaceId: '',
    fromMember: 'user',
    toMember: null,
    messageType: 'status',
    content,
    attachments: null,
    taskId: null,
    isRead: true,
    createdAt: new Date().toISOString(),
  };
}

interface BubbleProps {
  msg: CoworkMessage;
  linkedTasks: CoworkTask[];
  highlightTaskId: string | null;
  isOptimistic?: boolean;
  taskValidations: Record<string, import('../../types').OutputValidation>;
}

function Bubble({ msg, linkedTasks, highlightTaskId, isOptimistic, taskValidations }: BubbleProps) {
  const isUser = msg.fromMember === 'user';
  const color  = MSG_TYPE_COLOR[msg.messageType] ?? '#8c8c8c';

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: isUser ? 'row-reverse' : 'row',
        alignItems: 'flex-start',
        gap: 8,
        marginBottom: 12,
        opacity: isOptimistic ? 0.7 : 1,
      }}
    >
      <Avatar
        size="small"
        icon={isUser ? <UserOutlined /> : <RobotOutlined />}
        style={{
          background: isUser ? '#1890ff' : msg.fromMember === 'system' ? '#8c8c8c' : '#52c41a',
          flexShrink: 0,
          marginTop: 2,
        }}
      />

      <div style={{ maxWidth: '72%', minWidth: 0 }}>
        {/* Name + tag row — mirrored for user */}
        <div
          style={{
            display: 'flex',
            flexDirection: isUser ? 'row-reverse' : 'row',
            alignItems: 'center',
            gap: 6,
            marginBottom: 3,
          }}
        >
          <Text strong style={{ fontSize: 11 }}>{isUser ? 'you' : msg.fromMember}</Text>
          {msg.messageType !== 'status' && (
            <Tag
              color={color}
              style={{ margin: 0, fontSize: 10, lineHeight: '16px', padding: '0 4px' }}
            >
              {MSG_TYPE_ICON[msg.messageType]} {msg.messageType}
            </Tag>
          )}
          <Text type="secondary" style={{ fontSize: 10 }}>
            {isOptimistic
              ? <LoadingOutlined spin />
              : new Date(msg.createdAt).toLocaleTimeString()}
          </Text>
        </div>

        {/* Bubble */}
        <div
          style={{
            background: isUser ? '#1890ff' : '#f5f5f5',
            color: isUser ? '#fff' : 'inherit',
            borderRadius: isUser ? '12px 12px 4px 12px' : '12px 12px 12px 4px',
            padding: '8px 12px',
            display: 'inline-block',
            maxWidth: '100%',
          }}
        >
          <Paragraph
            style={{
              margin: 0,
              fontSize: 13,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              color: isUser ? '#fff' : 'inherit',
            }}
          >
            {msg.content}
          </Paragraph>
        </div>

        {/* Task results linked to this message — below the bubble, left-aligned even for user */}
        {linkedTasks.length > 0 && (
          <div style={{ marginTop: 8 }}>
            {linkedTasks.map(task => (
              <div key={task.id} style={{ marginBottom: 6 }}>
                <TaskResultCard 
                  task={task} 
                  highlight={task.id === highlightTaskId}
                  outputValidation={taskValidations[task.id]}
                />
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export function WorkspaceChatPanel({
  workspaceId,
  tasks,
  lastTaskResult,
  onSend,
  sending,
}: WorkspaceChatPanelProps) {
  const [messages, setMessages]       = useState<CoworkMessage[]>([]);
  const [optimistic, setOptimistic]   = useState<CoworkMessage | null>(null);
  const [loading, setLoading]         = useState(false);
  const [input, setInput]             = useState('');
  const [highlightTaskId, setHighlightTaskId] = useState<string | null>(null);
  const [taskValidations, setTaskValidations] = useState<Record<string, import('../../types').OutputValidation>>({});
  const bottomRef = useRef<HTMLDivElement>(null);

  const loadMessages = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetch(`/api/cowork/workspaces/${workspaceId}/messages?limit=100`);
      if (res.ok) {
        const data = await res.json();
        const raw: CoworkMessage[] = data.messages ?? [];
        const sorted = [...raw].sort(
          (a, b) => new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime(),
        );
        setMessages(sorted);
        setOptimistic(null); // server confirmed → drop optimistic
      }
    } finally {
      setLoading(false);
    }
  }, [workspaceId]);

  useEffect(() => { loadMessages(); }, [loadMessages]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, optimistic]);

  useEffect(() => {
    if (!lastTaskResult || lastTaskResult.workspaceId !== workspaceId) return;
    setHighlightTaskId(lastTaskResult.taskId);
    // Store validation data if present
    if (lastTaskResult.outputValidation) {
      setTaskValidations(prev => ({
        ...prev,
        [lastTaskResult.taskId]: lastTaskResult.outputValidation!,
      }));
    }
    loadMessages();
    const t = setTimeout(() => setHighlightTaskId(null), 4000);
    return () => clearTimeout(t);
  }, [lastTaskResult, workspaceId, loadMessages]);

  const handleSend = () => {
    const text = input.trim();
    if (!text) return;
    // Optimistic: show immediately
    setOptimistic(makeOptimistic(text));
    setInput('');
    onSend(text);
    // Reload after short delay to get server messages
    setTimeout(() => loadMessages(), 1200);
  };

  const activeTasks = tasks.filter(t => t.status === 'in_progress');

  // Build a map: taskId → task for inline rendering
  const taskMap = Object.fromEntries(tasks.map(t => [t.id, t]));

  const allMessages: (CoworkMessage & { isOptimistic?: boolean })[] = (
    [...messages, ...(optimistic ? [{ ...optimistic, isOptimistic: true }] : [])] as (CoworkMessage & {
      isOptimistic?: boolean;
    })[]
  ).sort((a, b) => new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime());

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>

      {/* Active tasks strip */}
      {activeTasks.length > 0 && (
        <div style={{ padding: '6px 12px', background: '#fffbe6', borderBottom: '1px solid #ffe58f', flexShrink: 0 }}>
          <Space wrap size={4}>
            <Text type="secondary" style={{ fontSize: 11 }}>Đang chạy:</Text>
            {activeTasks.map(t => (
              <Badge key={t.id} status="processing" text={<Text style={{ fontSize: 11 }}>{t.title}</Text>} />
            ))}
          </Space>
        </div>
      )}

      {/* Messages */}
      <div style={{ flex: 1, overflow: 'auto', padding: '12px 16px' }}>
        {loading && messages.length === 0 ? (
          <div style={{ textAlign: 'center', paddingTop: 40 }}><Spin /></div>
        ) : allMessages.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có tin nhắn" style={{ marginTop: 40 }} />
        ) : (
          allMessages.map(msg => {
            // Collect tasks linked to this message (taskId match) + done tasks linked via result messages
            const linked: CoworkTask[] = [];
            if (msg.taskId && taskMap[msg.taskId]) {
              const t = taskMap[msg.taskId];
              if (t.status === 'done') linked.push(t);
            }
            // Also: result-type messages → find task by content heuristic (taskId on msg)
            return (
              <Bubble
                key={msg.id}
                msg={msg}
                linkedTasks={linked}
                highlightTaskId={highlightTaskId}
                isOptimistic={msg.isOptimistic}
                taskValidations={taskValidations}
              />
            );
          })
        )}
        <div ref={bottomRef} />
      </div>

      {/* Input */}
      <div style={{ padding: '8px 12px', borderTop: '1px solid #f0f0f0', display: 'flex', gap: 8, flexShrink: 0 }}>
        <Input.TextArea
          value={input}
          onChange={e => setInput(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); handleSend(); }
          }}
          placeholder="Gửi yêu cầu tới workspace… (Enter để gửi)"
          autoSize={{ minRows: 1, maxRows: 4 }}
          style={{ flex: 1, fontSize: 13 }}
        />
        <Button
          type="primary"
          icon={<SendOutlined />}
          onClick={handleSend}
          loading={sending}
          disabled={!input.trim()}
        />
      </div>
    </div>
  );
}
