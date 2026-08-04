import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { api } from './api'
import type { Batch, Column, CompetencyQuestion, Source, SparqlResult, Tbox } from './api'
import { GraphViz } from './GraphViz'

export type PanelProps = {
  pid: number
  notify: (m: string, err?: boolean) => void
  onChanged: () => void
  /** Report what the user has selected, so AIP Assist can use it as context. */
  onSelection?: (name?: string) => void
}

// ---- shared bits ----------------------------------------------------------

// Module-level toast so AsyncButton can surface API failures (App wires this up).
let panelNotify: (m: string, err?: boolean) => void = () => {}
export function setPanelNotify(fn: (m: string, err?: boolean) => void) {
  panelNotify = fn
}

function AsyncButton({
  onClick,
  children,
  className,
  disabled,
  title,
}: {
  onClick: () => Promise<void>
  children: ReactNode
  className?: string
  disabled?: boolean
  title?: string
}) {
  const [busy, setBusy] = useState(false)
  return (
    <button
      className={className}
      disabled={busy || disabled}
      title={title}
      onClick={async () => {
        setBusy(true)
        try {
          await onClick()
        } catch (e) {
          panelNotify((e as Error).message || 'Request failed', true)
        } finally {
          setBusy(false)
        }
      }}
    >
      {busy ? '…' : children}
    </button>
  )
}

const short = (iri: string) => iri.replace(/^.*[#/]/, '') || iri

function useLoad<T>(fn: () => Promise<T>, deps: unknown[], notify: PanelProps['notify']) {
  const [data, setData] = useState<T | null>(null)
  const reload = useCallback(() => {
    fn()
      .then(setData)
      .catch((e) => notify((e as Error).message, true))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
  useEffect(() => reload(), [reload])
  return [data, reload, setData] as const
}

// ---- [1] Sources ----------------------------------------------------------

export function SourcesPanel({ pid, notify, onChanged, onSelection }: PanelProps) {
  const [sources, reload] = useLoad(() => api.listSources(pid), [pid], notify)
  const [sel, setSel] = useState<Source | null>(null)

  // Tell the shell which source is open — AIP Assist ranks its metadata higher.
  useEffect(() => {
    onSelection?.(sel?.name)
    return () => onSelection?.(undefined)
  }, [sel?.name, onSelection])
  const [name, setName] = useState('')
  const [content, setContent] = useState('')
  const [llmRoles, setLlmRoles] = useState<Record<string, { role: string; note: string; suggestedClass?: string }>>({})

  useEffect(() => {
    if (sources && sources.length && !sel) setSel(sources[0])
  }, [sources, sel])

  // AI role suggestions are per-source — clear them whenever the selection changes.
  useEffect(() => {
    setLlmRoles({})
  }, [sel?.id])

  function onFile(f: File) {
    const r = new FileReader()
    r.onload = () => {
      setContent(String(r.result || ''))
      setName(f.name)
    }
    r.readAsText(f)
  }

  return (
    <div className="grid2">
      <div>
        <div className="card">
          <h2>Add a source</h2>
          <p className="hint">
            Upload a <b>CSV</b> or <b>JSON array</b>. Every column is profiled — type, null ratio, uniqueness, and a
            heuristic ontology role (identifier / relation / attribute / enum). Don't let the file shape dictate the
            ontology; use the profile to inform the T-Box.
          </p>
          <label className="fld">
            <span>Logical source name (referenced by the mapping)</span>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="products" />
          </label>
          <label className="fld">
            <span>Paste content, or pick a file</span>
            <textarea className="mono" rows={6} value={content} onChange={(e) => setContent(e.target.value)} placeholder="sku,name,price&#10;A1,Widget,150000" />
          </label>
          <div className="row">
            <input type="file" accept=".csv,.json,.tsv,text/*" onChange={(e) => e.target.files?.[0] && onFile(e.target.files[0])} />
            <div className="spacer" />
            <AsyncButton
              className="primary"
              disabled={!name.trim() || !content.trim()}
              onClick={async () => {
                await api.addSource(pid, name.trim(), content, name.endsWith('.json') ? 'json' : 'csv')
                setContent('')
                setName('')
                reload()
                onChanged()
                notify('Source added & profiled')
              }}
            >
              Add & profile
            </AsyncButton>
          </div>
        </div>

        <div className="card">
          <h2>Sources</h2>
          {!sources?.length && <div className="empty">No sources yet.</div>}
          {sources?.map((s) => (
            <div key={s.id} className={'proj-item' + (sel?.id === s.id ? ' active' : '')} onClick={() => setSel(s)} style={{ flexDirection: 'row', alignItems: 'center' }}>
              <div style={{ flex: 1 }}>
                <span className="n mono">{s.name}</span>
                <span className="m"> · {s.kind} · {s.rowCount} rows · {s.columns.length} cols</span>
              </div>
              <AsyncButton className="sm danger" onClick={async () => { await api.deleteSource(pid, s.id); setSel(null); reload(); onChanged() }}>✕</AsyncButton>
            </div>
          ))}
        </div>
      </div>

      <div>
        {sel && (
          <div className="card">
            <div className="row">
              <h2 style={{ flex: 1 }}>Profile · <span className="mono">{sel.name}</span></h2>
              <AsyncButton
                className="ai sm"
                onClick={async () => {
                  const r = await api.profileSource(pid, sel.id, true)
                  if (r.llmError) return notify(r.llmError, true)
                  const roles = (r.llm?.roles as { columns?: Array<{ name: string; role: string; note: string; suggestedClass?: string }> })?.columns || []
                  const map: typeof llmRoles = {}
                  roles.forEach((c) => (map[c.name] = { role: c.role, note: c.note, suggestedClass: c.suggestedClass }))
                  setLlmRoles(map)
                  notify('AI roles via ' + (r.llm?.model || '?'))
                }}
              >
                ✨ AI suggest roles
              </AsyncButton>
            </div>
            <div className="table-wrap" style={{ marginTop: 10 }}>
              <table>
                <thead>
                  <tr><th>Column</th><th>Type</th><th>Role</th><th>Null</th><th>Distinct</th><th>Samples</th></tr>
                </thead>
                <tbody>
                  {sel.columns.map((c: Column) => (
                    <tr key={c.name}>
                      <td className="mono"><b>{c.name}</b>{c.isUnique && <span title="candidate key"> 🔑</span>}</td>
                      <td className="mono">{c.datatype}</td>
                      <td>
                        <span className={'badge ' + (llmRoles[c.name]?.role || c.role)}>{llmRoles[c.name]?.role || c.role}</span>
                        {llmRoles[c.name]?.note && <div className="m" style={{ fontSize: 11, color: 'var(--muted)' }}>{llmRoles[c.name].note}</div>}
                      </td>
                      <td>{(c.nullRatio * 100).toFixed(0)}%</td>
                      <td>{c.distinctCount}</td>
                      <td className="mono" style={{ fontSize: 11, color: 'var(--muted)' }}>{c.samples.slice(0, 3).join(', ')}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

// ---- [2] T-Box ------------------------------------------------------------

export function TboxPanel({ pid, notify, onChanged }: PanelProps) {
  const [tbox, reload] = useLoad<Tbox>(() => api.getTbox(pid), [pid], notify)
  const [viz, reloadViz] = useLoad(() => api.tboxGraph(pid), [pid], notify)
  const [draft, setDraft] = useState('')
  const [cls, setCls] = useState({ iri: '', label: '', subClassOf: '' })
  const [prop, setProp] = useState({ iri: '', kind: 'object', label: '', domain: '', range: '' })

  const refreshAll = () => { reload(); reloadViz(); onChanged() }

  return (
    <div>
      <div className="card">
        <div className="row">
          <div style={{ flex: 1 }}>
            <h2>T-Box (schema)</h2>
            <p className="hint" style={{ margin: 0 }}>Designed by hand from competency questions — not a copy of the table schema. Enum columns become SKOS individuals, not classes; a relation that carries attributes needs a reification class.</p>
          </div>
          <AsyncButton
            className="ai"
            onClick={async () => {
              const r = await api.draftTbox(pid)
              setDraft(JSON.stringify(r.draft, null, 2))
              notify('Drafted via ' + r.model + ' — review then Apply')
            }}
          >
            ✨ AI draft from competency Qs
          </AsyncButton>
        </div>

        {draft && (
          <div style={{ marginTop: 14 }}>
            <textarea className="mono" rows={10} value={draft} onChange={(e) => setDraft(e.target.value)} />
            <div className="row" style={{ marginTop: 8 }}>
              <AsyncButton
                className="primary"
                onClick={async () => {
                  let parsed: unknown
                  try {
                    parsed = JSON.parse(draft)
                  } catch (e) {
                    notify('Invalid JSON: ' + (e as Error).message, true)
                    return
                  }
                  const r = await api.applyTbox(pid, parsed)
                  setDraft('')
                  refreshAll()
                  notify(`Applied ${r.classes} classes, ${r.properties} properties`)
                }}
              >
                Apply draft
              </AsyncButton>
              <button className="ghost" onClick={() => setDraft('')}>Discard</button>
            </div>
          </div>
        )}
      </div>

      <div className="grid2">
        <div className="card">
          <h2>Classes {tbox && <span className="pill">{tbox.classes.length}</span>}</h2>
          <div className="table-wrap" style={{ margin: '10px 0' }}>
            <table>
              <thead><tr><th>Class</th><th>Label</th><th>subClassOf</th><th></th></tr></thead>
              <tbody>
                {tbox?.classes.map((c) => (
                  <tr key={c.iri}>
                    <td className="mono">{short(c.iri)}</td>
                    <td>{c.label || '—'}</td>
                    <td className="mono">{c.super ? short(c.super) : '—'}</td>
                    <td><AsyncButton className="sm danger" onClick={async () => { await api.removeTerm(pid, c.iri); refreshAll() }}>✕</AsyncButton></td>
                  </tr>
                ))}
                {!tbox?.classes.length && <tr><td colSpan={4} className="m">No classes.</td></tr>}
              </tbody>
            </table>
          </div>
          <div className="row">
            <input placeholder="ex:Product" value={cls.iri} onChange={(e) => setCls({ ...cls, iri: e.target.value })} style={{ flex: 2 }} />
            <input placeholder="label" value={cls.label} onChange={(e) => setCls({ ...cls, label: e.target.value })} style={{ flex: 2 }} />
            <input placeholder="subClassOf" value={cls.subClassOf} onChange={(e) => setCls({ ...cls, subClassOf: e.target.value })} style={{ flex: 2 }} />
            <AsyncButton disabled={!cls.iri.trim()} onClick={async () => { await api.addClass(pid, { iri: cls.iri, label: cls.label || undefined, super: cls.subClassOf || undefined }); setCls({ iri: '', label: '', subClassOf: '' }); refreshAll() }}>＋</AsyncButton>
          </div>
        </div>

        <div className="card">
          <h2>Properties {tbox && <span className="pill">{tbox.properties.length}</span>}</h2>
          <div className="table-wrap" style={{ margin: '10px 0' }}>
            <table>
              <thead><tr><th>Property</th><th>Kind</th><th>Domain → Range</th><th></th></tr></thead>
              <tbody>
                {tbox?.properties.map((p) => (
                  <tr key={p.iri}>
                    <td className="mono">{short(p.iri)}</td>
                    <td><span className={'badge ' + (p.kind?.includes('Datatype') ? 'data' : 'object')}>{p.kind?.includes('Datatype') ? 'data' : 'object'}</span></td>
                    <td className="mono">{p.domain ? short(p.domain) : '—'} → {p.range ? short(p.range) : '—'}</td>
                    <td><AsyncButton className="sm danger" onClick={async () => { await api.removeTerm(pid, p.iri); refreshAll() }}>✕</AsyncButton></td>
                  </tr>
                ))}
                {!tbox?.properties.length && <tr><td colSpan={4} className="m">No properties.</td></tr>}
              </tbody>
            </table>
          </div>
          <div className="row">
            <input placeholder="ex:hasPrice" value={prop.iri} onChange={(e) => setProp({ ...prop, iri: e.target.value })} style={{ flex: 2 }} />
            <select value={prop.kind} onChange={(e) => setProp({ ...prop, kind: e.target.value })} style={{ width: 100 }}>
              <option value="object">object</option>
              <option value="data">data</option>
              <option value="annotation">annotation</option>
            </select>
          </div>
          <div className="row" style={{ marginTop: 8 }}>
            <input placeholder="domain (ex:Product)" value={prop.domain} onChange={(e) => setProp({ ...prop, domain: e.target.value })} style={{ flex: 1 }} />
            <input placeholder="range (xsd:decimal)" value={prop.range} onChange={(e) => setProp({ ...prop, range: e.target.value })} style={{ flex: 1 }} />
            <AsyncButton disabled={!prop.iri.trim()} onClick={async () => { await api.addProperty(pid, { iri: prop.iri, kind: prop.kind, label: prop.label || undefined, domain: prop.domain || undefined, range: prop.range || undefined }); setProp({ iri: '', kind: prop.kind, label: '', domain: '', range: '' }); refreshAll() }}>＋</AsyncButton>
          </div>
        </div>
      </div>

      <div className="card">
        <h2>Schema graph</h2>
        {viz && <GraphViz data={viz} />}
      </div>
    </div>
  )
}

// ---- [3] Mapping ----------------------------------------------------------

const MAPPING_HINT = `{
  "base": "http://senclaw.local/onto/x",
  "prefixes": { "ex": "http://senclaw.local/onto/x#" },
  "triplesMaps": [
    {
      "name": "MainMap",
      "source": "<sourceName>",
      "subject": { "template": "thing/{id}", "class": "ex:Thing" },
      "predicateObjectMaps": [
        { "predicate": "rdfs:label", "object": { "column": "name" } },
        { "predicate": "ex:value", "object": { "column": "value", "datatype": "xsd:decimal" } }
      ]
    }
  ]
}`

export function MappingPanel({ pid, notify, onChanged }: PanelProps) {
  const [text, setText] = useState('')
  const [sources] = useLoad(() => api.listSources(pid), [pid], notify)
  const [report, setReport] = useState<{ samples?: string[][]; triples?: number; subjects?: number; skippedRows?: number; batch?: string } | null>(null)

  useEffect(() => {
    api.getMapping(pid).then((m) => setText(Object.keys(m).length ? JSON.stringify(m, null, 2) : MAPPING_HINT)).catch(() => setText(MAPPING_HINT))
  }, [pid])

  const parse = () => {
    try {
      return JSON.parse(text)
    } catch (e) {
      notify('Invalid JSON: ' + (e as Error).message, true)
      return null
    }
  }

  return (
    <div className="grid2">
      <div className="card">
        <div className="row">
          <h2 style={{ flex: 1 }}>Mapping (RML-lite)</h2>
          <select id="srcpick" defaultValue="" style={{ width: 130 }}>
            <option value="">source…</option>
            {sources?.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}
          </select>
          <AsyncButton
            className="ai sm"
            onClick={async () => {
              const el = document.getElementById('srcpick') as HTMLSelectElement | null
              const sid = el?.value ? Number(el.value) : undefined
              const r = await api.draftMapping(pid, sid)
              setText(JSON.stringify(r.mapping, null, 2))
              notify('Drafted via ' + r.model)
            }}
          >
            ✨ AI draft
          </AsyncButton>
        </div>
        <p className="hint">Mapping is <b>data, not code</b>: templated IRIs (<code>{'{col}'}</code>), stable hashed IRIs for keyless entities, typed literals, and object references. Re-running is idempotent.</p>
        <textarea className="mono" rows={20} value={text} onChange={(e) => setText(e.target.value)} />
        <div className="row" style={{ marginTop: 8 }}>
          <AsyncButton onClick={async () => { const m = parse(); if (!m) return; await api.setMapping(pid, m); notify('Mapping saved') }}>Save</AsyncButton>
          <AsyncButton onClick={async () => { const m = parse(); if (!m) return; const r = await api.previewMapping(pid, m); setReport(r) }}>Preview</AsyncButton>
          <div className="spacer" />
          <AsyncButton className="primary" onClick={async () => { const m = parse(); if (!m) return; const r = await api.liftMapping(pid, m); setReport(r); onChanged(); notify(`Lifted ${r.triples} triples`) }}>Run lift →</AsyncButton>
        </div>
      </div>

      <div className="card">
        <h2>{report?.batch ? 'Lift result' : 'Preview'}</h2>
        {!report && <div className="empty">Preview or lift to see the generated triples.</div>}
        {report && (
          <>
            <div className="stat" style={{ marginBottom: 12 }}>
              <div className="kv"><b>{report.triples ?? 0}</b><span>triples</span></div>
              <div className="kv"><b>{report.subjects ?? 0}</b><span>subjects</span></div>
              <div className="kv"><b>{report.skippedRows ?? 0}</b><span>skipped rows</span></div>
            </div>
            {report.batch && <div className="notice" style={{ marginBottom: 10 }}>Batch <span className="mono">{short(report.batch)}</span> — drop/reload it in Reason &amp; Provenance.</div>}
            {report.samples && (
              <pre className="code">{report.samples.map((s) => `${short(s[0].replace(/[<>]/g, ''))}  ${short(s[1].replace(/[<>]/g, ''))}  ${s[2].length > 42 ? s[2].slice(0, 42) + '…' : s[2]}`).join('\n')}</pre>
            )}
          </>
        )}
      </div>
    </div>
  )
}

// ---- [4] Explore ----------------------------------------------------------

const SAMPLE_QUERIES = [
  { label: 'All classes in use', q: 'SELECT ?type (COUNT(?s) AS ?n) WHERE { ?s a ?type } GROUP BY ?type ORDER BY DESC(?n)' },
  { label: 'First 25 triples', q: 'SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 25' },
  { label: 'Property usage', q: 'SELECT ?p (COUNT(*) AS ?n) WHERE { ?s ?p ?o } GROUP BY ?p ORDER BY DESC(?n)' },
]

export function ExplorerPanel({ pid, notify }: PanelProps) {
  const [query, setQuery] = useState(SAMPLE_QUERIES[1].q)
  const [nl, setNl] = useState('')
  const [res, setRes] = useState<SparqlResult | null>(null)
  const [viz, reloadViz] = useLoad(() => api.dataGraph(pid, 200), [pid], notify)

  return (
    <div>
      <div className="card">
        <div className="row">
          <h2 style={{ flex: 1 }}>SPARQL</h2>
          {SAMPLE_QUERIES.map((s) => <button key={s.label} className="sm ghost" onClick={() => setQuery(s.q)}>{s.label}</button>)}
        </div>
        <p className="hint">Standard prefixes (rdf/rdfs/owl/xsd/skos) and your project's <code>ex:</code> are auto-declared. The default graph is the union of all batches + inferred triples.</p>
        <div className="row" style={{ marginBottom: 8 }}>
          <input placeholder="Ask in natural language…" value={nl} onChange={(e) => setNl(e.target.value)} style={{ flex: 1 }} onKeyDown={(e) => { if (e.key === 'Enter') (document.getElementById('nlbtn') as HTMLButtonElement)?.click() }} />
          <AsyncButton className="ai" disabled={!nl.trim()} onClick={async () => { const r = await api.nl2sparql(pid, nl); setQuery(r.sparql); notify('SPARQL via ' + r.model) }}>
            <span id="nlbtn">✨ NL → SPARQL</span>
          </AsyncButton>
        </div>
        <textarea className="mono" rows={6} value={query} onChange={(e) => setQuery(e.target.value)} />
        <div className="row" style={{ marginTop: 8 }}>
          <AsyncButton className="primary" onClick={async () => { const r = await api.sparql(pid, query); setRes(r) }}>Run</AsyncButton>
          {res?.rows && <span className="pill">{res.rows.length} rows</span>}
          {res && 'boolean' in res && res.boolean !== undefined && <span className={'badge ' + (res.boolean ? 'pass' : 'fail')}>{String(res.boolean)}</span>}
        </div>

        {res?.rows && res.rows.length > 0 && (
          <div className="table-wrap" style={{ marginTop: 12 }}>
            <table>
              <thead><tr>{res.head?.map((h) => <th key={h}>{h}</th>)}</tr></thead>
              <tbody>
                {res.rows.map((row, i) => (
                  <tr key={i}>
                    {res.head?.map((h) => {
                      const cell = row[h]
                      return <td key={h} className="mono" title={cell?.value}>{cell ? (cell.type === 'uri' ? short(cell.value) : cell.value) : ''}</td>
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <div className="card">
        <div className="row"><h2 style={{ flex: 1 }}>Data graph</h2><button className="sm" onClick={() => reloadViz()}>↻ Refresh</button></div>
        {viz && <GraphViz data={viz} />}
      </div>
    </div>
  )
}

// ---- [5] Competency -------------------------------------------------------

export function CompetencyPanel({ pid, notify }: PanelProps) {
  const [cqs, reload] = useLoad<CompetencyQuestion[]>(() => api.listCq(pid), [pid], notify)
  const [q, setQ] = useState('')
  const [sparql, setSparql] = useState('')
  const [expect, setExpect] = useState('nonempty')
  const [results, setResults] = useState<Record<number, { pass: boolean; count?: number; error?: string }>>({})
  const [summary, setSummary] = useState<{ passed: number; total: number } | null>(null)

  return (
    <div>
      <div className="card">
        <div className="row">
          <div style={{ flex: 1 }}>
            <h2>Competency questions</h2>
            <p className="hint" style={{ margin: 0 }}>The ontology is "done" when every competency question is answered by one SPARQL query. This is the test suite that proves it.</p>
          </div>
          <AsyncButton
            className="primary"
            disabled={!cqs?.length}
            onClick={async () => {
              const r = await api.runCq(pid)
              const map: typeof results = {}
              r.results.forEach((x) => (map[x.id] = { pass: x.pass, count: x.count, error: x.error }))
              setResults(map)
              setSummary({ passed: r.passed, total: r.total })
              notify(`${r.passed}/${r.total} passed`)
            }}
          >
            ▶ Run all
          </AsyncButton>
        </div>
        {summary && (
          <div className="notice" style={{ marginTop: 12, background: summary.passed === summary.total ? 'color-mix(in srgb, var(--ok) 14%, var(--panel))' : undefined }}>
            <b>{summary.passed} / {summary.total}</b> competency questions pass.
          </div>
        )}
      </div>

      <div className="card">
        {!cqs?.length && <div className="empty">No competency questions yet.</div>}
        {cqs?.map((c) => {
          const r = results[c.id]
          return (
            <div key={c.id} style={{ borderBottom: '1px solid var(--border)', padding: '10px 0' }}>
              <div className="row">
                {r && <span className={'badge ' + (r.pass ? 'pass' : 'fail')}>{r.pass ? 'PASS' : 'FAIL'}{r.count !== undefined ? ` · ${r.count}` : ''}</span>}
                <b style={{ flex: 1 }}>{c.question}</b>
                <span className="pill">expect: {c.expect}</span>
                <AsyncButton className="sm danger" onClick={async () => { await api.deleteCq(pid, c.id); reload() }}>✕</AsyncButton>
              </div>
              <pre className="code" style={{ marginTop: 6 }}>{c.sparql || '(no SPARQL)'}</pre>
              {r?.error && <div className="notice warn" style={{ marginTop: 6 }}>{r.error}</div>}
            </div>
          )
        })}
      </div>

      <div className="card">
        <h2>Add a competency question</h2>
        <label className="fld"><span>Question (natural language)</span><input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Which products cost over 80,000?" /></label>
        <label className="fld"><span>SPARQL that answers it</span><textarea className="mono" rows={4} value={sparql} onChange={(e) => setSparql(e.target.value)} placeholder="SELECT ?p WHERE { ?p ex:hasPrice ?v FILTER(?v > 80000) }" /></label>
        <div className="row">
          <select value={expect} onChange={(e) => setExpect(e.target.value)} style={{ width: 160 }}>
            <option value="nonempty">expect: non-empty</option>
            <option value="empty">expect: empty</option>
            <option value="boolean">expect: true (ASK)</option>
          </select>
          <div className="spacer" />
          <AsyncButton className="primary" disabled={!q.trim()} onClick={async () => { await api.addCq(pid, q, sparql, expect); setQ(''); setSparql(''); reload(); notify('Added') }}>Add</AsyncButton>
        </div>
      </div>
    </div>
  )
}

// ---- [6] Validate ---------------------------------------------------------

export function ValidationPanel({ pid, notify }: PanelProps) {
  const [text, setText] = useState('')
  const [report, setReport] = useState<{ conforms: boolean; violationCount: number; checked: number; violations: Array<{ focusNode: string; path: string; constraint: string; value: string; message: string }> } | null>(null)

  useEffect(() => {
    api.getShapes(pid).then((s) => setText(JSON.stringify(s, null, 2))).catch(() => setText('{ "nodeShapes": [] }'))
  }, [pid])

  const parse = () => {
    try {
      return JSON.parse(text)
    } catch (e) {
      notify('Invalid JSON: ' + (e as Error).message, true)
      return null
    }
  }

  return (
    <div className="grid2">
      <div className="card">
        <div className="row">
          <h2 style={{ flex: 1 }}>SHACL-lite shapes</h2>
          <AsyncButton className="ai sm" onClick={async () => { const r = await api.draftShapes(pid); setText(JSON.stringify(r.shapes, null, 2)); notify('Drafted via ' + r.model) }}>✨ AI draft</AsyncButton>
        </div>
        <p className="hint">SHACL is closed-world (missing data = violation) — the counterpart to OWL's open-world reasoning. Constraints: datatype, class, nodeKind, min/maxCount, min/maxInclusive, pattern.</p>
        <textarea className="mono" rows={18} value={text} onChange={(e) => setText(e.target.value)} />
        <div className="row" style={{ marginTop: 8 }}>
          <AsyncButton onClick={async () => { const s = parse(); if (!s) return; await api.setShapes(pid, s); notify('Shapes saved') }}>Save</AsyncButton>
          <div className="spacer" />
          <AsyncButton className="primary" onClick={async () => { const s = parse(); if (!s) return; const r = await api.validate(pid, s); setReport(r) }}>Validate →</AsyncButton>
        </div>
      </div>

      <div className="card">
        <h2>Report</h2>
        {!report && <div className="empty">Validate to see conformance.</div>}
        {report && (
          <>
            <div className={'notice' + (report.conforms ? '' : ' warn')} style={{ background: report.conforms ? 'color-mix(in srgb, var(--ok) 14%, var(--panel))' : undefined }}>
              {report.conforms ? '✓ Conforms' : `✗ ${report.violationCount} violation(s)`} · {report.checked} constraints checked
            </div>
            {report.violations.length > 0 && (
              <div className="table-wrap" style={{ marginTop: 12 }}>
                <table>
                  <thead><tr><th>Focus node</th><th>Constraint</th><th>Message</th></tr></thead>
                  <tbody>
                    {report.violations.map((v, i) => (
                      <tr key={i}>
                        <td className="mono" title={v.focusNode}>{short(v.focusNode)}</td>
                        <td><span className="badge fail">{v.constraint}</span></td>
                        <td>{v.message}{v.value && <span className="mono" style={{ color: 'var(--muted)' }}> ({v.value})</span>}</td>
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
  )
}

// ---- [7] Reason & Provenance ---------------------------------------------

export function GovernancePanel({ pid, notify, onChanged, onDeleteProject }: PanelProps & { onDeleteProject: () => void }) {
  const [batches, reloadBatches] = useLoad<Batch[]>(() => api.listBatches(pid), [pid], notify)
  const [reason, setReason] = useState<{ inferred: number; iterations: number } | null>(null)
  const [rClass, setRClass] = useState('')
  const [rProp, setRProp] = useState('rdfs:label')
  const [rThresh, setRThresh] = useState(0.9)
  const [pairs, setPairs] = useState<Array<{ a: string; b: string; labelA: string; labelB: string; score: number }>>([])
  const [predicate, setPredicate] = useState('skos:closeMatch')
  const [text, setText] = useState('')

  return (
    <div>
      <div className="grid2">
        <div className="card">
          <h2>Reasoning</h2>
          <p className="hint">Materialize an RDFS/OWL-RL subset (subclass, subproperty, domain, range, inverse, sameAs) to a fixpoint. Open-world: it only adds facts, never flags missing ones.</p>
          <div className="row">
            <AsyncButton className="primary" onClick={async () => { const r = await api.materialize(pid); setReason(r); onChanged(); notify(`Inferred ${r.inferred} new triples`) }}>▶ Materialize</AsyncButton>
            <AsyncButton className="ghost" onClick={async () => { await api.clearInferred(pid); setReason(null); onChanged(); notify('Inferred cleared') }}>Clear</AsyncButton>
          </div>
          {reason && <div className="notice" style={{ marginTop: 12 }}>+{reason.inferred} inferred triples in {reason.iterations} iterations.</div>}
        </div>

        <div className="card">
          <h2>Provenance batches</h2>
          <p className="hint">Each import is its own named graph. Drop one to remove exactly that lot — the thing a pipeline without provenance can't do.</p>
          <div className="table-wrap">
            <table>
              <thead><tr><th>Batch</th><th>Triples</th><th>When</th><th></th></tr></thead>
              <tbody>
                {batches?.map((b) => (
                  <tr key={b.iri}>
                    <td className="mono" title={b.iri}>{b.label || short(b.iri)}</td>
                    <td>{b.tripleCount}</td>
                    <td className="mono" style={{ fontSize: 11 }}>{b.generatedAt?.replace('T', ' ').replace('Z', '')}</td>
                    <td><AsyncButton className="sm danger" onClick={async () => { await api.dropBatch(pid, b.iri); reloadBatches(); onChanged() }}>Drop</AsyncButton></td>
                  </tr>
                ))}
                {!batches?.length && <tr><td colSpan={4} className="m">No batches yet.</td></tr>}
              </tbody>
            </table>
          </div>
        </div>
      </div>

      <div className="card">
        <h2>Entity resolution</h2>
        <p className="hint">Find likely-duplicate individuals by label similarity (Jaro-Winkler). Default link is <code>skos:closeMatch</code> — safer than <code>owl:sameAs</code>, which is transitive and contaminates whole clusters on one bad link.</p>
        <div className="row">
          <input placeholder="class (ex:Supplier)" value={rClass} onChange={(e) => setRClass(e.target.value)} style={{ flex: 1 }} />
          <input placeholder="label prop" value={rProp} onChange={(e) => setRProp(e.target.value)} style={{ width: 130 }} />
          <input type="number" step="0.05" min="0.5" max="1" value={rThresh} onChange={(e) => setRThresh(Number(e.target.value))} style={{ width: 90 }} />
          <AsyncButton disabled={!rClass.trim()} onClick={async () => { const r = await api.resolveCandidates(pid, rClass, rProp, rThresh); setPairs(r.pairs); notify(`${r.count} candidate pairs`) }}>Find duplicates</AsyncButton>
        </div>
        {pairs.length > 0 && (
          <>
            <div className="table-wrap" style={{ marginTop: 12 }}>
              <table>
                <thead><tr><th>A</th><th>B</th><th>Score</th></tr></thead>
                <tbody>
                  {pairs.map((p, i) => (
                    <tr key={i}><td>{p.labelA} <span className="mono" style={{ color: 'var(--muted)' }}>({short(p.a)})</span></td><td>{p.labelB} <span className="mono" style={{ color: 'var(--muted)' }}>({short(p.b)})</span></td><td>{p.score}</td></tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="row" style={{ marginTop: 8 }}>
              <select value={predicate} onChange={(e) => setPredicate(e.target.value)} style={{ width: 200 }}>
                <option value="skos:closeMatch">skos:closeMatch (safe)</option>
                <option value="owl:sameAs">owl:sameAs (merges identity)</option>
              </select>
              <div className="spacer" />
              <AsyncButton className="primary" onClick={async () => { const r = await api.resolveApply(pid, predicate, pairs.map((p) => [p.a, p.b])); setPairs([]); onChanged(); notify(`Linked ${r.applied} pairs`) }}>Link all {pairs.length}</AsyncButton>
            </div>
          </>
        )}
      </div>

      <div className="card">
        <h2>Extract from text (unstructured)</h2>
        <p className="hint">NER + relation extraction via the LLM into a dedicated batch, with provenance. Never mixed with asserted data — kept in its own named graph.</p>
        <textarea rows={5} value={text} onChange={(e) => setText(e.target.value)} placeholder="Acme Ltd supplies widgets to Globex at 150,000 VND since January 2026…" />
        <div className="row" style={{ marginTop: 8 }}>
          <div className="spacer" />
          <AsyncButton className="ai" disabled={!text.trim()} onClick={async () => { const r = await api.extract(pid, text); onChanged(); notify(`Extracted ${r.inserted} triples via ${r.model}`) }}>✨ Extract triples</AsyncButton>
        </div>
      </div>

      <div className="card" style={{ borderColor: 'color-mix(in srgb, var(--danger) 30%, var(--border))' }}>
        <h2>Danger zone</h2>
        <div className="row"><span className="hint" style={{ flex: 1, margin: 0 }}>Delete this project and every triple in it.</span><button className="danger" onClick={onDeleteProject}>Delete project</button></div>
      </div>
    </div>
  )
}
