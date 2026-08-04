import { useEffect, useState } from 'react'
import {
  Card,
  Table,
  Tag,
  Space,
  Button,
  Drawer,
  Typography,
  Input,
  message,
  Timeline as AntTimeline,
  Empty,
  Select,
} from 'antd'
import { api, fmtTs, SEV_COLOR } from './api'
import type { CaseRow } from './api'

const { Paragraph, Text } = Typography

const STATUS = [
  { value: 'open', label: 'Đang mở' },
  { value: 'investigating', label: 'Đang điều tra' },
  { value: 'closed', label: 'Đã đóng' },
]

export default function Cases() {
  const [rows, setRows] = useState<CaseRow[]>([])
  const [sel, setSel] = useState<any>(null)
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState('')
  const [report, setReport] = useState<string | null>(null)

  const load = async () => {
    const r = await api.cases()
    setRows(r.cases ?? [])
  }
  useEffect(() => {
    load()
  }, [])

  const open = async (id: number) => {
    setReport(null)
    const r: any = await api.case(id)
    setSel(r.case)
  }

  const addNote = async () => {
    if (!note.trim() || !sel) return
    const r: any = await api.caseNote(sel.id, note)
    setNote('')
    if (r.ok) setSel(r.case)
  }

  const hypothesize = async () => {
    if (!sel) return
    setBusy('hyp')
    try {
      const r: any = await api.caseHypothesis(sel.id)
      if (r.ok) {
        message.success('AI đã đề xuất giả thuyết — đọc và sửa lại cho đúng')
        await open(sel.id)
      } else message.error(r.error)
    } finally {
      setBusy('')
    }
  }

  const makeReport = async () => {
    if (!sel) return
    setBusy('rep')
    try {
      const r: any = await api.caseReport(sel.id)
      setReport(r.ok ? r.report : 'Lỗi: ' + r.error)
    } finally {
      setBusy('')
    }
  }

  const setStatus = async (status: string) => {
    if (!sel) return
    const r: any = await api.updateCase(sel.id, { status })
    if (r.ok) {
      setSel(r.case)
      await load()
    }
  }

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <Table<CaseRow>
        rowKey="id"
        size="small"
        dataSource={rows}
        locale={{
          emptyText: (
            <Empty description="Chưa có vụ việc. Chọn vài phát hiện ở tab Phát hiện rồi bấm Tạo vụ việc." />
          ),
        }}
        columns={
          [
            { title: 'Vụ việc', dataIndex: 'title', ellipsis: true },
            {
              title: 'Mức',
              dataIndex: 'severity',
              width: 120,
              render: (s: any) => <Tag color={SEV_COLOR[s as keyof typeof SEV_COLOR]}>{s}</Tag>,
            },
            {
              title: 'Trạng thái',
              dataIndex: 'status',
              width: 140,
              render: (v: string) => <Tag>{STATUS.find((s) => s.value === v)?.label ?? v}</Tag>,
            },
            { title: 'Phát hiện', dataIndex: 'finding_count', width: 100 },
            { title: 'Mở lúc', dataIndex: 'created_at', width: 170, render: (v: string) => fmtTs(v) },
          ] as any
        }
        onRow={(r) => ({ onClick: () => open(r.id), className: 'evrow' })}
        pagination={false}
      />

      <Drawer width={800} open={!!sel} onClose={() => setSel(null)} title={sel?.title}>
        {sel && (
          <Space direction="vertical" size={14} style={{ width: '100%' }}>
            <Space wrap>
              <Select
                size="small"
                style={{ width: 170 }}
                value={sel.status}
                onChange={setStatus}
                options={STATUS}
              />
              <Button size="small" loading={busy === 'hyp'} onClick={hypothesize}>
                AI dựng giả thuyết
              </Button>
              <Button size="small" loading={busy === 'rep'} onClick={makeReport}>
                Sinh báo cáo
              </Button>
            </Space>

            {sel.hypothesis && (
              <Card size="small" title="Giả thuyết (bản nháp của AI — sửa được)">
                <Paragraph className="md-block">{sel.hypothesis}</Paragraph>
              </Card>
            )}

            <Card size="small" title={`Phát hiện đã gắn (${sel.findings?.length ?? 0})`}>
              {(sel.findings ?? []).map((f: any) => (
                <div key={f.id} style={{ marginBottom: 8 }}>
                  <Space size={6}>
                    <Tag color={SEV_COLOR[f.severity as keyof typeof SEV_COLOR]}>{f.severity}</Tag>
                    <Tag className="mono">{f.rule_id}</Tag>
                    <Text>{f.title}</Text>
                  </Space>
                </div>
              ))}
              {!sel.findings?.length && <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} />}
            </Card>

            <Card size="small" title={`Dòng thời gian (${sel.timeline?.length ?? 0} sự kiện)`}>
              {sel.timeline?.length ? (
                <AntTimeline
                  items={sel.timeline.slice(0, 40).map((e: any) => ({
                    color: e.ok === false ? 'red' : 'blue',
                    children: (
                      <div>
                        <Text type="secondary" style={{ fontSize: 11 }}>
                          {fmtTs(e.ts)}
                        </Text>
                        <div>
                          <span className="mono">{e.tool_name ?? e.kind}</span> — {e.summary}
                        </div>
                      </div>
                    ),
                  }))}
                />
              ) : (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Phát hiện chưa gắn chứng cứ nào" />
              )}
            </Card>

            <Card size="small" title="Ghi chú điều tra">
              {(sel.notes ?? []).map((n: any) => (
                <div key={n.id} style={{ marginBottom: 8 }}>
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    {fmtTs(n.ts)} · {n.author}
                  </Text>
                  <div>{n.body}</div>
                </div>
              ))}
              <Space.Compact style={{ width: '100%', marginTop: 8 }}>
                <Input
                  placeholder="Ghi lại điều bạn vừa kiểm chứng bên ngoài hệ thống…"
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  onPressEnter={addNote}
                />
                <Button onClick={addNote}>Thêm</Button>
              </Space.Compact>
            </Card>

            {report && (
              <Card size="small" title="Báo cáo">
                <Paragraph className="md-block" copyable={{ text: report }}>
                  {report}
                </Paragraph>
              </Card>
            )}
          </Space>
        )}
      </Drawer>
    </Space>
  )
}
