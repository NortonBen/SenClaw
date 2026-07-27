import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  App,
  Button,
  Card,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Tag,
  Tree,
  Typography,
} from 'antd'
import type { DataNode } from 'antd/es/tree'
import {
  ApiOutlined,
  DeleteOutlined,
  PartitionOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import {
  addConnection,
  deleteConnection,
  introspectConnection,
  listConnections,
  testConnection,
} from '../api'
import { DataTable } from '../components/DataTable'
import { errMsg, fmtTime } from '../util'
import type { ConnectionView, IntrospectResp } from '../types'

const KINDS = ['postgres', 'mysql', 'sqlite']

export function Connections() {
  const { message } = App.useApp()
  const qc = useQueryClient()
  const [form] = Form.useForm<{ id?: string; kind: string; dsn: string }>()
  const [introspect, setIntrospect] = useState<IntrospectResp | null>(null)

  const conns = useQuery({ queryKey: ['connections'], queryFn: listConnections })

  const add = useMutation({
    mutationFn: (v: { id?: string; kind: string; dsn: string }) => addConnection(v),
    onSuccess: () => {
      message.success('Đã lưu kết nối (đã test)')
      form.resetFields()
      qc.invalidateQueries({ queryKey: ['connections'] })
    },
    onError: (e) => message.error(errMsg(e)),
  })

  const test = useMutation({
    mutationFn: (id: string) => testConnection(id),
    onSuccess: (_d, id) => message.success(`Kết nối '${id}' còn sống`),
    onError: (e) => message.error(errMsg(e)),
  })

  const del = useMutation({
    mutationFn: (id: string) => deleteConnection(id),
    onSuccess: () => {
      message.success('Đã xoá kết nối')
      qc.invalidateQueries({ queryKey: ['connections'] })
    },
    onError: (e) => message.error(errMsg(e)),
  })

  const intro = useMutation({
    mutationFn: (id: string) => introspectConnection(id),
    onSuccess: (d) => setIntrospect(d),
    onError: (e) => message.error(errMsg(e)),
  })

  return (
    <div>
      <Typography.Title level={4}>Kết nối nguồn</Typography.Title>

      <Card size="small" title="Thêm kết nối" style={{ marginBottom: 16, maxWidth: 720 }}>
        <Form
          form={form}
          layout="inline"
          initialValues={{ kind: 'postgres' }}
          onFinish={(v) => add.mutate(v)}
        >
          <Form.Item name="id" label="ID">
            <Input placeholder="(mặc định = kind)" style={{ width: 140 }} />
          </Form.Item>
          <Form.Item name="kind" label="Loại" rules={[{ required: true }]}>
            <Select style={{ width: 130 }} options={KINDS.map((k) => ({ value: k, label: k }))} />
          </Form.Item>
          <Form.Item
            name="dsn"
            label="DSN"
            rules={[{ required: true, message: 'nhập DSN' }]}
            style={{ flex: 1, minWidth: 260 }}
          >
            <Input.Password placeholder="postgres://user:pass@host:5432/db" />
          </Form.Item>
          <Form.Item>
            <Button type="primary" htmlType="submit" loading={add.isPending}>
              Test &amp; Lưu
            </Button>
          </Form.Item>
        </Form>
        <Typography.Text type="secondary">
          Kết nối được test trước khi lưu — nguồn chết sẽ không được lưu.
        </Typography.Text>
      </Card>

      <DataTable<ConnectionView>
        rowKey="id"
        loading={conns.isLoading}
        dataSource={conns.data?.connections ?? []}
        columns={[
          { title: 'ID', dataIndex: 'id' },
          { title: 'Loại', dataIndex: 'kind', render: (v: string) => <Tag>{v}</Tag> },
          { title: 'DSN (đã ẩn)', dataIndex: 'dsn', ellipsis: true, render: (v: string) => <code>{v}</code> },
          {
            title: 'OK gần nhất',
            dataIndex: 'last_ok_at',
            render: (v: string | null) => fmtTime(v),
          },
          {
            title: 'Thao tác',
            key: 'act',
            render: (_: unknown, r: ConnectionView) => (
              <Space size="small">
                <Button
                  size="small"
                  icon={<ThunderboltOutlined />}
                  loading={test.isPending && test.variables === r.id}
                  onClick={() => test.mutate(r.id)}
                >
                  Test
                </Button>
                <Button
                  size="small"
                  icon={<PartitionOutlined />}
                  loading={intro.isPending && intro.variables === r.id}
                  onClick={() => intro.mutate(r.id)}
                >
                  Introspect
                </Button>
                <Popconfirm
                  title="Xoá kết nối?"
                  okButtonProps={{ danger: true }}
                  onConfirm={() => del.mutate(r.id)}
                >
                  <Button size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              </Space>
            ),
          },
        ]}
      />

      <Modal
        open={!!introspect}
        title={
          <Space>
            <ApiOutlined /> Introspect: {introspect?.connection_id}
          </Space>
        }
        footer={null}
        width={640}
        onCancel={() => setIntrospect(null)}
      >
        {introspect && (
          <>
            <Typography.Paragraph type="secondary">
              {introspect.total} bảng
            </Typography.Paragraph>
            <Tree treeData={introspectTree(introspect)} height={480} defaultExpandParent />
          </>
        )}
      </Modal>
    </div>
  )
}

function introspectTree(resp: IntrospectResp): DataNode[] {
  return resp.tables.map((t, ti) => {
    const key = `${t.schema ?? ''}.${t.name}.${ti}`
    return {
      key,
      title: (
        <span>
          {t.schema ? `${t.schema}.` : ''}
          {t.name} <Tag>{t.columns.length} cột</Tag>
        </span>
      ),
      children: t.columns.map((c, ci) => ({
        key: `${key}.${c.name}.${ci}`,
        title: `${c.name} : ${c.data_type}${c.nullable ? '' : ' NOT NULL'}`,
      })),
    }
  })
}
