import { useCallback, useEffect, useRef, useState } from 'react';
import { Empty, Tag, Typography, theme } from 'antd';
import {
  BulbOutlined, DownOutlined, MessageOutlined, RightOutlined, SyncOutlined,
  ToolOutlined, WarningOutlined,
} from '@ant-design/icons';
import { apiFetch } from './workflowShared';

const { Text } = Typography;

export interface ActivityEntry {
  ts: string;
  stepId: string;
  kind: 'think' | 'text' | 'tool' | 'tool_error' | 'message' | 'status';
  text: string;
}

const KIND_ICON: Record<ActivityEntry['kind'], React.ReactNode> = {
  think: <BulbOutlined />,
  text: <MessageOutlined />,
  message: <MessageOutlined />,
  tool: <ToolOutlined />,
  tool_error: <WarningOutlined />,
  status: <SyncOutlined />,
};

/** One-line summary shown while an entry is collapsed. */
function entryTitle(e: ActivityEntry): string {
  const firstLine = e.text.split('\n', 1)[0];
  switch (e.kind) {
    case 'tool':
    case 'tool_error': {
      // Backend formats "toolName — payload…" — header shows just the name.
      const sep = firstLine.indexOf(' — ');
      return sep > 0 ? firstLine.slice(0, sep) : firstLine.slice(0, 60);
    }
    case 'think':
      return `Suy nghĩ… (${e.text.length.toLocaleString('vi-VN')} ký tự)`;
    case 'text':
      return `Đang viết… (${e.text.length.toLocaleString('vi-VN')} ký tự)`;
    case 'message':
      return firstLine.length > 64 ? `${firstLine.slice(0, 64)}…` : firstLine;
    case 'status':
      return firstLine;
  }
}

/** Live agent activity of a run. Entries render collapsed (one summary line,
 *  like the chat box's tool bubbles) and expand on click. */
export function WorkflowActivityFeed({ runId, active }: { runId: string; active: boolean }) {
  const { token } = theme.useToken();
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const boxRef = useRef<HTMLDivElement>(null);
  const stickBottom = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const r = await apiFetch<{ entries: ActivityEntry[] }>(
        `/api/workflows/runs/${encodeURIComponent(runId)}/activity`);
      setEntries(r.entries ?? []);
    } catch { /* transient */ }
  }, [runId]);

  useEffect(() => { refresh(); }, [refresh]);
  useEffect(() => {
    if (!active) return;
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [active, refresh]);

  // Auto-follow the tail unless the user scrolled up.
  useEffect(() => {
    const el = boxRef.current;
    if (el && stickBottom.current) el.scrollTop = el.scrollHeight;
  }, [entries]);

  const toggle = (i: number) => setExpanded((prev) => {
    const next = new Set(prev);
    if (next.has(i)) next.delete(i); else next.add(i);
    return next;
  });

  const kindColor = (k: ActivityEntry['kind']) =>
    k === 'tool' ? token.colorPrimary
      : k === 'tool_error' ? token.colorError
      : k === 'status' ? token.colorWarning
      : token.colorTextTertiary;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <div style={{
        padding: '8px 12px', borderBottom: `1px solid ${token.colorBorderSecondary}`,
        display: 'flex', alignItems: 'center', gap: 6,
      }}>
        {active && <SyncOutlined spin style={{ color: token.colorPrimary, fontSize: 12 }} />}
        <Text strong style={{ fontSize: 12, letterSpacing: 0.5 }}>HOẠT ĐỘNG</Text>
        <Text type="secondary" style={{ fontSize: 11 }}>({entries.length})</Text>
      </div>
      <div
        ref={boxRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          stickBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
        }}
        style={{ flex: 1, overflowY: 'auto', padding: '8px 10px' }}
      >
        {entries.length === 0 ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} style={{ marginTop: 32 }}
            description={<Text type="secondary" style={{ fontSize: 12 }}>
              {active ? 'Đang chờ agent bắt đầu…' : 'Không có hoạt động ghi lại'}
            </Text>} />
        ) : entries.map((e, i) => {
          const open = expanded.has(i);
          // Status lines are short — always plain, no collapse chrome.
          if (e.kind === 'status') {
            return (
              <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 6, margin: '6px 0' }}>
                <span style={{ color: kindColor(e.kind), fontSize: 11 }}>{KIND_ICON[e.kind]}</span>
                <Text type="warning" style={{ fontSize: 11, flex: 1 }}>{e.text}</Text>
              </div>
            );
          }
          return (
            <div key={i} style={{
              marginBottom: 6, borderRadius: 8,
              border: `1px solid ${token.colorBorderSecondary}`,
              background: token.colorBgContainer,
              overflow: 'hidden',
            }}>
              {/* Collapsed header — click to expand (chat-box style). */}
              <div
                onClick={() => toggle(i)}
                style={{
                  display: 'flex', alignItems: 'center', gap: 6,
                  padding: '5px 8px', cursor: 'pointer', userSelect: 'none',
                }}
              >
                {open
                  ? <DownOutlined style={{ fontSize: 8, color: token.colorTextTertiary }} />
                  : <RightOutlined style={{ fontSize: 8, color: token.colorTextTertiary }} />}
                <span style={{ color: kindColor(e.kind), fontSize: 11 }}>{KIND_ICON[e.kind]}</span>
                <Text style={{
                  fontSize: 11.5, flex: 1, minWidth: 0,
                  fontStyle: e.kind === 'think' ? 'italic' : undefined,
                  color: e.kind === 'think' ? token.colorTextTertiary : token.colorText,
                }} ellipsis>
                  {entryTitle(e)}
                </Text>
                <Tag style={{ fontSize: 9, lineHeight: '15px', margin: 0, padding: '0 4px' }}>
                  {e.stepId}
                </Tag>
                <Text type="secondary" style={{ fontSize: 10 }}>
                  {new Date(e.ts).toLocaleTimeString('vi-VN')}
                </Text>
              </div>
              {open && (
                <div style={{
                  padding: '6px 10px 8px 24px',
                  borderTop: `1px solid ${token.colorBorderSecondary}`,
                  fontSize: 11.5,
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-word',
                  maxHeight: 320,
                  overflowY: 'auto',
                  fontFamily: e.kind === 'tool' || e.kind === 'tool_error' ? 'monospace' : undefined,
                  fontStyle: e.kind === 'think' ? 'italic' : undefined,
                  color: e.kind === 'think' ? token.colorTextTertiary : token.colorTextSecondary,
                  background: token.colorFillQuaternary,
                }}>
                  {e.text}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
