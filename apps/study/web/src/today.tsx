import { useCallback, useEffect, useRef, useState } from 'react'
import {
  App as AntApp,
  Alert,
  Button,
  Card,
  Checkbox,
  Divider,
  Empty,
  List,
  Radio,
  Space,
  Spin,
  Tag,
  Typography,
} from 'antd'
import {
  CheckCircleOutlined,
  PauseCircleOutlined,
  SoundOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import { get, post, type Question, type Session, type SessionItem } from './api'

const KIND_LABEL: Record<string, string> = {
  read: 'Đọc',
  flashcard: 'Ôn thẻ',
  review: 'Ôn lại',
  quiz: 'Trắc nghiệm',
  recall: 'Tự diễn giải',
}

export default function TodayView({
  sessionId,
  onOpen,
}: {
  sessionId: string | null
  onOpen: (id: string | null) => void
}) {
  const [data, setData] = useState<{ date: string; sessions: Session[]; cardsDue: number } | null>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    if (sessionId) return
    setLoading(true)
    get<{ date: string; sessions: Session[]; cardsDue: number }>('/today')
      .then(setData)
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [sessionId])

  if (sessionId) return <Lesson id={sessionId} onBack={() => onOpen(null)} />
  if (loading) return <Spin />

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Typography.Title level={4} style={{ margin: 0 }}>
        Hôm nay · {data?.date}
      </Typography.Title>
      {data && data.cardsDue > 0 && (
        <Alert
          type="info"
          showIcon
          icon={<ThunderboltOutlined />}
          message={`${data.cardsDue} thẻ đến hạn ôn — mở tab “Thẻ ôn”.`}
        />
      )}
      {!data?.sessions.length ? (
        <Empty description="Hôm nay không có buổi học nào được lên lịch" />
      ) : (
        <List
          dataSource={data.sessions}
          renderItem={(s) => (
            <List.Item actions={[<a key="o" onClick={() => onOpen(s.id)}>Vào học</a>]}>
              <List.Item.Meta
                title={
                  <Space wrap>
                    <b>{s.startHm} · {s.title}</b>
                    <Tag>{s.minutes} phút</Tag>
                    {s.status === 'done' && <Tag color="green">đã học</Tag>}
                  </Space>
                }
                description={`Buổi ${s.ord + 1}`}
              />
            </List.Item>
          )}
        />
      )}
    </Space>
  )
}

// ── Lesson ──────────────────────────────────────────────────────────────────

function Lesson({ id, onBack }: { id: string; onBack: () => void }) {
  const { message } = AntApp.useApp()
  const [sess, setSess] = useState<Session | null>(null)
  const [loading, setLoading] = useState(true)
  const [quizFor, setQuizFor] = useState<string | null>(null)

  const load = useCallback(() => {
    setLoading(true)
    get<Session>(`/sessions/${id}`)
      .then(setSess)
      .catch((e) => message.error(String(e.message ?? e)))
      .finally(() => setLoading(false))
  }, [id, message])
  useEffect(load, [load])

  if (loading) return <Spin />
  if (!sess) return <Empty description="Không tìm thấy buổi học" />

  return (
    <Space direction="vertical" style={{ width: '100%' }} size="middle">
      <Space wrap>
        <Button onClick={onBack}>← Hôm nay</Button>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {sess.title}
        </Typography.Title>
        <Tag>{sess.date} {sess.startHm}</Tag>
        <Tag>{sess.minutes} phút</Tag>
        <Button
          type={sess.status === 'done' ? 'default' : 'primary'}
          icon={<CheckCircleOutlined />}
          onClick={async () => {
            await post(`/sessions/${id}/complete`, { done: sess.status !== 'done' })
            message.success(sess.status === 'done' ? 'Đã bỏ đánh dấu' : 'Đã hoàn thành buổi học')
            load()
          }}
        >
          {sess.status === 'done' ? 'Bỏ đánh dấu' : 'Hoàn thành buổi'}
        </Button>
      </Space>

      {sess.items.map((it) => (
        <ItemCard
          key={it.id}
          item={it}
          onQuiz={() => setQuizFor(it.docId ?? null)}
          onChanged={load}
        />
      ))}

      {quizFor && <QuizPane docId={quizFor} onClose={() => setQuizFor(null)} />}
    </Space>
  )
}

function ItemCard({
  item,
  onQuiz,
  onChanged,
}: {
  item: SessionItem
  onQuiz: () => void
  onChanged: () => void
}) {
  const { message } = AntApp.useApp()
  const [busy, setBusy] = useState(false)

  return (
    <Card
      title={
        <Space wrap>
          <Tag color={item.kind === 'read' ? 'blue' : item.kind === 'quiz' ? 'orange' : 'green'}>
            {KIND_LABEL[item.kind] ?? item.kind}
          </Tag>
          <b>{item.sectionTitle}</b>
          {item.parts > 1 && <Tag>phần {item.part}/{item.parts}</Tag>}
          <Tag>{item.estMinutes} phút</Tag>
          {item.doneAt && <Tag color="green">xong</Tag>}
        </Space>
      }
      extra={
        <Space>
          {item.kind === 'quiz' && <Button onClick={onQuiz}>Làm bài</Button>}
          {item.sectionId && (item.kind === 'read' || item.kind === 'flashcard') && (
            <Button
              loading={busy}
              onClick={async () => {
                setBusy(true)
                try {
                  const r = await post<{ created: number; duplicates: number; rejected: number }>(
                    `/sections/${item.sectionId}/cards/generate`,
                    { count: 8 },
                  )
                  message.success(
                    `Thẻ: tạo ${r.created}, trùng ${r.duplicates}, loại ${r.rejected}`,
                  )
                } catch (e: any) {
                  message.error(String(e.message ?? e), 8)
                } finally {
                  setBusy(false)
                }
              }}
            >
              Sinh thẻ
            </Button>
          )}
          <Button
            type={item.doneAt ? 'default' : 'primary'}
            onClick={async () => {
              await post(`/items/${item.id}/complete`, { done: !item.doneAt })
              onChanged()
            }}
          >
            {item.doneAt ? 'Bỏ xong' : 'Xong'}
          </Button>
        </Space>
      }
    >
      {item.summary && (
        <Alert type="info" style={{ marginBottom: 12 }} message={item.summary} />
      )}
      {(item.keyPoints ?? []).length > 0 && (
        <ul>
          {(item.keyPoints ?? []).map((k, i) => (
            <li key={i}>{k}</li>
          ))}
        </ul>
      )}
      {item.text && (
        <>
          <Speaker text={item.text} />
          <Divider style={{ margin: '12px 0' }} />
          <div className="study-prose">{item.text}</div>
        </>
      )}
    </Card>
  )
}

// ── Audio ───────────────────────────────────────────────────────────────────

/** Read a passage aloud, sentence by sentence.
 *
 *  Sentence-at-a-time is not decoration: local TTS takes seconds per clip, so
 *  synthesising a whole chapter first would mean a long silence and then a wall
 *  of audio. This starts playing after the first sentence. */
export function Speaker({ text }: { text: string }) {
  const { message } = AntApp.useApp()
  const [clips, setClips] = useState<{ text: string; url: string }[] | null>(null)
  const [idx, setIdx] = useState(-1)
  const [busy, setBusy] = useState(false)
  const audio = useRef<HTMLAudioElement | null>(null)

  useEffect(() => () => audio.current?.pause(), [])

  const stop = () => {
    audio.current?.pause()
    audio.current = null
    setIdx(-1)
  }

  const playFrom = (list: { url: string }[], i: number) => {
    if (i >= list.length) return stop()
    setIdx(i)
    const a = new Audio(list[i].url)
    audio.current = a
    a.onended = () => playFrom(list, i + 1)
    a.play().catch(() => stop())
  }

  return (
    <Space wrap>
      <Button
        icon={idx >= 0 ? <PauseCircleOutlined /> : <SoundOutlined />}
        loading={busy}
        onClick={async () => {
          if (idx >= 0) return stop()
          if (clips) return playFrom(clips, 0)
          setBusy(true)
          try {
            const r = await post<{ clips: { text: string; url: string }[]; problems?: string[] }>(
              '/speak',
              { text, split: true },
            )
            setClips(r.clips)
            ;(r.problems ?? []).forEach((p) => message.warning(p, 6))
            playFrom(r.clips, 0)
          } catch (e: any) {
            // "no TTS model installed" arrives here — showing it is the point.
            message.error(String(e.message ?? e), 10)
          } finally {
            setBusy(false)
          }
        }}
      >
        {idx >= 0 ? 'Dừng đọc' : 'Đọc thành tiếng'}
      </Button>
      {idx >= 0 && clips && (
        <Typography.Text type="secondary">
          câu {idx + 1}/{clips.length}
        </Typography.Text>
      )}
    </Space>
  )
}

// ── Quiz ────────────────────────────────────────────────────────────────────

export function QuizPane({ docId, onClose }: { docId: string; onClose: () => void }) {
  const { message } = AntApp.useApp()
  const [quiz, setQuiz] = useState<{ quizId: string; questions: Question[] } | null>(null)
  const [answers, setAnswers] = useState<Record<string, any>>({})
  const [result, setResult] = useState<any>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    setLoading(true)
    post<{ quizId: string; questions: Question[] }>('/quiz', { doc_id: docId, count: 8 })
      .then(setQuiz)
      .catch((e) => message.error(String(e.message ?? e), 8))
      .finally(() => setLoading(false))
  }, [docId, message])

  if (loading) return <Spin />
  if (!quiz) return null

  if (result) {
    return (
      <Card title={`Kết quả: ${result.correct}/${result.total} (${result.score}%)`} extra={<a onClick={onClose}>Đóng</a>}>
        {result.newCards > 0 && (
          <Alert
            type="info"
            showIcon
            style={{ marginBottom: 12 }}
            message={`Đã tạo ${result.newCards} thẻ ôn từ các câu sai.`}
          />
        )}
        <List
          dataSource={result.results}
          renderItem={(r: any) => (
            <List.Item>
              <Space direction="vertical" size={2} style={{ width: '100%' }}>
                <Space>
                  <Tag color={r.correct ? 'green' : 'red'}>{r.correct ? 'Đúng' : 'Sai'}</Tag>
                  <Typography.Text type="secondary">
                    {quiz.questions.find((q) => q.id === r.questionId)?.stem}
                  </Typography.Text>
                </Space>
                {r.explain && <Typography.Text>{r.explain}</Typography.Text>}
                {r.quote && (
                  <div className="study-quote">
                    <Typography.Text type="secondary">“{r.quote}”</Typography.Text>
                  </div>
                )}
              </Space>
            </List.Item>
          )}
        />
      </Card>
    )
  }

  return (
    <Card title="Trắc nghiệm" extra={<a onClick={onClose}>Đóng</a>}>
      {quiz.questions.map((q, i) => (
        <div key={q.id} style={{ marginBottom: 20 }}>
          <Typography.Text strong>
            {i + 1}. {q.stem}
          </Typography.Text>
          <div style={{ marginTop: 8 }}>
            <AnswerInput q={q} value={answers[q.id]} onChange={(v) => setAnswers({ ...answers, [q.id]: v })} />
          </div>
        </div>
      ))}
      <Button
        type="primary"
        onClick={async () => {
          try {
            const r = await post('/quiz/grade', {
              quiz_id: quiz.quizId,
              answers: quiz.questions.map((q) => ({
                question_id: q.id,
                answer: answers[q.id] ?? null,
              })),
            })
            setResult(r)
          } catch (e: any) {
            message.error(String(e.message ?? e), 8)
          }
        }}
      >
        Nộp bài
      </Button>
    </Card>
  )
}

function AnswerInput({
  q,
  value,
  onChange,
}: {
  q: Question
  value: any
  onChange: (v: any) => void
}) {
  const opts: string[] = Array.isArray(q.options) ? q.options : []
  switch (q.kind) {
    case 'single':
      return (
        <Radio.Group value={value} onChange={(e) => onChange(e.target.value)}>
          <Space direction="vertical">
            {opts.map((o, i) => (
              <Radio key={i} value={i}>{o}</Radio>
            ))}
          </Space>
        </Radio.Group>
      )
    case 'multi':
      return (
        <Checkbox.Group value={value ?? []} onChange={(v) => onChange(v)}>
          <Space direction="vertical">
            {opts.map((o, i) => (
              <Checkbox key={i} value={i}>{o}</Checkbox>
            ))}
          </Space>
        </Checkbox.Group>
      )
    case 'truefalse':
      return (
        <Radio.Group value={value} onChange={(e) => onChange(e.target.value)}>
          <Radio value={true}>Đúng</Radio>
          <Radio value={false}>Sai</Radio>
        </Radio.Group>
      )
    case 'order':
    case 'match': {
      const left: string[] = q.kind === 'match' ? (q.options?.left ?? []) : opts
      const right: string[] = q.kind === 'match' ? (q.options?.right ?? []) : opts
      const cur: number[] = value ?? []
      return (
        <Space direction="vertical" style={{ width: '100%' }}>
          {left.map((l, i) => (
            <Space key={i}>
              <Typography.Text>{q.kind === 'order' ? `Vị trí ${i + 1}` : l}</Typography.Text>
              <Radio.Group
                value={cur[i]}
                onChange={(e) => {
                  const next = [...cur]
                  next[i] = e.target.value
                  onChange(next)
                }}
              >
                {right.map((r, j) => (
                  <Radio key={j} value={j}>{r}</Radio>
                ))}
              </Radio.Group>
            </Space>
          ))}
        </Space>
      )
    }
    default:
      return (
        <input
          style={{ width: '100%', padding: 6 }}
          value={value ?? ''}
          onChange={(e) => onChange(e.target.value)}
          placeholder="Điền vào chỗ trống"
        />
      )
  }
}
