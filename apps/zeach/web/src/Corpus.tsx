import { useEffect, useState } from 'react'
import { App as AntApp, Button, Card, Table, Typography, Upload } from 'antd'
import { DeleteOutlined, InboxOutlined } from '@ant-design/icons'
import type { UploadProps } from 'antd'
import { api, type CorpusDoc, type UploadResult } from './api'

const { Text } = Typography
const { Dragger } = Upload

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

const ACCEPT = '.txt,.md,.markdown,.csv,.tsv,.json,.jsonl,.log,.html,.htm,.pdf,.docx'

/** Uploaded documents: the one source whose data this app owns. */
export default function Corpus({ onChanged }: { onChanged: () => void }) {
  const { message } = AntApp.useApp()
  const [docs, setDocs] = useState<CorpusDoc[]>([])
  const [busy, setBusy] = useState(false)

  const load = () =>
    api
      .corpus()
      .then((r) => setDocs(r.documents))
      .catch((e: Error) => message.error(e.message))

  useEffect(() => {
    load()
  }, [])

  function reportUpload(result: UploadResult) {
    result.added.forEach((a) =>
      a.duplicate
        ? message.warning(`${a.name}: ${a.message}`)
        : message.success(`${a.name}: ${a.chunks} đoạn`),
    )
    result.failed.forEach((f) => message.error(`${f.name}: ${f.error}`))
  }

  const uploadProps: UploadProps = {
    multiple: true,
    accept: ACCEPT,
    showUploadList: false,
    disabled: busy,
    beforeUpload: (_file, fileList) => {
      // Batch all files of one drop into a single multipart request.
      if (fileList[fileList.length - 1] !== _file) return false
      const dt = new DataTransfer()
      fileList.forEach((f) => dt.items.add(f as unknown as File))
      void (async () => {
        setBusy(true)
        try {
          reportUpload(await api.uploadDocs(dt.files))
          await load()
          onChanged()
        } catch (e) {
          message.error((e as Error).message)
        } finally {
          setBusy(false)
        }
      })()
      return false // never let AntD do its own XHR upload
    },
  }

  async function remove(idv: string) {
    setBusy(true)
    try {
      await api.removeDoc(idv)
      await load()
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card size="small" title="Tài liệu">
      <Dragger {...uploadProps} style={{ padding: '8px 0' }}>
        <p className="ant-upload-drag-icon" style={{ marginBottom: 4 }}>
          <InboxOutlined />
        </p>
        <p className="ant-upload-text">Kéo thả hoặc bấm để tải tài liệu</p>
        <p className="ant-upload-hint" style={{ fontSize: 12 }}>
          Cắt đoạn + lập chỉ mục toàn văn (tìm được cả khi gõ không dấu). PDF scan không có lớp văn
          bản sẽ bị từ chối — hãy OCR trước.
        </p>
      </Dragger>

      <Table<CorpusDoc>
        size="small"
        rowKey="id"
        style={{ marginTop: 12 }}
        dataSource={docs}
        pagination={false}
        locale={{ emptyText: 'Chưa có tài liệu. Nguồn “Tài liệu” sẽ không trả kết quả tới khi bạn tải lên.' }}
        columns={[
          { title: 'Tên', dataIndex: 'name', render: (n: string) => <Text>{n}</Text> },
          {
            title: 'Chi tiết',
            key: 'meta',
            render: (_, d) => (
              <Text type="secondary" style={{ fontSize: 12.5 }}>
                {d.chunks} đoạn · {humanBytes(d.bytes)} ·{' '}
                {d.uploaded_at.slice(0, 16).replace('T', ' ')}
              </Text>
            ),
          },
          {
            title: '',
            key: 'action',
            width: 44,
            render: (_, d) => (
              <Button
                size="small"
                type="text"
                danger
                icon={<DeleteOutlined />}
                disabled={busy}
                onClick={() => remove(d.id)}
              />
            ),
          },
        ]}
      />
    </Card>
  )
}
