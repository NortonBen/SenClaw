import { useEffect, useState, type CSSProperties } from 'react'

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

const ellipsis: CSSProperties = {
  whiteSpace: 'nowrap',
  overflow: 'hidden',
  textOverflow: 'ellipsis',
}

export function EmailInboxWidget() {
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

  const unread = emails ? emails.filter((e) => isUnread(e.flags)).slice(0, 5) : []
  const loading = emails === null && !error

  return (
    <div
      style={{
        height: '100vh',
        padding: 14,
        display: 'flex',
        flexDirection: 'column',
        background: 'var(--bg-card)',
        borderRadius: 20,
        color: 'var(--text)',
        overflow: 'hidden',
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 10,
        }}
      >
        <span style={{ fontSize: 13, fontWeight: 700, color: 'var(--text)' }}>Chưa đọc</span>
        {emails && (
          <span
            style={{
              fontSize: 12,
              fontWeight: 600,
              color: 'var(--accent)',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            {emails.filter((e) => isUnread(e.flags)).length}
          </span>
        )}
      </div>

      {loading ? (
        <div style={{ color: 'var(--text-secondary)', fontSize: 13, margin: 'auto' }}>Đang tải…</div>
      ) : error ? (
        <div style={{ color: 'var(--text-secondary)', fontSize: 13, margin: 'auto' }}>Không tải được</div>
      ) : unread.length === 0 ? (
        <div
          style={{
            margin: 'auto',
            textAlign: 'center',
            color: 'var(--text-secondary)',
            fontSize: 13,
          }}
        >
          Không có thư chưa đọc
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2, minHeight: 0 }}>
          {unread.map((e) => (
            <div
              key={e.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 9,
                padding: '7px 4px',
                borderBottom: '1px solid var(--border)',
              }}
            >
              <span
                style={{
                  flex: '0 0 auto',
                  width: 8,
                  height: 8,
                  borderRadius: '50%',
                  background: 'var(--accent)',
                }}
              />
              <div style={{ minWidth: 0, flex: 1 }}>
                <div style={{ ...ellipsis, fontSize: 13, fontWeight: 600, color: 'var(--text)' }}>
                  {e.subject || '(không tiêu đề)'}
                </div>
                <div style={{ ...ellipsis, fontSize: 11, color: 'var(--text-secondary)', marginTop: 1 }}>
                  {e.from || 'Không rõ người gửi'}
                </div>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
