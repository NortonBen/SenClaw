import Markdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import type { Agent, OfficeEvent } from './types'

function hhmmss(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString('vi-VN', { hour12: false })
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
  const nameOf = (key: string): string => {
    if (key === 'sep') return 'SẾP'
    if (key === 'he-thong') return 'HỆ THỐNG'
    const found = agents.find((a) => a.key === key)
    return found ? found.name : key.replace(/-/g, ' ').toUpperCase()
  }

  return (
    <div className="feed">
      <div className="feed-day">{dayLabel}</div>
      {events.map((e) => {
        switch (e.kind) {
          case 'chat': {
            const fromBoss = e.actor === 'sep'
            return (
              <div key={e.id} className={`msg${fromBoss ? ' sep-msg' : ''}`}>
                <div className="who">
                  {nameOf(e.actor)} · {hhmmss(e.created_at)}
                </div>
                <div className="box">{e.text}</div>
              </div>
            )
          }
          case 'report':
            return (
              <div key={e.id} className="msg report">
                <div className="who">
                  {nameOf(e.actor)} — BÁO CÁO · {hhmmss(e.created_at)}
                </div>
                <div className="box report-md">
                  <Markdown remarkPlugins={[remarkGfm]}>{e.text}</Markdown>
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
