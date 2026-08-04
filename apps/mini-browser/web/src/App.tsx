import { useCallback, useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { api, connectLiveView, type TabInfo, type HistRow, type Dialog, type AgentEvent, type RunRow, type StepRow, type Lesson, type Settings } from './api'

type PanelMode = 'chat' | 'act' | 'history' | 'bookmarks' | 'settings'
interface ChatMsg { role: 'user' | 'assistant'; content: string; runId?: number | null }

export default function App() {
  const [addr, setAddr] = useState('')
  const [info, setInfo] = useState({ url: '', title: '' })
  const [frame, setFrame] = useState<string>('')
  // The page's real viewport, sent with every frame. It used to be a hardcoded
  // 1280x800 here, so the moment the browser window was any other size every
  // click the user made landed somewhere other than where they aimed.
  const [view, setView] = useState({ w: 1280, h: 800 })
  const [dialog, setDialog] = useState<Dialog | null>(null)
  // True while the user is driving the real Chrome window. The agent refuses to
  // act in this state; this is only the UI's copy of that fact.
  const [takeover, setTakeover] = useState(false)
  const [handing, setHanding] = useState(false)
  // Live agent progress. Both panels read it, so it is held once here rather
  // than subscribed to twice.
  const [agent, setAgent] = useState<AgentEvent[]>([])
  const [running, setRunning] = useState(false)

  // The conversation and the in-flight request live here, above the panel
  // switcher, because the panels unmount when you change tab. Holding them in
  // ChatPanel meant switching to Act mid-run destroyed the component the reply
  // was going to resolve into: the progress vanished and the answer never
  // arrived at all. Anything that must outlive a tab switch belongs up here.
  const [msgs, setMsgs] = useState<ChatMsg[]>([])
  const [chatBusy, setChatBusy] = useState(false)

  useEffect(() => {
    api.chatHistory()
      .then((h) => setMsgs(h.map((r) => ({
        role: r.role === 'user' ? 'user' : 'assistant', content: r.content, runId: r.run_id,
      }))))
      .catch(() => {})
  }, [])

  const sendChat = useCallback(async (text: string) => {
    const next: ChatMsg[] = [...msgs, { role: 'user', content: text }]
    setMsgs(next); setChatBusy(true); setAgent([])
    try {
      let ctx = `${info.title} — ${info.url}`
      try {
        const snap = await api.snapshot()
        ctx = `Title: ${snap.title}\nURL: ${snap.url}\n\n${(snap.tree || '').slice(0, 6000)}`
      } catch {}
      const r = await api.chat(next.map((m) => ({ role: m.role, content: m.content })), ctx)
      setMsgs((m) => [...m, { role: 'assistant', content: r.answer, runId: r.run ?? null }])
    } catch (e: any) {
      setMsgs((m) => [...m, { role: 'assistant', content: '⚠️ ' + (e.message || 'error') }])
    } finally { setChatBusy(false) }
  }, [msgs, info])

  useEffect(() => {
    if (!takeover) return
    const t = setInterval(() => {
      api.pingTakeover().then((r) => { if (!r.takeover) setTakeover(false) }).catch(() => {})
    }, 60_000)
    return () => clearInterval(t)
  }, [takeover])

  const clearChat = useCallback(async () => {
    if (!confirm('Xoá toàn bộ lịch sử chat?')) return
    await api.chatClear().catch(() => {})
    setMsgs([])
  }, [])
  const [tabs, setTabs] = useState<TabInfo[]>([])
  const [showPanel, setShowPanel] = useState(true)
  const [panel, setPanel] = useState<PanelMode>('chat')

  const wsRef = useRef<WebSocket | null>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)

  // ---- Live view WebSocket ----
  useEffect(() => {
    const onFrame = (f: any) => {
      setFrame(f.data)
      setInfo({ url: f.url, title: f.title })
      if (f.w && f.h) setView({ w: f.w, h: f.h })
      setAddr((cur) => (document.activeElement?.tagName === 'INPUT' ? cur : f.url))
    }
    api.takeover().then((t) => setTakeover(t.takeover)).catch(() => {})

    const onAgent = (e: AgentEvent) => {
      if (e.kind === 'takeover:start') setTakeover(true)
      if (e.kind === 'takeover:end') setTakeover(false)
      if (e.kind === 'run:start') { setAgent([e]); setRunning(true) }
      else if (e.kind === 'run:end') setRunning(false)
      else setAgent((a) => [...a.slice(-200), e])
    }
    const ws = connectLiveView(onFrame, setDialog, onAgent)
    wsRef.current = ws
    const reconnect = () => {
      setTimeout(() => {
        if (wsRef.current === ws) wsRef.current = connectLiveView(onFrame, setDialog, onAgent)
      }, 1000)
    }
    ws.onclose = reconnect
    refreshTabs()
    return () => { ws.onclose = null; ws.close() }
  }, [])

  const send = (m: any) => wsRef.current?.readyState === 1 && wsRef.current.send(JSON.stringify(m))
  const refreshTabs = () => api.tabs().then((t) => setTabs(t.tabs)).catch(() => {})

  const go = (raw?: string) => {
    const url = (raw ?? addr).trim()
    if (!url) return
    send({ action: 'navigate', url })
    setTimeout(refreshTabs, 800)
  }

  // ---- Viewport interaction: map screen coords → backend viewport coords ----
  const toViewport = (e: React.MouseEvent) => {
    const img = imgRef.current!
    const rect = img.getBoundingClientRect()
    const scale = Math.min(rect.width / view.w, rect.height / view.h)
    const dispW = view.w * scale, dispH = view.h * scale
    const offX = (rect.width - dispW) / 2, offY = (rect.height - dispH) / 2
    const x = (e.clientX - rect.left - offX) / scale
    const y = (e.clientY - rect.top - offY) / scale
    return { x: Math.max(0, Math.min(view.w, x)), y: Math.max(0, Math.min(view.h, y)) }
  }
  const onViewClick = (e: React.MouseEvent) => {
    const { x, y } = toViewport(e)
    send({ action: 'click', x, y })
    imgRef.current?.focus()
  }
  const onViewWheel = (e: React.WheelEvent) => {
    send({ action: 'scroll', dx: e.deltaX, dy: e.deltaY })
  }
  const onViewKey = (e: React.KeyboardEvent) => {
    if (e.metaKey || e.ctrlKey) return
    const special = ['Enter', 'Backspace', 'Delete', 'Tab', 'Escape', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Home', 'End', 'PageUp', 'PageDown']
    if (special.includes(e.key)) { e.preventDefault(); send({ action: 'press', key: e.key }) }
    else if (e.key.length === 1) { e.preventDefault(); send({ action: 'type', text: e.key }) }
  }

  return (
    <div className="app">
      <div className="toolbar">
        <button className="nav-btn" title="Back" onClick={() => send({ action: 'back' })}>←</button>
        <button className="nav-btn" title="Forward" onClick={() => send({ action: 'forward' })}>→</button>
        <button className="nav-btn" title="Reload" onClick={() => send({ action: 'reload' })}>⟳</button>
        <form className="address" onSubmit={(e) => { e.preventDefault(); go() }}>
          <span className="lock">{info.url.startsWith('https') ? '🔒' : '🌐'}</span>
          <input
            value={addr}
            onChange={(e) => setAddr(e.target.value)}
            placeholder="Search Google or type a URL"
            spellCheck={false}
          />
        </form>
        <button className="nav-btn" title="Bookmark" onClick={() => info.url && api.addBookmark(info.url, info.title)}>☆</button>
        <button className={'pill ' + (showPanel ? 'accent' : '')} onClick={() => setShowPanel((v) => !v)}>✨ AI</button>
      </div>

      <div className="tabs">
        {tabs.map((t) => (
          <div key={t.index} className={'tab ' + (t.active ? 'active' : '')}
               onClick={() => { api.switchTab(t.index).then(refreshTabs) }}>
            <span className="tab-title">{t.title || t.url || 'New tab'}</span>
            {tabs.length > 1 && (
              <span className="x" onClick={(e) => { e.stopPropagation(); api.closeTab(t.index).then(refreshTabs) }}>✕</span>
            )}
          </div>
        ))}
        <div className="tab" onClick={() => api.newTab().then(refreshTabs)}>＋</div>
      </div>

      {takeover && (
        <div className="takeover-bar">
          <span className="takeover-dot" />
          <span className="takeover-text">
            <b>Bạn đang điều khiển trình duyệt.</b>{' '}
            Cửa sổ Chrome thật đã mở — đăng nhập ở đó bằng trình quản lý mật khẩu, mã 2FA hoặc
            passkey của bạn. AI không thao tác và <b>không đọc được trang</b>; luồng hình cũng đã
            tạm dừng, nên trang bạn đang nhập mật khẩu không bị truyền đi.
          </span>
          <button className="pill accent" disabled={handing} onClick={async () => {
            setHanding(true)
            try { await api.setTakeover(false); setTakeover(false) } finally { setHanding(false) }
          }}>Đã xong — trả quyền</button>
        </div>
      )}

      {dialog && (
        <div className="dialog-bar">
          <span className="dialog-kind">{dialog.type}</span>
          <span className="dialog-msg">{dialog.message || '(no message)'}</span>
          {dialog.type === 'prompt' && (
            <input
              className="dialog-input"
              defaultValue={dialog.defaultText || ''}
              ref={(el) => { if (el) (window as any).__dlg = el }}
              placeholder="your answer"
            />
          )}
          <button className="pill accent" onClick={() =>
            api.answerDialog(true, (window as any).__dlg?.value).catch(() => {})}>OK</button>
          <button className="pill" onClick={() => api.answerDialog(false).catch(() => {})}>Cancel</button>
        </div>
      )}

      <div className="body">
        <div className="viewport">
          {dialog ? (
            <div className="placeholder">
              The page is waiting on a dialog.<br />Answer it above to carry on.
            </div>
          ) : frame ? (
            <img
              ref={imgRef}
              src={`data:image/jpeg;base64,${frame}`}
              tabIndex={0}
              onClick={onViewClick}
              onWheel={onViewWheel}
              onKeyDown={onViewKey}
              draggable={false}
              alt="page"
            />
          ) : (
            <div className="placeholder">Connecting to browser…<br />Type a URL above to start.</div>
          )}
        </div>
        {showPanel && (
          <SidePanel panel={panel} setPanel={setPanel} pageInfo={info} onOpen={go}
                     agent={agent} running={running}
                     msgs={msgs} chatBusy={chatBusy} onSend={sendChat} onClear={clearChat} />
        )}
      </div>
    </div>
  )
}

// ---------- Side panel: Chat / Act / History / Bookmarks ----------

function SidePanel({ panel, setPanel, pageInfo, onOpen, agent, running, msgs, chatBusy, onSend, onClear }: {
  panel: PanelMode; setPanel: (p: PanelMode) => void
  pageInfo: { url: string; title: string }; onOpen: (u: string) => void
  agent: AgentEvent[]; running: boolean
  msgs: ChatMsg[]; chatBusy: boolean
  onSend: (text: string) => void; onClear: () => void
}) {
  const [showRun, setShowRun] = useState<number | null>(null)
  return (
    <div className="panel-side">
      <div className="panel-tabs">
        {(['chat', 'act', 'history', 'bookmarks', 'settings'] as PanelMode[]).map((p) => (
          <div key={p} className={'panel-tab ' + (panel === p ? 'active' : '')} onClick={() => setPanel(p)}>
            {p === 'chat' ? '💬 Chat' : p === 'act' ? '▶ Act' : p === 'history' ? '🕘 History'
              : p === 'bookmarks' ? '⭐ Marks' : '⚙︎'}
            {((p === 'act' && running) || (p === 'chat' && chatBusy)) && (
              <span className="spinner" style={{ marginLeft: 6 }} />
            )}
          </div>
        ))}
      </div>
      {panel === 'chat' && (
        <ChatPanel msgs={msgs} busy={chatBusy} onSend={onSend} onClear={onClear}
                   agent={agent} running={running}
                   onOpenRun={(id) => { setShowRun(id); setPanel('act') }} />
      )}
      {panel === 'act' && <ActPanel agent={agent} running={running} openRun={showRun} setOpenRun={setShowRun} />}
      {panel === 'settings' && <SettingsPanel />}
      {panel === 'history' && <ListPanel load={api.history} onOpen={onOpen} />}
      {panel === 'bookmarks' && <ListPanel load={api.bookmarks} onOpen={onOpen} />}
    </div>
  )
}

/// One line of live agent progress.
function AgentLine({ e }: { e: AgentEvent }) {
  const d = e.body?.detail || e.body?.text || ''
  const label =
    e.kind === 'plan' ? `Plan ${e.plan}` :
    e.kind === 'step' ? `Step ${e.body?.step}` :
    e.kind === 'step:start' ? `Step ${e.body?.step}` :
    e.kind === 'action' ? 'Action' :
    e.kind === 'verify' ? 'Check' :
    e.kind === 'reject' ? 'Rejected' :
    e.kind === 'ack' ? '' : e.kind
  const bad = e.body?.ok === false
  return (
    <div className={'agent-line ' + (bad ? 'bad' : '')}>
      {label && <span className="agent-tag">{label}</span>}
      <span className="agent-text">{d || (e.body as any)?.goal || ''}</span>
    </div>
  )
}

function ChatPanel({ msgs, busy, onSend, onClear, agent, running, onOpenRun }: {
  msgs: ChatMsg[]; busy: boolean
  onSend: (text: string) => void; onClear: () => void
  agent: AgentEvent[]; running: boolean; onOpenRun: (id: number) => void
}) {
  const [input, setInput] = useState('')
  const endRef = useRef<HTMLDivElement | null>(null)
  useEffect(() => { endRef.current?.scrollIntoView({ behavior: 'smooth' }) }, [msgs, busy, agent])

  const send = () => {
    const text = input.trim()
    if (!text || busy) return
    setInput('')
    onSend(text)
  }

  return (
    <>
      <div className="panel-content">
        {msgs.length === 0 && !busy && !running && (
          <div className="hint">
            Ask about this page, or just tell me what to do — "mở 4 bài báo và đọc giá vàng".
            I will plan it, carry it out, and check the result before answering.
          </div>
        )}
        {msgs.map((m, i) => (
          <div key={i} className={'msg ' + m.role}>
            <div className="who">
              {m.role === 'user' ? 'You' : 'SenClaw Browser'}
              {m.runId != null && (
                <span className="run-link" onClick={() => onOpenRun(m.runId!)}>▶ run #{m.runId}</span>
              )}
            </div>
            <div className="bubble"><ReactMarkdown remarkPlugins={[remarkGfm]}>{m.content}</ReactMarkdown></div>
          </div>
        ))}
        {/* The run in progress. Both this and the transcript are owned by App,
            so leaving for the Act tab and coming back shows it still going —
            and the answer still lands when it arrives. */}
        {(busy || running) && (
          <div className="msg assistant">
            <div className="who">SenClaw Browser</div>
            <div className="bubble">
              {agent.length === 0 && <><span className="spinner" /> thinking…</>}
              {agent.map((e, i) => <AgentLine key={i} e={e} />)}
              {running && <div className="agent-line"><span className="spinner" /> working…</div>}
            </div>
          </div>
        )}
        <div ref={endRef} />
      </div>
      <div className="composer">
        <textarea value={input} onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send() } }}
          placeholder="Ask, or tell me what to do on this page…" />
        <div className="composer-row">
          <button className="pill" onClick={onClear} disabled={busy}>Clear</button>
          <button className="pill accent" onClick={send} disabled={busy}>Send</button>
        </div>
      </div>
    </>
  )
}

/// The Act panel: every run this browser has performed, what it planned, what it
/// did, and whether the independent check passed.
function ActPanel({ agent, running, openRun, setOpenRun }: {
  agent: AgentEvent[]; running: boolean
  openRun: number | null; setOpenRun: (id: number | null) => void
}) {
  const [goal, setGoal] = useState('')
  const [runs, setRuns] = useState<RunRow[]>([])
  const [steps, setSteps] = useState<StepRow[]>([])

  const refresh = () => api.runs().then(setRuns).catch(() => {})
  useEffect(() => { refresh() }, [])
  useEffect(() => { if (!running) refresh() }, [running])
  useEffect(() => {
    if (openRun == null) { setSteps([]); return }
    api.run(openRun).then((r) => setSteps(r.steps)).catch(() => setSteps([]))
  }, [openRun])

  const run = async () => {
    const g = goal.trim()
    if (!g || running) return
    setGoal('')
    try { const r = await api.act(g); setOpenRun(r.run ?? null) }
    finally { refresh() }
  }

  const detail = runs.find((r) => r.id === openRun)

  return (
    <>
      <div className="panel-content">
        <div className="hint">
          Give a goal. It is planned, carried out step by step, then checked against the page —
          and replanned if the check fails, up to the plan budget.
        </div>

        {running && (
          <div className="act-step">
            <span className="k">Running</span>
            {agent.map((e, i) => <AgentLine key={i} e={e} />)}
          </div>
        )}

        {detail && (
          <div className="act-step">
            <span className="k">Run #{detail.id}</span>
            <span className="badge">{detail.source}</span>
            <div style={{ marginTop: 4 }}>{detail.goal}</div>
            <div style={{ marginTop: 4, color: detail.verified ? 'var(--muted)' : 'var(--danger)' }}>
              {detail.verified === true ? '✅ ' : detail.verified === false ? '⚠️ ' : ''}
              {detail.outcome || detail.status} · {detail.plans_used} plan(s)
            </div>
            {steps.map((st) => (
              <div key={st.id} className={'agent-line ' + (st.ok ? '' : 'bad')}>
                <span className="agent-tag">
                  {st.kind === 'plan' ? `Plan ${st.plan_no}` :
                   st.kind === 'verify' ? 'Check' :
                   st.kind === 'step' ? `Step ${st.plan_no}.${st.step_no}` : st.kind}
                </span>
                <span className="agent-text">{st.detail}</span>
              </div>
            ))}
            <button className="pill" style={{ marginTop: 6 }} onClick={() => setOpenRun(null)}>Close</button>
          </div>
        )}

        {!detail && runs.length === 0 && <div className="hint">No runs yet.</div>}
        {!detail && runs.map((r) => (
          <div key={r.id} className="list-item" onClick={() => setOpenRun(r.id)}>
            <div>
              {r.verified === true ? '✅ ' : r.verified === false ? '⚠️ ' : '⏳ '}
              {r.goal}
            </div>
            <div className="u">
              #{r.id} · {r.source} · {r.plans_used} plan(s) · {r.outcome || r.status}
            </div>
          </div>
        ))}
      </div>
      <div className="composer">
        <textarea value={goal} onChange={(e) => setGoal(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); run() } }}
          placeholder="e.g. mở 4 bài báo đầu và đọc giá vàng ở mỗi bài" />
        <div className="composer-row"><button className="pill accent" onClick={run} disabled={running}>▶ Run</button></div>
      </div>
    </>
  )
}

/// Settings, plus what the agent has learned so far.
///
/// The knowledge list is here rather than hidden away because it is the one
/// thing in the app that changes the agent's behaviour without anyone asking it
/// to. If a note is wrong — a site redesigned, a shortcut that stopped working —
/// the user needs to be able to see it and delete it.
function SettingsPanel() {
  const [s, setS] = useState<Settings | null>(null)
  const [lessons, setLessons] = useState<Lesson[]>([])

  const load = () => {
    api.settings().then(setS).catch(() => {})
    api.knowledge().then(setLessons).catch(() => setLessons([]))
  }
  useEffect(load, [])

  const save = async (patch: { max_plans?: number; learning?: boolean }) => {
    setS((cur) => (cur ? { ...cur, ...patch } as Settings : cur))
    await api.saveSettings(patch).catch(() => {})
    load()
  }

  const forget = async (l: Lesson) => {
    if (!confirm(`Quên điều này?\n\n${l.note}`)) return
    await api.forgetLesson(l.id).catch(() => {})
    load()
  }

  if (!s) return <div className="panel-content"><div className="hint">Loading…</div></div>

  const byHost = lessons.reduce<Record<string, Lesson[]>>((acc, l) => {
    (acc[l.host] ||= []).push(l)
    return acc
  }, {})

  return (
    <div className="panel-content">
      <div className="setting-group">Agent</div>

      <div className="setting-row">
        <label>
          Max plans per request
          <div className="u">How many times it may replan before giving up.</div>
        </label>
        <input type="number" min={1} max={s.hard_max_plans} value={s.max_plans}
               onChange={(e) => save({
                 max_plans: Math.max(1, Math.min(s.hard_max_plans, Number(e.target.value) || 1)),
               })} />
        <span className="u">max {s.hard_max_plans}</span>
      </div>

      <div className="setting-row">
        <label>
          Learn from successful runs
          <div className="u">
            After a run passes its check, note what worked on that site and use it next time.
          </div>
        </label>
        <input type="checkbox" checked={s.learning}
               onChange={(e) => save({ learning: e.target.checked })} />
      </div>

      <div className="setting-group">Browser</div>
      <div className="setting-row">
        <label>
          Sign in yourself
          <div className="u">
            Opens the real Chrome window so you can log in to Google, Facebook, X and the rest
            with your own password manager or passkey. The AI never types credentials, and cannot
            act at all while you hold control. What you sign into stays in the profile and is
            available to later runs.
          </div>
        </label>
        <button className="pill" onClick={() => api.setTakeover(true).catch(() => {})}>
          Take control
        </button>
      </div>
      <div className="setting-row">
        <label>
          Window
          <div className="u">
            {s.headful
              ? 'A real Chrome window is showing (MB_HEADFUL=1).'
              : 'Running out of sight — the page appears only here. Set MB_HEADFUL=1 to show the real window.'}
          </div>
        </label>
      </div>
      <div className="setting-row">
        <label>
          Languages
          <div className="u">Sent as Accept-Language and reported by the page. Set MB_ACCEPT_LANGUAGE to change.</div>
        </label>
        <span className="u">{s.accept_language}</span>
      </div>

      <div className="setting-group">
        What it has learned{lessons.length > 0 ? ` (${lessons.length})` : ''}
      </div>
      {lessons.length === 0 && (
        <div className="hint">
          Nothing yet. Notes appear here after a run finishes and passes its check —
          and only when the run taught something that was not obvious.
        </div>
      )}
      {Object.entries(byHost).map(([host, ls]) => (
        <div key={host} style={{ marginBottom: 10 }}>
          <div className="u" style={{ marginBottom: 4 }}>{host === '*' ? 'any site' : host}</div>
          {ls.map((l) => (
            <div key={l.id} className="lesson">
              <span className={'lesson-kind ' + l.kind}>{l.kind}</span>
              <span className="lesson-note">{l.note}</span>
              <span className="u" title="runs that succeeded / failed while this was in use">
                {l.wins}✓ {l.losses}✗
              </span>
              <span className="x" onClick={() => forget(l)} title="forget this">✕</span>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

function ListPanel({ load, onOpen }: { load: () => Promise<HistRow[]>; onOpen: (u: string) => void }) {
  const [rows, setRows] = useState<HistRow[]>([])
  useEffect(() => { load().then(setRows).catch(() => setRows([])) }, [])
  return (
    <div className="panel-content">
      {rows.length === 0 && <div className="hint">Nothing here yet.</div>}
      {rows.map((r) => (
        <div key={r.id} className="list-item" onClick={() => onOpen(r.url)}>
          <div>{r.title || r.url}</div>
          <div className="u">{r.url}</div>
        </div>
      ))}
    </div>
  )
}
