import { useEffect, useState } from 'react'
import type { CachedPost } from '../api'
import { api } from '../api'

export function FeedWidget() {
  const [posts, setPosts] = useState<CachedPost[]>([])
  const [source, setSource] = useState('')

  useEffect(() => {
    const load = () =>
      api
        .feed({ limit: 6 })
        .then((r) => {
          setPosts(r.posts)
          setSource(r.source)
        })
        .catch(() => {})
    load()
    const t = setInterval(load, 120000)
    return () => clearInterval(t)
  }, [])

  return (
    <div className="mb-widget">
      {posts.length === 0 ? (
        <div className="muted">Chưa có bài nào.</div>
      ) : (
        posts.slice(0, 6).map((p) => (
          <div className="row" key={p.post_id}>
            <span className="sub">{p.submolt}</span>
            <span className="title">{p.title}</span>
            <span className="score">▲ {p.score}</span>
          </div>
        ))
      )}
      {source === 'demo' && <div className="muted" style={{ marginTop: 6 }}>demo · kết nối agent để xem feed thật</div>}
    </div>
  )
}
