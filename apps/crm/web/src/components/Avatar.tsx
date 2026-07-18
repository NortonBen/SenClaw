import { useState } from 'react'
import { hueFromName, initials } from '../api'

/// Image avatar with a deterministic coloured-initials fallback. Falls back on
/// load error too, so a dead avatar_url never leaves a broken-image icon.
export function Avatar({ name, url, size = 40 }: { name: string; url?: string; size?: number }) {
  const [broken, setBroken] = useState(false)
  const hue = hueFromName(name || '?')
  if (url && !broken) {
    return (
      <img
        className="avatar"
        src={url}
        alt={name}
        style={{ width: size, height: size }}
        onError={() => setBroken(true)}
      />
    )
  }
  return (
    <div
      className="avatar fallback"
      style={{ width: size, height: size, background: `hsl(${hue} 65% 55%)`, fontSize: size * 0.4 }}
      aria-label={name}
    >
      {initials(name)}
    </div>
  )
}

/// Overlapping avatar stack for a table cell — the reference CRM's Contacts
/// column. Shows `+N` once the list outruns `max`.
export function AvatarGroup({
  people,
  max = 4,
  size = 28,
}: {
  people: Array<{ name: string; avatar_url?: string }>
  max?: number
  size?: number
}) {
  const shown = people.slice(0, max)
  const rest = people.length - shown.length
  if (people.length === 0) return <span className="muted">—</span>
  return (
    <div className="avatar-group">
      {shown.map((p, i) => (
        <span key={i} className="avatar-group-item" title={p.name} style={{ zIndex: max - i }}>
          <Avatar name={p.name} url={p.avatar_url} size={size} />
        </span>
      ))}
      {rest > 0 && (
        <span className="avatar-group-more" style={{ width: size, height: size, fontSize: size * 0.36 }}>
          +{rest}
        </span>
      )}
    </div>
  )
}
