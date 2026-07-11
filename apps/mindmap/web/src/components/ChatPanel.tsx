import { useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { ChatMsg, ChatSession } from '../api'

interface Props {
  messages: ChatMsg[]
  sessions: ChatSession[]
  activeSession: number | null
  busy: boolean
  importing: boolean
  hasMap: boolean
  pins: { id: number; text: string; note: string }[]
  onSend: (text: string) => void
  onUnpin: (id: number) => void
  onClearPins: () => void
  onNewSession: () => void
  onSwitchSession: (id: number) => void
  onRenameSession: (id: number, title: string) => void
  onDeleteSession: (id: number) => void
  onAttach: (file: File) => void
  onGenerateFromText: (text: string) => void
  onSaveNote: (text: string) => void
  onClose: () => void
}

export default function ChatPanel(p: Props) {
  const [text, setText] = useState('')
  const listRef = useRef<HTMLDivElement>(null)
  const taRef = useRef<HTMLTextAreaElement>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    const el = listRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [p.messages, p.busy])

  const send = () => {
    const t = text.trim()
    if (!t || p.busy) return
    p.onSend(t)
    setText('')
    if (taRef.current) taRef.current.style.height = 'auto'
  }

  const cur = p.sessions.find((s) => s.id === p.activeSession)

  return (
    <div className="chat">
      <div className="chead">
        <span>💬 Trợ lý AI</span>
        <div className="spacer" />
        <button className="icon-btn" title="Đổi tên hội thoại" disabled={!cur} onClick={() => {
          if (!cur) return
          const t = prompt('Tên hội thoại:', cur.title)
          if (t && t.trim()) p.onRenameSession(cur.id, t.trim())
        }}>
          ✎
        </button>
        <button className="icon-btn" title="Xoá hội thoại" disabled={!cur} onClick={() => {
          if (cur && confirm('Xoá hội thoại này?')) p.onDeleteSession(cur.id)
        }}>
          🗑
        </button>
        <button className="icon-btn" title="Ẩn panel" onClick={p.onClose}>
          ✕
        </button>
      </div>

      {p.hasMap && (
        <div className="chat-sessions">
          <select
            value={p.activeSession ?? ''}
            onChange={(e) => p.onSwitchSession(Number(e.target.value))}
            disabled={p.sessions.length === 0}
          >
            {p.sessions.length === 0 && <option value="">Chưa có hội thoại</option>}
            {p.sessions.map((s) => (
              <option key={s.id} value={s.id}>
                {s.title} · {s.message_count}
              </option>
            ))}
          </select>
          <button className="btn" title="Hội thoại mới" onClick={p.onNewSession}>
            ＋
          </button>
        </div>
      )}

      <div className="msgs" ref={listRef}>
        {p.messages.length === 0 && (
          <div className="empty-chat">
            Hỏi AI để brainstorm ý tưởng, phân tích, hoặc gợi ý cấu trúc cho mindmap.
            <br />
            <br />
            Dùng nút <b>✨ AI</b> trên mỗi nút để sinh nhánh con, hoặc <b>📎</b> để đính kèm file
            (ảnh sẽ được OCR) và tạo sơ đồ từ nội dung.
          </div>
        )}
        {p.messages.map((m, i) => (
          <div key={i} className={`msg ${m.role}`}>
            {m.role === 'assistant' ? (
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{m.content}</ReactMarkdown>
            ) : (
              m.content
            )}
            {m.role === 'assistant' && (
              <div className="msg-foot">
                <button
                  className="mk-map"
                  title="Chuyển câu trả lời này thành sơ đồ tư duy"
                  disabled={!p.hasMap || p.busy}
                  onClick={() => p.onGenerateFromText(m.content)}
                >
                  🧠 Tạo sơ đồ
                </button>
                <button
                  className="mk-map"
                  title="Lưu câu trả lời này làm ghi chú của nút đang chọn"
                  disabled={!p.hasMap || p.busy}
                  onClick={() => p.onSaveNote(m.content)}
                >
                  📝 Ghi chú
                </button>
                {(m.model || m.ms) && (
                  <span className="tag">
                    {m.model}
                    {m.ms ? ` · ${(m.ms / 1000).toFixed(1)}s` : ''}
                  </span>
                )}
              </div>
            )}
          </div>
        ))}
        {p.busy && (
          <div className="msg assistant">
            <span className="spin" /> đang suy nghĩ…
          </div>
        )}
      </div>

      {p.pins.length > 0 && (
        <div className="chat-pins">
          <span className="pins-count" title="Ngữ cảnh đang ghim cho AI">
            📌 {p.pins.length}
          </span>
          {p.pins.map((pin) => (
            <span className="pin-chip" key={pin.id} title={pin.note || pin.text}>
              {pin.text}
              <button onClick={() => p.onUnpin(pin.id)} title="Bỏ ghim">
                ×
              </button>
            </span>
          ))}
          <button className="pins-clear" onClick={p.onClearPins}>
            xoá hết
          </button>
        </div>
      )}

      <div className="cinput">
        <input
          ref={fileRef}
          type="file"
          style={{ display: 'none' }}
          accept="image/*,.txt,.md,.markdown,.csv,.json,.log,.html,.xml,.yaml,.yml"
          onChange={(e) => {
            const f = e.target.files?.[0]
            if (f) p.onAttach(f)
            e.target.value = ''
          }}
        />
        <button
          className="icon-btn attach"
          title="Đính kèm file / ảnh → OCR → sinh sơ đồ"
          disabled={!p.hasMap || p.importing}
          onClick={() => fileRef.current?.click()}
        >
          {p.importing ? <span className="spin" /> : '📎'}
        </button>
        <textarea
          ref={taRef}
          value={text}
          placeholder={p.hasMap ? 'Nhắn cho trợ lý…' : 'Tạo hoặc chọn một map trước…'}
          disabled={!p.hasMap}
          onChange={(e) => {
            setText(e.target.value)
            e.target.style.height = 'auto'
            e.target.style.height = Math.min(e.target.scrollHeight, 140) + 'px'
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
        />
        <button className="btn primary" onClick={send} disabled={p.busy || !text.trim()}>
          ➤
        </button>
      </div>
    </div>
  )
}
