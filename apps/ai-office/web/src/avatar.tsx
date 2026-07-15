/* Per-staff visual identity, derived deterministically from the agent key so
   the same person looks the same everywhere (office scene, roster, feed).
   Traits: an accent color, a hairstyle and optional glasses. */

export interface AvatarTraits {
  color: string
  hair: 0 | 1 | 2 | 3
  glasses: boolean
}

/** Muted accents that read on both the paper and blackboard themes. */
const PALETTE = ['#e07b39', '#3b8c5a', '#4a6fd1', '#b0568c', '#8a7f2f', '#c25450', '#3f8f9c']

function hash(s: string): number {
  let h = 5381
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0
  return h
}

export function traitsFor(key: string): AvatarTraits {
  if (key === 'sep') return { color: '#8a6d1a', hair: 0, glasses: false }
  const h = hash(key)
  return {
    color: PALETTE[h % PALETTE.length],
    hair: ((h >> 4) % 4) as AvatarTraits['hair'],
    glasses: ((h >> 7) & 3) === 0 ? false : ((h >> 9) & 1) === 1,
  }
}

/** Hair shapes for the small HTML avatar (24×24 viewBox, head at 12,13 r5.2). */
function Hair({ hair }: { hair: AvatarTraits['hair'] }) {
  switch (hair) {
    case 0: // side part
      return <path d="M 6.8 13 A 5.2 5.2 0 0 1 17.2 13 L 15.5 10.6 L 8 9.8 Z" fill="var(--ink)" opacity="0.75" />
    case 1: // spiky
      return (
        <g fill="var(--ink)" opacity="0.75">
          <path d="M 7 11.5 A 5.2 5.2 0 0 1 17 11.5 L 12 10 Z" />
          <polygon points="8.5,9.5 9.6,6.6 10.8,9" />
          <polygon points="11.2,8.8 12.2,6 13.3,8.8" />
          <polygon points="13.8,9 15,6.7 15.7,9.5" />
        </g>
      )
    case 2: // bun
      return (
        <g fill="var(--ink)" opacity="0.75">
          <path d="M 6.8 12.4 A 5.2 5.2 0 0 1 17.2 12.4 Z" />
          <circle cx="12" cy="6.6" r="1.9" />
        </g>
      )
    default: // cap
      return (
        <g fill="var(--ink)" opacity="0.75">
          <path d="M 6.8 12.2 A 5.2 5.2 0 0 1 17.2 12.2 Z" />
          <rect x="12" y="11.2" width="7" height="1.6" rx="0.8" />
        </g>
      )
  }
}

/** Small round avatar chip for HTML contexts (sidebar, tables, feed). */
export function Avatar({ agentKey, size = 18 }: { agentKey: string; size?: number }) {
  const t = traitsFor(agentKey)
  const boss = agentKey === 'sep'
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      style={{ verticalAlign: 'middle', flexShrink: 0 }}
      aria-hidden
    >
      <circle cx="12" cy="12" r="11" fill={t.color} fillOpacity="0.16" stroke={t.color} strokeWidth="1.2" />
      <circle cx="12" cy="13" r="5.2" fill="var(--panel)" stroke="var(--ink)" strokeWidth="1" />
      <Hair hair={t.hair} />
      {t.glasses && (
        <g stroke="var(--ink)" strokeWidth="0.7" fill="none">
          <circle cx="9.9" cy="13.4" r="1.7" />
          <circle cx="14.1" cy="13.4" r="1.7" />
          <line x1="11.6" y1="13.4" x2="12.4" y2="13.4" />
        </g>
      )}
      {boss && (
        <polygon points="8.5,7.4 10.2,9.6 12,7.2 13.8,9.6 15.5,7.4 15,10.4 9,10.4" fill={t.color} opacity="0.9" />
      )}
    </svg>
  )
}
