// Modal thêm workspace: duyệt chọn thư mục local, hoặc clone repo git.
import { useEffect, useState } from 'react'
import {
  Alert,
  App as AntApp,
  Breadcrumb,
  Button,
  Form,
  Input,
  Modal,
  Space,
  Spin,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import { api, type FsEntry, type Workspace } from './api'

const { Text } = Typography

function FolderPicker({
  onAdded,
}: {
  onAdded: (ws: Workspace, runId?: number) => void
}) {
  const { message } = AntApp.useApp()
  const [path, setPath] = useState('')
  const [pathInput, setPathInput] = useState('')
  const [parent, setParent] = useState<string | null>(null)
  const [hasTf, setHasTf] = useState(false)
  const [entries, setEntries] = useState<FsEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [name, setName] = useState('')
  const [submitting, setSubmitting] = useState(false)

  const nav = async (p?: string) => {
    setLoading(true)
    try {
      const r = await api.fs(p)
      setPath(r.path)
      setPathInput(r.path)
      setParent(r.parent)
      setEntries(r.entries)
      setHasTf(r.has_tf)
      const base = r.path.split('/').filter(Boolean).pop() ?? ''
      setName(base)
    } catch (e) {
      message.error(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    nav()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const submit = async () => {
    setSubmitting(true)
    try {
      const r = await api.wsAdd({ source: 'folder', path, name: name || undefined })
      onAdded(r.workspace)
    } catch (e) {
      message.error(String(e))
    } finally {
      setSubmitting(false)
    }
  }

  const crumbs = ['/', ...path.split('/').filter(Boolean)]
  return (
    <div>
      <Space.Compact style={{ width: '100%', marginBottom: 8 }}>
        <Input
          value={pathInput}
          onChange={(e) => setPathInput(e.target.value)}
          onPressEnter={() => nav(pathInput)}
          placeholder="/đường/dẫn/thư/mục"
        />
        <Button onClick={() => nav(pathInput)}>Đi tới</Button>
      </Space.Compact>
      <Breadcrumb
        style={{ marginBottom: 8 }}
        items={crumbs.map((c, i) => ({
          title: (
            <a onClick={() => nav(i === 0 ? '/' : '/' + crumbs.slice(1, i + 1).join('/'))}>
              {c === '/' ? '💻' : c}
            </a>
          ),
        }))}
      />
      <div
        style={{
          border: '1px solid var(--card-border)',
          borderRadius: 8,
          height: 260,
          overflow: 'auto',
          padding: 4,
        }}
      >
        {loading ? (
          <Spin style={{ display: 'block', margin: '40px auto' }} />
        ) : (
          <>
            {parent && (
              <div className="dir-row" onClick={() => nav(parent)}>
                <span>⬆︎</span>
                <Text type="secondary">.. (lên trên)</Text>
              </div>
            )}
            {entries.map((e) => (
              <div key={e.path} className="dir-row" onClick={() => nav(e.path)}>
                <span>📁</span>
                <span style={{ flex: 1 }}>{e.name}</span>
                {e.has_tf && <Tag color="purple">.tf</Tag>}
              </div>
            ))}
            {entries.length === 0 && (
              <Text type="secondary" style={{ display: 'block', padding: 12 }}>
                (không có thư mục con)
              </Text>
            )}
          </>
        )}
      </div>
      <Space style={{ marginTop: 12, width: '100%' }} direction="vertical">
        {!hasTf && (
          <Alert
            type="warning"
            showIcon
            message="Thư mục đang chọn chưa thấy file .tf — vẫn thêm được nếu bạn chắc."
          />
        )}
        <Space.Compact style={{ width: '100%' }}>
          <Input
            addonBefore="Tên"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="tên workspace"
          />
          <Button type="primary" loading={submitting} onClick={submit}>
            Chọn thư mục này
          </Button>
        </Space.Compact>
        <Text type="secondary" style={{ fontSize: 12 }}>
          Đang chọn: <Text code>{path}</Text>
        </Text>
      </Space>
    </div>
  )
}

function GitClone({
  onAdded,
}: {
  onAdded: (ws: Workspace, runId?: number) => void
}) {
  const { message } = AntApp.useApp()
  const [form] = Form.useForm<{ repo_url: string; branch?: string; name?: string; subdir?: string }>()
  const [submitting, setSubmitting] = useState(false)

  const submit = async () => {
    const v = await form.validateFields()
    setSubmitting(true)
    try {
      const r = await api.wsAdd({
        source: 'git',
        repo_url: v.repo_url.trim(),
        branch: v.branch?.trim() || undefined,
        name: v.name?.trim() || undefined,
        subdir: v.subdir?.trim().replace(/^\/+|\/+$/g, '') || undefined,
      })
      message.success('Đang clone repo — xem console')
      onAdded(r.workspace, r.run_id)
    } catch (e) {
      message.error(String(e))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Form form={form} layout="vertical" onFinish={submit}>
      <Form.Item
        name="repo_url"
        label="URL repo"
        rules={[{ required: true, message: 'Nhập URL repo git' }]}
      >
        <Input placeholder="https://github.com/acme/infra.git hoặc git@github.com:acme/infra.git" />
      </Form.Item>
      <Space size={12} style={{ display: 'flex' }}>
        <Form.Item name="branch" label="Nhánh (tuỳ chọn)" style={{ flex: 1 }}>
          <Input placeholder="main" />
        </Form.Item>
        <Form.Item name="name" label="Tên workspace (tuỳ chọn)" style={{ flex: 1 }}>
          <Input placeholder="tự lấy tên repo" />
        </Form.Item>
      </Space>
      <Form.Item
        name="subdir"
        label="Thư mục Terraform trong repo (tuỳ chọn)"
        extra="Khi file .tf không nằm ở gốc repo. Đổi được sau ở tab Thông tin — app sẽ tự dò thư mục có .tf."
      >
        <Input placeholder="vd terraform hoặc infra/prod — trống = gốc repo" />
      </Form.Item>
      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 12 }}
        message="Repo sẽ clone về thư mục app quản lý. Trước mỗi plan/apply app tự git pull để chạy trên code mới nhất (tắt được trong tab Thông tin)."
      />
      <Button type="primary" htmlType="submit" loading={submitting}>
        Clone về
      </Button>
    </Form>
  )
}

export function AddWorkspaceModal({
  open,
  onClose,
  onAdded,
}: {
  open: boolean
  onClose: () => void
  onAdded: (ws: Workspace, runId?: number) => void
}) {
  return (
    <Modal open={open} onCancel={onClose} footer={null} width={640} title="Thêm workspace Terraform" destroyOnHidden>
      <Tabs
        items={[
          { key: 'folder', label: '📁 Thư mục trên máy', children: <FolderPicker onAdded={onAdded} /> },
          { key: 'git', label: '🔗 Clone từ Git', children: <GitClone onAdded={onAdded} /> },
        ]}
      />
    </Modal>
  )
}
