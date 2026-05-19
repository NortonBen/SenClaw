import { useEffect, useState } from 'react';
import { Collapse, Empty, List, Tag, Typography, theme } from 'antd';
import type { PlanFull, PlanSummary } from '../hooks/useWebSocket';

const { Text } = Typography;

interface Props {
  /** Active chat JID — required to scope the list. */
  groupJid: string;
  /** Plan summaries keyed by JID, from `useWebSocket`. */
  plansByJid: Record<string, PlanSummary[]>;
  /** Full plans keyed by id, from `useWebSocket`. */
  planById: Record<string, PlanFull>;
  /** Trigger the list fetch (sends `plan:list` over WS). */
  requestPlanList: (jid: string) => void;
  /** Trigger a full-plan fetch by id (sends `plan:get`). */
  requestPlan: (id: string) => void;
}

/**
 * Side panel listing past plans for the active group. Persistent — replays
 * across daemon restart since plans live in SQLite (see `db::plans`).
 * Pending plans (awaiting user response in PlanExitDialog) are tagged so
 * the user can spot them without opening the dialog modal.
 */
export function PlanHistoryPanel({
  groupJid,
  plansByJid,
  planById,
  requestPlanList,
  requestPlan,
}: Props) {
  const { token } = theme.useToken();
  const [expandedId, setExpandedId] = useState<string | null>(null);

  // Fetch the list whenever the active group changes.
  useEffect(() => {
    if (groupJid) requestPlanList(groupJid);
  }, [groupJid, requestPlanList]);

  const plans = plansByJid[groupJid] ?? [];

  if (plans.length === 0) {
    return (
      <div style={{ padding: 16 }}>
        <Empty description="No plans yet" image={Empty.PRESENTED_IMAGE_SIMPLE} />
      </div>
    );
  }

  const items = plans.map(p => ({
    key: p.id,
    label: (
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, width: '100%' }}>
        <Text strong ellipsis style={{ fontSize: 13, flex: 1 }}>
          {p.title || '(untitled plan)'}
        </Text>
        <Tag color={tagColorFor(p.approval)} style={{ marginInlineEnd: 0 }}>
          {p.approval}
        </Tag>
      </div>
    ),
    children: (
      <PlanContent
        plan={p}
        full={planById[p.id]}
        onRequest={() => requestPlan(p.id)}
      />
    ),
    style: {
      background: token.colorBgContainer,
      border: `1px solid ${token.colorBorderSecondary}`,
      borderRadius: 6,
      marginBottom: 6,
    },
  }));

  return (
    <div style={{ padding: 8 }}>
      <Collapse
        bordered={false}
        accordion
        activeKey={expandedId ?? undefined}
        onChange={(k) => {
          const next = (Array.isArray(k) ? k[0] : k) as string | undefined;
          setExpandedId(next ?? null);
          if (next && !planById[next]) requestPlan(next);
        }}
        items={items}
        style={{ background: 'transparent' }}
      />
      <List
        size="small"
        style={{ marginTop: 8 }}
        dataSource={plans}
        renderItem={(p) => (
          <List.Item style={{ padding: '2px 0', border: 'none' }}>
            <Text type="secondary" style={{ fontSize: 10 }}>
              {new Date(p.createdAt).toLocaleString()} · {p.agentId}
            </Text>
          </List.Item>
        )}
      />
    </div>
  );
}

function PlanContent({
  plan,
  full,
  onRequest,
}: {
  plan: PlanSummary;
  full: PlanFull | undefined;
  onRequest: () => void;
}) {
  const { token } = theme.useToken();
  useEffect(() => {
    if (!full) onRequest();
    // intentional: fetch once on first expand
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plan.id]);

  if (!full) {
    return <Text type="secondary" style={{ fontSize: 11 }}>Loading…</Text>;
  }

  return (
    <div>
      <Text type="secondary" style={{ fontSize: 10 }}>{plan.filePath}</Text>
      <pre
        style={{
          marginTop: 6,
          padding: 8,
          background: token.colorBgLayout,
          border: `1px solid ${token.colorBorderSecondary}`,
          borderRadius: 4,
          fontSize: 11,
          maxHeight: 320,
          overflow: 'auto',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
      >
        {full.contentMd}
      </pre>
    </div>
  );
}

function tagColorFor(approval: string): string {
  switch (approval) {
    case 'startEditing': return 'success';
    case 'clearContextAndStart': return 'processing';
    case 'pending': return 'warning';
    default: return 'default';
  }
}
