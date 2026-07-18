/// Labelled form cell used inside `.edit-grid`. `full` spans both columns.
export function Field({
  label,
  children,
  full,
}: {
  label: string
  children: React.ReactNode
  full?: boolean
}) {
  return (
    <label className={'field' + (full ? ' full' : '')}>
      <div className="lbl">{label}</div>
      {children}
    </label>
  )
}

/// Read `File` → data URL, for the base64-inlined avatars.
export function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve(String(r.result ?? ''))
    r.onerror = () => reject(r.error)
    r.readAsDataURL(file)
  })
}

/// Tiny markdown → HTML for the AI report card: **bold**, `- ` bullets, blank
/// lines as paragraph breaks. Escapes < > & so a wayward model can't inject.
export function renderMd(md: string): string {
  const esc = (s: string) => s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]!)
  const inline = (s: string) => esc(s).replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
  const lines = md.split(/\r?\n/)
  let out = ''
  let inList = false
  const closeList = () => {
    if (inList) {
      out += '</ul>'
      inList = false
    }
  }
  for (const raw of lines) {
    const line = raw.trim()
    if (!line) {
      closeList()
      continue
    }
    if (line.startsWith('- ')) {
      if (!inList) {
        out += '<ul>'
        inList = true
      }
      out += `<li>${inline(line.slice(2))}</li>`
    } else {
      closeList()
      out += `<p>${inline(line)}</p>`
    }
  }
  closeList()
  return out
}

/// Relative time for the inbox list ("5m", "3h", "2d", else a date).
export function relTime(secs: number | null | undefined): string {
  if (!secs) return '—'
  const d = Math.floor(Date.now() / 1000) - secs
  if (d < 60) return 'now'
  if (d < 3600) return `${Math.floor(d / 60)}m`
  if (d < 86400) return `${Math.floor(d / 3600)}h`
  if (d < 604800) return `${Math.floor(d / 86400)}d`
  return new Date(secs * 1000).toLocaleDateString(undefined, { month: '2-digit', day: '2-digit' })
}
