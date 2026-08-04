import { useEffect, useState } from 'react'
import {
  Card,
  Table,
  Tag,
  Space,
  Input,
  Select,
  Button,
  Drawer,
  Typography,
  Descriptions,
  Empty,
} from 'antd'
import { api, fmtTs, SEV_COLOR } from './api'
import type { Ev } from './api'

const { Text, Paragraph } = Typography

const KIND_LABEL: Record<string, string> = {
  tool_call: 'gọi tool',
  permission_request: 'hỏi phê duyệt',
  permission_resolved: 'kết quả phê duyệt',
  question_request: 'hỏi người dùng',
  question_resolved: 'trả lời',
  schedule_run: 'lịch chạy',
  message: 'tin nhắn',
}

const PIVOT_LABEL: Record<string, string> = {
  actor: 'Cùng đối tượng (±30 phút)',
  tool: 'Cùng tool',
  schedule: 'Cùng lịch',
  preceding: 'Ngay TRƯỚC đó — tìm nguồn injection',
}

export default function Timeline() {
  const [rows, setRows] = useState<Ev[]>([])
  const [loading, setLoading] = useState(false)
  const [f, setF] = useState<{ actor?: string; kind?: string; tool?: string; q?: string }>({})
  const [sel, setSel] = useState<any>(null)
  const [pivot, setPivot] = useState<{ mode: string; rows: Ev[] } | null>(null)

  const load = async () => {
    setLoading(true)
    try {
      const r = await api.events({ ...f, limit: 300 })
      setRows(r.events ?? [])
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const open = async (id: number) => {
    setPivot(null)
    setSel(await api.event(id))
  }

  const doPivot = async (mode: string) => {
    if (!sel?.event) return
    const r: any = await api.pivot(sel.event.id, mode)
    setPivot({ mode, rows: r.events ?? [] })
  }

  const cols = [
    { title: 'Thời điểm', dataIndex: 'ts', width: 165, render: (v: string) => fmtTs(v) },
    {
      title: 'Loại',
      dataIndex: 'kind',
      width: 140,
      render: (k: string) => <Tag>{KIND_LABEL[k] ?? k}</Tag>,
    },
    {
      title: 'Đối tượng',
      dataIndex: 'actor',
      width: 210,
      ellipsis: true,
      render: (v: string) => <span className="mono">{v}</span>,
    },
    {
      title: 'Tool',
      dataIndex: 'tool_name',
      width: 230,
      ellipsis: true,
      render: (v: string | null, r: Ev) => (
        <Space size={4}>
          {r.ok === false && <Tag color="red">lỗi</Tag>}
          <span className="mono">{v ?? '—'}</span>
        </Space>
      ),
    },
    { title: 'Tóm tắt', dataIndex: 'summary', ellipsis: true },
  ]

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <Card size="small">
        <Space wrap>
          <Input
            placeholder="Đối tượng (chat_jid / schedule:id)"
            style={{ width: 240 }}
            allowClear
            value={f.actor}
            onChange={(e) => setF({ ...f, actor: e.target.value })}
          />
          <Select
            placeholder="Loại sự kiện"
            style={{ width: 190 }}
            allowClear
            value={f.kind}
            onChange={(v) => setF({ ...f, kind: v })}
            options={Object.entries(KIND_LABEL).map(([value, label]) => ({ value, label }))}
          />
          <Input
            placeholder="Tên tool"
            style={{ width: 200 }}
            allowClear
            value={f.tool}
            onChange={(e) => setF({ ...f, tool: e.target.value })}
          />
          <Input
            placeholder="Tìm toàn văn"
            style={{ width: 220 }}
            allowClear
            value={f.q}
            onChange={(e) => setF({ ...f, q: e.target.value })}
            onPressEnter={load}
          />
          <Button type="primary" onClick={load}>
            Lọc
          </Button>
          <Button onClick={() => { setF({}); setTimeout(load, 0) }}>Xoá lọc</Button>
        </Space>
      </Card>

      <Table<Ev>
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={rows}
        columns={cols as any}
        pagination={{ pageSize: 50, showSizeChanger: false }}
        onRow={(r) => ({ onClick: () => open(r.id), className: 'evrow' })}
      />

      <Drawer
        width={720}
        open={!!sel}
        onClose={() => setSel(null)}
        title={sel?.event ? `Sự kiện #${sel.event.id}` : ''}
      >
        {sel?.event && (
          <Space direction="vertical" size={14} style={{ width: '100%' }}>
            <Descriptions column={1} size="small" bordered>
              <Descriptions.Item label="Thời điểm">{fmtTs(sel.event.ts)}</Descriptions.Item>
              <Descriptions.Item label="Loại">{KIND_LABEL[sel.event.kind] ?? sel.event.kind}</Descriptions.Item>
              <Descriptions.Item label="Đối tượng">
                <span className="mono">{sel.event.actor}</span>
              </Descriptions.Item>
              <Descriptions.Item label="Tool">
                <span className="mono">{sel.event.tool_name ?? '—'}</span>
              </Descriptions.Item>
              <Descriptions.Item label="Nguồn">{sel.event.source}</Descriptions.Item>
              <Descriptions.Item label="Tóm tắt">{sel.event.summary}</Descriptions.Item>
            </Descriptions>

            {!!sel.findings?.length && (
              <Card size="small" title="Phát hiện trích dẫn sự kiện này">
                {sel.findings.map((x: any) => (
                  <div key={x.id} style={{ marginBottom: 6 }}>
                    <Tag color={SEV_COLOR[x.severity as keyof typeof SEV_COLOR]}>{x.rule_id}</Tag>
                    <Text>{x.title}</Text>
                  </div>
                ))}
              </Card>
            )}

            <Card size="small" title="Xoay quanh sự kiện này">
              <Space wrap>
                {Object.entries(PIVOT_LABEL).map(([m, label]) => (
                  <Button key={m} size="small" onClick={() => doPivot(m)}>
                    {label}
                  </Button>
                ))}
              </Space>
              {pivot && (
                <div style={{ marginTop: 12 }}>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {PIVOT_LABEL[pivot.mode]} — {pivot.rows.length} sự kiện
                  </Text>
                  <Table<Ev>
                    rowKey="id"
                    size="small"
                    style={{ marginTop: 8 }}
                    pagination={{ pageSize: 10, showSizeChanger: false }}
                    dataSource={pivot.rows}
                    columns={[
                      { title: 'Thời điểm', dataIndex: 'ts', width: 160, render: (v: string) => fmtTs(v) },
                      { title: 'Tool', dataIndex: 'tool_name', width: 200, ellipsis: true },
                      { title: 'Tóm tắt', dataIndex: 'summary', ellipsis: true },
                    ] as any}
                    onRow={(r) => ({ onClick: () => open(r.id), className: 'evrow' })}
                  />
                </div>
              )}
            </Card>

            <Card size="small" title="Chi tiết đã lưu (đã lọc bí mật)">
              <Paragraph className="md-block mono" style={{ maxHeight: 320, overflow: 'auto' }}>
                {JSON.stringify(sel.event.detail, null, 2)}
              </Paragraph>
            </Card>
          </Space>
        )}
        {!sel?.event && <Empty />}
      </Drawer>
    </Space>
  )
}
