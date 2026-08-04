// Phòng họp isometric — thuần SVG theo kỹ thuật apps/ai-office (không three.js):
// chiếu iso(x,y,z) = [(x-y)*46, (x+y)*23 - z], khối 3D = polygon đáy + đỉnh
// dịch lên theo trục màn hình, vẽ back-to-front. Vị trí nhân vật là hàm thuần
// của trạng thái — đổi status trong DB là cảnh tự sống.

import { useMemo } from 'react'
import type { Discussion, Member, Message } from './types'
import { HAT_COLORS } from './types'

const ISO_X = 46
const ISO_Y = 23
const FLOOR_W = 10
const FLOOR_D = 7

function iso(x: number, y: number, z = 0): [number, number] {
  return [(x - y) * ISO_X, (x + y) * ISO_Y - z]
}
function pts(list: [number, number][]): string {
  return list.map((p) => p.join(',')).join(' ')
}

// Ghế member quanh bàn (x, y) — thứ tự cấp chỗ
const SEATS: [number, number][] = [
  [3.4, 1.5],
  [5.0, 1.5],
  [6.6, 1.5],
  [3.4, 4.9],
  [5.0, 4.9],
  [6.6, 4.9],
  [2.1, 3.2],
  [7.9, 2.6],
]
const SECRETARY_SEAT: [number, number] = [7.9, 3.9]
const BOSS_SEAT: [number, number] = [5.0, 6.1]
const MANAGER_POS: [number, number] = [1.7, 1.05]

// Màu deterministic cho member từ key (djb2 như ai-office)
function colorFor(key: string): string {
  let h = 5381
  for (let i = 0; i < key.length; i++) h = ((h << 5) + h + key.charCodeAt(i)) | 0
  const palette = ['#6c8cff', '#5fb3a3', '#c98bdb', '#e0a458', '#7fb069', '#d1798f', '#5fa8d3', '#b0a47f']
  return palette[Math.abs(h) % palette.length]
}

function Floor() {
  const corners: [number, number][] = [iso(0, 0), iso(FLOOR_W, 0), iso(FLOOR_W, FLOOR_D), iso(0, FLOOR_D)]
  // Thảm giữa phòng
  const rug: [number, number][] = [iso(2.3, 1.7), iso(7.7, 1.7), iso(7.7, 5.3), iso(2.3, 5.3)]
  return (
    <g>
      <polygon points={pts(corners)} fill="var(--floor)" stroke="var(--floor-edge)" strokeWidth="1.5" />
      {Array.from({ length: FLOOR_W - 1 }, (_, i) => (
        <line key={`gx${i}`} x1={iso(i + 1, 0)[0]} y1={iso(i + 1, 0)[1]} x2={iso(i + 1, FLOOR_D)[0]} y2={iso(i + 1, FLOOR_D)[1]} stroke="var(--floor-line)" strokeWidth="0.6" />
      ))}
      {Array.from({ length: FLOOR_D - 1 }, (_, i) => (
        <line key={`gy${i}`} x1={iso(0, i + 1)[0]} y1={iso(0, i + 1)[1]} x2={iso(FLOOR_W, i + 1)[0]} y2={iso(FLOOR_W, i + 1)[1]} stroke="var(--floor-line)" strokeWidth="0.6" />
      ))}
      <polygon points={pts(rug)} fill="var(--rug)" opacity="0.55" rx="4" />
    </g>
  )
}

function Walls() {
  const H = 86
  const left: [number, number][] = [iso(0, 0), iso(0, FLOOR_D), iso(0, FLOOR_D, H), iso(0, 0, H)]
  const back: [number, number][] = [iso(0, 0), iso(FLOOR_W, 0), iso(FLOOR_W, 0, H), iso(0, 0, H)]
  // Cửa sổ trên tường sau
  const win = (x0: number, x1: number): [number, number][] => [
    iso(x0, 0, 32), iso(x1, 0, 32), iso(x1, 0, 66), iso(x0, 0, 66),
  ]
  return (
    <g>
      <polygon points={pts(left)} fill="var(--wall-left)" />
      <polygon points={pts(back)} fill="var(--wall-back)" />
      <polygon points={pts(win(5.6, 7.2))} fill="var(--window)" opacity="0.9" />
      <polygon points={pts(win(7.7, 9.3))} fill="var(--window)" opacity="0.9" />
    </g>
  )
}

function Whiteboard({ score, missing, round, maxRounds }: { score: number; missing: number; round: number; maxRounds: number }) {
  // Bảng theo dõi của Manager trên tường sau (x 0.9..3.3)
  const b: [number, number][] = [iso(0.9, 0.02, 26), iso(3.3, 0.02, 26), iso(3.3, 0.02, 78), iso(0.9, 0.02, 78)]
  const [tx, ty] = iso(2.1, 0.02, 60)
  const barX0 = iso(1.1, 0.02, 40)
  const barX1 = iso(3.1, 0.02, 40)
  const w = barX1[0] - barX0[0]
  return (
    <g>
      <polygon points={pts(b)} fill="var(--board)" stroke="var(--board-edge)" strokeWidth="1.5" />
      <text x={tx} y={ty - 8} textAnchor="middle" className="board-title">TIẾN ĐỘ vs YÊU CẦU BOSS</text>
      <text x={tx} y={ty + 10} textAnchor="middle" className="board-score">{score}/100 · vòng {round}/{maxRounds}</text>
      <rect x={barX0[0]} y={barX0[1]} width={w} height={6} rx={3} fill="var(--bar-bg)" />
      <rect x={barX0[0]} y={barX0[1]} width={(w * Math.max(0, Math.min(100, score))) / 100} height={6} rx={3} fill={score >= 80 ? 'var(--ok)' : 'var(--warn)'} />
      {missing > 0 && (
        <text x={tx} y={barX0[1] + 22} textAnchor="middle" className="board-note">còn thiếu {missing} mục</text>
      )}
    </g>
  )
}

function Table() {
  const x = 2.8, y = 2.3, w = 4.4, d = 2.2, h = 30
  const base: [number, number][] = [iso(x, y), iso(x + w, y), iso(x + w, y + d), iso(x, y + d)]
  const top = base.map(([a, b]) => [a, b - h] as [number, number])
  return (
    <g>
      {/* thân bàn: 2 mặt thấy được */}
      <polygon points={pts([base[3], base[2], top[2], top[3]])} fill="var(--table-front)" />
      <polygon points={pts([base[1], base[2], top[2], top[1]])} fill="var(--table-side)" />
      <polygon points={pts(top)} fill="var(--table-top)" stroke="var(--table-edge)" strokeWidth="1.2" />
      {/* giấy tờ + laptop trên bàn */}
      <polygon points={pts([iso(3.6, 2.9, h), iso(4.2, 2.9, h), iso(4.2, 3.3, h), iso(3.6, 3.3, h)])} fill="#f2f4f8" opacity="0.92" />
      <polygon points={pts([iso(5.4, 3.4, h), iso(6.0, 3.4, h), iso(6.0, 3.8, h), iso(5.4, 3.8, h)])} fill="#dfe5ee" opacity="0.9" />
      <polygon points={pts([iso(4.8, 2.6, h), iso(5.3, 2.6, h), iso(5.3, 2.95, h), iso(4.8, 2.95, h)])} fill="#2f3542" />
    </g>
  )
}

function Plant({ x, y }: { x: number; y: number }) {
  const [px, py] = iso(x, y)
  return (
    <g transform={`translate(${px},${py})`}>
      <ellipse cx="0" cy="0" rx="10" ry="5" fill="rgba(0,0,0,0.25)" />
      <rect x="-6" y="-14" width="12" height="14" rx="2" fill="#8a5a3b" />
      <circle cx="-4" cy="-22" r="8" fill="#3f7d4e" />
      <circle cx="5" cy="-25" r="9" fill="#478a57" />
      <circle cx="0" cy="-31" r="7" fill="#529a63" />
    </g>
  )
}

type FigureStatus = 'idle' | 'thinking' | 'tools' | 'speaking'

function statusColor(s: FigureStatus): string {
  switch (s) {
    case 'thinking': return 'var(--st-thinking)'
    case 'tools': return 'var(--st-tools)'
    case 'speaking': return 'var(--ok)'
    default: return 'var(--st-idle)'
  }
}
function statusLabel(s: FigureStatus): string {
  switch (s) {
    case 'thinking': return 'đang suy nghĩ'
    case 'tools': return 'đang tra cứu'
    case 'speaking': return 'phát biểu'
    default: return ''
  }
}

function Figure({
  x, y, name, color, hat, status, variant, bubble,
}: {
  x: number; y: number; name: string; color: string; hat: string
  status: FigureStatus; variant: 'member' | 'boss' | 'manager' | 'secretary'
  bubble: string | null
}) {
  const [px, py] = iso(x, y)
  const busy = status === 'thinking' || status === 'tools'
  const hatColor = HAT_COLORS[hat] || 'transparent'
  return (
    <g className="walker" transform={`translate(${px},${py})`}>
      <ellipse cx="0" cy="2" rx="11" ry="5" fill="rgba(0,0,0,0.28)" />
      <g className={busy ? 'bob' : undefined}>
        {/* thân */}
        <polygon points="-8,0 8,0 6,-22 -6,-22" fill={color} />
        {variant === 'boss' && <polygon points="-6,-6 6,-6 5,-10 -5,-10" fill="#1f242e" opacity="0.35" />}
        {variant === 'manager' && <rect x="-1.4" y="-21" width="2.8" height="10" fill="#22252d" />}
        {variant === 'secretary' && <rect x="6" y="-14" width="7" height="9" rx="1.2" fill="#f5f7fb" stroke="#9aa4b5" strokeWidth="0.8" />}
        {/* đầu */}
        <circle cx="0" cy="-29" r="6.4" fill="#f0c8a0" />
        {/* vành mũ tư duy thiên hướng */}
        {hatColor !== 'transparent' && <path d="M -6.6 -31.5 A 6.6 6.6 0 0 1 6.6 -31.5" fill="none" stroke={hatColor} strokeWidth="3" strokeLinecap="round" />}
        {variant === 'boss' && <text x="0" y="-38" textAnchor="middle" fontSize="9">👑</text>}
        {/* chấm trạng thái */}
        {status !== 'idle' && (
          <g>
            <circle cx="9" cy="-34" r="4" fill={statusColor(status)} className={busy ? 'pulse' : undefined} />
            {status === 'tools' && <text x="9" y="-31.4" textAnchor="middle" fontSize="5.4">🔍</text>}
          </g>
        )}
      </g>
      <text x="0" y="14" textAnchor="middle" className="fig-name">{name}</text>
      {status !== 'idle' && <text x="0" y="24" textAnchor="middle" className="fig-status" fill={statusColor(status)}>{statusLabel(status)}</text>}
      {bubble && (
        <g className="bubble">
          {(() => {
            const text = bubble.length > 42 ? bubble.slice(0, 42) + '…' : bubble
            const bw = Math.max(56, 12 + text.length * 5.4)
            return (
              <>
                <rect x={-bw / 2} y={-66} width={bw} height={20} rx={7} fill="var(--bubble)" stroke="var(--bubble-edge)" strokeWidth="1" />
                <polygon points="-4,-46 6,-46 0,-39" fill="var(--bubble)" />
                <text x="0" y={-52.5} textAnchor="middle" className="bubble-text">{text}</text>
              </>
            )
          })()}
        </g>
      )}
    </g>
  )
}

export default function MeetingScene({
  discussion, members, statuses, messages,
}: {
  discussion: Discussion | null
  members: Member[]
  statuses: Record<string, string>
  messages: Message[]
}) {
  const now = Date.now() / 1000
  const roster = useMemo(() => {
    const manager = members.find((m) => m.role === 'manager')
    const secretary = members.find((m) => m.role === 'secretary')
    const crew = members.filter((m) => m.role === 'member')
    return { manager, secretary, crew }
  }, [members])

  // Bubble = tin mới nhất (<8s) của mỗi người; BOSS cũng có ghế + bubble.
  const bubbleOf = (memberId: number | null, authorKind?: string): string | null => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (m.kind === 'minutes_note') continue
      if (authorKind === 'boss' ? m.author_kind === 'boss' : m.member_id === memberId) {
        if (now - m.created_at < 8) return m.content
        return null
      }
    }
    return null
  }
  const statusOf = (id: number): FigureStatus => {
    const s = statuses[String(id)]
    if (s === 'thinking' || s === 'tools' || s === 'speaking') return s
    return 'idle'
  }
  const bossRecent = messages.some((m) => m.author_kind === 'boss' && now - m.created_at < 120)

  // viewBox từ 4 góc sàn + chiều cao tường
  const corners = [iso(0, 0, 96), iso(FLOOR_W, 0, 96), iso(FLOOR_W, FLOOR_D, -30), iso(0, FLOOR_D, -30)]
  const xs = corners.map((c) => c[0])
  const ys = corners.map((c) => c[1])
  const pad = 46
  const vb = `${Math.min(...xs) - pad} ${Math.min(...ys) - pad} ${Math.max(...xs) - Math.min(...xs) + pad * 2} ${Math.max(...ys) - Math.min(...ys) + pad * 2}`

  // Nhân vật sort back-to-front theo y màn hình
  type Fig = { y: number; el: React.ReactElement }
  const figs: Fig[] = []
  if (roster.manager) {
    const st = statusOf(roster.manager.id)
    figs.push({
      y: iso(...MANAGER_POS)[1],
      el: (
        <Figure key="mgr" x={MANAGER_POS[0]} y={MANAGER_POS[1]} name={roster.manager.name} color="#5b6478"
          hat="blue" status={st} variant="manager" bubble={bubbleOf(roster.manager.id)} />
      ),
    })
  }
  if (roster.secretary) {
    const st = statusOf(roster.secretary.id)
    figs.push({
      y: iso(...SECRETARY_SEAT)[1],
      el: (
        <Figure key="sec" x={SECRETARY_SEAT[0]} y={SECRETARY_SEAT[1]} name={roster.secretary.name} color="#8f7fb8"
          hat="white" status={st} variant="secretary" bubble={bubbleOf(roster.secretary.id)} />
      ),
    })
  }
  roster.crew.forEach((m, i) => {
    const seat = SEATS[i % SEATS.length]
    figs.push({
      y: iso(...seat)[1],
      el: (
        <Figure key={m.id} x={seat[0]} y={seat[1]} name={m.name.split('•')[0].trim()} color={colorFor(m.key)}
          hat={(m.hat || '').split(',')[0]?.trim() || ''} status={statusOf(m.id)} variant="member" bubble={bubbleOf(m.id)} />
      ),
    })
  })
  figs.push({
    y: iso(...BOSS_SEAT)[1],
    el: (
      <Figure key="boss" x={BOSS_SEAT[0]} y={BOSS_SEAT[1]} name="BOSS (bạn)" color={bossRecent ? '#c9a227' : '#8a8f9c'}
        hat="" status={bossRecent ? 'speaking' : 'idle'} variant="boss" bubble={bubbleOf(null, 'boss')} />
    ),
  })
  figs.sort((a, b) => a.y - b.y)

  return (
    <div className="scene-wrap">
      <svg viewBox={vb} className="scene" preserveAspectRatio="xMidYMid meet">
        <Walls />
        <Whiteboard
          score={discussion?.manager_score ?? 0}
          missing={discussion?.manager_missing?.length ?? 0}
          round={discussion?.round ?? 0}
          maxRounds={discussion?.max_rounds ?? 0}
        />
        <Floor />
        <Plant x={0.6} y={6.4} />
        <Plant x={9.4} y={0.7} />
        <Table />
        {figs.map((f) => f.el)}
      </svg>
      {!discussion && <div className="scene-empty">Tạo phiên thảo luận để đội vào phòng họp</div>}
    </div>
  )
}
