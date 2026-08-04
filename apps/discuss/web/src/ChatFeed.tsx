import { useEffect, useRef } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkBreaks from 'remark-breaks'
import remarkGfm from 'remark-gfm'
import type { Member, Message } from './types'
import { CLAIM_LABEL, HAT_COLORS, HAT_NAMES, PROV_LABEL } from './types'

/** Nội dung chat là markdown (member hay trả **đậm**, gạch đầu dòng, bảng…);
 * remark-breaks giữ xuống dòng đơn đúng như chat. Link mở tab mới. */
function Md({ text }: { text: string }) {
  return (
    <div className="md-chat">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkBreaks]}
        components={{
          a: (props) => <a {...props} target="_blank" rel="noreferrer" />,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  )
}

function fmtTime(ts: number): string {
  const d = new Date(ts * 1000)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

export default function ChatFeed({
  messages, members, hasDiscussion, onOpenDoc,
}: {
  messages: Message[]
  members: Member[]
  hasDiscussion: boolean
  onOpenDoc: (id: number) => void
}) {
  const endRef = useRef<HTMLDivElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const stickRef = useRef(true)

  useEffect(() => {
    if (stickRef.current) endRef.current?.scrollIntoView({ behavior: 'smooth', block: 'end' })
  }, [messages.length])

  const memberOf = (id: number | null) => members.find((m) => m.id === id)
  const msgById = (id: number | null) => messages.find((m) => m.id === id)

  const onScroll = () => {
    const el = boxRef.current
    if (!el) return
    stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80
  }

  const renderCitations = (m: Message) => {
    if (!m.citations?.length) return null
    return (
      <div className="citations">
        {m.citations.map((c, i) => {
          const label = c.kind === 'doc' ? `📄 ${c.ref}` : c.kind === 'url' ? `🔗 ${shortUrl(c.ref)}` : `🛠 ${c.ref}`
          const title = c.quote ? `“${c.quote}”` : c.ref
          if (c.kind === 'doc') {
            const id = parseInt(c.ref.replace('doc:', ''), 10)
            return (
              <button key={i} className={`cite ${c.verified ? '' : 'cite-bad'}`} title={title}
                onClick={() => !Number.isNaN(id) && onOpenDoc(id)}>
                {label}{!c.verified && ' ⚠'}
              </button>
            )
          }
          if (c.kind === 'url') {
            return (
              <a key={i} className="cite" href={c.ref} target="_blank" rel="noreferrer" title={title}>
                {label}
              </a>
            )
          }
          return (
            <span key={i} className="cite cite-tool" title={title}>{label}</span>
          )
        })}
      </div>
    )
  }

  if (messages.length === 0) {
    return (
      <div className="feed">
        <div className="feed-empty">
          {hasDiscussion
            ? 'Phòng họp im ắng — bấm ▶ Bắt đầu để đội thảo luận, hoặc BOSS phát biểu trước ở ô bên dưới.'
            : 'Chưa có phiên thảo luận. Bấm “➕ Phiên mới”: đặt chủ đề + yêu cầu kết quả, chọn thành viên rồi để đội AI tranh luận cho bạn.'}
        </div>
      </div>
    )
  }

  return (
    <div className="feed" ref={boxRef} onScroll={onScroll}>
      {messages.filter((m) => m.kind !== 'minutes_note').map((m) => {
        if (m.author_kind === 'system') {
          return (
            <div key={m.id} className={`sys ${m.flags?.error ? 'sys-err' : ''}`}>
              {m.content}
            </div>
          )
        }
        if (m.author_kind === 'manager') {
          return (
            <div key={m.id} className="mgr-note">
              <div className="mgr-head">🔵 Điều phối · vòng {m.round} · {fmtTime(m.created_at)}</div>
              <div className="mgr-body"><Md text={m.content} /></div>
            </div>
          )
        }
        const isBoss = m.author_kind === 'boss'
        const mem = memberOf(m.member_id)
        const name = isBoss ? 'BOSS' : mem?.name || (m.author_kind === 'secretary' ? 'Thư Ký' : '—')
        const replied = m.reply_to ? msgById(m.reply_to) : null
        const repliedMem = replied ? (replied.author_kind === 'boss' ? 'BOSS' : memberOf(replied.member_id)?.name || '#' + replied.id) : null
        return (
          <div key={m.id} className={`msg ${isBoss ? 'msg-boss' : ''}`}>
            <div className="msg-head">
              <span className="msg-name" style={isBoss ? { color: 'var(--boss)' } : undefined}>
                {isBoss ? '👑 BOSS' : name}
              </span>
              {m.hat && (
                <span className="hat-dot" title={HAT_NAMES[m.hat] || m.hat}
                  style={{ background: HAT_COLORS[m.hat] || '#888' }} />
              )}
              {m.claim_type && <span className={`badge badge-${m.claim_type}`}>{CLAIM_LABEL[m.claim_type]}</span>}
              {m.provability && (
                <span className={`badge badge-prov-${m.provability}`}>{PROV_LABEL[m.provability]}</span>
              )}
              {m.stance === 'agree' && <span className="badge badge-agree">✅ đồng tình</span>}
              {m.stance === 'disagree' && <span className="badge badge-disagree">⚔️ phản đối</span>}
              {Boolean(m.flags?.missing_evidence) && <span className="badge badge-warn">thiếu dẫn chứng</span>}
              {Boolean(m.flags?.downgraded) && <span className="badge badge-warn" title={String(m.flags.downgraded)}>đã hạ nhãn</span>}
              <span className="msg-time">v{m.round} · {fmtTime(m.created_at)}</span>
            </div>
            {replied && (
              <div className="reply-quote" title={replied.content}>
                ↩ {repliedMem}: {replied.content.length > 90 ? replied.content.slice(0, 90) + '…' : replied.content}
              </div>
            )}
            {m.kind === 'result_note' ? (
              <div className="msg-body result-note">📋 {m.content}</div>
            ) : (
              <div className="msg-body"><Md text={m.content} /></div>
            )}
            {renderCitations(m)}
          </div>
        )
      })}
      <div ref={endRef} />
    </div>
  )
}

function shortUrl(u: string): string {
  try {
    const url = new URL(u)
    return url.hostname + (url.pathname.length > 18 ? url.pathname.slice(0, 18) + '…' : url.pathname)
  } catch {
    return u.length > 40 ? u.slice(0, 40) + '…' : u
  }
}
