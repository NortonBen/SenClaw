import { useEffect, useState } from 'react'
import { api } from '../api'

export function DraftsWidget() {
  const [pending, setPending] = useState<number>(0)
  const [needsInput, setNeedsInput] = useState<number>(0)

  useEffect(() => {
    const load = () =>
      api
        .draftsCount()
        .then((c) => {
          setPending(c.pending)
          setNeedsInput(c.needs_input)
        })
        .catch(() => {})
    load()
    const t = setInterval(load, 30000)
    return () => clearInterval(t)
  }, [])

  return (
    <div className="mb-widget" style={{ textAlign: 'center' }}>
      <div className="big" style={{ color: pending + needsInput > 0 ? '#ff7a5c' : '#e8e6ea' }}>
        {pending + needsInput}
      </div>
      <div className="muted">
        bản nháp chờ duyệt{needsInput > 0 ? ` · ${needsInput} cần trả lời` : ''}
      </div>
    </div>
  )
}
