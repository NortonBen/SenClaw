import { useEffect, useState } from 'react'
import { api } from '../api'

export function DraftsWidget() {
  const [pending, setPending] = useState<number>(0)

  useEffect(() => {
    const load = () => api.draftsCount().then(setPending).catch(() => {})
    load()
    const t = setInterval(load, 30000)
    return () => clearInterval(t)
  }, [])

  return (
    <div className="mb-widget" style={{ textAlign: 'center' }}>
      <div className="big" style={{ color: pending > 0 ? '#ff7a5c' : '#e8e6ea' }}>
        {pending}
      </div>
      <div className="muted">bản nháp chờ duyệt</div>
    </div>
  )
}
