import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  App as AntApp, Button, Checkbox, Input, InputNumber, Modal, Select, Tabs, Tag,
} from 'antd'
import {
  CaretRightOutlined, FlagOutlined, MoonOutlined, PauseOutlined, PlusOutlined,
  SendOutlined, SunOutlined,
} from '@ant-design/icons'
import { useTheme } from './theme'
import { api } from './api'
import ChatFeed from './ChatFeed'
import MeetingScene from './MeetingScene'
import { DocsPanel, MinutesPanel, ProgressPanel, ResultPanel, TeamPanel } from './Panels'
import type { Discussion, Member, Message, MinutesRow, Progress, ResultRow } from './types'

const STATUS_VN: Record<string, string> = {
  draft: 'nháp', running: 'đang họp', paused: 'tạm dừng', review: 'chờ nghiệm thu', done: 'đã chốt',
}
const STATUS_COLOR: Record<string, string> = {
  draft: 'default', running: 'green', paused: 'orange', review: 'blue', done: 'purple',
}

export default function App() {
  const { message, modal } = AntApp.useApp()
  const { mode, toggle } = useTheme()
  const [discussions, setDiscussions] = useState<Discussion[]>([])
  const [currentId, setCurrentId] = useState<number | null>(null)
  const [discussion, setDiscussion] = useState<Discussion | null>(null)
  const [crew, setCrew] = useState<Member[]>([])
  const [roster, setRoster] = useState<Member[]>([])
  const [messages, setMessages] = useState<Message[]>([])
  const [progress, setProgress] = useState<Progress | null>(null)
  const [minutes, setMinutes] = useState<MinutesRow | null>(null)
  const [result, setResult] = useState<ResultRow | null>(null)
  const [tab, setTab] = useState('progress')
  const [showScene, setShowScene] = useState(true)
  const [composer, setComposer] = useState('')
  const [creating, setCreating] = useState(false)
  const [openDocId, setOpenDocId] = useState<number | null>(null)
  const lastMsgId = useRef(0)
  const tick = useRef(0)

  const flash = useCallback((e: unknown) => {
    message.error(e instanceof Error ? e.message : String(e))
  }, [message])

  const loadRoster = useCallback(async () => {
    try {
      const r = await api.get<{ members: Member[] }>('/members')
      setRoster(r.members)
    } catch { /* daemon chưa chạy vẫn xem được UI */ }
  }, [])

  const loadList = useCallback(async () => {
    try {
      const r = await api.get<{ discussions: Discussion[] }>('/discussions?limit=30')
      setDiscussions(r.discussions)
      return r.discussions
    } catch {
      return []
    }
  }, [])

  const loadDetail = useCallback(async (id: number) => {
    try {
      const r = await api.get<{ discussion: Discussion; members: Member[]; minutes: MinutesRow | null; result: ResultRow | null }>(`/discussions/${id}`)
      setDiscussion(r.discussion)
      setCrew(r.members)
      setMinutes(r.minutes)
      setResult(r.result)
    } catch { /* im lặng */ }
  }, [])

  const switchTo = useCallback((id: number | null) => {
    setCurrentId(id)
    setMessages([])
    setProgress(null)
    setMinutes(null)
    setResult(null)
    setDiscussion(null)
    lastMsgId.current = 0
    if (id != null) void loadDetail(id)
  }, [loadDetail])

  useEffect(() => {
    void (async () => {
      await loadRoster()
      const list = await loadList()
      const p = new URLSearchParams(location.search)
      const want = parseInt(p.get('discussion') || '', 10)
      const pick = list.find((d) => d.id === want)?.id ?? list[0]?.id ?? null
      switchTo(pick)
    })()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const t = setInterval(async () => {
      tick.current++
      if (currentId == null) {
        if (tick.current % 5 === 0) void loadList()
        return
      }
      try {
        const r = await api.get<{ messages: Message[] }>(`/discussions/${currentId}/messages?after=${lastMsgId.current}&limit=200`)
        if (r.messages.length > 0) {
          lastMsgId.current = r.messages[r.messages.length - 1].id
          setMessages((prev) => [...prev.slice(-600), ...r.messages])
        }
        if (tick.current % 2 === 0) {
          const p = await api.get<Progress>(`/discussions/${currentId}/progress`)
          setProgress(p)
        }
        if (tick.current % 4 === 0) {
          void loadDetail(currentId)
          void loadList()
        }
      } catch { /* app restart — poll tiếp */ }
    }, 1200)
    return () => clearInterval(t)
  }, [currentId, loadDetail, loadList])

  const act = useCallback(async (path: string, body?: unknown) => {
    if (currentId == null) return
    try {
      await api.post(`/discussions/${currentId}/${path}`, body)
      await loadDetail(currentId)
      await loadList()
    } catch (e) {
      flash(e)
    }
  }, [currentId, flash, loadDetail, loadList])

  const say = async () => {
    const text = composer.trim()
    if (!text || currentId == null) return
    setComposer('')
    try {
      await api.post(`/discussions/${currentId}/say`, { content: text })
    } catch (e) {
      flash(e)
      setComposer(text)
    }
  }

  const sceneMembers = useMemo(() => {
    const specials = roster.filter((m) => (m.role === 'manager' || m.role === 'secretary') && m.enabled)
    return [...specials, ...crew]
  }, [roster, crew])

  const st = discussion?.status ?? ''

  const discOptions = discussions.map((d) => ({
    value: d.id,
    label: `#${d.id} · ${d.title} · ${STATUS_VN[d.status] ?? d.status}`,
  }))

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">🗣️ AI Discuss Team</div>
        <Select
          className="disc-select"
          popupMatchSelectWidth={false}
          value={currentId ?? undefined}
          placeholder="(chưa có phiên)"
          options={discOptions}
          onChange={(v) => switchTo(v)}
          styles={{ root: { minWidth: 260, maxWidth: 380 } }}
        />
        {discussion && <Tag color={STATUS_COLOR[st]}>{STATUS_VN[st] ?? st}</Tag>}
        {discussion && <Tag>vòng {discussion.round}/{discussion.max_rounds}</Tag>}
        <div className="spacer" />
        {discussion && (
          <>
            <span className="pace">Nhịp</span>
            <Select size="small" value={discussion.pace_secs}
              onChange={(v) => void act('pace', { pace_secs: v })}
              options={[
                { value: 0, label: 'nhanh nhất' },
                { value: 10, label: '10s/lượt' },
                { value: 20, label: '20s/lượt' },
                { value: 40, label: '40s/lượt' },
                { value: 60, label: '60s/lượt' },
              ]} />
            <span className="pace">Chế độ</span>
            <Select size="small" value={discussion.mode}
              onChange={(v) => void act('pace', { mode: v })}
              options={[
                { value: 'sequential', label: 'lần lượt' },
                { value: 'parallel', label: 'song song' },
              ]} />
            {st === 'draft' && <Button type="primary" icon={<CaretRightOutlined />} onClick={() => void act('start')}>Bắt đầu</Button>}
            {st === 'running' && <Button icon={<PauseOutlined />} onClick={() => void act('pause')}>Tạm dừng</Button>}
            {st === 'paused' && <Button type="primary" icon={<CaretRightOutlined />} onClick={() => void act('resume')}>Tiếp tục</Button>}
            {(st === 'running' || st === 'paused') && (
              <Button icon={<FlagOutlined />} onClick={() => {
                modal.confirm({
                  title: 'Chốt phiên ngay?',
                  content: 'Thư ký sẽ tổng hợp kết quả để bạn nghiệm thu.',
                  okText: 'Chốt', cancelText: 'Huỷ',
                  onOk: () => act('conclude'),
                })
              }}>Chốt ngay</Button>
            )}
          </>
        )}
        <Button
          icon={mode === 'dark' ? <SunOutlined /> : <MoonOutlined />}
          onClick={toggle}
          title={mode === 'dark' ? 'Chuyển giao diện sáng' : 'Chuyển giao diện tối'}
        />
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreating(true)}>Phiên mới</Button>
      </header>

      <div className="layout">
        <main className="main">
          {showScene && (
            <MeetingScene discussion={discussion} members={sceneMembers}
              statuses={progress?.member_statuses ?? {}} messages={messages} />
          )}
          <button className="scene-toggle" onClick={() => setShowScene((v) => !v)}>
            {showScene ? '▾ Thu gọn phòng họp' : '🏢 Mở phòng họp 3D'}
          </button>
          <ChatFeed messages={messages} members={roster} hasDiscussion={!!discussion}
            onOpenDoc={(id) => { setTab('docs'); setOpenDocId(id) }} />
          {st === 'review' && (
            <div className="review-banner">
              📋 Kết quả đang chờ BOSS nghiệm thu —
              <Button size="small" type="primary" onClick={() => setTab('result')}>Xem &amp; duyệt</Button>
            </div>
          )}
          <div className="composer">
            <Input.TextArea
              autoSize={{ minRows: 1, maxRows: 5 }}
              value={composer}
              placeholder={discussion ? 'BOSS phát biểu — đội sẽ ưu tiên trả lời bạn trước… (Enter gửi, Shift+Enter xuống dòng)' : 'Tạo phiên thảo luận trước đã…'}
              disabled={!discussion || st === 'done'}
              onChange={(e) => setComposer(e.target.value)}
              onPressEnter={(e) => {
                if (!e.shiftKey) {
                  e.preventDefault()
                  void say()
                }
              }}
            />
            <Button type="primary" icon={<SendOutlined />}
              disabled={!composer.trim() || !discussion || st === 'done'}
              onClick={() => void say()}>
              BOSS
            </Button>
          </div>
        </main>

        <aside className="side">
          <Tabs
            activeKey={tab}
            onChange={setTab}
            size="small"
            className="side-tabs"
            items={[
              { key: 'progress', label: 'Tiến độ' },
              { key: 'minutes', label: 'Biên bản' },
              { key: 'result', label: 'Kết quả' },
              { key: 'docs', label: 'Tài liệu' },
              { key: 'team', label: 'Đội' },
            ]}
          />
          <div className="side-body">
            {tab === 'progress' && <ProgressPanel progress={progress} />}
            {tab === 'minutes' && <MinutesPanel minutes={minutes} />}
            {tab === 'result' && (
              <ResultPanel result={result} status={st}
                onApprove={() => void act('approve')}
                onReject={(fb) => void act('reject', { feedback: fb })} />
            )}
            {tab === 'docs' && (
              <DocsPanel discussionId={currentId} openDocId={openDocId} onOpenedDoc={() => setOpenDocId(null)} />
            )}
            {tab === 'team' && <TeamPanel members={roster} onChanged={() => void loadRoster()} />}
          </div>
        </aside>
      </div>

      {creating && (
        <NewDiscussionDialog roster={roster} onClose={() => setCreating(false)}
          onCreated={async (id) => {
            setCreating(false)
            await loadList()
            switchTo(id)
          }} />
      )}
    </div>
  )
}

function NewDiscussionDialog({
  roster, onClose, onCreated,
}: {
  roster: Member[]
  onClose: () => void
  onCreated: (id: number) => void
}) {
  const { message } = AntApp.useApp()
  const candidates = roster.filter((m) => m.role === 'member')
  const [title, setTitle] = useState('')
  const [requirement, setRequirement] = useState('')
  const [mode, setMode] = useState<'sequential' | 'parallel'>('sequential')
  const [pace, setPace] = useState(20)
  const [maxRounds, setMaxRounds] = useState(12)
  const [startNow, setStartNow] = useState(true)
  const [picked, setPicked] = useState<number[]>(candidates.filter((m) => m.enabled).map((m) => m.id))

  const create = async () => {
    try {
      const r = await api.post<{ discussion: Discussion }>('/discussions', {
        title: title.trim(),
        requirement: requirement.trim(),
        mode,
        pace_secs: pace,
        max_rounds: maxRounds,
        member_ids: picked,
        start: startNow,
      })
      onCreated(r.discussion.id)
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Modal open onCancel={onClose} width={680}
      title="➕ Mở phiên thảo luận — BOSS đặt đề bài"
      okText="Mở phiên" cancelText="Huỷ"
      okButtonProps={{ disabled: !title.trim() || !requirement.trim() || picked.length === 0 }}
      onOk={() => void create()}>
      <div className="v-form">
        <label className="fld">Chủ đề
          <Input value={title} onChange={(e) => setTitle(e.target.value)}
            placeholder="VD: Có nên mở thị trường Indonesia năm 2027?" />
        </label>
        <label className="fld">Yêu cầu kết quả (tiêu chí để Manager biết khi nào ĐỦ và đề nghị chốt)
          <Input.TextArea autoSize={{ minRows: 3, maxRows: 7 }} value={requirement}
            onChange={(e) => setRequirement(e.target.value)}
            placeholder={'VD: 1) Kết luận nên/không nên kèm mức chứng minh. 2) Ít nhất 3 dẫn chứng kiểm được. 3) Danh sách rủi ro chính. 4) Bước đi đầu tiên nếu làm.'} />
        </label>
        <div className="form-grid">
          <label className="fld">Chế độ
            <Select value={mode} onChange={setMode}
              options={[
                { value: 'sequential', label: 'lần lượt (dễ theo dõi)' },
                { value: 'parallel', label: 'song song (nhanh, tối đa 3 cùng lúc)' },
              ]} />
          </label>
          <label className="fld">Nhịp (giây/lượt)
            <Select value={pace} onChange={setPace}
              options={[0, 10, 20, 40, 60].map((v) => ({ value: v, label: v === 0 ? '0 — nhanh nhất' : String(v) }))} />
          </label>
          <label className="fld">Trần vòng
            <InputNumber min={1} max={100} value={maxRounds} onChange={(v) => setMaxRounds(v ?? 12)} styles={{ root: { width: '100%' } }} />
          </label>
          <label className="fld" style={{ justifyContent: 'flex-end' }}>
            <Checkbox checked={startNow} onChange={(e) => setStartNow(e.target.checked)}>Chạy ngay</Checkbox>
          </label>
        </div>
        <div className="panel-sub">Thành viên tham gia ({picked.length})</div>
        <Checkbox.Group value={picked} onChange={(v) => setPicked(v as number[])}
          options={candidates.map((m) => ({
            value: m.id,
            label: `${m.name} — ${m.expertise || m.role}${m.use_tools ? '' : ' (không tool)'}`,
          }))}
          className="pick-grid" />
      </div>
    </Modal>
  )
}
