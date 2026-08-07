// Dialog chọn file .tfvars: quét TOÀN workspace (kể cả ngoài thư mục Terraform),
// chọn xong trả về đường dẫn tương đối so với work_dir để dùng làm -var-file.
import { useEffect, useState } from 'react'
import { App as AntApp, Empty, Modal, Spin, Tag, Typography } from 'antd'
import { api } from './api'

const { Text } = Typography

interface PickFile {
  rel: string
  display: string
  in_work_dir: boolean
}

export function TfvarsPickerModal({
  wsId,
  open,
  onClose,
  onPicked,
}: {
  wsId: number
  open: boolean
  onClose: () => void
  onPicked: (rel: string) => void
}) {
  const { message } = AntApp.useApp()
  const [files, setFiles] = useState<PickFile[] | null>(null)
  const [pick, setPick] = useState('')

  useEffect(() => {
    if (!open) return
    setFiles(null)
    api
      .tfvarsFiles(wsId)
      .then((r) => {
        setFiles(r.files)
        setPick(r.current || r.files[0]?.rel || '')
      })
      .catch((e) => {
        message.error(String(e))
        setFiles([])
      })
  }, [open, wsId, message])

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title="Chọn file .tfvars trong workspace"
      width={560}
      okText="Dùng file này"
      cancelText="Thôi"
      okButtonProps={{ disabled: !pick }}
      onOk={() => {
        onPicked(pick)
        onClose()
      }}
    >
      {files == null ? (
        <Spin style={{ display: 'block', margin: '32px auto' }} />
      ) : files.length === 0 ? (
        <Empty description={
          <span>
            Không tìm thấy file .tfvars nào trong workspace.
            <br />
            <Text type="secondary">Tạo mới bằng nút &quot;File mới&quot; ở tab Biến &amp; Chạy.</Text>
          </span>
        } />
      ) : (
        <div style={{ maxHeight: 320, overflow: 'auto' }}>
          {files.map((f) => (
            <div
              key={f.rel}
              className="dir-row"
              onClick={() => setPick(f.rel)}
              style={
                pick === f.rel
                  ? { background: 'var(--accent-bg)', border: '1px solid var(--accent)' }
                  : undefined
              }
            >
              <span>{pick === f.rel ? '✅' : '📄'}</span>
              <span style={{ flex: 1 }}>
                <Text code>{f.display}</Text>
              </span>
              {f.in_work_dir ? (
                <Tag color="green">trong thư mục Terraform</Tag>
              ) : f.rel.startsWith('../') ? (
                <Tag color="orange">ngoài — dùng {f.rel}</Tag>
              ) : (
                <Tag color="blue">thư mục con — dùng {f.rel}</Tag>
              )}
            </div>
          ))}
        </div>
      )}
      <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 10 }}>
        File ngoài thư mục Terraform vẫn dùng được — app truyền đường dẫn tương đối cho{' '}
        <Text code>-var-file</Text>.
      </Text>
    </Modal>
  )
}
