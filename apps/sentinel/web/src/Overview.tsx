import { useEffect, useState } from 'react'
import {
  Card,
  Col,
  Row,
  Statistic,
  Tag,
  Alert,
  Space,
  Typography,
  Input,
  Button,
  Table,
  Empty,
  Spin,
} from 'antd'
import { api, fmtTs, SEV_COLOR, SEV_LABEL } from './api'
import type { Finding } from './api'

const { Paragraph, Text } = Typography

/** Thẻ tư thế: trả lời "hệ thống đang ở trạng thái nào" ngay ở màn hình đầu. */
function PostureCard({ p }: { p: any }) {
  if (!p) return null
  const items: { ok: boolean; label: string; detail: string }[] = [
    {
      ok: !p.hitl_disabled,
      label: 'Phê duyệt của con người',
      detail: p.hitl_disabled
        ? 'ĐANG TẮT — agent chạy mọi tool không hỏi'
        : 'Đang bật',
    },
    {
      ok: (p.wildcard_autoaccept_rules ?? 0) === 0,
      label: 'Luật tự động cho qua',
      detail: p.wildcard_autoaccept_rules
        ? `${p.wildcard_autoaccept_rules} luật cho qua nguyên một server rủi ro`
        : 'Không có luật quá rộng',
    },
    {
      ok: !p.apps_exposed_on_lan,
      label: 'Space App trên mạng',
      detail: p.apps_exposed_on_lan ? 'Có app nghe được từ máy khác trong LAN' : 'Chỉ nghe cục bộ',
    },
    {
      ok: (p.shell_schedules ?? 0) === 0,
      label: 'Lịch chạy shell',
      detail: p.shell_schedules
        ? `${p.shell_schedules} lịch chạy bash không qua kiểm tra`
        : 'Không có lịch chạy shell tuỳ ý',
    },
  ]
  return (
    <Card title="Tư thế bảo mật" size="small">
      <Row gutter={[12, 12]}>
        {items.map((it) => (
          <Col xs={24} sm={12} key={it.label}>
            <Space direction="vertical" size={2} style={{ width: '100%' }}>
              <Space size={8}>
                <Tag color={it.ok ? 'green' : 'red'}>{it.ok ? 'OK' : 'CẦN XEM'}</Tag>
                <Text strong>{it.label}</Text>
              </Space>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {it.detail}
              </Text>
            </Space>
          </Col>
        ))}
      </Row>
    </Card>
  )
}

/**
 * Biểu đồ cột thuần CSS — tránh kéo thêm thư viện chart cho một sparkline.
 *
 * Cột chồng: phần lỗi vẽ đỏ ở đáy theo đúng tỉ lệ. Bản đầu tô đỏ cả cột khi
 * `failed > 0`, nghĩa là một lỗi lẻ trong 200 lượt cũng làm cả ngày đỏ rực —
 * nhìn vào tưởng cả tháng có vấn đề.
 */
function ActivityChart({ data }: { data: any[] }) {
  if (!data?.length) return <Empty description="Chưa đủ dữ liệu" image={Empty.PRESENTED_IMAGE_SIMPLE} />
  const max = Math.max(...data.map((d) => d.count), 1)
  return (
    <div className="sen-chart">
      {data.map((d) => {
        const h = Math.max(3, (d.count / max) * 72)
        const failed = d.failed ?? 0
        const failH = d.count > 0 ? (failed / d.count) * h : 0
        return (
          <div
            key={d.day}
            className="sen-chart-col"
            title={`${d.day}: ${d.count} sự kiện${failed ? `, ${failed} lỗi` : ''}`}
          >
            <div className="sen-bar" style={{ height: h }}>
              {failH > 0 && <div className="sen-bar-fail" style={{ height: failH }} />}
            </div>
            <div className="sen-chart-label">{d.day.slice(5)}</div>
          </div>
        )
      })}
    </div>
  )
}

export default function Overview({ onGoTab }: { onGoTab: (t: string) => void }) {
  const [d, setD] = useState<any>(null)
  const [q, setQ] = useState('')
  const [answer, setAnswer] = useState<{ text: string; model: string } | null>(null)
  const [asking, setAsking] = useState(false)

  useEffect(() => {
    api.dashboard().then(setD).catch(() => setD(null))
  }, [])

  const doAsk = async () => {
    if (!q.trim()) return
    setAsking(true)
    setAnswer(null)
    try {
      const r: any = await api.ask(q)
      setAnswer(r.ok ? { text: r.answer, model: r.model } : { text: 'Lỗi: ' + r.error, model: '' })
    } catch (e: any) {
      setAnswer({ text: 'Lỗi: ' + e?.message, model: '' })
    } finally {
      setAsking(false)
    }
  }

  if (!d) return <Spin />

  const sev = d.findings?.by_severity ?? {}
  const cols = [
    {
      title: 'Mức',
      dataIndex: 'severity',
      width: 130,
      render: (s: any) => <Tag color={SEV_COLOR[s as keyof typeof SEV_COLOR]}>{SEV_LABEL[s as keyof typeof SEV_LABEL]}</Tag>,
    },
    { title: 'Điểm', dataIndex: 'score', width: 70 },
    { title: 'Luật', dataIndex: 'rule_id', width: 160, render: (v: string) => <span className="mono">{v}</span> },
    { title: 'Phát hiện', dataIndex: 'title' },
    { title: 'Gần nhất', dataIndex: 'last_ts', width: 170, render: (v: string) => fmtTs(v) },
  ]

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      {d.chain?.intact === false && (
        <Alert
          type="error"
          showIcon
          message="Chuỗi băm của kho chứng cứ đã GÃY"
          description={`Sự kiện #${d.chain.broken_at} bị sửa hoặc xoá sau khi ghi. Bản thân dấu vết không còn đáng tin từ điểm này trở đi.`}
        />
      )}

      <Row gutter={[12, 12]}>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Sự kiện đã bảo toàn" value={d.events} />
            <Text type="secondary" style={{ fontSize: 11 }}>
              {fmtTs(d.event_span?.from)} → nay
            </Text>
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Nghiêm trọng" value={sev.critical ?? 0} valueStyle={{ color: '#ff4d4f' }} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Cao" value={sev.high ?? 0} valueStyle={{ color: '#fa541c' }} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Vụ việc đang mở" value={d.cases_open ?? 0} />
          </Card>
        </Col>
      </Row>

      <PostureCard p={d.posture} />

      <Card title="Hoạt động 14 ngày" size="small">
        <ActivityChart data={d.activity ?? []} />
      </Card>

      <Card title="Hỏi về hoạt động gần đây" size="small">
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="ví dụ: tuần này có gì bất thường không?"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            onPressEnter={doAsk}
          />
          <Button type="primary" loading={asking} onClick={doAsk}>
            Hỏi
          </Button>
        </Space.Compact>
        {answer && (
          <Paragraph className="md-block" style={{ marginTop: 12 }}>
            {answer.text}
            {answer.model && (
              <div style={{ marginTop: 8 }}>
                <Tag>{answer.model}</Tag>
              </div>
            )}
          </Paragraph>
        )}
      </Card>

      <Card
        title="Phát hiện điểm cao nhất"
        size="small"
        extra={<Button size="small" onClick={() => onGoTab('findings')}>Xem tất cả</Button>}
      >
        <Table<Finding>
          rowKey="id"
          size="small"
          pagination={false}
          columns={cols as any}
          dataSource={d.top_findings ?? []}
          locale={{ emptyText: 'Chưa có phát hiện nào' }}
        />
      </Card>
    </Space>
  )
}
