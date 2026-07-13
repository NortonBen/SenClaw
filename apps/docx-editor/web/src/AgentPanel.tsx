import { useCallback, useEffect, useRef, useState } from 'react'

type ChatMsg = { role: 'user' | 'assistant'; content: string; rewrite?: string | null }

type Props = {
  docId: number
  getDocText: () => string
  onApplyRewrite: (text: string) => Promise<void> | void
  close: () => void
}

const SUGGESTIONS = [
  'Tóm tắt tài liệu thành 5 gạch đầu dòng',
  'Đề xuất chỉnh sửa để giọng văn trang trọng hơn',
  'Đâu là đoạn dài dòng nhất và nên rút gọn thế nào?',
  'Rewrite the whole document to be clearer and shorter',
]

export default function AgentPanel({ docId, getDocText, onApplyRewrite, close }: Props) {
  const [messages, setMessages] = useState<ChatMsg[]>([])
  const [draft, setDraft] = useState('')
  const [busy, setBusy] = useState(false)
  const [modelName, setModelName] = useState('')
  const bodyRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    if (bodyRef.current) bodyRef.current.scrollTop = bodyRef.current.scrollHeight
  }, [messages, busy])

  const send = useCallback(async (text: string) => {
    if (!text.trim() || busy) return
    const userMsg: ChatMsg = { role: 'user', content: text.trim() }
    const nextMessages = [...messages, userMsg]
    setMessages(nextMessages)
    setDraft('')
    setBusy(true)
    try {
      const res = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          doc_id: docId,
          doc_text: getDocText(),
          messages: nextMessages,
        }),
      })
      if (!res.ok) throw new Error(`${res.status} ${res.statusText}`)
      const data = await res.json() as { reply: string; model: string; rewrite: string | null }
      setModelName(data.model || '')
      setMessages(m => [...m, { role: 'assistant', content: data.reply, rewrite: data.rewrite }])
    } catch (e) {
      setMessages(m => [...m, { role: 'assistant', content: `⚠️ ${String(e)}` }])
    } finally {
      setBusy(false)
    }
  }, [busy, docId, getDocText, messages])

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send(draft)
    }
  }

  return (
    <div className="agent-panel">
      <div className="agent-header">
        <h3>✨ AI Agent</h3>
        {modelName && <span className="badge">{modelName}</span>}
        <button onClick={close} title="Đóng panel" style={{ padding: '2px 8px' }}>✕</button>
      </div>

      <div className="agent-body" ref={bodyRef}>
        {messages.length === 0 && (
          <>
            <div className="agent-intro">
              Hỏi Agent về tài liệu này — tóm tắt, phản hồi giọng văn, viết lại, đề xuất bố cục.
              Khi Agent trả về một bản viết lại toàn văn, sẽ có nút <b>Áp dụng</b>.
            </div>
            <div className="agent-suggestions">
              {SUGGESTIONS.map(s => (
                <button key={s} onClick={() => send(s)}>{s}</button>
              ))}
            </div>
          </>
        )}
        {messages.map((m, i) => (
          <div key={i} className={`msg ${m.role}`}>
            {stripRewriteBlock(m.content)}
            {m.rewrite && (
              <div className="rewrite-actions">
                <button className="primary" onClick={() => onApplyRewrite(m.rewrite!)}>
                  ✓ Áp dụng bản viết lại
                </button>
                <span style={{ fontSize: 11, alignSelf: 'center', color: 'var(--muted)' }}>
                  {m.rewrite.length} ký tự
                </span>
              </div>
            )}
          </div>
        ))}
        {busy && <div className="msg assistant thinking">Agent đang soạn…</div>}
      </div>

      <div className="agent-input">
        <textarea
          value={draft}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Hỏi Agent về tài liệu (Enter để gửi, Shift+Enter xuống dòng)…"
          disabled={busy}
          rows={2}
        />
        <button className="primary" onClick={() => send(draft)} disabled={busy || !draft.trim()}>
          Gửi
        </button>
      </div>
      <div className="agent-footer">
        Agent đọc plain-text của tài liệu để trả lời. Áp dụng viết lại sẽ ghi đè nội dung.
      </div>
    </div>
  )
}

function stripRewriteBlock(s: string): string {
  const m = s.match(/([\s\S]*?)<<<DOC>>>[\s\S]*?<<<END>>>([\s\S]*)/)
  if (!m) return s
  return (m[1].trim() + (m[2].trim() ? '\n\n' + m[2].trim() : '')).trim() || '(Bản viết lại đã sẵn sàng)'
}
