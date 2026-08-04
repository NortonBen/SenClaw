import { useEffect, useState } from 'react'
import { Card, Table, Tag, Space, Switch, Select, Typography, message, Button, Input, Modal, Empty } from 'antd'
import { api, SEV_LABEL, fmtTs } from './api'
import type { Rule, Severity } from './api'

const { Text, Paragraph } = Typography

const GROUP_LABEL: Record<string, string> = {
  persistence: 'Cắm chốt lâu dài',
  control: 'Vô hiệu hoá kiểm soát',
  exfil: 'Rò rỉ dữ liệu',
  injection: 'Prompt injection & đầu độc',
  anomaly: 'Bất thường hành vi',
  posture: 'Tư thế bảo mật',
}

export default function Rules() {
  const [rules, setRules] = useState<Rule[]>([])
  const [sups, setSups] = useState<any[]>([])
  const [addFor, setAddFor] = useState<string | null>(null)
  const [reason, setReason] = useState('')

  const load = async () => {
    const r = await api.rules()
    setRules(r.rules ?? [])
    const s: any = await api.suppressions()
    setSups(s.suppressions ?? [])
  }
  useEffect(() => {
    load()
  }, [])

  const toggle = async (id: string, enabled: boolean) => {
    await api.setRule(id, { enabled })
    message.success(enabled ? 'Đã bật luật' : 'Đã tắt luật')
    await load()
  }

  const changeSeverity = async (id: string, severity: Severity) => {
    await api.setRule(id, { severity })
    await load()
  }

  const addSuppression = async () => {
    if (!addFor || !reason.trim()) {
      message.warning('Phải nêu lý do — sáu tháng sau còn phải hiểu vì sao đã bỏ qua')
      return
    }
    const r: any = await api.addSuppression({ rule_id: addFor, reason })
    if (r.ok !== false) {
      message.success('Đã tạo ngoại lệ')
      setAddFor(null)
      setReason('')
      await load()
    } else message.error(r.error)
  }

  const cols = [
    { title: 'Mã', dataIndex: 'id', width: 165, render: (v: string) => <span className="mono">{v}</span> },
    {
      title: 'Nhóm',
      dataIndex: 'group',
      width: 190,
      render: (g: string) => <Tag>{GROUP_LABEL[g] ?? g}</Tag>,
      filters: Object.entries(GROUP_LABEL).map(([value, text]) => ({ text, value })),
      onFilter: (v: any, r: Rule) => r.group === v,
    },
    { title: 'Luật', dataIndex: 'title', width: 260, ellipsis: true },
    { title: 'Tín hiệu', dataIndex: 'about', ellipsis: true },
    {
      title: 'Chuẩn',
      dataIndex: 'standards',
      width: 175,
      render: (s: string[]) => (
        <Space size={2} wrap>
          {s.map((x) => (
            <Tag key={x} color="geekblue" style={{ marginInlineEnd: 2 }}>
              {x}
            </Tag>
          ))}
        </Space>
      ),
    },
    {
      title: 'Mức',
      dataIndex: 'severity',
      width: 155,
      render: (v: Severity, r: Rule) => (
        <Select
          size="small"
          style={{ width: 135 }}
          value={v}
          onChange={(s) => changeSeverity(r.id, s)}
          options={Object.entries(SEV_LABEL).map(([value, label]) => ({ value, label }))}
        />
      ),
    },
    {
      title: 'Bật',
      dataIndex: 'enabled',
      width: 70,
      render: (v: boolean, r: Rule) => <Switch size="small" checked={v} onChange={(c) => toggle(r.id, c)} />,
    },
    {
      title: '',
      width: 110,
      render: (_: any, r: Rule) => (
        <Button size="small" onClick={() => setAddFor(r.id)}>
          Bỏ qua
        </Button>
      ),
    },
  ]

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <Card size="small">
        <Text type="secondary" style={{ fontSize: 12 }}>
          {rules.length} luật phát hiện. Luật là mã Rust tất định có kiểm thử — AI không tham gia
          vào việc chấm điểm hay quyết định mức nghiêm trọng, vì dữ liệu được phân tích chính là nội
          dung do agent sinh ra và có thể chứa prompt injection.
        </Text>
      </Card>

      <Table<Rule>
        rowKey="id"
        size="small"
        dataSource={rules}
        columns={cols as any}
        pagination={false}
        expandable={{
          expandedRowRender: (r) => (
            <Paragraph style={{ margin: 0 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {r.about} · mức mặc định: {SEV_LABEL[r.default_severity]} · tham số:{' '}
                <span className="mono">{JSON.stringify(r.params)}</span>
              </Text>
            </Paragraph>
          ),
        }}
      />

      <Card size="small" title={`Ngoại lệ đang có (${sups.length})`}>
        {sups.length ? (
          <Table
            rowKey="id"
            size="small"
            pagination={false}
            dataSource={sups}
            columns={
              [
                { title: 'Luật', dataIndex: 'rule_id', width: 170, render: (v: string) => <span className="mono">{v}</span> },
                { title: 'Lý do', dataIndex: 'reason' },
                { title: 'Hết hạn', dataIndex: 'until', width: 175, render: (v: string) => (v ? fmtTs(v) : 'vĩnh viễn') },
                { title: 'Tạo lúc', dataIndex: 'created_at', width: 175, render: (v: string) => fmtTs(v) },
                {
                  title: '',
                  width: 90,
                  render: (_: any, r: any) => (
                    <Button
                      size="small"
                      danger
                      onClick={async () => {
                        await api.delSuppression(r.id)
                        await load()
                      }}
                    >
                      Xoá
                    </Button>
                  ),
                },
              ] as any
            }
          />
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa bỏ qua luật nào" />
        )}
      </Card>

      <Modal
        open={!!addFor}
        title={`Bỏ qua luật ${addFor}`}
        onCancel={() => setAddFor(null)}
        onOk={addSuppression}
        okText="Tạo ngoại lệ"
        cancelText="Huỷ"
      >
        <Input.TextArea
          rows={3}
          placeholder="Lý do (bắt buộc) — ví dụ: máy này vốn chạy tác vụ nền lúc 2h sáng"
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </Modal>
    </Space>
  )
}
