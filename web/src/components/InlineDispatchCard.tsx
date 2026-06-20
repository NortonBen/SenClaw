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
              the SVG tree is empty (e.g. on first paint, or for done DAGs). */}
          <div style={{ marginTop: 8, display: 'flex', flexDirection: 'column', gap: 4 }}>
            {parent.tasks.map(t => (
              <div
                key={t.label}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 8,
                  fontSize: 12,
                  padding: '4px 6px',
                  borderRadius: 6,
                  background: t.status === 'processing' ? token.colorPrimaryBg : 'transparent',
                }}
              >
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
              </div>
            ))}
          </div>

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
