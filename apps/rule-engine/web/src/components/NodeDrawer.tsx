// Per-node inspector: generated config form, runtime options, docs, and the
// list of edges touching this node.

import { useMemo, useRef } from 'react'
import {
  Alert,
  Button,
  Drawer,
  Form,
  Input,
  InputNumber,
  Popconfirm,
  Select,
  Switch,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import { DeleteOutlined } from '@ant-design/icons'
import type { Edge as RFEdge } from '@xyflow/react'
import Markdown from './Markdown'
import SchemaForm from './SchemaForm'
import type { RuleFlowNode, RuleNodeData } from './RuleNode'
import { DEFAULT_OPTS, type JoinPolicy, type JsonObject, type NodeOpts } from '../types'

const JOIN_HELP: Record<JoinPolicy, string> = {
  any: 'Mỗi message tới là node chạy một lần (mặc định).',
  all: 'Chờ đủ một message ở mỗi cổng vào đã nối rồi mới chạy một lần, dữ liệu gói theo tên cổng.',
  merge: 'Như `all`, nhưng các phần được deep-merge thành một object.',
}

interface EdgeRow {
  key: string
  dir: 'in' | 'out'
  myPort: string
  otherNode: string
  otherLabel: string
  otherPort: string
}

export default function NodeDrawer({
  node,
  nodes,
  edges,
  onClose,
  onPatch,
  onDelete,
  onDeleteEdge,
}: {
  node: RuleFlowNode | null
  nodes: RuleFlowNode[]
  edges: RFEdge[]
  onClose: () => void
  onPatch: (patch: Partial<RuleNodeData>) => void
  onDelete: () => void
  onDeleteEdge: (edgeId: string) => void
}) {
  // Keep the last selection alive through the close animation so the header
  // does not flash an empty "Node" title.
  const lastNode = useRef<RuleFlowNode | null>(null)
  if (node) lastNode.current = node
  const shown = node ?? lastNode.current

  const data = shown?.data
  const spec = data?.spec
  const opts: NodeOpts = { ...DEFAULT_OPTS, ...(data?.opts ?? {}) }

  const label = useMemo(() => {
    const map = new Map(nodes.map((n) => [n.id, n.data.name || n.id]))
    return (id: string) => map.get(id) ?? id
  }, [nodes])

  const edgeRows: EdgeRow[] = useMemo(() => {
    if (!shown) return []
    const rows: EdgeRow[] = []
    for (const e of edges) {
      if (e.source === shown.id) {
        rows.push({
          key: e.id,
          dir: 'out',
          myPort: e.sourceHandle ?? 'out',
          otherNode: e.target,
          otherLabel: label(e.target),
          otherPort: e.targetHandle ?? 'in',
        })
      } else if (e.target === shown.id) {
        rows.push({
          key: e.id,
          dir: 'in',
          myPort: e.targetHandle ?? 'in',
          otherNode: e.source,
          otherLabel: label(e.source),
          otherPort: e.sourceHandle ?? 'out',
        })
      }
    }
    return rows
  }, [edges, shown, label])

  const setOpt = <K extends keyof NodeOpts>(key: K, v: NodeOpts[K]) =>
    onPatch({ opts: { ...opts, [key]: v } })

  return (
    <Drawer
      open={Boolean(node)}
      onClose={onClose}
      size={620}
      mask={false}
      title={
        <span>
          {spec?.icon ?? '❓'} {spec?.name ?? data?.ruleId ?? 'Node'}{' '}
          <Typography.Text type="secondary" style={{ fontSize: 12, fontWeight: 400 }}>
            #{shown?.id}
          </Typography.Text>
        </span>
      }
      extra={
        <Popconfirm
          title="Xoá node này?"
          description="Mọi cạnh nối tới nó cũng bị xoá."
          okText="Xoá"
          cancelText="Huỷ"
          okButtonProps={{ danger: true }}
          onConfirm={onDelete}
        >
          <Button danger size="small" icon={<DeleteOutlined />}>
            Xoá node
          </Button>
        </Popconfirm>
      }
    >
      {shown && data && (
        <Tabs
          defaultActiveKey="config"
          items={[
            {
              key: 'config',
              label: 'Cấu hình',
              children: (
                <div>
                  <Form layout="vertical">
                    <Form.Item label="Tên node" style={{ marginBottom: 14 }}>
                      <Input
                        value={data.name}
                        placeholder="Tên hiển thị trên canvas"
                        onChange={(e) => onPatch({ name: e.target.value })}
                      />
                    </Form.Item>
                  </Form>

                  {data.errors.length > 0 && (
                    <Alert
                      type="error"
                      showIcon
                      style={{ marginBottom: 14 }}
                      title="Node đang có lỗi"
                      description={
                        <ul style={{ margin: 0, paddingLeft: 18 }}>
                          {data.errors.map((m, i) => (
                            <li key={i}>{m}</li>
                          ))}
                        </ul>
                      }
                    />
                  )}

                  <SchemaForm
                    key={shown.id}
                    schema={spec?.config_schema}
                    value={(data.config ?? {}) as JsonObject}
                    onChange={(config) => onPatch({ config })}
                  />

                  <Typography.Title level={5} style={{ marginTop: 20 }}>
                    Liên kết
                  </Typography.Title>
                  <Table<EdgeRow>
                    size="small"
                    pagination={false}
                    dataSource={edgeRows}
                    locale={{ emptyText: 'Node này chưa nối với node nào' }}
                    columns={[
                      {
                        title: 'Chiều',
                        dataIndex: 'dir',
                        width: 70,
                        render: (d: 'in' | 'out') =>
                          d === 'in' ? <Tag color="blue">vào</Tag> : <Tag color="green">ra</Tag>,
                      },
                      {
                        title: 'Cổng của node',
                        dataIndex: 'myPort',
                        render: (p: string) => <code>{p}</code>,
                      },
                      {
                        title: 'Node đối diện',
                        key: 'other',
                        render: (_, r) => (
                          <span>
                            {r.otherLabel} · <code>{r.otherPort}</code>
                          </span>
                        ),
                      },
                      {
                        title: '',
                        key: 'act',
                        width: 44,
                        render: (_, r) => (
                          <Button
                            size="small"
                            type="text"
                            danger
                            icon={<DeleteOutlined />}
                            onClick={() => onDeleteEdge(r.key)}
                          />
                        ),
                      },
                    ]}
                  />
                </div>
              ),
            },
            {
              key: 'opts',
              label: 'Chạy & gộp',
              children: (
                <Form layout="vertical">
                  <Form.Item label="Cách gộp cổng vào (join)" extra={JOIN_HELP[opts.join]}>
                    <Select<JoinPolicy>
                      value={opts.join}
                      onChange={(v) => setOpt('join', v)}
                      options={[
                        { value: 'any', label: 'any — chạy mỗi khi có message' },
                        { value: 'all', label: 'all — chờ đủ mọi cổng vào' },
                        { value: 'merge', label: 'merge — chờ đủ rồi trộn thành một object' },
                      ]}
                    />
                  </Form.Item>

                  <Form.Item
                    label="Khoá tương quan (corrKey)"
                    extra="Gộp theo một giá trị trong message thay vì theo run. Vd `device_id`."
                  >
                    <Input
                      allowClear
                      placeholder="để trống = gộp theo run"
                      value={opts.corrKey ?? ''}
                      onChange={(e) => setOpt('corrKey', e.target.value || null)}
                    />
                  </Form.Item>

                  <Form.Item
                    label="Hết hạn chờ gộp (ms)"
                    extra="Bỏ trống = chờ vô hạn trong phạm vi một run."
                  >
                    <InputNumber
                      style={{ width: '100%' }}
                      min={0}
                      step={500}
                      value={opts.joinTimeoutMs ?? undefined}
                      onChange={(v) => setOpt('joinTimeoutMs', v ?? null)}
                    />
                  </Form.Item>

                  <Form.Item
                    label="Số worker song song (concurrency)"
                    extra="1 giữ đúng thứ tự message của node. Phải >= 1."
                  >
                    <InputNumber
                      style={{ width: '100%' }}
                      min={1}
                      max={64}
                      value={opts.concurrency}
                      onChange={(v) => setOpt('concurrency', v ?? 1)}
                    />
                  </Form.Item>

                  <Form.Item label="Số lần thử lại khi lỗi (retries)">
                    <InputNumber
                      style={{ width: '100%' }}
                      min={0}
                      max={20}
                      value={opts.retries}
                      onChange={(v) => setOpt('retries', v ?? 0)}
                    />
                  </Form.Item>

                  <Form.Item label="Giãn cách giữa các lần thử (ms)">
                    <InputNumber
                      style={{ width: '100%' }}
                      min={0}
                      step={100}
                      value={opts.retryBackoffMs}
                      onChange={(v) => setOpt('retryBackoffMs', v ?? 0)}
                    />
                  </Form.Item>

                  <Form.Item
                    label="Debug node này"
                    extra="Ghi trace từng bước cho riêng node này, kể cả khi luồng không bật debug."
                  >
                    <Switch checked={data.debug} onChange={(v) => onPatch({ debug: v })} />
                  </Form.Item>
                </Form>
              ),
            },
            {
              key: 'doc',
              label: 'Tài liệu',
              children: <Markdown text={spec?.doc ?? ''} />,
            },
          ]}
        />
      )}
    </Drawer>
  )
}
