import { Alert, List, Progress, Space, Tag, Tooltip, Typography } from 'antd'
import { WarningOutlined } from '@ant-design/icons'
import type { Claim, Contradiction, Evidence } from './api'
import { tierColor, tierLabelVi } from './theme'

const { Text } = Typography

/**
 * Claims with provenance chips. Two invariants: a `disputed` claim must never
 * read as settled, and "confidence" must never read as "probability of being
 * true" — the note stays inline, not hidden in a tooltip.
 */
export default function Claims({
  claims,
  contradictions,
  evidence,
  note,
}: {
  claims: Claim[]
  contradictions: Contradiction[]
  evidence: Evidence[]
  note?: string
}) {
  const byId = new Map(evidence.map((e, i) => [e.id, { e, n: i + 1 }]))
  const claimText = (id: string) => claims.find((c) => c.id === id)?.text ?? id

  // History rows store only `tier`; normalise so the same component renders
  // both live results and persisted ones.
  const norm = (c: Claim) => ({
    label: c.tier_label ?? tierLabelVi(c.tier),
    supports: c.supports ?? [],
    refutes: c.refutes ?? [],
    dropped: c.dropped_citations ?? [],
  })

  const cite = (ids: string[]) => (
    <>
      {ids
        .map((id) => byId.get(id))
        .filter((x): x is { e: Evidence; n: number } => !!x)
        .map(({ e, n }) => (
          <Tooltip key={e.id} title={`${e.title}${e.domain ? ` · ${e.domain}` : ''}`}>
            <a
              href={e.url ?? undefined}
              target="_blank"
              rel="noreferrer"
              style={{ fontWeight: 600, marginRight: 2 }}
            >
              [{n}]
            </a>
          </Tooltip>
        ))}
    </>
  )

  return (
    <div>
      {contradictions.length > 0 && (
        // Contradictions come FIRST and are never resolved by picking a side.
        <Alert
          type="warning"
          showIcon
          icon={<WarningOutlined />}
          style={{ marginBottom: 14 }}
          message="Các nguồn mâu thuẫn nhau"
          description={
            <Space direction="vertical" size={8} style={{ width: '100%' }}>
              {contradictions.map((c) => (
                <div key={c.id}>
                  <div>{c.summary}</div>
                  <Text type="secondary" style={{ fontSize: 12.5 }}>
                    · {claimText(c.claim_a)}
                    <br />· {claimText(c.claim_b)}
                  </Text>
                </div>
              ))}
            </Space>
          }
        />
      )}

      <List
        size="small"
        dataSource={claims}
        rowKey={(c) => c.id}
        renderItem={(c) => {
          const nc = norm(c)
          return (
          <List.Item style={{ display: 'block', paddingInline: 0 }}>
            <Space wrap size={8} style={{ marginBottom: 4 }}>
              <Tag color={tierColor(c.tier)} style={{ marginInlineEnd: 0 }}>
                {nc.label}
              </Tag>
              {c.high_stakes && (
                <Tooltip title="Sai ở loại khẳng định này thì tốn kém — số liệu, pháp lý, y tế, tài chính hoặc quy kết phát ngôn">
                  <Tag color="volcano" style={{ marginInlineEnd: 0 }}>
                    hệ trọng
                  </Tag>
                </Tooltip>
              )}
              <Text type="secondary" style={{ fontSize: 12.5 }}>
                {c.independent_count} nguồn độc lập
              </Text>
              <Progress
                percent={Math.round(c.agreement * 100)}
                size="small"
                style={{ width: 90, marginBottom: 0 }}
                strokeColor={ACCENT_BY_TIER[c.tier]}
                format={(p) => `${p}%`}
              />
            </Space>
            <div style={{ fontSize: 14.5, marginBottom: 5 }}>{c.text}</div>
            <Text type="secondary" style={{ fontSize: 12.5 }}>
              {nc.supports.length > 0 && <span>ủng hộ {cite(nc.supports)}</span>}
              {nc.refutes.length > 0 && <span> · phản bác {cite(nc.refutes)}</span>}
              {nc.dropped.length > 0 && (
                <Tooltip title={nc.dropped.join(', ')}>
                  <Text type="danger" style={{ fontSize: 12.5 }}>
                    {' '}
                    · {nc.dropped.length} trích dẫn không tồn tại đã bị bỏ
                  </Text>
                </Tooltip>
              )}
            </Text>
          </List.Item>
          )
        }}
      />

      {note && (
        <Text type="secondary" style={{ fontSize: 12.5, display: 'block', marginTop: 10 }}>
          {note}
        </Text>
      )}
    </div>
  )
}

const ACCENT_BY_TIER: Record<string, string> = {
  verified: '#34c759',
  supported: '#5e4ae3',
  'single-source': '#9199a8',
  disputed: '#ff9500',
  unverified: '#ff3b30',
}
