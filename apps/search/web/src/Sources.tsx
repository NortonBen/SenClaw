import { useEffect, useState } from 'react'
import {
  api,
  type McpToolInfo,
  type SourceInfo,
  type SourceTemplate,
  type SyncReport,
} from './api'

function reasonOf(s: SourceInfo): string | null {
  return 'reason' in s.health ? s.health.reason : null
}

const BUILT_IN = ['web', 'knowledge', 'wiki', 'memory', 'corpus']

const QUERY_PARAM = /^(query|q|text|keyword|search|term)$/
const LIMIT_PARAM = /^(limit|count|num|top_k|max_results|n)$/

/** How likely is this tool to be a full-text search over a corpus? */
function searchiness(t: McpToolInfo): number {
  const name = t.name.toLowerCase()
  const params = Object.keys(t.inputSchema?.properties ?? {})
  let score = 0
  if (/(^|_)search$/.test(name)) score += 3
  else if (name.includes('search')) score += 1
  if (params.some((p) => QUERY_PARAM.test(p))) score += 2
  if (params.some((p) => LIMIT_PARAM.test(p))) score += 0.5
  // `*_by_email`, `*_by_id` … are keyed lookups; they take an exact value, not
  // a query, so a search that fed them free text would always return nothing.
  if (/_by_/.test(name)) score -= 3
  if (/(create|update|delete|remove|send|post|write)/.test(name)) score -= 5
  return score
}

/**
 * Source management: toggle, weight, and — the P1 point — register any MCP
 * tool as a search source without writing code.
 */
export default function Sources({
  sources,
  onChanged,
}: {
  sources: SourceInfo[]
  onChanged: () => void
}) {
  const [templates, setTemplates] = useState<SourceTemplate[]>([])
  const [sync, setSync] = useState<SyncReport[] | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  // add-source form
  const [target, setTarget] = useState('')
  const [tools, setTools] = useState<McpToolInfo[] | null>(null)
  const [tool, setTool] = useState('')
  const [id, setId] = useState('')
  const [queryArg, setQueryArg] = useState('query')
  const [limitArg, setLimitArg] = useState('')
  const [extraArgs, setExtraArgs] = useState('')

  useEffect(() => {
    api.templates().then((r) => setTemplates(r.templates)).catch(() => {})
  }, [])

  async function guard(fn: () => Promise<unknown>) {
    setBusy(true)
    setError(null)
    try {
      await fn()
      onChanged()
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  /** `app_id` if it looks like a bare id, otherwise a full rpc_url. */
  function targetPayload() {
    const t = target.trim()
    return t.startsWith('http') ? { rpc_url: t } : { app_id: t }
  }

  async function loadTools() {
    setTools(null)
    // Pointing at a different MCP means a different source; carrying over the
    // previous id would silently name it after the wrong app.
    setId('')
    setTool('')
    setExtraArgs('')
    await guard(async () => {
      const r = await api.mcpTools(targetPayload())
      setTools(r.tools)
      // Pre-select the most search-like tool so the common case is one click.
      // Ranked, not first-match: on a CRM with 83 tools, first-match picked
      // `crm_find_by_email` — a lookup by key, not a search — and pre-filled a
      // `query` parameter it does not have. A wrong guess is worse than none.
      const best = r.tools
        .map((t) => ({ t, score: searchiness(t) }))
        .sort((a, b) => b.score - a.score)[0]
      if (best && best.score >= 2) pickTool(best.t)
    })
  }

  function pickTool(t: McpToolInfo) {
    setTool(t.name)
    const props = Object.keys(t.inputSchema?.properties ?? {})
    // Match the real parameter names instead of assuming "query"/"limit" —
    // sending an argument the tool does not declare is an error, not a no-op.
    // (crm_search takes `q`; guessing `query` returns "q is required".)
    setQueryArg(props.find((p) => QUERY_PARAM.test(p)) ?? 'query')
    setLimitArg(props.find((p) => LIMIT_PARAM.test(p)) ?? '')
    if (!id) setId(t.name.replace(/_?(search|query|find).*$/, '') || t.name)
    // Required args that the query alone cannot supply must be filled in by
    // hand — this is exactly the social_search platform/handle case.
    const missing = (t.inputSchema?.required ?? []).filter(
      (r) => !QUERY_PARAM.test(r) && !LIMIT_PARAM.test(r),
    )
    setExtraArgs(
      missing.length
        ? JSON.stringify(Object.fromEntries(missing.map((m) => [m, ''])), null, 1)
        : '',
    )
  }

  async function add() {
    let extra: Record<string, unknown> | undefined
    if (extraArgs.trim()) {
      try {
        extra = JSON.parse(extraArgs)
      } catch {
        setError('`extra_args` không phải JSON hợp lệ')
        return
      }
    }
    await guard(async () => {
      await api.addSource({
        id: id.trim(),
        label: id.trim(),
        ...targetPayload(),
        tool,
        query_arg: queryArg || 'query',
        limit_arg: limitArg || undefined,
        extra_args: extra,
      })
      setTools(null)
      setTool('')
      setId('')
      setExtraArgs('')
    })
  }

  return (
    <div className="panel">
      <h2>Nguồn</h2>

      {sources.map((s) => (
        <div className="srcrow cfg" key={s.id}>
          <input
            type="checkbox"
            checked={s.enabled}
            disabled={busy}
            onChange={(e) =>
              guard(() => api.setSource(s.id, { enabled: e.target.checked }))
            }
          />
          <span className="id">{s.label}</span>
          <span className={`dot ${s.health.state}`} />
          <input
            className="w"
            type="number"
            step="0.1"
            min="0"
            max="10"
            value={s.weight.toFixed(1)}
            disabled={busy}
            title="Trọng số tin cậy khi hợp nhất hạng"
            onChange={(e) =>
              guard(() => api.setSource(s.id, { weight: Number(e.target.value) }))
            }
          />
          <span className="why">{reasonOf(s) ?? s.kind}</span>
          {!BUILT_IN.includes(s.id) && (
            <button
              className="link"
              disabled={busy}
              onClick={() => guard(() => api.removeSource(s.id))}
            >
              gỡ
            </button>
          )}
        </div>
      ))}

      <div className="row-actions">
        <button disabled={busy} onClick={() => guard(async () => setSync((await api.sync()).sources))}>
          Quét lại app đã cài
        </button>
      </div>

      {sync && (
        <div className="synced">
          {sync.map((r, i) => (
            <div key={`${r.id}-${i}`}>
              <span className={r.registered ? 'status ok' : 'status skipped'}>
                {r.registered ? '+' : '−'}
              </span>{' '}
              <b>{r.id}</b> <span className="why">{r.reason}</span>
            </div>
          ))}
        </div>
      )}

      <h2 style={{ marginTop: 18 }}>Thêm nguồn từ MCP bất kỳ</h2>
      <div className="addform">
        <input
          placeholder="id app đã cài (vd: youtube) hoặc URL JSON-RPC"
          value={target}
          onChange={(e) => setTarget(e.target.value)}
        />
        <button disabled={busy || !target.trim()} onClick={loadTools}>
          Xem công cụ
        </button>
      </div>

      {tools && tools.length === 0 && <p className="why">MCP này không có công cụ nào.</p>}

      {tools && tools.length > 0 && (
        <>
          <div className="toolpick">
            {tools.map((t) => (
              <span
                key={t.name}
                className={`chip${tool === t.name ? ' on' : ''}`}
                title={t.description}
                onClick={() => pickTool(t)}
              >
                {t.name}
              </span>
            ))}
          </div>
          {tool && (
            <div className="addform col">
              <label>
                Tên nguồn
                <input value={id} onChange={(e) => setId(e.target.value)} />
              </label>
              <label>
                Tham số truy vấn
                <input value={queryArg} onChange={(e) => setQueryArg(e.target.value)} />
              </label>
              <label>
                Tham số giới hạn (để trống nếu công cụ không có)
                <input value={limitArg} onChange={(e) => setLimitArg(e.target.value)} />
              </label>
              <label>
                Tham số cố định (JSON)
                <textarea
                  rows={3}
                  placeholder='{"platform":"threads","handle":"@ten_cua_ban"}'
                  value={extraArgs}
                  onChange={(e) => setExtraArgs(e.target.value)}
                />
              </label>
              <button disabled={busy || !id.trim()} onClick={add}>
                Thêm nguồn
              </button>
            </div>
          )}
        </>
      )}

      {templates.length > 0 && (
        <div className="templates">
          <h2 style={{ marginTop: 16 }}>Cần bạn cấu hình thêm</h2>
          {templates.map((t) => (
            <div key={t.id} className="tmpl">
              <b>{t.label}</b> <span className="why">— {t.why}</span>
              <div className="why">
                cần: {t.required_args.map((a) => a.name).join(', ')} · app{' '}
                <code>{t.app_id}</code>, công cụ <code>{t.tool}</code>
              </div>
            </div>
          ))}
        </div>
      )}

      {error && <div className="error" style={{ marginTop: 10 }}>{error}</div>}
    </div>
  )
}
