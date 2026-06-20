import { useState } from 'react';
import { theme, Typography, Tag, Button } from 'antd';
import {
  CheckCircleFilled,
  CloseCircleFilled,
  LoadingOutlined,
  ClockCircleOutlined,
  DownOutlined,
  RightOutlined,
} from '@ant-design/icons';
import type { DispatchParent, DispatchTask, SubAgentActivityEntry } from '../types';
import { DispatchTree } from './DispatchTree';

const { Text } = Typography;

interface InlineDispatchCardProps {
  parent: DispatchParent;
  /** Recent activity events keyed by task label, if any. */
  activity?: Record<string, SubAgentActivityEntry[]>;
}

function statusBadge(status: DispatchParent['status']) {
  if (status === 'active')
    return <Tag icon={<LoadingOutlined spin />} color="processing">running</Tag>;
  if (status === 'queued')
    return <Tag icon={<ClockCircleOutlined />} color="default">queued</Tag>;
  return <Tag icon={<CheckCircleFilled />} color="success">done</Tag>;
}

/** Hour-minute timestamp formatter matching the rest of the chat surfaces. */
function formatTime(iso?: string | null): string {
  if (!iso) return '';
  try {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  } catch {
    return '';
  }
}

function taskStatusIcon(t: DispatchTask) {
  if (t.status === 'done') return <CheckCircleFilled style={{ color: '#10b981' }} />;
  if (t.status === 'error' || t.status === 'timeout')
    return <CloseCircleFilled style={{ color: '#ef4444' }} />;
  if (t.status === 'processing') return <LoadingOutlined spin style={{ color: '#5BBFE8' }} />;
  return <ClockCircleOutlined style={{ color: '#9ca3af' }} />;
}

/**
 * Compact inline card that surfaces a single DispatchParent (DAG) directly
 * inside the chat scroll, claude-code-style. Replaces the side-panel-only
 * surfacing of orchestration progress so the user sees the DAG growing
 * alongside their own messages without flipping panels.
 */
export function InlineDispatchCard({ parent, activity }: InlineDispatchCardProps) {
  const { token } = theme.useToken();
  const [expanded, setExpanded] = useState(parent.status !== 'done');
  /** Label of the task whose details panel is open. `null` = none. */
  const [openTaskLabel, setOpenTaskLabel] = useState<string | null>(null);
  const openTask = openTaskLabel
    ? parent.tasks.find(t => t.label === openTaskLabel) ?? null
    : null;
  const toggleTask = (label: string) => {
    setOpenTaskLabel(prev => (prev === label ? null : label));
  };
  /** Truncate result/prompt to keep the inline card compact. */
  const previewText = (s: string | null | undefined, limit = 1200): string => {
    const t = (s ?? '').trim();
    if (t.length <= limit) return t;
    return `${t.slice(0, limit)}\n…(truncated, ${t.length - limit} more chars)`;
  };
  /** Format the start/end of a task as a short duration line. */
  const taskDuration = (t: DispatchTask): string | null => {
    if (!t.startedAt) return null;
    const startMs = new Date(t.startedAt).getTime();
    const endMs = t.completedAt
      ? new Date(t.completedAt).getTime()
      : Date.now();
    const sec = Math.max(0, Math.round((endMs - startMs) / 1000));
    if (sec < 60) return `${sec}s`;
    return `${Math.floor(sec / 60)}m ${sec % 60}s`;
  };

  const total = parent.tasks.length;
  const done = parent.tasks.filter(t => t.status === 'done').length;
  const failed = parent.tasks.filter(t => t.status === 'error' || t.status === 'timeout').length;

  const headerBg =
    parent.status === 'active'
      ? `linear-gradient(90deg, ${token.colorPrimaryBg} 0%, ${token.colorBgContainer} 80%)`
      : token.colorBgContainer;

  // Pick the most recent activity entry across tasks for a one-line summary
  const recentActivityLine = (() => {
    if (!activity) return null;
    const all = parent.tasks.flatMap(t => (activity[t.label] ?? []).map(e => ({ task: t.label, ...e })));
    if (!all.length) return null;
    const sorted = all.sort((a, b) => (a.ts < b.ts ? 1 : -1));
    const e = sorted[0];
    const label =
      e.entryType === 'tool'
        ? `${e.task}: ${e.toolName ?? 'tool'}`
        : e.entryType === 'message'
        ? `${e.task}: ${(e.text ?? '').slice(0, 60)}`
        : `${e.task}: thinking…`;
    return label;
  })();

  return (
    <div
      role="region"
      aria-label={`DAG orchestration: ${parent.goal}`}
      style={{
        marginBlock: 12,
        borderRadius: 12,
        border: `1px solid ${token.colorBorderSecondary}`,
        background: token.colorBgContainer,
        overflow: 'hidden',
        boxShadow: '0 1px 3px rgba(0,0,0,0.04)',
      }}
    >
      {/* Header row — always visible */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          padding: '8px 12px',
          background: headerBg,
          borderBottom: expanded ? `1px solid ${token.colorBorderSecondary}` : 'none',
          cursor: 'pointer',
        }}
        onClick={() => setExpanded(e => !e)}
      >
        <Button
          type="text"
          size="small"
          icon={expanded ? <DownOutlined /> : <RightOutlined />}
          aria-label={expanded ? 'Collapse' : 'Expand'}
          onClick={e => { e.stopPropagation(); setExpanded(v => !v); }}
          style={{ padding: 0, minWidth: 0 }}
        />
        <Text style={{ fontSize: 11, color: token.colorTextSecondary, fontFamily: 'ui-monospace, monospace' }}>
          {parent.id}
        </Text>
        {statusBadge(parent.status)}
        <Text strong style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={parent.goal}>
          {parent.goal}
        </Text>
        <Text style={{ fontSize: 12, color: token.colorTextSecondary }}>
          {done}/{total}
          {failed > 0 && <span style={{ color: '#ef4444', marginLeft: 6 }}>· {failed} failed</span>}
        </Text>
        {/* Timestamp on the right — matches the per-message time displayed
            by MessageBubble / TextMessage so the DAG card slots cleanly into
            the chronological timeline of the conversation. Shows creation
            time, and (when done) the completion time too. */}
        <Text style={{ fontSize: 11, color: token.colorTextTertiary, whiteSpace: 'nowrap', marginLeft: 4 }}>
          {formatTime(parent.createdAt)}
          {parent.status === 'done' && parent.completedAt
            ? ` → ${formatTime(parent.completedAt)}`
            : ''}
        </Text>
      </div>

      {/* Body — DAG tree + per-task status list */}
      {expanded && (
        <div style={{ padding: '8px 12px' }}>
          {/* Reuse the existing DAG visualiser. Wrap in a single-parent array
              so it renders just this one DAG. */}
          <div style={{ marginTop: 4 }}>
            <DispatchTree parents={[parent]} />
          </div>

          {/* Per-task list — accessible fallback that always renders even if
              the SVG tree is empty (e.g. on first paint, or for done DAGs).
              Click a row to open its details panel inline (prompt, agent,
              duration, result/error). Mirrors the AgentConsole side panel's
              task-detail behaviour but stays inside the chat scroll. */}
          <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 4 }}>
            {parent.tasks.map(t => {
              const isOpen = openTaskLabel === t.label;
              return (
                <div
                  key={t.label}
                  role="button"
                  aria-expanded={isOpen}
                  aria-controls={`task-detail-${parent.id}-${t.label}`}
                  tabIndex={0}
                  onClick={() => toggleTask(t.label)}
                  onKeyDown={e => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      toggleTask(t.label);
                    }
                  }}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    fontSize: 12,
                    padding: '4px 6px',
                    borderRadius: 6,
                    cursor: 'pointer',
                    background: isOpen
                      ? token.colorPrimaryBgHover
                      : t.status === 'processing'
                        ? token.colorPrimaryBg
                        : 'transparent',
                    border: isOpen
                      ? `1px solid ${token.colorPrimaryBorder}`
                      : `1px solid transparent`,
                  }}
                  title={isOpen ? 'Click to collapse' : 'Click for task details'}
                >
                  {isOpen ? (
                    <DownOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} />
                  ) : (
                    <RightOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} />
                  )}
                  {taskStatusIcon(t)}
                  <Text strong style={{ fontSize: 12 }}>{t.label}</Text>
                  <Text style={{ fontSize: 11, color: token.colorTextSecondary }}>
                    {t.personaName ?? t.agentId}
                  </Text>
                  {t.dependsOn.length > 0 && (
                    <Text style={{ fontSize: 10, color: token.colorTextTertiary }}>
                      ← {t.dependsOn.join(', ')}
                    </Text>
                  )}
                  <span style={{ flex: 1 }} />
                  {taskDuration(t) && (
                    <Text style={{ fontSize: 10, color: token.colorTextTertiary }}>
                      {taskDuration(t)}
                    </Text>
                  )}
                </div>
              );
            })}
          </div>

          {/* Task-details panel — inline expanded view of the selected task.
              Renders right under the task list so the user doesn't have to
              hunt in the side panel. Shows prompt, status, agent, depends-on,
              duration, and result/error text. */}
          {openTask && (
            <div
              id={`task-detail-${parent.id}-${openTask.label}`}
              role="region"
              aria-label={`Task details: ${openTask.label}`}
              style={{
                marginTop: 6,
                padding: '10px 12px',
                background: token.colorBgLayout,
                border: `1px solid ${token.colorBorderSecondary}`,
                borderRadius: 8,
                fontSize: 12,
                display: 'flex',
                flexDirection: 'column',
                gap: 8,
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                {taskStatusIcon(openTask)}
                <Text strong>{openTask.label}</Text>
                <Tag color={openTask.status === 'done' ? 'success'
                  : openTask.status === 'processing' ? 'processing'
                  : openTask.status === 'error' || openTask.status === 'timeout' ? 'error'
                  : 'default'}>
                  {openTask.status}
                </Tag>
                <Text style={{ fontSize: 11, color: token.colorTextSecondary }}>
                  {openTask.personaName ?? openTask.agentId}
                </Text>
                {taskDuration(openTask) && (
                  <Text style={{ fontSize: 11, color: token.colorTextTertiary }}>
                    · {taskDuration(openTask)}
                  </Text>
                )}
                <span style={{ flex: 1 }} />
                <Button
                  size="small"
                  type="text"
                  aria-label="Close task details"
                  onClick={e => { e.stopPropagation(); setOpenTaskLabel(null); }}
                >
                  ×
                </Button>
              </div>

              {openTask.dependsOn.length > 0 && (
                <div style={{ fontSize: 11, color: token.colorTextSecondary }}>
                  <Text strong style={{ fontSize: 11 }}>Depends on: </Text>
                  {openTask.dependsOn.join(', ')}
                </div>
              )}

              <div>
                <Text strong style={{ fontSize: 11, color: token.colorTextSecondary }}>Prompt</Text>
                <pre
                  style={{
                    margin: '4px 0 0',
                    padding: 8,
                    background: token.colorBgContainer,
                    border: `1px solid ${token.colorBorderSecondary}`,
                    borderRadius: 6,
                    fontSize: 11,
                    lineHeight: 1.5,
                    whiteSpace: 'pre-wrap',
                    wordBreak: 'break-word',
                    maxHeight: 200,
                    overflowY: 'auto',
                    fontFamily: 'inherit',
                  }}
                >
                  {previewText(openTask.prompt, 800)}
                </pre>
              </div>

              {(openTask.result || openTask.status === 'error' || openTask.status === 'timeout') && (
                <div>
                  <Text strong style={{
                    fontSize: 11,
                    color: openTask.status === 'error' || openTask.status === 'timeout'
                      ? '#ef4444'
                      : token.colorTextSecondary,
                  }}>
                    {openTask.status === 'error' ? 'Error' :
                     openTask.status === 'timeout' ? 'Timeout' : 'Result'}
                  </Text>
                  <pre
                    style={{
                      margin: '4px 0 0',
                      padding: 8,
                      background: token.colorBgContainer,
                      border: `1px solid ${
                        openTask.status === 'error' || openTask.status === 'timeout'
                          ? '#ef444444'
                          : token.colorBorderSecondary
                      }`,
                      borderRadius: 6,
                      fontSize: 11,
                      lineHeight: 1.5,
                      whiteSpace: 'pre-wrap',
                      wordBreak: 'break-word',
                      maxHeight: 300,
                      overflowY: 'auto',
                      fontFamily: 'ui-monospace, monospace',
                    }}
                  >
                    {previewText(openTask.result, 1500) || '(no output)'}
                  </pre>
                </div>
              )}

              {/* Sub-agent activity for this task — the tool calls / thinks /
                  messages emitted while the worker was running. Compact list
                  ordered chronologically so the user can scroll the worker's
                  step-by-step trace. */}
              {activity?.[openTask.label]?.length ? (
                <div>
                  <Text strong style={{ fontSize: 11, color: token.colorTextSecondary }}>
                    Activity ({activity[openTask.label].length})
                  </Text>
                  <div
                    style={{
                      marginTop: 4,
                      maxHeight: 180,
                      overflowY: 'auto',
                      border: `1px solid ${token.colorBorderSecondary}`,
                      borderRadius: 6,
                      background: token.colorBgContainer,
                    }}
                  >
                    {activity[openTask.label].map((e, i) => (
                      <div
                        key={i}
                        style={{
                          padding: '4px 8px',
                          borderBottom: i < activity[openTask.label].length - 1
                            ? `1px solid ${token.colorBorderSecondary}`
                            : 'none',
                          fontSize: 11,
                          display: 'flex',
                          gap: 6,
                          alignItems: 'flex-start',
                        }}
                      >
                        <span style={{
                          fontSize: 10,
                          color: token.colorTextTertiary,
                          fontFamily: 'ui-monospace, monospace',
                          flexShrink: 0,
                        }}>
                          {formatTime(e.ts)}
                        </span>
                        <span style={{
                          fontSize: 10,
                          padding: '0 4px',
                          borderRadius: 3,
                          background: e.entryType === 'tool'
                            ? '#dbeafe'
                            : e.entryType === 'think'
                              ? '#f3e8ff'
                              : '#dcfce7',
                          color: '#374151',
                          flexShrink: 0,
                        }}>
                          {e.entryType}
                        </span>
                        <span style={{ flex: 1, minWidth: 0, wordBreak: 'break-word' }}>
                          {e.toolName ? <code>{e.toolName}</code> : ''}
                          {e.title ? <span style={{ marginLeft: 4 }}>{e.title}</span> : ''}
                          {e.summary ? <span style={{ marginLeft: 4, color: token.colorTextSecondary }}>{e.summary}</span> : ''}
                          {e.text ? <span style={{ marginLeft: 4 }}>{previewText(e.text, 120)}</span> : ''}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              ) : null}
            </div>
          )}

          {recentActivityLine && (
            <div
              style={{
                marginTop: 8,
                paddingTop: 8,
                borderTop: `1px dashed ${token.colorBorderSecondary}`,
                fontSize: 11,
                color: token.colorTextSecondary,
                display: 'flex',
                alignItems: 'center',
                gap: 6,
              }}
            >
              <LoadingOutlined spin style={{ fontSize: 10 }} />
              <Text style={{ fontSize: 11, color: token.colorTextSecondary }}>{recentActivityLine}</Text>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
