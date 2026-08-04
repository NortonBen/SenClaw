/** Tổng quan dự án — 4 KPI + việc gấp + tiến độ pipeline từng tính năng +
 * kanban lifecycle, đúng bố cục dashboard của BA-Kit. */
import { useCallback, useEffect, useState } from 'react'
import {
  App,
  Button,
  Card,
  Col,
  Empty,
  Input,
  Modal,
  Progress,
  Row,
  Select,
  Space,
  Statistic,
  Table,
  Tag,
  Typography,
} from 'antd'
import { ImportOutlined, PlusOutlined } from '@ant-design/icons'
import { get, post, STATUS_LABEL, fmtTime } from './api'

export default function Dashboard({
  projectId,
  onOpenFeature,
  onOpenDoc,
  refreshKey,
}: {
  projectId: number
  onOpenFeature: (id: number) => void
  onOpenDoc: (id: number) => void
  refreshKey: number
}) {
  const { message } = App.useApp()
  const [dash, setDash] = useState<any>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [newName, setNewName] = useState('')
  const [newDesc, setNewDesc] = useState('')
  const [newPrio, setNewPrio] = useState('P1')

  const load = useCallback(async () => {
    try {
      setDash(await get(`/projects/${projectId}/dashboard`))
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }, [projectId, message])

  useEffect(() => {
    load()
  }, [load, refreshKey])

  const addFeature = async () => {
    try {
      await post(`/projects/${projectId}/features`, { name: newName, description: newDesc, priority: newPrio })
      message.success('Đã thêm tính năng')
      setAddOpen(false)
      setNewName('')
      setNewDesc('')
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  const importFromPrd = async () => {
    try {
      const r = await post(`/projects/${projectId}/import-features`, {})
      message.success(`Đã thêm ${r.added?.length ?? 0} tính năng từ PRD`)
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e), 6)
    }
  }

  if (!dash) return null
  const kpi = dash.kpi ?? {}

  return (
    <div>
      <Row gutter={[12, 12]}>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Truy vết (coverage)" value={kpi.coverage ?? '—'} suffix={kpi.coverage != null ? '%' : ''} />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Tiến độ pipeline" value={kpi.pipeline ?? 0} suffix="%" />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic title="Độ tươi tài liệu" value={kpi.freshness ?? 100} suffix="đ" />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic
              title="Việc gấp"
              value={kpi.urgent ?? 0}
              valueStyle={{ color: (kpi.urgent ?? 0) > 0 ? '#ffa940' : undefined }}
            />
          </Card>
        </Col>
      </Row>

      {(dash.urgent ?? []).length > 0 && (
        <Card size="small" title="⚡ Việc gấp nhất" style={{ marginTop: 12 }}>
          {dash.urgent.map((u: any, i: number) => (
            <div key={i} className={`urgent-item ${u.level === 'P0' ? 'p0' : ''}`}>
              <Tag color={u.level === 'P0' ? 'red' : u.level === 'P1' ? 'orange' : 'default'}>{u.level}</Tag>
              {u.text}
            </div>
          ))}
        </Card>
      )}

      <Card
        size="small"
        title="🚦 Tính năng & pipeline 8 chặng"
        style={{ marginTop: 12 }}
        extra={
          <Space>
            <Button size="small" icon={<ImportOutlined />} onClick={importFromPrd}>
              Bóc từ PRD
            </Button>
            <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setAddOpen(true)}>
              Thêm tính năng
            </Button>
          </Space>
        }
      >
        {(dash.features ?? []).length === 0 ? (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <span>
                Chưa có tính năng. Sinh <code>/prd</code> ở tab "Tài liệu dự án" rồi bấm <b>Bóc từ PRD</b>, hoặc thêm tay.
              </span>
            }
          />
        ) : (
          <Table
            size="small"
            rowKey={(r: any) => r.feature.id}
            pagination={false}
            dataSource={dash.features}
            onRow={(r: any) => ({ onClick: () => onOpenFeature(r.feature.id), style: { cursor: 'pointer' } })}
            columns={[
              {
                title: 'Tính năng',
                render: (_: any, r: any) => (
                  <Space>
                    <b>{r.feature.name}</b>
                    <Tag color={r.feature.priority === 'P0' ? 'red' : r.feature.priority === 'P1' ? 'orange' : 'default'}>
                      {r.feature.priority}
                    </Tag>
                  </Space>
                ),
              },
              {
                title: 'Pipeline URD→Test',
                width: 320,
                render: (_: any, r: any) => (
                  <Space size={2} wrap>
                    {(r.pipeline?.stages ?? []).map((s: any) => (
                      <Tag key={s.stage} color={s.done ? 'green' : 'default'} style={{ fontSize: 10, marginInlineEnd: 2 }}>
                        {s.stage}
                      </Tag>
                    ))}
                  </Space>
                ),
              },
              {
                title: '%',
                width: 120,
                render: (_: any, r: any) => <Progress percent={r.pipeline?.pct ?? 0} size="small" />,
              },
              {
                title: 'Coverage',
                width: 100,
                render: (_: any, r: any) =>
                  r.coverage?.coverage_pct != null ? `${r.coverage.coverage_pct}%` : '—',
              },
              {
                title: 'Tài liệu',
                width: 80,
                render: (_: any, r: any) => r.feature.documents,
              },
            ]}
          />
        )}
      </Card>

      <Card size="small" title="📋 Kanban tài liệu theo lifecycle" style={{ marginTop: 12 }}>
        <Row gutter={8}>
          {Object.keys(STATUS_LABEL).map((st) => (
            <Col key={st} xs={24} sm={12} md={Math.floor(24 / 5)} flex="1">
              <div className="kanban-col">
                <Typography.Text type="secondary" style={{ fontSize: 11, textTransform: 'uppercase' }}>
                  {STATUS_LABEL[st]} · {(dash.kanban?.[st] ?? []).length}
                </Typography.Text>
                <div style={{ marginTop: 8 }}>
                  {(dash.kanban?.[st] ?? []).map((d: any) => (
                    <div key={d.id} className="kanban-card" onClick={() => onOpenDoc(d.id)}>
                      <div>{d.title}</div>
                      <Typography.Text type="secondary" style={{ fontSize: 10.5 }}>
                        {d.doc_type}
                        {d.subtype ? `/${d.subtype}` : ''} · {fmtTime(d.updated_at)}
                      </Typography.Text>
                    </div>
                  ))}
                </div>
              </div>
            </Col>
          ))}
        </Row>
      </Card>

      {(dash.stale_chain ?? []).length > 0 && (
        <Card size="small" title="⏰ Stale chain (upstream đổi sau tài liệu)" style={{ marginTop: 12 }}>
          {dash.stale_chain.map((c: any, i: number) => (
            <div key={i} style={{ fontSize: 12.5, marginBottom: 4 }}>
              <Tag color="orange">{c.upstream}</Tag> →{' '}
              <a onClick={() => onOpenDoc(c.doc_id)}>{c.doc_title}</a>
            </div>
          ))}
        </Card>
      )}

      <Modal open={addOpen} onCancel={() => setAddOpen(false)} onOk={addFeature} title="Thêm tính năng" okText="Thêm">
        <Space direction="vertical" style={{ width: '100%' }}>
          <Input placeholder="Tên tính năng (vd: Xác thực người dùng)" value={newName} onChange={(e) => setNewName(e.target.value)} />
          <Input.TextArea rows={3} placeholder="Mô tả ngắn" value={newDesc} onChange={(e) => setNewDesc(e.target.value)} />
          <Select
            value={newPrio}
            onChange={setNewPrio}
            style={{ width: 140 }}
            options={[
              { value: 'P0', label: 'P0 — lõi' },
              { value: 'P1', label: 'P1 — nên có' },
              { value: 'P2', label: 'P2 — để sau' },
            ]}
          />
        </Space>
      </Modal>
    </div>
  )
}
