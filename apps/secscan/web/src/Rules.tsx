import { useCallback, useEffect, useMemo, useState } from 'react'
import { Alert, Card, Col, Empty, Input, Row, Segmented, Space, Statistic, Tag, Typography } from 'antd'
import {
  api, SEV_COLOR, SEV_LABEL,
  type CustomRule, type Override, type Rule, type Severity,
} from './api'
import { CustomRules, OverrideControl } from './RuleEditor'

const { Text, Paragraph, Title } = Typography

const CAT_LABEL: Record<string, string> = {
  headers: 'Security header',
  cookies: 'Cookie',
  exposure: 'Lộ thông tin',
  dns: 'DNS & email',
  tls: 'TLS',
  active: 'Dò chủ động',
  vuln: 'CVE',
  host: 'Máy chủ',
}

export default function Rules() {
  const [rules, setRules] = useState<Rule[]>([])
  const [notCovered, setNotCovered] = useState<string[]>([])
  const [total, setTotal] = useState(0)
  const [done, setDone] = useState(0)
  const [custom, setCustom] = useState<CustomRule[]>([])
  const [overrides, setOverrides] = useState<Override[]>([])
  const [q, setQ] = useState('')
  const [filter, setFilter] = useState<'all' | 'implemented' | 'planned'>('all')

  const load = useCallback(async () => {
    const [r, c] = await Promise.all([api.rules(), api.customRules()])
    setRules(r.rules ?? [])
    setNotCovered(r.not_covered ?? [])
    setTotal(r.total ?? 0)
    setDone(r.implemented ?? 0)
    setCustom(c.custom ?? [])
    setOverrides(c.overrides ?? [])
  }, [])

  useEffect(() => { void load() }, [load])

  const ovFor = (id: string) => overrides.find((o) => o.rule_id === id)

  const shown = useMemo(() => {
    const needle = q.trim().toLowerCase()
    return rules.filter((r) => {
      if (filter === 'implemented' && !r.implemented) return false
      if (filter === 'planned' && r.implemented) return false
      if (!needle) return true
      return (
        r.title.toLowerCase().includes(needle) ||
        r.rationale.toLowerCase().includes(needle) ||
        r.id.toLowerCase().includes(needle) ||
        (CAT_LABEL[r.category] ?? r.category).toLowerCase().includes(needle)
      )
    })
  }, [rules, q, filter])

  const grouped = useMemo(() => {
    const g = new Map<string, Rule[]>()
    for (const r of shown) {
      const k = r.category
      if (!g.has(k)) g.set(k, [])
      g.get(k)!.push(r)
    }
    return [...g.entries()]
  }, [shown])

  return (
    <>
      <Row gutter={12} style={{ marginBottom: 16 }}>
        <Col xs={12} sm={8}>
          <Card size="small"><Statistic title="Phép kiểm đã cài" value={done} suffix={`/ ${total}`} /></Card>
        </Col>
        <Col xs={12} sm={16}>
          <Card size="small" styles={{ body: { paddingTop: 12, paddingBottom: 12 } }}>
            <Space wrap>
              <Input.Search
                placeholder="tìm trong danh mục…" allowClear
                value={q} onChange={(e) => setQ(e.target.value)}
                style={{ width: 240 }}
              />
              <Segmented
                value={filter}
                onChange={(v) => setFilter(v as typeof filter)}
                options={[
                  { label: 'Tất cả', value: 'all' },
                  { label: 'Đã cài', value: 'implemented' },
                  { label: 'Chưa cài', value: 'planned' },
                ]}
              />
            </Space>
          </Card>
        </Col>
      </Row>

      <Paragraph type="secondary" style={{ fontSize: 13 }}>
        Danh mục này nói rõ app kiểm những gì và <strong>vì sao mức độ được đặt như vậy</strong>.
        Không có nó thì "không phát hiện gì" là câu mơ hồ — không rõ nghĩa là an toàn hay là không kiểm.
      </Paragraph>

      <CustomRules rules={custom} onChanged={load} />

      {grouped.length === 0 && <Empty description="không có mục nào khớp" />}

      {grouped.map(([cat, list]) => (
        <div key={cat} style={{ marginBottom: 20 }}>
          <Title level={5} style={{ marginBottom: 8 }}>
            {CAT_LABEL[cat] ?? cat} <Text type="secondary" style={{ fontWeight: 400 }}>({list.length})</Text>
          </Title>
          {list.map((r) => (
            <div
              key={r.id}
              className="finding"
              style={{
                borderLeftColor: r.implemented ? SEV_COLOR[r.max_severity as Severity] : 'var(--sc-border)',
                opacity: r.implemented ? 1 : 0.7,
              }}
            >
              <h4>{r.title}</h4>
              <p>{r.rationale}</p>
              <div className="tag-row">
                <Tag color={r.implemented ? SEV_COLOR[r.max_severity as Severity] : undefined}>
                  tối đa: {SEV_LABEL[r.max_severity as Severity]}
                </Tag>
                <Tag>{r.layer_label}</Tag>
                {r.wstg && <Tag color="blue">{r.wstg}</Tag>}
                <Tag color={r.implemented ? 'green' : 'default'}>
                  {r.implemented ? 'đã cài' : 'chưa cài'}
                </Tag>
                {ovFor(r.id) && (
                  <Tag color="purple">
                    {ovFor(r.id)!.enabled
                      ? `đổi mức → ${SEV_LABEL[ovFor(r.id)!.severity as Severity]}`
                      : 'đã tắt'}
                  </Tag>
                )}
                <Text className="muted" style={{ fontFamily: 'ui-monospace, monospace' }}>{r.id}</Text>
                {r.implemented && (
                  <span style={{ marginLeft: 'auto' }}>
                    <OverrideControl ruleId={r.id} current={ovFor(r.id)} onChanged={load} />
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      ))}

      <Alert
        type="warning" showIcon
        message="Những gì app này KHÔNG kiểm được"
        description={
          <ul style={{ margin: '6px 0 0', paddingLeft: 18, fontSize: 13, lineHeight: 1.7 }}>
            {notCovered.map((x) => <li key={x}>{x}</li>)}
          </ul>
        }
      />
    </>
  )
}
