// Chain list — the app's landing view.

import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Button,
  Card,
  Dropdown,
  Flex,
  Input,
  Modal,
  Popconfirm,
  Space,
  Statistic,
  Table,
  Tag,
  Typography,
  theme,
} from 'antd'
import {
  DeleteOutlined,
  EditOutlined,
  MoreOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  PoweroffOutlined,
  ReloadOutlined,
  SelectOutlined,
} from '@ant-design/icons'
import dayjs from 'dayjs'
import { api } from '../api'
import type { Chain, ChainStatus, EngineStatus } from '../types'

const STATUS_TAG: Record<ChainStatus, { color: string; text: string }> = {
  ACTIVE: { color: 'green', text: 'ACTIVE' },
  INACTIVE: { color: 'default', text: 'INACTIVE' },
  ERROR: { color: 'red', text: 'ERROR' },
}

export default function ChainList({ onOpen }: { onOpen: (id: number) => void }) {
  const { message } = AntApp.useApp()
  const { token } = theme.useToken()
  const [chains, setChains] = useState<Chain[]>([])
  const [status, setStatus] = useState<EngineStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [creating, setCreating] = useState(false)
  const [newName, setNewName] = useState('')
  const [newDesc, setNewDesc] = useState('')
  const [renaming, setRenaming] = useState<Chain | null>(null)
  const [renameText, setRenameText] = useState('')

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const [list, st] = await Promise.all([api.listChains(), api.status().catch(() => null)])
      setChains(list)
      setStatus(st)
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }, [message])

  useEffect(() => {
    void reload()
  }, [reload])

  const run = async (fn: () => Promise<unknown>, ok: string) => {
    try {
      await fn()
      message.success(ok)
      await reload()
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  const create = async () => {
    if (!newName.trim()) {
      message.warning('Nhập tên luồng đã.')
      return
    }
    try {
      const chain = await api.createChain(newName.trim(), newDesc.trim())
      setCreating(false)
      setNewName('')
      setNewDesc('')
      message.success('Đã tạo luồng.')
      onOpen(chain.id)
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div style={{ padding: 20, maxWidth: 1200, margin: '0 auto' }}>
      <Flex align="center" justify="space-between" wrap gap={12} style={{ marginBottom: 16 }}>
        <div>
          <Typography.Title level={3} style={{ margin: 0 }}>
            🕸️ Luồng xử lý
          </Typography.Title>
          <Typography.Text type="secondary">
            Kéo-thả node, nối cổng vào/ra, chạy realtime.
          </Typography.Text>
        </div>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={() => void reload()}>
            Tải lại
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreating(true)}>
            Tạo luồng
          </Button>
        </Space>
      </Flex>

      {status && (
        <Flex gap={12} wrap style={{ marginBottom: 16 }}>
          <Card size="small" style={{ minWidth: 130 }}>
            <Statistic title="Tổng luồng" value={status.chains} />
          </Card>
          <Card size="small" style={{ minWidth: 130 }}>
            <Statistic
              title="Đang bật"
              value={status.active}
              valueStyle={{ color: token.colorSuccess }}
            />
          </Card>
          <Card size="small" style={{ minWidth: 130 }}>
            <Statistic title="Đang chạy" value={status.runningRuns} />
          </Card>
          <Card size="small" style={{ minWidth: 130 }}>
            <Statistic title="Loại node" value={status.nodeTypes} />
          </Card>
        </Flex>
      )}

      <Card size="small" styles={{ body: { padding: 0 } }}>
        <Table<Chain>
          rowKey="id"
          loading={loading}
          dataSource={chains}
          pagination={{ pageSize: 20, hideOnSinglePage: true }}
          scroll={{ x: 720 }}
          locale={{ emptyText: 'Chưa có luồng nào. Bấm “Tạo luồng” để bắt đầu.' }}
          columns={[
            {
              title: 'Tên',
              dataIndex: 'name',
              render: (_, r) => (
                <div>
                  <Typography.Link onClick={() => onOpen(r.id)} strong>
                    {r.name}
                  </Typography.Link>
                  {r.description && (
                    <div>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        {r.description}
                      </Typography.Text>
                    </div>
                  )}
                </div>
              ),
            },
            {
              title: 'Trạng thái',
              dataIndex: 'status',
              width: 190,
              render: (s: ChainStatus, r) => (
                <Space size={6}>
                  <Tag color={STATUS_TAG[s]?.color ?? 'default'}>{STATUS_TAG[s]?.text ?? s}</Tag>
                  {r.deployed && (
                    <Tag
                      color="processing"
                      style={{ marginInlineEnd: 0 }}
                      icon={<span style={{ marginRight: 4 }}>●</span>}
                    >
                      đang chạy
                    </Tag>
                  )}
                  {r.debug && <Tag>🐞</Tag>}
                </Space>
              ),
            },
            {
              title: 'Cập nhật',
              dataIndex: 'updated_at',
              width: 170,
              render: (v: string) => (v ? dayjs(v).format('DD/MM/YYYY HH:mm') : '—'),
            },
            {
              title: 'Thao tác',
              key: 'act',
              width: 90,
              align: 'right',
              render: (_, r) => (
                <Dropdown
                  trigger={['click']}
                  menu={{
                    items: [
                      { key: 'open', icon: <SelectOutlined />, label: 'Mở' },
                      r.status === 'ACTIVE'
                        ? { key: 'stop', icon: <PoweroffOutlined />, label: 'Dừng' }
                        : { key: 'start', icon: <PlayCircleOutlined />, label: 'Kích hoạt' },
                      { key: 'rename', icon: <EditOutlined />, label: 'Đổi tên' },
                      { type: 'divider' as const },
                      {
                        key: 'delete',
                        icon: <DeleteOutlined />,
                        danger: true,
                        label: (
                          <Popconfirm
                            title="Xoá luồng này?"
                            description="Node, cạnh, run và log đều bị xoá."
                            okText="Xoá"
                            cancelText="Huỷ"
                            okButtonProps={{ danger: true }}
                            onConfirm={() =>
                              void run(() => api.deleteChain(r.id), 'Đã xoá luồng.')
                            }
                          >
                            <span onClick={(e) => e.stopPropagation()}>Xoá</span>
                          </Popconfirm>
                        ),
                      },
                    ],
                    onClick: ({ key, domEvent }) => {
                      domEvent.stopPropagation()
                      if (key === 'open') onOpen(r.id)
                      else if (key === 'start')
                        void run(() => api.activate(r.id), 'Đã kích hoạt luồng.')
                      else if (key === 'stop')
                        void run(() => api.deactivate(r.id), 'Đã dừng luồng.')
                      else if (key === 'rename') {
                        setRenaming(r)
                        setRenameText(r.name)
                      }
                    },
                  }}
                >
                  <Button type="text" icon={<MoreOutlined />} />
                </Dropdown>
              ),
            },
          ]}
        />
      </Card>

      <Modal
        open={creating}
        title="Tạo luồng mới"
        okText="Tạo"
        cancelText="Huỷ"
        onCancel={() => setCreating(false)}
        onOk={() => void create()}
      >
        <Space direction="vertical" style={{ width: '100%' }} size={12}>
          <Input
            autoFocus
            placeholder="Tên luồng"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onPressEnter={() => void create()}
          />
          <Input.TextArea
            rows={3}
            placeholder="Mô tả (không bắt buộc)"
            value={newDesc}
            onChange={(e) => setNewDesc(e.target.value)}
          />
        </Space>
      </Modal>

      <Modal
        open={Boolean(renaming)}
        title="Đổi tên luồng"
        okText="Lưu"
        cancelText="Huỷ"
        onCancel={() => setRenaming(null)}
        onOk={() => {
          const target = renaming
          if (!target) return
          setRenaming(null)
          void run(() => api.patchChain(target.id, { name: renameText.trim() }), 'Đã đổi tên.')
        }}
      >
        <Input
          autoFocus
          value={renameText}
          onChange={(e) => setRenameText(e.target.value)}
        />
      </Modal>
    </div>
  )
}
