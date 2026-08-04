import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Breadcrumb,
  Button,
  Empty,
  Input,
  List,
  Popconfirm,
  Space,
  Spin,
  Typography,
} from 'antd'
import { DeleteOutlined, FileOutlined, FolderFilled, ReloadOutlined, SaveOutlined } from '@ant-design/icons'
import { api, type FileEntry } from './api'
import { useT } from './i18n'

/** Files inside a sandbox, browsable and editable. */
export function FilesPanel({ sandboxId }: { sandboxId: string }) {
  const { message } = AntApp.useApp()
  const t = useT()
  const [dir, setDir] = useState('')
  const [entries, setEntries] = useState<FileEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [open, setOpen] = useState<{ path: string; content: string } | null>(null)
  const [saving, setSaving] = useState(false)

  const load = useCallback(
    async (path: string) => {
      setLoading(true)
      try {
        const r = await api.listFiles(sandboxId, path)
        setEntries(r.entries)
        setDir(path)
      } catch (e) {
        message.error((e as Error).message)
      } finally {
        setLoading(false)
      }
    },
    [sandboxId, message],
  )

  useEffect(() => {
    void load('')
    setOpen(null)
  }, [sandboxId, load])

  const openEntry = async (e: FileEntry) => {
    if (e.dir) return void load(e.path)
    try {
      const r = await api.readFile(sandboxId, e.path)
      setOpen({ path: e.path, content: r.content })
    } catch (err) {
      message.error((err as Error).message)
    }
  }

  const save = async () => {
    if (!open) return
    setSaving(true)
    try {
      await api.writeFile(sandboxId, open.path, open.content)
      message.success(t.saved(open.path))
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setSaving(false)
    }
  }

  const remove = async (e: FileEntry) => {
    try {
      await api.deleteFile(sandboxId, e.path)
      if (open?.path === e.path) setOpen(null)
      void load(dir)
    } catch (err) {
      message.error((err as Error).message)
    }
  }

  const segments = dir ? dir.split('/').filter(Boolean) : []

  return (
    <Space direction="vertical" style={{ width: '100%' }} size={12}>
      <Space wrap>
        <Breadcrumb
          items={[
            { title: <a onClick={() => void load('')}>sandbox</a> },
            ...segments.map((s, i) => ({
              title: (
                <a onClick={() => void load(segments.slice(0, i + 1).join('/'))}>{s}</a>
              ),
            })),
          ]}
        />
        <Button size="small" icon={<ReloadOutlined />} onClick={() => void load(dir)}>
          {t.reload}
        </Button>
      </Space>

      <Spin spinning={loading}>
        {entries.length === 0 ? (
          <Empty description={t.emptyFolder} image={Empty.PRESENTED_IMAGE_SIMPLE} />
        ) : (
          <List
            size="small"
            bordered
            dataSource={entries}
            renderItem={(e) => (
              <List.Item
                actions={[
                  <Popconfirm
                    key="del"
                    title={t.deleteEntry(e.name)}
                    description={e.dir ? t.deleteFolderWarning : undefined}
                    okText={t.delete}
                    cancelText={t.cancel}
                    onConfirm={() => void remove(e)}
                  >
                    <Button size="small" type="text" danger icon={<DeleteOutlined />} />
                  </Popconfirm>,
                ]}
              >
                <List.Item.Meta
                  avatar={e.dir ? <FolderFilled style={{ color: '#e8b339' }} /> : <FileOutlined />}
                  title={<a onClick={() => void openEntry(e)}>{e.name}</a>}
                  description={e.dir ? t.folder : t.bytes(e.size.toLocaleString())}
                />
              </List.Item>
            )}
          />
        )}
      </Spin>

      {open && (
        <Space direction="vertical" style={{ width: '100%' }} size={8}>
          <Space>
            <Typography.Text strong className="sbx-mono">
              {open.path}
            </Typography.Text>
            <Button
              size="small"
              type="primary"
              icon={<SaveOutlined />}
              loading={saving}
              onClick={() => void save()}
            >
              {t.save}
            </Button>
            <Button size="small" onClick={() => setOpen(null)}>
              {t.close}
            </Button>
          </Space>
          <Input.TextArea
            className="sbx-mono sbx-editor"
            rows={16}
            value={open.content}
            onChange={(ev) => setOpen({ ...open, content: ev.target.value })}
          />
        </Space>
      )}
    </Space>
  )
}
