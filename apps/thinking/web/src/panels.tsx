import { useEffect, useState } from 'react'
import {
  App as AntApp,
  Button,
  Form,
  Input,
  Modal,
  Popconfirm,
  Progress,
  Select,
  Space,
  Table,
  Tag,
} from 'antd'
import { PlusOutlined, ReloadOutlined } from '@ant-design/icons'
import {
  api,
  fmtTime,
  PRIORITY_COLORS,
  PRIORITY_LABELS,
  STATUS_COLORS,
  STATUS_LABELS,
  type ActivityRow,
  type Problem,
} from './api'

export function ProblemsTab({ onOpen, onChange }: { onOpen: (id: number) => void; onChange: () => void }) {
  const { message } = AntApp.useApp()
  const [rows, setRows] = useState<Problem[]>([])
  const [loading, setLoading] = useState(false)
  const [q, setQ] = useState('')
  const [status, setStatus] = useState<string | undefined>()
  const [creating, setCreating] = useState(false)
  const [form] = Form.useForm()

  const load = (query = q, st = status) => {
    setLoading(true)
    api
      .problems({ q: query, status: st })
      .then((r) => setRows(r.problems))
      .finally(() => setLoading(false))
  }
  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status])

  const create = async () => {
    const v = await form.validateFields()
    const r = await api.problemAdd(v)
    if (r.error) {
      message.error(r.error)
      return
    }
    message.success('Đã tạo vấn đề')
    setCreating(false)
    form.resetFields()
    load()
    onChange()
    if (r.problem) onOpen(r.problem.id)
  }

  const remove = async (id: number) => {
    const r = await api.problemDelete(id)
    if (r.error) message.error(r.error)
    else message.success('Đã xoá')
    load()
    onChange()
  }

  return (
    <div>
      <Space style={{ marginBottom: 12 }} wrap>
        <Input.Search
          placeholder="Tìm tiêu đề / mô tả / tags"
          allowClear
          style={{ width: 260 }}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onSearch={() => load()}
        />
        <Select
          placeholder="Trạng thái"
          allowClear
          style={{ width: 160 }}
          value={status}
          onChange={setStatus}
          options={Object.entries(STATUS_LABELS).map(([value, label]) => ({ value, label }))}
        />
        <Button icon={<ReloadOutlined />} onClick={() => load()} />
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreating(true)}>
          Vấn đề mới
        </Button>
      </Space>

      <Table
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={rows}
        pagination={{ pageSize: 15, hideOnSinglePage: true }}
        columns={[
          {
            title: 'Vấn đề',
            dataIndex: 'title',
            render: (t: string, r: Problem) => (
              <a onClick={() => onOpen(r.id)}>
                {t} {r.tags && r.tags.split(',').filter(Boolean).map((x) => <Tag key={x}>{x.trim()}</Tag>)}
              </a>
            ),
          },
          {
            title: 'Trạng thái',
            dataIndex: 'status',
            width: 130,
            render: (s: string) => <Tag color={STATUS_COLORS[s]}>{STATUS_LABELS[s] ?? s}</Tag>,
          },
          {
            title: 'Ưu tiên',
            dataIndex: 'priority',
            width: 110,
            render: (p: string) => <Tag color={PRIORITY_COLORS[p]}>{PRIORITY_LABELS[p] ?? p}</Tag>,
          },
          {
            title: 'Phân tích (5W 40% + 6 mũ 60%)',
            dataIndex: 'completeness',
            width: 200,
            render: (c: number) => <Progress percent={c} size="small" />,
          },
          { title: 'GP', dataIndex: 'solution_count', width: 60, align: 'center' as const },
          { title: 'Cập nhật', dataIndex: 'updated_at', width: 150, render: fmtTime },
          {
            title: '',
            width: 70,
            render: (_: unknown, r: Problem) => (
              <Popconfirm title={`Xoá "${r.title}" cùng toàn bộ phân tích?`} onConfirm={() => remove(r.id)}>
                <Button size="small" danger type="text">
                  Xoá
                </Button>
              </Popconfirm>
            ),
          },
        ]}
      />

      <Modal title="Tạo vấn đề mới" open={creating} onOk={create} onCancel={() => setCreating(false)} okText="Tạo" cancelText="Huỷ">
        <Form form={form} layout="vertical" initialValues={{ priority: 'normal' }}>
          <Form.Item name="title" label="Tiêu đề" rules={[{ required: true, message: 'Nhập tiêu đề vấn đề' }]}>
            <Input placeholder="VD: Doanh số quý này giảm 30%" />
          </Form.Item>
          <Form.Item name="description" label="Mô tả — chuyện gì đang xảy ra?">
            <Input.TextArea rows={3} placeholder="Mô tả càng cụ thể, phân tích 5W/6 mũ càng chất lượng" />
          </Form.Item>
          <Form.Item name="context" label="Bối cảnh — ai liên quan, quy mô, ràng buộc">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Form.Item name="goal" label="Mục tiêu — kết quả mong muốn">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Space style={{ width: '100%' }} size="large">
            <Form.Item name="priority" label="Ưu tiên">
              <Select
                style={{ width: 160 }}
                options={Object.entries(PRIORITY_LABELS).map(([value, label]) => ({ value, label }))}
              />
            </Form.Item>
            <Form.Item name="tags" label="Tags (phẩy)" style={{ flex: 1 }}>
              <Input placeholder="kinh doanh, nhân sự…" />
            </Form.Item>
          </Space>
        </Form>
      </Modal>
    </div>
  )
}

export function ActivityTab() {
  const [rows, setRows] = useState<ActivityRow[]>([])
  useEffect(() => {
    api.activity().then((r) => setRows(r.activity))
  }, [])
  return (
    <Table
      rowKey={(r) => `${r.created_at}-${r.text}`}
      size="small"
      dataSource={rows}
      pagination={{ pageSize: 20, hideOnSinglePage: true }}
      columns={[
        { title: 'Lúc', dataIndex: 'created_at', width: 170, render: fmtTime },
        { title: 'Loại', dataIndex: 'kind', width: 100, render: (k: string) => <Tag>{k}</Tag> },
        { title: 'Hành động', dataIndex: 'text' },
      ]}
    />
  )
}
