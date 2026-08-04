/** Modal sinh tài liệu AI — chảy đúng interview mode của backend:
 * gọi generate → nếu needs_input thì hiện câu hỏi phỏng vấn, người dùng trả
 * lời → gọi lại kèm answers. */
import { useEffect, useState } from 'react'
import { Alert, App, Button, Form, Input, Modal, Space, Tag, Typography } from 'antd'
import { ThunderboltOutlined } from '@ant-design/icons'
import { post, waitJob, type CatalogItem, type Doc } from './api'

export default function GenerateModal({
  open,
  onClose,
  projectId,
  featureId,
  item,
  onDone,
}: {
  open: boolean
  onClose: () => void
  projectId: number
  featureId: number | null
  item: CatalogItem | null
  onDone: (doc: Doc, warnings: string[]) => void
}) {
  const { message } = App.useApp()
  const [input, setInput] = useState('')
  const [questions, setQuestions] = useState<string[]>([])
  const [answers, setAnswers] = useState<Record<number, string>>({})
  const [running, setRunning] = useState(false)
  const [elapsed, setElapsed] = useState(0)
  const [note, setNote] = useState('')

  useEffect(() => {
    if (open) {
      setInput('')
      setQuestions([])
      setAnswers({})
      setNote('')
      setElapsed(0)
    }
  }, [open, item])

  if (!item) return null

  const run = async (force: boolean) => {
    setRunning(true)
    setElapsed(0)
    try {
      const answersText = questions.length
        ? questions
            .map((q, i) => (answers[i]?.trim() ? `Hỏi: ${q}\nĐáp: ${answers[i].trim()}` : ''))
            .filter(Boolean)
            .join('\n\n')
        : ''
      const r = await post('/generate', {
        project: String(projectId),
        feature: featureId != null ? String(featureId) : '',
        doc_type: item.doc_type,
        subtype: item.subtype,
        input,
        answers: answersText,
        force,
      })
      const result = await waitJob(r.job_id, (ms) => setElapsed(Math.round(ms / 1000)))
      if (result.needs_input) {
        setQuestions(result.questions ?? [])
        setNote(result.note ?? '')
        return
      }
      message.success(`Đã sinh ${item.title}`)
      onDone(result.document, result.warnings ?? [])
      onClose()
    } catch (e: any) {
      message.error(String(e.message ?? e), 6)
    } finally {
      setRunning(false)
    }
  }

  return (
    <Modal
      open={open}
      onCancel={running ? undefined : onClose}
      footer={null}
      width={720}
      title={
        <Space>
          <ThunderboltOutlined />
          <span>
            Sinh {item.title} <span className="skill-chip">{item.skill}</span>
          </span>
        </Space>
      }
    >
      <Typography.Paragraph type="secondary" style={{ marginTop: 4 }}>
        {item.desc}. AI tự đọc tài liệu upstream làm ngữ cảnh — đầu vào dưới đây là dữ kiện bổ sung
        (ý tưởng thô, ghi chú họp, tài liệu API, source code...).
      </Typography.Paragraph>
      <Input.TextArea
        rows={questions.length ? 3 : 7}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        placeholder="Đầu vào cho AI (bỏ trống nếu tài liệu upstream đã đủ)..."
        disabled={running}
      />
      {questions.length > 0 && (
        <div style={{ marginTop: 14 }}>
          <Alert
            type="info"
            showIcon
            message="AI cần làm rõ trước khi viết (trả lời được phần nào hay phần đó)"
            description={note}
            style={{ marginBottom: 10 }}
          />
          <Form layout="vertical">
            {questions.map((q, i) => (
              <Form.Item key={i} label={`${i + 1}. ${q}`} style={{ marginBottom: 10 }}>
                <Input.TextArea
                  rows={2}
                  value={answers[i] ?? ''}
                  onChange={(e) => setAnswers((a) => ({ ...a, [i]: e.target.value }))}
                  disabled={running}
                />
              </Form.Item>
            ))}
          </Form>
        </div>
      )}
      <Space style={{ marginTop: 14 }}>
        <Button type="primary" loading={running} onClick={() => run(false)}>
          {questions.length ? 'Sinh với câu trả lời' : 'Sinh tài liệu'}
        </Button>
        <Button disabled={running} onClick={() => run(true)}>
          Bỏ phỏng vấn, AI tự giả định
        </Button>
        {running && <Tag color="processing">đang sinh… {elapsed}s (tài liệu dài có thể vài phút)</Tag>}
      </Space>
    </Modal>
  )
}
