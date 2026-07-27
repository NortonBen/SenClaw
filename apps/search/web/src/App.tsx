import { useCallback, useEffect, useState } from 'react'
import { api, type SearchOutcome, type SourceInfo, type Evidence } from './api'
import Sources from './Sources'
import Corpus from './Corpus'
import Claims from './Claims'

function healthState(s: SourceInfo) {
  return s.health.state
}

function healthReason(s: SourceInfo): string | null {
  return 'reason' in s.health ? s.health.reason : null
}

/** Provenance line: which sources found this, and whether they agree independently. */
function Provenance({ e }: { e: Evidence }) {
  return (
    <div className="prov">
      {e.domain && <span className="tag">{e.domain}</span>}
      {e.hits.map((h) => (
        <span className="tag" key={h.source_id}>
          {h.source_id} · #{h.rank + 1}
        </span>
      ))}
      {e.independent_kinds > 1 && (
        <span className="tag corroborated">
          {e.independent_kinds} loại nguồn độc lập
        </span>
      )}
      {e.full_text && <span className="tag">đã tải toàn văn</span>}
      <span>rrf {e.fused_score.toFixed(4)}</span>
    </div>
  )
}

export default function App() {
  const [sources, setSources] = useState<SourceInfo[]>([])
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [query, setQuery] = useState('')
  const [depth, setDepth] = useState(1)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [out, setOut] = useState<SearchOutcome | null>(null)
  const [showSources, setShowSources] = useState(false)
  const [mode, setMode] = useState<'search' | 'ask'>('search')

  const loadSources = useCallback(
    (resetSelection: boolean) =>
      api
        .sources()
        .then((next) => {
          setSources((previous) => {
            if (resetSelection) {
              setSelected(new Set(next.sources.filter((s) => s.enabled).map((s) => s.id)))
            } else {
              const before = new Set(previous.map((s) => s.id))
              const now = new Set(next.sources.map((s) => s.id))
              setSelected((picked) => {
                // Drop sources that no longer exist, keep the user's picks,
                // and opt in only sources that appeared since the last load —
                // re-adding a deliberately deselected source on every refresh
                // would quietly widen the search behind the user's back.
                const merged = new Set([...picked].filter((id) => now.has(id)))
                for (const s of next.sources) {
                  if (!before.has(s.id) && s.enabled) merged.add(s.id)
                }
                return merged
              })
            }
            return next.sources
          })
        })
        .catch((e: Error) => setError(e.message)),
    [],
  )

  useEffect(() => {
    loadSources(true)
  }, [loadSources])

  function toggle(id: string) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  async function run(nextMode: 'search' | 'ask' = 'search') {
    if (!query.trim() || busy) return
    setBusy(true)
    setError(null)
    setMode(nextMode)
    try {
      const body = {
        query: query.trim(),
        sources: selected.size ? [...selected] : undefined,
        depth,
      }
      const r = nextMode === 'ask' ? await api.ask(body) : await api.search(body)
      setOut(r)
      // Health can change between page load and the search (the extension may
      // have dropped), so refresh the dots afterwards.
      loadSources(false)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const failed = out?.sources.filter((s) => s.status !== 'ok') ?? []

  return (
    <div className="app">
      <header>
        <h1>🔎 Search</h1>
        <p>
          Gom kết quả từ nhiều nguồn, khử trùng lặp, xếp hạng theo Reciprocal Rank
          Fusion và ưu tiên những gì nhiều <em>loại</em> nguồn độc lập cùng xác nhận.
        </p>
      </header>

      <div className="searchbar">
        <input
          value={query}
          placeholder="Bạn muốn tìm gì?"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && run('search')}
        />
        <button onClick={() => run('search')} disabled={busy || !query.trim()}>
          {busy && mode === 'search' ? 'Đang tìm…' : 'Tìm'}
        </button>
        <button
          className="secondary"
          onClick={() => run('ask')}
          disabled={busy || !query.trim()}
          title="Tìm rồi rút ra các khẳng định, mỗi khẳng định gắn bằng chứng và xếp hạng theo số nguồn độc lập. Chậm hơn vì có gọi LLM."
        >
          {busy && mode === 'ask' ? 'Đang kiểm chứng…' : 'Hỏi & kiểm chứng'}
        </button>
      </div>

      <div className="sources">
        {sources.map((s) => (
          <span
            key={s.id}
            className={`chip${selected.has(s.id) ? ' on' : ''}`}
            title={healthReason(s) ?? 'sẵn sàng'}
            onClick={() => toggle(s.id)}
          >
            <span className={`dot ${healthState(s)}`} />
            {s.label}
          </span>
        ))}
        <span
          className={`chip${depth === 2 ? ' on' : ''}`}
          title="Tải thêm toàn văn cho các kết quả web đầu bảng — chậm hơn nhiều"
          onClick={() => setDepth(depth === 2 ? 1 : 2)}
        >
          ⤓ toàn văn
        </span>
        <span className="chip" onClick={() => setShowSources((v) => !v)}>
          ⚙ nguồn
        </span>
      </div>

      {showSources && (
        <>
          <Sources sources={sources} onChanged={() => loadSources(false)} />
          <Corpus onChanged={() => loadSources(false)} />
        </>
      )}

      {error && <div className="error">Lỗi: {error}</div>}

      {out && (
        <>
          <div className="meta">
            {out.evidence.length} bằng chứng · gom từ {out.total_before_dedupe} kết quả thô
            {out.deepened > 0 && ` · ${out.deepened} trang đã tải toàn văn`} · {out.ms} ms
            {out.run_id && ` · ${out.run_id}`}
          </div>

          {out.claims_error && (
            <div className="error">
              {out.claims_error}
            </div>
          )}

          {out.claims && out.claims.length > 0 && (
            <Claims
              claims={out.claims}
              contradictions={out.contradictions ?? []}
              evidence={out.evidence}
              note={out.confidence_note}
            />
          )}

          {out.claims_note && <p className="muted">{out.claims_note}</p>}

          {out.unknown_sources.length > 0 && (
            <div className="error">
              Không có nguồn: {out.unknown_sources.join(', ')}
            </div>
          )}

          {/* A thin result set must never be mistaken for "there is nothing
              out there" — show exactly which sources failed and why. */}
          {failed.length > 0 && (
            <div className="panel">
              <h2>Nguồn không trả kết quả</h2>
              {failed.map((s, i) => (
                <div className="srcrow" key={`${s.source_id}-${i}`}>
                  <span className="id">{s.source_id}</span>
                  <span className={`status ${s.status}`}>{s.status}</span>
                  <span className="why">{s.error ?? '—'}</span>
                </div>
              ))}
            </div>
          )}

          <div className="panel">
            <h2>Nguồn đã chạy</h2>
            {out.sources
              .filter((s) => s.status === 'ok')
              .map((s, i) => (
                <div className="srcrow" key={`${s.source_id}-${i}`}>
                  <span className="id">{s.source_id}</span>
                  <span className="status ok">{s.item_count} kết quả</span>
                  <span className="why">
                    {s.ms} ms
                    {s.dropped_count > 0 && ` · đã bỏ bớt ${s.dropped_count} do giới hạn`}
                  </span>
                </div>
              ))}
          </div>

          <ol className="results">
            {out.evidence.map((e) => (
              <li className="result" key={e.id}>
                {e.url ? (
                  <a href={e.url} target="_blank" rel="noreferrer">
                    {e.title || e.url}
                  </a>
                ) : (
                  <div className="title-plain">{e.title || '(không có tiêu đề)'}</div>
                )}
                <div className="snippet">{e.snippet}</div>
                <Provenance e={e} />
              </li>
            ))}
          </ol>

          {out.evidence.length === 0 && (
            <p className="muted">
              Không có bằng chứng nào. Kiểm tra bảng “Nguồn không trả kết quả” phía
              trên trước khi kết luận là không có thông tin.
            </p>
          )}
        </>
      )}
    </div>
  )
}
