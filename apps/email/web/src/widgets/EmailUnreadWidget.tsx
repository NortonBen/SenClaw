import { useEffect, useState } from 'react'

interface Email {
  id: string
  account_id: string
  subject: string | null
  from: string | null
  date: number | null
  flags: string
}

const REFRESH_MS = 30000

/** Defensive: flags is a JSON-array-string like "[\"\\Seen\"]". Unread when it
 * lacks the "Seen" substring (or is missing entirely). */
function isUnread(flags: string | null | undefined): boolean {
  if (!flags) return true
  return !flags.includes('Seen')
}

export function EmailUnreadWidget() {
  const [emails, setEmails] = useState<Email[] | null>(null)
  const [error, setError] = useState(false)

  useEffect(() => {
    let alive = true
    const load = async () => {
      try {
        const res = await fetch('/api/inbox?limit=50')
        if (!res.ok) throw new Error(String(res.status))
        const data = await res.json()
        if (!alive) return
        setEmails(Array.isArray(data) ? data : [])
        setError(false)
      } catch {
        if (!alive) return
        setError(true)
      }
    }
    load()
    const t = setInterval(load, REFRESH_MS)
    return () => { alive = false; clearInterval(t) }
  }, [])

  const total = emails?.length ?? 0
  const unread = emails ? emails.filter((e) => isUnread(e.flags)).length : 0
  const loading = emails === null && !error

  return (
    <div
      style={{
        height: '100vh',
        padding: 14,
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'center',
        alignItems: 'center',
        textAlign: 'center',
        background: 'var(--bg-card)',
        borderRadius: 20,
        color: 'var(--text)',
      }}
    >
      {loading ? (
        <div style={{ color: 'var(--text-secondary)', fontSize: 13 }}>Đang tải…</div>
      ) : error ? (
        <div style={{ color: 'var(--text-secondary)', fontSize: 13 }}>Không tải được</div>
      ) : unread === 0 ? (
        <>
          <div style={{ fontSize: 30, lineHeight: 1, marginBottom: 8 }}>📭</div>
          <div style={{ fontSize: 15, fontWeight: 600, color: 'var(--success)' }}>Đã đọc hết ✓</div>
          <div style={{ marginTop: 6, fontSize: 12, color: 'var(--text-secondary)' }}>
            {total} thư trong hộp thư
          </div>
        </>
      ) : (
        <>
          <div
            style={{
              fontSize: 52,
              fontWeight: 700,
              lineHeight: 1,
              color: 'var(--accent)',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            {unread}
          </div>
          <div style={{ marginTop: 6, fontSize: 14, fontWeight: 600, color: 'var(--text)' }}>Chưa đọc</div>
          <div style={{ marginTop: 4, fontSize: 12, color: 'var(--text-secondary)' }}>
            {total} thư trong hộp thư
          </div>
        </>
      )}
    </div>
  )
}
