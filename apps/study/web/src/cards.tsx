import { useCallback, useEffect, useRef, useState } from 'react'
import { App as AntApp, Alert, Button, Card, Empty, Progress, Space, Switch, Tag, Typography } from 'antd'
import { SoundOutlined } from '@ant-design/icons'
import { get, post, type Card as CardT } from './api'

const GRADES: { key: string; label: string; color: string }[] = [
  { key: 'again', label: 'Quên', color: 'red' },
  { key: 'hard', label: 'Khó', color: 'orange' },
  { key: 'good', label: 'Được', color: 'blue' },
  { key: 'easy', label: 'Dễ', color: 'green' },
]

export default function CardsView() {
  const { message } = AntApp.useApp()
  const [due, setDue] = useState<CardT[]>([])
  const [total, setTotal] = useState(0)
  const [i, setI] = useState(0)
  const [flipped, setFlipped] = useState(false)
  const [handsFree, setHandsFree] = useState(false)
  const [loading, setLoading] = useState(true)
  const audio = useRef<HTMLAudioElement | null>(null)

  const load = useCallback(() => {
    setLoading(true)
    get<{ due: CardT[]; total: number }>('/cards/due?limit=50')
      .then((r) => {
        setDue(r.due)
        setTotal(r.total)
        setI(0)
        setFlipped(false)
      })
      .catch((e) => message.error(String(e.message ?? e)))
      .finally(() => setLoading(false))
  }, [message])
  useEffect(load, [load])

  useEffect(() => () => audio.current?.pause(), [])

  const card = due[i]

  const say = useCallback(
    async (text: string) => {
      try {
        const r = await post<{ url: string }>('/speak', { text })
        audio.current?.pause()
        const a = new Audio(r.url)
        audio.current = a
        await a.play()
        return new Promise<void>((res) => {
          a.onended = () => res()
        })
      } catch (e: any) {
        message.error(String(e.message ?? e), 10)
        setHandsFree(false)
      }
    },
    [message],
  )

  // Hands-free: read the front, pause, read the back. Nothing is graded
  // automatically — a card the learner never answered must not climb the
  // ladder, so this mode is for exposure, and grading stays manual.
  useEffect(() => {
    if (!handsFree || !card) return
    let cancelled = false
    ;(async () => {
      await say(card.front)
      if (cancelled) return
      await new Promise((r) => setTimeout(r, 2500))
      if (cancelled) return
      setFlipped(true)
      await say(card.back)
    })()
    return () => {
      cancelled = true
      audio.current?.pause()
    }
  }, [handsFree, card, say])

  if (loading) return <Typography.Text>Đang tải…</Typography.Text>

  if (!card) {
    return (
      <Space direction="vertical" style={{ width: '100%' }}>
        <Empty description="Không còn thẻ nào đến hạn — quay lại sau nhé" />
        <Button onClick={load}>Tải lại</Button>
      </Space>
    )
  }

  const grade = async (g: string) => {
    try {
      await post(`/cards/${card.id}/review`, { grade: g })
      if (i + 1 < due.length) {
        setI(i + 1)
        setFlipped(false)
      } else {
        load()
      }
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  return (
    <Space direction="vertical" style={{ width: '100%', maxWidth: 720, margin: '0 auto' }} size="middle">
      <Space wrap style={{ width: '100%', justifyContent: 'space-between' }}>
        <Typography.Text type="secondary">
          Thẻ {i + 1}/{due.length} · còn {total} đến hạn
        </Typography.Text>
        <Space>
          <Typography.Text type="secondary">Rảnh tay</Typography.Text>
          <Switch checked={handsFree} onChange={setHandsFree} />
        </Space>
      </Space>
      <Progress percent={Math.round(((i) / Math.max(due.length, 1)) * 100)} showInfo={false} />

      <Card
        onClick={() => setFlipped((f) => !f)}
        title={
          <Space wrap>
            <Tag>{card.kind}</Tag>
            <Tag color="purple">cấp {card.level}</Tag>
            {card.isUrgent && <Tag color="red">nhắc gấp</Tag>}
            {card.source === 'quiz-miss' && <Tag color="orange">từ câu làm sai</Tag>}
          </Space>
        }
        extra={
          <Button
            size="small"
            icon={<SoundOutlined />}
            onClick={(e) => {
              e.stopPropagation()
              say(flipped ? card.back : card.front)
            }}
          >
            Đọc
          </Button>
        }
      >
        <div className="study-card-face">{flipped ? card.back : card.front}</div>
        {!flipped && (
          <Typography.Text type="secondary">Bấm vào thẻ để lật</Typography.Text>
        )}
      </Card>

      {flipped ? (
        <Space wrap>
          {GRADES.map((g) => (
            <Button key={g.key} onClick={() => grade(g.key)}>
              <Tag color={g.color} style={{ margin: 0 }}>{g.label}</Tag>
            </Button>
          ))}
        </Space>
      ) : (
        <Alert
          type="info"
          showIcon
          message="Cố nhớ lại trước khi lật — chính lúc cố nhớ mới là lúc học."
        />
      )}
    </Space>
  )
}
