import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Alert,
  Button,
  Card,
  Checkbox,
  Descriptions,
  Empty,
  Input,
  List,
  Modal,
  Popconfirm,
  Space,
  Spin,
  Tag,
  Typography,
  Upload,
} from 'antd'
import {
  DeleteOutlined,
  FileAddOutlined,
  ReloadOutlined,
  RobotOutlined,
  UploadOutlined,
} from '@ant-design/icons'
import { del, get, post, upload, type Doc, type Section, type Suspect } from './api'

const STATUS: Record<string, { color: string; label: string }> = {
  new: { color: 'default', label: 'mới nạp' },
  outlined: { color: 'blue', label: 'đã chia mục' },
  enriched: { color: 'green', label: 'đã mô tả' },
  error: { color: 'red', label: 'lỗi' },
}

export default function DocsView() {
  const { message } = AntApp.useApp()
  const [docs, setDocs] = useState<Doc[]>([])
  const [loading, setLoading] = useState(true)
  const [active, setActive] = useState<Doc | null>(null)
  const [pasteOpen, setPasteOpen] = useState(false)

  const load = useCallback(() => {
    setLoading(true)
    get<Doc[]>('/docs')
      .then(setDocs)
      .catch((e) => message.error(String(e.message ?? e)))
      .finally(() => setLoading(false))
  }, [message])

  useEffect(load, [load])

  const doUpload = async (file: File) => {
    const form = new FormData()
    form.append('file', file)
    try {
      const r = await upload<{ sections: number; chunks: number; note?: string }>(
        '/docs/upload',
        form,
      )
      message.success(`Đã nạp: ${r.sections} mục, ${r.chunks} đoạn`)
      if (r.note) message.warning(r.note)
      load()
    } catch (e: any) {
      // Extraction failures are informative (scanned PDF, unsupported type) —
      // show the whole thing rather than "upload failed".
      message.error(String(e.message ?? e), 8)
    }
    return false
  }

  if (active) return <DocDetail doc={active} onBack={() => { setActive(null); load() }} />

  return (
    <>
      <Space style={{ marginBottom: 16 }} wrap>
        <Upload beforeUpload={doUpload} showUploadList={false} accept=".pdf,.docx,.txt,.md,.markdown,.html,.htm,.csv,.tsv,.json,.jsonl,.log">
          <Button type="primary" icon={<UploadOutlined />}>Tải tài liệu lên</Button>
        </Upload>
        <Button icon={<FileAddOutlined />} onClick={() => setPasteOpen(true)}>Dán văn bản</Button>
        <Button icon={<ReloadOutlined />} onClick={load}>Tải lại</Button>
      </Space>

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="PDF bản scan không có lớp văn bản sẽ bị từ chối kèm lý do — hãy OCR trước rồi tải lại."
      />

      {loading ? (
        <Spin />
      ) : docs.length === 0 ? (
        <Empty description="Chưa có tài liệu nào" />
      ) : (
        <List
          grid={{ gutter: 16, xs: 1, sm: 2, lg: 3 }}
          dataSource={docs}
          renderItem={(d) => (
            <List.Item>
              <Card
                hoverable
                title={d.title}
                onClick={() => setActive(d)}
                extra={<Tag color={STATUS[d.status]?.color}>{STATUS[d.status]?.label ?? d.status}</Tag>}
                actions={[
                  <Popconfirm
                    key="del"
                    title="Xoá tài liệu?"
                    description="Xoá cả mục, thẻ và câu hỏi sinh ra từ nó."
                    onConfirm={async (e) => {
                      e?.stopPropagation()
                      await del(`/docs/${d.id}`)
                      message.success('Đã xoá')
                      load()
                    }}
                  >
                    <DeleteOutlined onClick={(e) => e.stopPropagation()} />
                  </Popconfirm>,
                ]}
              >
                <Typography.Text type="secondary">
                  {d.sectionCount} mục · {d.chunkCount} đoạn · {d.chars.toLocaleString('vi-VN')} ký tự
                </Typography.Text>
              </Card>
            </List.Item>
          )}
        />
      )}

      <PasteModal open={pasteOpen} onClose={() => setPasteOpen(false)} onDone={load} />
    </>
  )
}

function PasteModal({ open, onClose, onDone }: { open: boolean; onClose: () => void; onDone: () => void }) {
  const { message } = AntApp.useApp()
  const [title, setTitle] = useState('')
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)

  return (
    <Modal
      title="Dán nội dung tài liệu"
      open={open}
      onCancel={onClose}
      okText="Nạp"
      confirmLoading={busy}
      onOk={async () => {
        if (!text.trim()) return message.warning('Chưa có nội dung')
        setBusy(true)
        try {
          const r = await post<{ sections: number }>('/docs/text', { title, text })
          message.success(`Đã nạp ${r.sections} mục`)
          setTitle('')
          setText('')
          onDone()
          onClose()
        } catch (e: any) {
          message.error(String(e.message ?? e))
        } finally {
          setBusy(false)
        }
      }}
      width={720}
    >
      <Space direction="vertical" style={{ width: '100%' }}>
        <Input placeholder="Tên tài liệu" value={title} onChange={(e) => setTitle(e.target.value)} />
        <Input.TextArea
          rows={14}
          placeholder="Dán nội dung… (Markdown có tiêu đề # sẽ được chia mục theo đúng cấu trúc)"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
      </Space>
    </Modal>
  )
}

/**
 * The review step for repeated lines.
 *
 * The app deliberately does not guess here: a PDF's running header and a
 * textbook's "Bài tập 1" under every chapter look identical to any heuristic —
 * short, verbatim, evenly spread. Deleting on a guess eventually eats the
 * structure a learner needs, silently. So the candidates are listed with their
 * repeat counts and the reader ticks the ones that are really page furniture.
 */
function FurnitureReview({
  docId,
  suspects,
  onDone,
}: {
  docId: string
  suspects: Suspect[]
  onDone: (msg: string) => void
}) {
  const { message } = AntApp.useApp()
  const [picked, setPicked] = useState<string[]>([])
  const [busy, setBusy] = useState(false)

  return (
    <Alert
      type="info"
      showIcon
      message={`${suspects.length} dòng lặp lại nhiều lần — có thể là đầu/chân trang`}
      description={
        <Space direction="vertical" style={{ width: '100%' }}>
          <Typography.Text type="secondary">
            App không tự xoá: “Bài tập 1” lặp dưới mỗi chương trông y hệt một dòng đầu trang.
            Bạn tích những dòng đúng là rác, phần còn lại giữ nguyên.
          </Typography.Text>
          <Checkbox.Group
            value={picked}
            onChange={(v) => setPicked(v as string[])}
            style={{ display: 'flex', flexDirection: 'column', gap: 4 }}
          >
            {suspects.map((s) => (
              <Checkbox key={s.line} value={s.line}>
                <Typography.Text code>{s.line}</Typography.Text>{' '}
                <Tag>{s.count} lần</Tag>
              </Checkbox>
            ))}
          </Checkbox.Group>
          <Button
            danger
            disabled={picked.length === 0}
            loading={busy}
            onClick={async () => {
              setBusy(true)
              try {
                const r = await post<{
                  removedLines: number
                  questionsOrphaned: number
                  warning?: string
                }>(`/docs/${docId}/strip-lines`, { lines: picked })
                setPicked([])
                if (r.warning) message.warning(r.warning, 8)
                onDone(`Đã bỏ ${r.removedLines} dòng và lập chỉ mục lại`)
              } catch (e: any) {
                message.error(String(e.message ?? e), 8)
              } finally {
                setBusy(false)
              }
            }}
          >
            Bỏ {picked.length > 0 ? `${picked.length} dòng đã chọn` : 'các dòng đã chọn'}
          </Button>
        </Space>
      }
    />
  )
}

function DocDetail({ doc, onBack }: { doc: Doc; onBack: () => void }) {
  const { message } = AntApp.useApp()
  const [sections, setSections] = useState<Section[]>([])
  const [busy, setBusy] = useState<string | null>(null)
  const [summary, setSummary] = useState(doc.summary ?? '')
  const [suspects, setSuspects] = useState<Suspect[]>([])

  const load = useCallback(() => {
    get<Section[]>(`/docs/${doc.id}/sections`).then(setSections).catch(() => {})
    get<{ suspectedFurniture?: Suspect[] }>(`/docs/${doc.id}`)
      .then((d) => setSuspects(d.suspectedFurniture ?? []))
      .catch(() => {})
  }, [doc.id])
  useEffect(load, [load])

  const run = async (label: string, fn: () => Promise<void>) => {
    setBusy(label)
    try {
      await fn()
    } catch (e: any) {
      message.error(String(e.message ?? e), 8)
    } finally {
      setBusy(null)
    }
  }

  const unenriched = sections.filter((s) => !s.enrichedAt).length

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Space wrap>
        <Button onClick={onBack}>← Danh sách</Button>
        <Button
          icon={<RobotOutlined />}
          type={unenriched > 0 ? 'primary' : 'default'}
          loading={busy === 'enrich'}
          onClick={() =>
            run('enrich', async () => {
              const r = await post<{ enriched: number; problems: string[] }>(`/docs/${doc.id}/enrich`)
              message.success(`AI đã mô tả ${r.enriched} mục`)
              r.problems.forEach((p) => message.warning(p, 6))
              load()
            })
          }
        >
          AI mô tả các mục{unenriched > 0 ? ` (${unenriched} chưa có)` : ''}
        </Button>
        <Button
          loading={busy === 'sum'}
          onClick={() =>
            run('sum', async () => {
              const r = await post<{ summary: string }>(`/docs/${doc.id}/summarize`)
              setSummary(r.summary)
            })
          }
        >
          Tổng hợp tài liệu
        </Button>
        <Button
          icon={<ReloadOutlined />}
          loading={busy === 'idx'}
          onClick={() =>
            run('idx', async () => {
              const r = await post<{ sections: number; note?: string }>(`/docs/${doc.id}/reindex`)
              message.success(`Đã chia lại ${r.sections} mục`)
              if (r.note) message.warning(r.note, 8)
              load()
            })
          }
        >
          Chia mục lại
        </Button>
      </Space>

      <Descriptions bordered size="small" column={2} title={doc.title}>
        <Descriptions.Item label="Tệp">{doc.filename}</Descriptions.Item>
        <Descriptions.Item label="Bóc tách">{doc.extractNote ?? '—'}</Descriptions.Item>
        <Descriptions.Item label="Số mục">{sections.length}</Descriptions.Item>
        <Descriptions.Item label="Tổng thời lượng ước tính">
          {sections.reduce((a, s) => a + s.estMinutes, 0)} phút
        </Descriptions.Item>
      </Descriptions>

      {summary && (
        <Card title="Tổng hợp">
          <div className="study-prose">{summary}</div>
        </Card>
      )}

      {suspects.length > 0 && (
        <FurnitureReview
          docId={doc.id}
          suspects={suspects}
          onDone={(msg) => {
            message.success(msg)
            load()
          }}
        />
      )}

      {unenriched > 0 && (
        <Alert
          type="warning"
          showIcon
          message={`${unenriched}/${sections.length} mục chưa được AI mô tả`}
          description="Số phút học đang ước từ độ dài chứ không từ nội dung. Lộ trình vẫn lập được, nhưng sẽ sát hơn sau khi mô tả."
        />
      )}

      <List
        header={<b>Dàn ý</b>}
        bordered
        dataSource={sections}
        renderItem={(s) => (
          <List.Item>
            <Space direction="vertical" size={2} style={{ width: '100%' }}>
              <Space wrap>
                <Typography.Text strong>
                  {s.ord + 1}. {s.title}
                </Typography.Text>
                <Tag>{s.estMinutes} phút</Tag>
                <Tag color={['', 'green', 'cyan', 'blue', 'orange', 'red'][s.difficulty] ?? 'blue'}>
                  độ khó {s.difficulty}
                </Tag>
                {!s.enrichedAt && <Tag color="default">chưa mô tả</Tag>}
              </Space>
              {s.summary && <Typography.Text type="secondary">{s.summary}</Typography.Text>}
              {s.keyPoints.length > 0 && (
                <ul style={{ margin: '4px 0 0 18px' }}>
                  {s.keyPoints.map((k, i) => (
                    <li key={i}>
                      <Typography.Text type="secondary">{k}</Typography.Text>
                    </li>
                  ))}
                </ul>
              )}
            </Space>
          </List.Item>
        )}
      />
    </Space>
  )
}
