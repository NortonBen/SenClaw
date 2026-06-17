import { useState } from 'react';
import { Tag, theme, Typography, Tooltip } from 'antd';
import { CheckCircleFilled, CloseCircleFilled, FileTextOutlined, EditOutlined, CodeOutlined, SearchOutlined, GlobalOutlined, ToolOutlined } from '@ant-design/icons';
import type { ToolMessage } from '../types';
import { ToolDetail } from './tool';
import { normalizeMcpName } from '../utils/toolName';

const { Text } = Typography;

interface Props {
  /** Consecutive ToolMessages from the same agent turn. */
  messages: ToolMessage[];
}

/** Map a tool name to a human-readable verb used in the collapsed summary
 *  (claude-code uses "Read a file, edited a file, ran a command"). */
function verbFor(rawName: string): string {
  // Fold the full registered form and the stripped bridge form onto one key
  // (e.g. `mcp__senclaw-space__space_event_create` → `mcp__space__event_create`).
  const toolName = normalizeMcpName(rawName);
  if (toolName === 'Read' || toolName === 'mcp__code__read_file') return 'Read a file';
  if (toolName === 'Write' || toolName === 'mcp__code__write_file') return 'Created a file';
  if (toolName === 'Edit' || toolName === 'NotebookEdit' || toolName === 'mcp__code__edit_file') return 'Edited a file';
  if (toolName === 'Bash' || toolName === 'mcp__code__bash') return 'Ran a command';
  if (toolName === 'Glob') return 'Searched files';
  if (toolName === 'Grep') return 'Searched content';
  if (toolName === 'WebFetch') return 'Fetched a URL';
  if (toolName === 'ToolSearch') return 'Discovered a tool';
  if (toolName === 'Skill') return 'Invoked a skill';
  if (toolName === 'Task') return 'Spawned a subagent';
  if (toolName === 'EnterPlanMode') return 'Entered Plan mode';
  if (toolName === 'ExitPlanMode') return 'Requested plan approval';
  if (toolName.startsWith('mcp__browser__')) return 'Browser action';
  if (toolName.startsWith('mcp__memory__')) return 'Memory lookup';
  if (toolName.startsWith('mcp__wiki__')) return 'Wiki action';
  if (toolName.startsWith('mcp__schedule__') || toolName.startsWith('mcp__space__recurring_')) return 'Scheduled task';
  if (toolName.startsWith('mcp__space__event_')) return 'Calendar action';
  if (toolName.startsWith('mcp__space__note_')) return 'Note action';
  if (toolName.startsWith('mcp__space__')) return 'Space action';
  if (toolName.startsWith('mcp__')) return toolName.split('__').slice(-1)[0]?.replace(/_/g, ' ') ?? 'Tool';
  return toolName;
}

function iconFor(rawName: string) {
  const toolName = normalizeMcpName(rawName);
  if (toolName === 'Read' || toolName.endsWith('read_file')) return <FileTextOutlined />;
  if (toolName === 'Write' || toolName === 'Edit' || toolName === 'NotebookEdit' || toolName.endsWith('write_file') || toolName.endsWith('edit_file')) return <EditOutlined />;
  if (toolName === 'Bash' || toolName === 'bash') return <CodeOutlined />;
  if (toolName === 'Glob' || toolName === 'Grep') return <SearchOutlined />;
  if (toolName === 'WebFetch' || toolName.startsWith('mcp__browser__')) return <GlobalOutlined />;
  return <ToolOutlined />;
}

/**
 * Build the collapsed one-line summary used as the panel header.
 *
 * Groups verbs and pluralises: "Read 3 files, edited 2 files, ran 1 command".
 * Returns lowercase first word for natural sentence flow.
 */
function summariseGroup(messages: ToolMessage[]): string {
  const counts = new Map<string, number>();
  for (const m of messages) {
    const verb = verbFor(m.toolName);
    counts.set(verb, (counts.get(verb) ?? 0) + 1);
  }
  const parts: string[] = [];
  for (const [verb, n] of counts.entries()) {
    if (n === 1) {
      parts.push(verb);
    } else {
      // Pluralise the verb's noun ("Read a file" -> "Read 3 files").
      const plural = verb.replace(/\ba file\b/, `${n} files`)
                        .replace(/\ba command\b/, `${n} commands`)
                        .replace(/\ba URL\b/, `${n} URLs`)
                        .replace(/\ba subagent\b/, `${n} subagents`)
                        .replace(/\ba skill\b/, `${n} skills`)
                        .replace(/\ba tool\b/, `${n} tools`);
      parts.push(plural === verb ? `${verb} ×${n}` : plural);
    }
  }
  // Lowercase the first letter for sentence-like flow.
  const out = parts.join(', ');
  return out.charAt(0).toLowerCase() + out.slice(1);
}

/**
 * Claude-code style tool-call card. Collapsed: one-line aggregate summary
 * with a chevron. Expanded: list of individual tool rows (icon · title ·
 * summary · status), each with their own raw payload behind a sub-toggle.
 */
export function ToolGroupCard({ messages }: Props) {
  const { token } = theme.useToken();
  const [expanded, setExpanded] = useState(false);

  if (messages.length === 0) return null;
  const summary = summariseGroup(messages);
  const anyError = messages.some(m => !m.ok);

  return (
    <div
      style={{
        margin: '4px 0',
        padding: 0,
        background: 'transparent',
      }}
    >
      <button
        type="button"
        onClick={() => setExpanded(v => !v)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          background: 'transparent',
          border: 'none',
          padding: '4px 0',
          cursor: 'pointer',
          color: anyError ? token.colorError : token.colorTextSecondary,
          fontSize: 13,
          textAlign: 'left',
          width: '100%',
        }}
      >
        {anyError
          ? <CloseCircleFilled style={{ fontSize: 11 }} />
          : <CheckCircleFilled style={{ color: token.colorSuccess, fontSize: 11 }} />}
        <Text style={{ color: 'inherit', fontSize: 13 }}>{summary}</Text>
        <Text style={{ color: token.colorTextQuaternary, fontSize: 13 }}>
          {expanded ? '▾' : '›'}
        </Text>
      </button>

      {expanded && (
        <div
          style={{
            marginTop: 6,
            marginLeft: 18,
            paddingLeft: 12,
            borderLeft: `2px solid ${token.colorBorderSecondary}`,
            display: 'flex',
            flexDirection: 'column',
            gap: 6,
          }}
        >
          {messages.map(m => (
            <ToolRow key={m.id} message={m} />
          ))}
        </div>
      )}
    </div>
  );
}

/** Tools whose detail view is shown inline by default (no extra click).
 *  Mirrors claude-code's behaviour of always rendering the diff/snippet
 *  immediately under file-mutation tool calls. */
function shouldAutoExpand(toolName: string): boolean {
  return (
    toolName === 'Edit' ||
    toolName === 'Write' ||
    toolName === 'NotebookEdit' ||
    toolName.endsWith('edit_file') ||
    toolName.endsWith('write_file')
  );
}

function ToolRow({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const [showRaw, setShowRaw] = useState(() => shouldAutoExpand(message.toolName));
  const verb = verbFor(message.toolName);
  return (
    <div style={{ fontSize: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
        <span style={{ color: message.ok ? token.colorTextSecondary : token.colorError }}>
          {iconFor(message.toolName)}
        </span>
        <Tooltip title={message.toolName}>
          <Text strong style={{ fontSize: 12 }}>{verb}</Text>
        </Tooltip>
        {message.title && (
          <Text type="secondary" style={{ fontSize: 12 }} ellipsis={{ tooltip: message.title }}>
            {message.title}
          </Text>
        )}
        {!message.ok && <Tag color="error">error</Tag>}
        <button
          type="button"
          onClick={() => setShowRaw(v => !v)}
          style={{
            marginLeft: 'auto',
            border: 'none',
            background: 'transparent',
            color: token.colorTextQuaternary,
            cursor: 'pointer',
            fontSize: 11,
          }}
        >
          {showRaw ? 'hide' : 'detail'}
        </button>
      </div>
      {message.summary && (
        <div style={{ marginLeft: 22, color: token.colorTextTertiary, fontSize: 11 }}>
          {message.summary}
        </div>
      )}
      {showRaw && (
        <div style={{ marginLeft: 22, marginTop: 4 }}>
          <ToolDetail message={message} />
        </div>
      )}
    </div>
  );
}
