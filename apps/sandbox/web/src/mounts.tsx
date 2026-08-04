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
import { useT } from './i18n'

/** Host folders bound into the sandbox. */
export function MountsPanel({
  sandbox,
  onChange,
}: {
  sandbox: Sandbox
  onChange: () => void
}) {
  const { message } = AntApp.useApp()
  const t = useT()
  const [source, setSource] = useState('')
  const [target, setTarget] = useState('')
  const [readOnly, setReadOnly] = useState(true)
  const [busy, setBusy] = useState(false)

  const add = async () => {
    if (!source.trim()) return message.warning(t.needHostPath)
    setBusy(true)
    try {
      await api.addMount(sandbox.id, source.trim(), target.trim(), readOnly)
      setSource('')
      setTarget('')
      onChange()
      message.success(t.mountAdded)
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
        message={t.mountsWarnTitle}
        description={t.mountsWarnBody}
      />

      <Space.Compact style={{ width: '100%' }}>
        <Input
          placeholder={t.mountPathPlaceholder}
          value={source}
          onChange={(e) => setSource(e.target.value)}
          className="sbx-mono"
          onPressEnter={() => void add()}
        />
        <Input
          placeholder={t.mountTargetPlaceholder}
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
          {t.mount}
        </Button>
      </Space.Compact>
      <Checkbox checked={readOnly} onChange={(e) => setReadOnly(e.target.checked)}>
        {t.readOnlyLabel}
      </Checkbox>

      {sandbox.mounts.length === 0 ? (
        <Empty description={t.noMounts} image={Empty.PRESENTED_IMAGE_SIMPLE} />
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
                  title={t.unmount(m.target)}
                  description={t.unmountBody}
                  okText={t.delete}
                  cancelText={t.cancel}
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
                      {m.readOnly ? t.readOnly : t.readWrite}
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
