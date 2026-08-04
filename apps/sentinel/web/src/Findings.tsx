import { useEffect, useState } from 'react'
import {
  Card,
  Table,
  Tag,
  Space,
  Select,
  Button,
  Drawer,
  Typography,
  message,
  Modal,
  Input,
  Alert,
} from 'antd'
import { api, fmtTs, SEV_COLOR, SEV_LABEL, STATUS_LABEL } from './api'
import type { Finding, FindingStatus } from './api'

const { Paragraph, Text } = Typography

export default function Findings() {
  const [rows, setRows] = useState<Finding[]>([])
  const [loading, setLoading] = useState(false)
  const [f, setF] = useState<{ status?: string; severity?: string }>({ status: 'open' })
  const [sel, setSel] = useState<any>(null)
  const [explain, setExplain] = useState<{ text: string; model: string } | null>(null)
  const [explaining, setExplaining] = useState(false)
  const [picked, setPicked] = useState<number[]>([])
  const [caseTitle, setCaseTitle] = useState('')
  const [caseOpen, setCaseOpen] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      const r = await api.findings({ ...f, limit: 200 })
      setRows(r.findings ?? [])
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [f.status, f.severity])

  const open = async (id: number) => {
    setExplain(null)
    setSel(await api.finding(id))
  }

  const setStatus = async (id: number, status: FindingStatus) => {
    await api.setFindingStatus(id, status)
    message.success('Đã đổi trạng thái')
    await load()
    if (sel?.finding?.id === id) await open(id)
  }

  const doExplain = async () => {
    if (!sel?.finding) return
    setExplaining(true)
    try {
      const r: any = await api.explain(sel.finding.id)
      setExplain(r.ok ? { text: r.explanation, model: r.model } : { text: 'Lỗi: ' + r.error, model: '' })
    } finally {
      setExplaining(false)
    }
  }

  const createCase = async () => {
    if (!caseTitle.trim()) return
    const r: any = await api.createCase({ title: caseTitle, finding_ids: picked, severity: 'high' })
    if (r.ok) {
      message.success('Đã mở vụ việc — xem tab Vụ việc')
      setCaseOpen(false)
      setCaseTitle('')
      setPicked([])
      await load()
    } else message.error(r.error)
  }

  const cols = [
    {
      title: 'Mức',
      dataIndex: 'severity',
      width: 130,
      render: (s: any) => <Tag color={SEV_COLOR[s as keyof typeof SEV_COLOR]}>{SEV_LABEL[s as keyof typeof SEV_LABEL]}</Tag>,
    },
    { title: 'Điểm', dataIndex: 'score', width: 70, sorter: (a: Finding, b: Finding) => a.score - b.score },
    { title: 'Luật', dataIndex: 'rule_id', width: 165, render: (v: string) => <span className="mono">{v}</span> },
    { title: 'Phát hiện', dataIndex: 'title', ellipsis: true },
    {
      title: 'Đối tượng',
      dataIndex: 'actor',
      width: 190,
      ellipsis: true,
      render: (v: string | null) => <span className="mono">{v ?? 'toàn hệ'}</span>,
    },
    {
      title: 'Trạng thái',
      dataIndex: 'status',
      width: 145,
      render: (v: any) => <Tag>{STATUS_LABEL[v as keyof typeof STATUS_LABEL] ?? v}</Tag>,
    },
    { title: 'Gần nhất', dataIndex: 'last_ts', width: 165, render: (v: string) => fmtTs(v) },
  ]

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <Card size="small">
        <Space wrap>
          <Select
            style={{ width: 180 }}
            allowClear
            placeholder="Trạng thái"
            value={f.status}
            onChange={(v) => setF({ ...f, status: v })}
            options={Object.entries(STATUS_LABEL).map(([value, label]) => ({ value, label }))}
          />
          <Select
            style={{ width: 160 }}
            allowClear
            placeholder="Mức"
            value={f.severity}
            onChange={(v) => setF({ ...f, severity: v })}
            options={Object.entries(SEV_LABEL).map(([value, label]) => ({ value, label }))}
          />
          <Button onClick={load}>Làm mới</Button>
          <Button
            type="primary"
            disabled={picked.length === 0}
            onClick={() => setCaseOpen(true)}
          >
            Tạo vụ việc từ {picked.length} phát hiện
          </Button>
        </Space>
      </Card>

      <Table<Finding>
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={rows}
        columns={cols as any}
        rowSelection={{ selectedRowKeys: picked, onChange: (k) => setPicked(k as number[]) }}
        pagination={{ pageSize: 30, showSizeChanger: false }}
        onRow={(r) => ({ onClick: () => open(r.id), className: 'evrow' })}
      />

      <Drawer width={760} open={!!sel} onClose={() => setSel(null)} title={sel?.finding?.title}>
        {sel?.finding && (
          <Space direction="vertical" size={14} style={{ width: '100%' }}>
            <Space wrap>
              <Tag color={SEV_COLOR[sel.finding.severity as keyof typeof SEV_COLOR]}>
                {SEV_LABEL[sel.finding.severity as keyof typeof SEV_LABEL]} · {sel.finding.score} điểm
              </Tag>
              <Tag className="mono">{sel.finding.rule_id}</Tag>
              {(sel.finding.standards ?? []).map((s: string) => (
                <Tag key={s} color="geekblue">
                  {s}
                </Tag>
              ))}
            </Space>

            <Alert type="info" showIcon message={sel.rule?.title} description={sel.rule?.about} />

            <Card size="small" title="Chi tiết">
              <Paragraph className="md-block">{sel.finding.detail}</Paragraph>
              <Text type="secondary" style={{ fontSize: 12 }}>
                Từ {fmtTs(sel.finding.first_ts)} đến {fmtTs(sel.finding.last_ts)}
              </Text>
            </Card>

            <Card
              size="small"
              title="Giải thích bằng AI"
              extra={
                <Button size="small" loading={explaining} onClick={doExplain}>
                  Nhờ AI giải thích
                </Button>
              }
            >
              {explain ? (
                <>
                  <Paragraph className="md-block">{explain.text}</Paragraph>
                  {explain.model && <Tag>{explain.model}</Tag>}
                </>
              ) : (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  AI chỉ diễn giải phát hiện đã có — mức nghiêm trọng do hệ thống chấm, không phải do AI.
                </Text>
              )}
            </Card>

            {!!sel.evidence?.length && (
              <Card size="small" title={`Sự kiện chứng cứ (${sel.evidence.length})`}>
                <Table
                  rowKey="id"
                  size="small"
                  pagination={false}
                  dataSource={sel.evidence}
                  columns={[
                    { title: 'Thời điểm', dataIndex: 'ts', width: 160, render: (v: string) => fmtTs(v) },
                    { title: 'Tool', dataIndex: 'tool_name', width: 200, ellipsis: true },
                    { title: 'Tóm tắt', dataIndex: 'summary', ellipsis: true },
                  ] as any}
                />
              </Card>
            )}

            <Card size="small" title="Phân loại">
              <Space wrap>
                {(['triaged', 'accepted_risk', 'false_positive', 'resolved'] as FindingStatus[]).map((s) => (
                  <Button key={s} size="small" onClick={() => setStatus(sel.finding.id, s)}>
                    {STATUS_LABEL[s]}
                  </Button>
                ))}
              </Space>
            </Card>
          </Space>
        )}
      </Drawer>

      <Modal
        open={caseOpen}
        title="Mở hồ sơ vụ việc"
        onCancel={() => setCaseOpen(false)}
        onOk={createCase}
        okText="Tạo"
        cancelText="Huỷ"
      >
        <Input
          placeholder="Tiêu đề vụ việc"
          value={caseTitle}
          onChange={(e) => setCaseTitle(e.target.value)}
          onPressEnter={createCase}
        />
        <Text type="secondary" style={{ fontSize: 12 }}>
          Sẽ gắn {picked.length} phát hiện đã chọn vào vụ việc này.
        </Text>
      </Modal>
    </Space>
  )
}
