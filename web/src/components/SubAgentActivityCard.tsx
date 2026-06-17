import { useState } from 'react';
import { theme, Typography, Tag, Tooltip } from 'antd';
import {
  CheckCircleFilled,
  CloseCircleFilled,
  FileTextOutlined,
  EditOutlined,
  CodeOutlined,
  SearchOutlined,
  GlobalOutlined,
  ToolOutlined,
  MessageOutlined,
  LoadingOutlined,
} from '@ant-design/icons';
import type { SubAgentActivityEntry, DispatchTask } from '../types';
import { normalizeMcpName } from '../utils/toolName';

const { Text } = Typography;

interface Props {
  task: DispatchTask;
  entries: SubAgentActivityEntry[];
}

/** Map a tool name to a short verb. */
function verbFor(toolName: string): string {
  if (toolName === 'Read') return 'Read';
  if (toolName === 'Write') return 'Write';
  if (toolName === 'Edit' || toolName === 'NotebookEdit') return 'Edit';
  if (toolName === 'Bash') return 'Bash';
  if (toolName === 'Glob') return 'Glob';
  if (toolName === 'Grep') return 'Grep';
  if (toolName === 'WebFetch') return 'Fetch';
  if (toolName === 'ToolSearch') return 'Search tools';
  if (toolName.startsWith('mcp__')) return normalizeMcpName(toolName).split('__').slice(-1)[0]?.replace(/_/g, ' ') ?? 'Tool';
  return toolName;
}

function iconFor(toolName: string) {
  if (toolName === 'Read') return <FileTextOutlined />;
  if (toolName === 'Write' || toolName === 'Edit' || toolName === 'NotebookEdit') return <EditOutlined />;
  if (toolName === 'Bash') return <CodeOutlined />;
  if (toolName === 'Glob' || toolName === 'Grep') return <SearchOutlined />;
  if (toolName === 'WebFetch') return <GlobalOutlined />;
  return <ToolOutlined />;
}

/** Build one-line summary from tool entries. */
function summarise(entries: SubAgentActivityEntry[]): string {
  const toolEntries = entries.filter(e => e.entryType === 'tool');
  if (toolEntries.length === 0) return 'Starting...';
  const counts = new Map<string, number>();
  for (const e of toolEntries) {
    const verb = verbFor(e.toolName ?? 'Tool');
    counts.set(verb, (counts.get(verb) ?? 0) + 1);
  }
  const parts: string[] = [];
  for (const [verb, n] of counts.entries()) {
    parts.push(n === 1 ? verb : `${verb} x${n}`);
  }
  return parts.join(', ');
}

/**
 * Renders a live activity feed for a dispatch sub-agent task.
 * Collapsed: one-line summary (like ToolGroupCard).
 * Expanded: full log of tool calls and messages.
 */
export function SubAgentActivityCard({ task, entries }: Props) {
  const { token } = theme.useToken();
  const [expanded, setExpanded] = useState(false);

  const isActive = task.status === 'processing';
  const isDone = task.status === 'done';
  const isError = task.status === 'error' || task.status === 'timeout';
  const summary = summarise(entries);
  const anyError = entries.some(e => e.ok === false);
  const personaLabel = task.personaName ?? task.agentId;

  return (
    <div
      style={{
        border: `1px solid ${isError ? token.colorErrorBorder : isActive ? token.colorPrimaryBorder : token.colorBorderSecondary}`,
        borderRadius: 8,
        background: isError ? token.colorErrorBg : isDone ? token.colorSuccessBg : token.colorBgContainer,
        overflow: 'hidden',
        marginBottom: 6,
      }}
    >
      {/* Header */}
      <button
        type="button"
        onClick={() => setExpanded(v => !v)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          width: '100%',
          padding: '8px 10px',
          background: 'transparent',
          border: 'none',
          cursor: 'pointer',
          textAlign: 'left',
        }}
      >
        {isActive ? (
          <LoadingOutlined style={{ fontSize: 12, color: token.colorPrimary }} />
        ) : isDone ? (
          <CheckCircleFilled style={{ fontSize: 12, color: token.colorSuccess }} />
        ) : isError ? (
          <CloseCircleFilled style={{ fontSize: 12, color: token.colorError }} />
        ) : (
          <span style={{ width: 12, height: 12, borderRadius: '50%', background: token.colorTextQuaternary, display: 'inline-block' }} />
        )}

        <Tag
          color="purple"
          style={{ margin: 0, fontSize: 10, lineHeight: '16px', padding: '0 4px' }}
        >
          {personaLabel}
        </Tag>

        <Text
          style={{
            fontSize: 12,
            color: anyError ? token.colorError : token.colorTextSecondary,
            flex: 1,
          }}
          ellipsis
        >
          {task.label}: {summary}
        </Text>

        <Text style={{ color: token.colorTextQuaternary, fontSize: 12, flexShrink: 0 }}>
          {entries.length > 0 && `${entries.filter(e => e.entryType === 'tool').length} tool${entries.filter(e => e.entryType === 'tool').length !== 1 ? 's' : ''}`}
          {' '}{expanded ? '▾' : '›'}
        </Text>
      </button>

      {/* Expanded log */}
      {expanded && (
        <div
          style={{
            padding: '0 10px 8px 30px',
            display: 'flex',
            flexDirection: 'column',
            gap: 4,
            maxHeight: 400,
            overflowY: 'auto',
            borderTop: `1px solid ${token.colorBorderSecondary}`,
          }}
        >
          {entries.map((entry, i) => (
            <ActivityRow key={`${entry.ts}-${i}`} entry={entry} />
          ))}
          {entries.length === 0 && (
            <Text type="secondary" style={{ fontSize: 11, padding: '4px 0' }}>
              Waiting for activity...
            </Text>
          )}
        </div>
      )}
    </div>
  );
}

function ActivityRow({ entry }: { entry: SubAgentActivityEntry }) {
  const { token } = theme.useToken();

  if (entry.entryType === 'message') {
    return (
      <div style={{ fontSize: 11, padding: '3px 0', color: token.colorText }}>
        <MessageOutlined style={{ fontSize: 10, marginRight: 6, color: token.colorTextSecondary }} />
        <Text style={{ fontSize: 11 }} ellipsis={{ tooltip: entry.text }}>
          {(entry.text ?? '').slice(0, 200)}
        </Text>
      </div>
    );
  }

  // Tool entry
  const toolName = entry.toolName ?? 'Tool';
  const isOk = entry.ok !== false;

  return (
    <div style={{ fontSize: 11, padding: '2px 0', display: 'flex', alignItems: 'center', gap: 5 }}>
      <span style={{ color: isOk ? token.colorTextSecondary : token.colorError, fontSize: 11 }}>
        {iconFor(toolName)}
      </span>
      <Tooltip title={toolName}>
        <Text strong style={{ fontSize: 11 }}>{verbFor(toolName)}</Text>
      </Tooltip>
      {entry.title && (
        <Text type="secondary" style={{ fontSize: 11, flex: 1 }} ellipsis={{ tooltip: entry.title }}>
          {entry.title}
        </Text>
      )}
      {entry.summary && (
        <Text type="secondary" style={{ fontSize: 10 }}>
          {entry.summary}
        </Text>
      )}
      {!isOk && <Tag color="error" style={{ fontSize: 9, margin: 0, padding: '0 3px', lineHeight: '14px' }}>err</Tag>}
    </div>
  );
}
