// [0] Studio — the one screen that does the whole thing.
//
// Drop any file, press Auto-build, ask a question. The seven numbered tabs are
// still there for anyone who wants to steer each stage by hand; this tab exists
// so that the common case ("here is my data, make it useful") is three clicks
// rather than a tour of RDF theory.

import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from './api'
import type { AskResult, AutoJob, IngestedSource, Source } from './api'

type Props = {
  pid: number
  notify: (m: string, err?: boolean) => void
  onChanged: () => void
  onGoTo: (tab: 'tbox' | 'mapping' | 'explore' | 'competency' | 'validate' | 'governance') => void
}

/** ArrayBuffer → base64, chunked so a large file cannot blow the call stack. */
function toBase64(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf)
  let bin = ''
  const CHUNK = 0x8000
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK))
  }
  return btoa(bin)
}

function readFile(f: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onerror = () => reject(new Error('could not read ' + f.name))
    r.onload = () => resolve(toBase64(r.result as ArrayBuffer))
    r.readAsArrayBuffer(f)
  })
}

const STATUS_ICON: Record<string, string> = {
  pending: '○',
  running: '◐',
  ok: '●',
  warn: '▲',
  error: '✕',
  skipped: '–',
}

export function StudioPanel({ pid, notify, onChanged, onGoTo }: Props) {
  const [sources, setSources] = useState<Source[]>([])
  const [drag, setDrag] = useState(false)
  const [busy, setBusy] = useState('')
  const [job, setJob] = useState<AutoJob | null>(null)
  const [question, setQuestion] = useState('')
  const [answer, setAnswer] = useState<AskResult | null>(null)
  const [asking, setAsking] = useState(false)
  const [showSparql, setShowSparql] = useState(false)
  const [doReason, setDoReason] = useState(true)
  const [doExtract, setDoExtract] = useState(true)
  const fileInput = useRef<HTMLInputElement>(null)

  const reload = useCallback(() => {
    api.listSources(pid).then(setSources).catch((e) => notify((e as Error).message, true))
  }, [pid, notify])

  useEffect(() => {
    reload()
    setJob(null)
    setAnswer(null)
  }, [reload])

  // Completion has to be handled wherever the terminal state first arrives —
  // against a fast backend the very first status read can already be `done`,
  // and the polling loop would then never run at all.
  const announced = useRef<string | null>(null)
  const applyJob = useCallback(
    (next: AutoJob) => {
      setJob(next)
      if (next.done && announced.current !== next.id) {
        announced.current = next.id
        onChanged()
        reload()
        notify(next.error ? 'Auto-build stopped: ' + next.error : 'Auto-build finished', !!next.error)
      }
    },
    [notify, onChanged, reload],
  )

  // Poll a running job. The interval is cleared on unmount and whenever the job
  // finishes, so switching projects mid-build cannot leave a timer behind.
  useEffect(() => {
    if (!job || job.done) return
    const t = setInterval(async () => {
      try {
        applyJob(await api.autobuildStatus(pid, job.id))
      } catch {
        /* transient — keep polling */
      }
    }, 1200)
    return () => clearInterval(t)
  }, [job, pid, applyJob])

  async function ingestFiles(files: FileList | File[]) {
    const list = Array.from(files)
    if (!list.length) return
    const created: IngestedSource[] = []
    for (const f of list) {
      setBusy(`Reading ${f.name}…`)
      try {
        const contentBase64 = await readFile(f)
        const r = await api.ingest(pid, f.name, { contentBase64 })
        created.push(...r.sources)
      } catch (e) {
        notify(`${f.name}: ${(e as Error).message}`, true)
      }
    }
    setBusy('')
    if (created.length) {
      notify(created.map((s) => `${s.name} (${s.origin})`).join(', ') + ' ingested')
      reload()
      onChanged()
    }
  }

  async function build() {
    setBusy('Starting…')
    try {
      const { jobId } = await api.autobuild(pid, { reason: doReason, extract: doExtract })
      applyJob(await api.autobuildStatus(pid, jobId))
    } catch (e) {
      notify((e as Error).message, true)
    } finally {
      setBusy('')
    }
  }

  async function ask() {
    const q = question.trim()
    if (!q) return
    setAsking(true)
    setAnswer(null)
    try {
      setAnswer(await api.ask(pid, q))
    } catch (e) {
      notify((e as Error).message, true)
    } finally {
      setAsking(false)
    }
  }

  const running = !!job && !job.done
  const r = job?.result ?? {}

  return (
    <div className="grid2">
      {/* ---------- left: get data in ---------- */}
      <div>
        <div
          className={'dropzone' + (drag ? ' over' : '')}
          onDragOver={(e) => {
            e.preventDefault()
            setDrag(true)
          }}
          onDragLeave={() => setDrag(false)}
          onDrop={(e) => {
            e.preventDefault()
            setDrag(false)
            void ingestFiles(e.dataTransfer.files)
          }}
          onClick={() => fileInput.current?.click()}
        >
          <div className="big">⇪</div>
          <b>Drop any file here</b>
          <p className="hint" style={{ margin: '6px 0 0' }}>
            CSV · TSV · Excel (.xlsx/.xls/.ods) · JSON (nested or wrapped) · JSON&nbsp;Lines · YAML · XML · Word
            (.docx) · PDF · HTML · Markdown · plain text
            <br />
            Spreadsheets become one source per sheet; nested structures are flattened; prose becomes text the AI
            extracts triples from.
          </p>
          <input
            ref={fileInput}
            type="file"
            multiple
            style={{ display: 'none' }}
            onChange={(e) => {
              if (e.target.files) void ingestFiles(e.target.files)
              e.target.value = ''
            }}
          />
        </div>
        {busy && <div className="notice" style={{ marginBottom: 16 }}>{busy}</div>}

        <div className="card">
          <div className="row">
            <h2 style={{ flex: 1 }}>Sources</h2>
            <span className="pill">{sources.length}</span>
          </div>
          {!sources.length && <div className="empty">Nothing loaded yet.</div>}
          {sources.map((s) => (
            <div key={s.id} className="src-row">
              <div style={{ flex: 1, minWidth: 0 }}>
                <span className="n mono">{s.name}</span>
                <span className={'badge ' + (s.kind === 'text' ? 'enum' : 'identifier')} style={{ marginLeft: 8 }}>
                  {s.origin || s.kind}
                </span>
                <div className="m">
                  {s.kind === 'text'
                    ? `${s.rowCount} paragraph(s)`
                    : `${s.rowCount} rows · ${s.columns.length} columns`}
                  {s.note ? ` · ${s.note}` : ''}
                </div>
              </div>
              <button
                className="sm danger"
                title="Remove"
                onClick={async () => {
                  await api.deleteSource(pid, s.id)
                  reload()
                  onChanged()
                }}
              >
                ✕
              </button>
            </div>
          ))}
        </div>
      </div>

      {/* ---------- right: build & ask ---------- */}
      <div>
        <div className="card">
          <h2>✨ Auto-build the knowledge graph</h2>
          <p className="hint">
            One pass over everything above: profile → competency questions → ontology (T-Box) → mapping (drafted, then
            mechanically checked against your real columns) → lift → extract from documents → SHACL shapes → answer the
            competency suite → reason. Every AI draft is verified before it touches the store, and each step stays
            editable in the numbered tabs afterwards.
          </p>
          <div className="row">
            <button className="primary" disabled={running || !sources.length} onClick={build}>
              {running ? 'Building…' : 'Build it for me'}
            </button>
            <label className="chk">
              <input type="checkbox" checked={doExtract} onChange={(e) => setDoExtract(e.target.checked)} /> extract
              from documents
            </label>
            <label className="chk">
              <input type="checkbox" checked={doReason} onChange={(e) => setDoReason(e.target.checked)} /> reason
            </label>
          </div>

          {job && (
            <>
              <ol className="steps">
                {job.steps.map((s) => (
                  <li key={s.key} className={s.status}>
                    <span className="ic">{STATUS_ICON[s.status] ?? '○'}</span>
                    <div>
                      <b>{s.label}</b>
                      {s.detail && <div className="m">{s.detail}</div>}
                    </div>
                  </li>
                ))}
              </ol>
              {job.error && <div className="notice warn">{job.error}</div>}
              {job.done && !job.error && (
                <>
                  <div className="stat" style={{ marginTop: 4 }}>
                    <div className="kv">
                      <b>{(r.tripleCount ?? 0).toLocaleString()}</b>
                      <span>triples</span>
                    </div>
                    <div className="kv">
                      <b>{r.tbox?.classes ?? 0}</b>
                      <span>classes</span>
                    </div>
                    <div className="kv">
                      <b>{r.tbox?.properties ?? 0}</b>
                      <span>properties</span>
                    </div>
                    <div className="kv">
                      <b>
                        {r.competency?.passed ?? 0}/{r.competency?.total ?? 0}
                      </b>
                      <span>questions pass</span>
                    </div>
                    <div className="kv">
                      <b>{r.validation?.violationCount ?? 0}</b>
                      <span>violations</span>
                    </div>
                  </div>
                  <div className="chip-row" style={{ marginTop: 12 }}>
                    <button className="sm" onClick={() => onGoTo('tbox')}>Review the ontology →</button>
                    <button className="sm" onClick={() => onGoTo('mapping')}>Review the mapping →</button>
                    <button className="sm" onClick={() => onGoTo('validate')}>Violations →</button>
                    <button className="sm" onClick={() => onGoTo('governance')}>Provenance →</button>
                  </div>
                  {!!r.repairs?.length && (
                    <details className="repairs">
                      <summary>{r.repairs.length} mapping repair(s) applied</summary>
                      <ul>
                        {r.repairs.map((x, i) => (
                          <li key={i}>{x}</li>
                        ))}
                      </ul>
                    </details>
                  )}
                </>
              )}
            </>
          )}
        </div>

        <div className="card">
          <h2>💬 Ask the graph</h2>
          <p className="hint">
            Plain language in, an answer out — with the SPARQL it ran and the rows it read, so you can always check the
            answer instead of trusting it.
          </p>
          <div className="row">
            <input
              style={{ flex: 1 }}
              value={question}
              placeholder="e.g. Which supplier has the most products?"
              onChange={(e) => setQuestion(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && ask()}
            />
            <button className="ai" disabled={asking || !question.trim()} onClick={ask}>
              {asking ? '…' : 'Ask'}
            </button>
          </div>

          {answer && (
            <>
              <div className="answer">{answer.answer}</div>
              <div className="row" style={{ marginTop: 8 }}>
                <span className="m">
                  {answer.count} row(s){answer.model ? ` · ${answer.model}` : ''}
                  {answer.repaired ? ' · query self-repaired' : ''}
                </span>
                <div className="spacer" />
                <button className="sm ghost" onClick={() => setShowSparql(!showSparql)}>
                  {showSparql ? 'Hide' : 'Show'} SPARQL
                </button>
              </div>
              {showSparql && <pre className="mono sparql">{answer.sparql}</pre>}
              {!!answer.rows?.length && (
                <div className="table-wrap" style={{ marginTop: 10 }}>
                  <table>
                    <thead>
                      <tr>
                        {Object.keys(answer.rows[0]).map((h) => (
                          <th key={h}>{h}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {answer.rows.slice(0, 50).map((row, i) => (
                        <tr key={i}>
                          {Object.keys(answer.rows![0]).map((h) => (
                            <td key={h} className="mono" style={{ fontSize: 11 }}>
                              {row[h]?.value ?? ''}
                            </td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  )
}
