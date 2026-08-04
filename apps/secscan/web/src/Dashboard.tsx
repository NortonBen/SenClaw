import { useEffect, useState } from 'react'
import { Alert, Card, Col, Empty, Progress, Row, Space, Statistic, Tag, Tooltip, Typography } from 'antd'
import {
  api, gradeColor, SEV_COLOR, SEV_LABEL, SEV_ORDER,
  type Dashboard as Dash, type Severity,
} from './api'

const { Text } = Typography

const CAT_LABEL: Record<string, string> = {
  headers: 'Security header', cookies: 'Cookie', exposure: 'Lộ thông tin', dns: 'DNS & email',
}

export default function Dashboard({ assetId, reloadKey }: { assetId: number | null; reloadKey: number }) {
  const [d, setD] = useState<Dash | null>(null)

  useEffect(() => {
    if (assetId == null) return
    void api.dashboard(assetId).then(setD)
  }, [assetId, reloadKey])

  if (!d) return <Empty description="chưa có dữ liệu" />
  if (d.trend.length === 0) {
    return <Empty description="Chưa quét lần nào — bấm Quét để bắt đầu" />
  }

  const latest = d.trend[d.trend.length - 1]
  const prev = d.trend.length > 1 ? d.trend[d.trend.length - 2] : null
  const delta = prev && latest.score != null && prev.score != null ? latest.score - prev.score : null

  const totalFindings = SEV_ORDER.reduce((n, s) => n + (d.by_severity[s] ?? 0), 0)
  const maxScore = 100

  return (
    <>
      <Row gutter={[12, 12]}>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Space direction="vertical" size={0} style={{ width: '100%' }}>
              <Text className="muted">hạng hiện tại</Text>
              <div className="grade" style={{ color: gradeColor(latest.grade) }}>{latest.grade ?? '—'}</div>
            </Space>
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="điểm" value={latest.score ?? 0} suffix={`/ ${maxScore}`}
              valueStyle={{ fontSize: 26 }}
            />
            {delta != null && delta !== 0 && (
              <Text style={{ fontSize: 12, color: delta > 0 ? '#389e0d' : '#cf1322' }}>
                {delta > 0 ? '▲' : '▼'} {Math.abs(delta)} so với lần trước
              </Text>
            )}
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic title="phát hiện" value={totalFindings} valueStyle={{ fontSize: 26 }} />
            {d.acked > 0 && <Text className="muted">{d.acked} đã chấp nhận rủi ro</Text>}
          </Card>
        </Col>
        <Col xs={12} sm={6}>
          <Card size="small">
            <Statistic
              title="tái phát" value={d.regressed}
              valueStyle={{ fontSize: 26, color: d.regressed > 0 ? '#cf1322' : undefined }}
            />
            {d.regressed > 0 && <Text className="muted">đã vá rồi quay lại</Text>}
          </Card>
        </Col>
      </Row>

      {d.regressed > 0 && (
        <Alert
          style={{ marginTop: 12 }} type="error" showIcon
          message={`${d.regressed} phát hiện đã từng được vá nhưng nay quay lại`}
          description="Tái phát thường có nghĩa là bản vá bị ghi đè khi triển khai, hoặc chỉ sửa ở một máy chủ trong cụm."
        />
      )}

      <Row gutter={[12, 12]} style={{ marginTop: 12 }}>
        <Col xs={24} md={14}>
          <Card size="small" title="Xu hướng điểm">
            <ScoreTrend points={d.trend} />
          </Card>
        </Col>
        <Col xs={24} md={10}>
          <Card size="small" title="Phân bố theo mức">
            {SEV_ORDER.filter((s) => (d.by_severity[s] ?? 0) > 0).map((s) => (
              <div key={s} style={{ marginBottom: 8 }}>
                <Row>
                  <Col flex="auto"><Text style={{ fontSize: 13 }}>{SEV_LABEL[s]}</Text></Col>
                  <Col><Text strong>{d.by_severity[s]}</Text></Col>
                </Row>
                <Progress
                  percent={Math.round(((d.by_severity[s] ?? 0) / Math.max(totalFindings, 1)) * 100)}
                  strokeColor={SEV_COLOR[s]} showInfo={false} size="small"
                />
              </div>
            ))}
            <div style={{ marginTop: 12 }}>
              <Text className="muted">theo nhóm: </Text>
              <Space size={4} wrap style={{ marginTop: 4 }}>
                {Object.entries(d.by_category).map(([c, n]) => (
                  <Tag key={c}>{CAT_LABEL[c] ?? c}: {n as number}</Tag>
                ))}
              </Space>
            </div>
          </Card>
        </Col>
      </Row>

      {d.top_open.length > 0 && (
        <Card size="small" title="Nên xử lý trước" style={{ marginTop: 12 }}>
          {d.top_open.map((f) => (
            <div
              key={f.id} className="finding"
              style={{ borderLeftColor: SEV_COLOR[f.severity as Severity], marginBottom: 6 }}
            >
              <h4>{f.title}</h4>
              {f.fix && <div className="fix">{f.fix}</div>}
              <div className="tag-row">
                <Tag color={SEV_COLOR[f.severity as Severity]}>{SEV_LABEL[f.severity as Severity]}</Tag>
                {f.kev && <Tag color="red">KEV — đang bị khai thác thật</Tag>}
                {f.status === 'regressed' && <Tag color="volcano">tái phát</Tag>}
              </div>
            </div>
          ))}
        </Card>
      )}
    </>
  )
}

/** Biểu đồ đường thuần SVG — không kéo thêm thư viện chart cho một dãy số. */
function ScoreTrend({ points }: { points: { at: string; score: number | null; grade: string | null }[] }) {
  const pts = points.filter((p) => p.score != null)
  if (pts.length < 2) {
    return <Text className="muted">Cần ít nhất hai lần quét để thấy xu hướng.</Text>
  }
  const W = 520, H = 140, PAD = 22
  const n = pts.length
  const x = (i: number) => PAD + (i * (W - PAD * 2)) / (n - 1)
  const y = (v: number) => H - PAD - (v / 100) * (H - PAD * 2)
  const path = pts.map((p, i) => `${i === 0 ? 'M' : 'L'}${x(i)},${y(p.score!)}`).join(' ')

  return (
    <svg viewBox={`0 0 ${W} ${H}`} style={{ width: '100%', height: 'auto' }}>
      {[0, 50, 100].map((v) => (
        <g key={v}>
          <line x1={PAD} y1={y(v)} x2={W - PAD} y2={y(v)} stroke="var(--sc-border)" strokeWidth={1} />
          <text x={2} y={y(v) + 4} fontSize={10} fill="var(--sc-muted)">{v}</text>
        </g>
      ))}
      <path d={path} fill="none" stroke="#1677ff" strokeWidth={2} />
      {pts.map((p, i) => (
        <Tooltip key={i} title={`${new Date(p.at).toLocaleString('vi-VN')} — ${p.score} điểm (${p.grade})`}>
          <circle cx={x(i)} cy={y(p.score!)} r={4} fill={gradeColor(p.grade)} />
        </Tooltip>
      ))}
    </svg>
  )
}
