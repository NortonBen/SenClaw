import { useEffect, useRef } from 'react'
import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { Avatar } from './avatar'
import { SpeakButton } from './voice'
import type { Agent, OfficeEvent } from './types'
import { tr, getLang } from './i18n'

function hhmmss(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString(getLang() === 'en' ? 'en-US' : 'vi-VN', {
    hour12: false,
  })
}

/** Single newlines become hard breaks so plain-text messages (bullets,
 *  handover notes) keep their line structure under markdown rendering. */
function withBreaks(text: string): string {
  return text.replace(/\n/g, '  \n')
}

export function Md({ children }: { children: string }) {
  return (
    <div className="report-md">
      <Markdown remarkPlugins={[remarkGfm]}>{withBreaks(children)}</Markdown>
    </div>
  )
}

export function Feed({
  events,
  agents,
  dayLabel,
}: {
  events: OfficeEvent[]
  agents: Agent[]
  dayLabel: string
}) {
  const boxRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = boxRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [events.length])

  const nameOf = (key: string): string => {
    if (key === 'sep') return tr('SẾP')
    if (key === 'he-thong') return tr('HỆ THỐNG')
    const found = agents.find((a) => a.key === key)
    return found ? found.name : key.replace(/-/g, ' ').toUpperCase()
  }

  return (
    <div className="feed" ref={boxRef}>
      <div className="feed-day">{dayLabel}</div>
      {events.map((e) => {
        switch (e.kind) {
          case 'chat': {
            const fromBoss = e.actor === 'sep'
            return (
              <div key={e.id} className={`msg${fromBoss ? ' sep-msg' : ''}`}>
                <div className="who">
                  {!fromBoss && <Avatar agentKey={e.actor} size={15} />} {nameOf(e.actor)} ·{' '}
                  {hhmmss(e.created_at)} {fromBoss && <Avatar agentKey="sep" size={15} />}
                </div>
                <div className="box">
                  <Md>{e.text}</Md>
                </div>
              </div>
            )
          }
          case 'report':
            return (
              <div key={e.id} className="msg report">
                <div className="who" style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  <Avatar agentKey={e.actor} size={15} /> {nameOf(e.actor)} — {tr('BÁO CÁO')} ·{' '}
                  {hhmmss(e.created_at)}
                  <span style={{ marginLeft: 'auto' }}>
                    <SpeakButton text={e.text} label={tr('Đọc')} />
                  </span>
                </div>
                <div className="box">
                  <Md>{e.text}</Md>
                </div>
              </div>
            )
          case 'assign':
          case 'handoff':
            return (
              <div key={e.id} className="logline">
                · {hhmmss(e.created_at)} — {nameOf(e.actor)} → {nameOf(e.target)}: {e.text}
              </div>
            )
          case 'wiki':
            return (
              <div key={e.id} className="logline">
                📚 {hhmmss(e.created_at)} — {nameOf(e.actor)}: {e.text}
              </div>
            )
          case 'file':
            return (
              <div key={e.id} className="logline">
                📁 {hhmmss(e.created_at)} — {nameOf(e.actor)}: {e.text}
              </div>
            )
          case 'tool':
            return (
              <div key={e.id} className="logline" style={{ color: 'var(--working)' }}>
                🔧 {hhmmss(e.created_at)} — {nameOf(e.actor)}: {e.text}
              </div>
            )
          case 'boss':
            // Vòng duyệt của Sếp: nộp chờ duyệt / duyệt / trả lại.
            return (
              <div key={e.id} className="logline" style={{ color: 'var(--working)', fontStyle: 'normal' }}>
                👑 {hhmmss(e.created_at)} — {e.text}
              </div>
            )
          case 'system':
            return (
              <div key={e.id} className="sysline">
                ⚠ {e.text}
              </div>
            )
          default:
            return null // bubbles live in the scene, not the feed
        }
      })}
    </div>
  )
}
