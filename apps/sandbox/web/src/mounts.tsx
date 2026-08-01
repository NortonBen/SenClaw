import { useState } from 'react'
import {
  Alert,
  App as AntApp,
  Button,
  Checkbox,
  Empty,
  Input,
  List,
  Popconfirm,
  Space,
  Tag,
  Typography,
} from 'antd'
import { DeleteOutlined, FolderAddOutlined } from '@ant-design/icons'
import { api, type Sandbox } from './api'

/** Host folders bound into the sandbox. */
export function MountsPanel({
  sandbox,
  onChange,
}: {
  sandbox: Sandbox
  onChange: () => void
}) {
  const { message } = AntApp.useApp()
  const [source, setSource] = useState('')
  const [target, setTarget] = useState('')
  const [readOnly, setReadOnly] = useState(true)
  const [busy, setBusy] = useState(false)

  const add = async () => {
    if (!source.trim()) return message.warning('Nhập đường dẫn thư mục trên máy')
    setBusy(true)
    try {
      await api.addMount(sandbox.id, source.trim(), target.trim(), readOnly)
      setSource('')
      setTarget('')
      onChange()
      message.success('Đã gắn thư mục')
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const remove = async (t: string) => {
    try {
      await api.removeMount(sandbox.id, t)
      onChange()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={14}>
      <Alert
        type="info"
        showIcon
        message="Thư mục gắn là lỗ hổng có chủ ý trên hàng rào sandbox"
        description="Mã trong sandbox đọc và ghi thẳng vào thư mục thật trên máy bạn. Nếu chỉ cần đọc dữ liệu, hãy để 'Chỉ đọc'. Không gắn được thư mục nhà, thư mục hệ thống hay nơi chứa khoá bí mật."
      />

      <Space.Compact style={{ width: '100%' }}>
        <Input
          placeholder="/Users/ban/du-an/du-lieu"
          value={source}
          onChange={(e) => setSource(e.target.value)}
          className="sbx-mono"
          onPressEnter={() => void add()}
        />
        <Input
          placeholder="tên trong sandbox (bỏ trống = tên thư mục)"
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          style={{ maxWidth: 280 }}
          onPressEnter={() => void add()}
        />
        <Button
          type="primary"
          icon={<FolderAddOutlined />}
          loading={busy}
          onClick={() => void add()}
        >
          Gắn
        </Button>
      </Space.Compact>
      <Checkbox checked={readOnly} onChange={(e) => setReadOnly(e.target.checked)}>
        Chỉ đọc — sandbox không sửa được dữ liệu thật
      </Checkbox>

      {sandbox.mounts.length === 0 ? (
        <Empty description="Chưa gắn thư mục nào" image={Empty.PRESENTED_IMAGE_SIMPLE} />
      ) : (
        <List
          size="small"
          bordered
          dataSource={sandbox.mounts}
          renderItem={(m) => (
            <List.Item
              actions={[
                <Popconfirm
                  key="x"
                  title={`Gỡ "${m.target}"?`}
                  description="Chỉ gỡ khỏi sandbox. Dữ liệu trên máy vẫn nguyên."
                  okText="Gỡ"
                  cancelText="Thôi"
                  onConfirm={() => void remove(m.target)}
                >
                  <Button size="small" type="text" danger icon={<DeleteOutlined />} />
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                title={
                  <Space size={6} wrap>
                    <Typography.Text className="sbx-mono">{m.target}</Typography.Text>
                    <Tag color={m.readOnly ? 'blue' : 'orange'}>
                      {m.readOnly ? 'chỉ đọc' : 'đọc-ghi'}
                    </Tag>
                  </Space>
                }
                description={
                  <Typography.Text type="secondary" className="sbx-mono">
                    ← {m.source}
                  </Typography.Text>
                }
              />
            </List.Item>
          )}
        />
      )}
    </Space>
  )
}
