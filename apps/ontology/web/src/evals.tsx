// AIP Evals — the "eval from day one" panel.
//
// For one logic function: pin down input→expected cases, then run them across
// one or more configured SenClaw models at once. Each case runs twice per model
// so run-to-run variance is visible, not hidden — the thing that decides whether
// a model is trustworthy for a task, or just lucky once. This is how you compare
// models before you commit a function to a model.

import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import type { LogicFunction } from './api'

type Model = { id: string; label?: string; modelName?: string }
type EvalCase = { id: number; input: string; expect: string }
type ModelResult = {
  model: string
  passed: number
  total: number
  varied: number
  cases: Array<{ input: string; expect: string; pass: boolean; varied: boolean; run1: string[]; run2: string[] }>
}

export function EvalsPanel({
  pid,
  fn,
  notify,
  onClose,
}: {
  pid: number
  fn: LogicFunction
  notify: (m: string, err?: boolean) => void
  onClose: () => void
}) {
  const [cases, setCases] = useState<EvalCase[]>([])
  const [models, setModels] = useState<Model[]>([])
  const [chosen, setChosen] = useState<string[]>([])
  const [results, setResults] = useState<ModelResult[]>([])
  const [running, setRunning] = useState(false)
  const [input, setInput] = useState('')
  const [expect, setExpect] = useState('')

  const load = useCallback(() => {
    api.listEvals(pid, fn.id).then(setCases).catch((e) => notify((e as Error).message, true))
  }, [pid, fn.id, notify])

  useEffect(() => {
    load()
    setResults([])
    api.models().then((m) => setModels(m.configs ?? [])).catch(() => {})
  }, [load])

  async function add() {
    if (!input.trim()) return notify('Input is required', true)
    try {
      await api.addEval(pid, fn.id, input.trim(), expect.trim())
      setInput('')
      setExpect('')
      load()
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  async function run() {
    if (!cases.length) return notify('Add at least one case first', true)
    setRunning(true)
    setResults([])
    try {
      const r = await api.runEvals(pid, fn.id, chosen)
      setResults(r.results as ModelResult[])
    } catch (e) {
      notify((e as Error).message, true)
    } finally {
      setRunning(false)
    }
  }

  const name = (m: Model) => m.label ?? m.modelName ?? m.id
  const toggle = (id: string) =>
    setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]))

  return (
    <div className="card evals">
      <div className="row">
        <h2 style={{ flex: 1 }}>
          Evals · <span className="mono">{fn.name}</span>
        </h2>
        <button className="ghost sm" onClick={onClose}>✕</button>
      </div>
      <p className="hint">
        Save input→expected cases and run them across models. Each case runs twice per model, so run-to-run{' '}
        <b>variance</b> shows up next to the pass rate — that is what tells you a model is reliable for this function,
        not lucky once.
      </p>

      {/* add case */}
      <div className="row" style={{ alignItems: 'flex-end', gap: 8 }}>
        <label className="fld" style={{ flex: 2, marginBottom: 0 }}>
          <span>Input {fn.kind === 'classify' ? '(a row, e.g. {"name":"Áo khoác"})' : '(text)'}</span>
          <input value={input} onChange={(e) => setInput(e.target.value)} placeholder={fn.kind === 'classify' ? '{"name":"Áo khoác"}' : 'Some input text'} />
        </label>
        <label className="fld" style={{ flex: 1, marginBottom: 0 }}>
          <span>Expected (substring)</span>
          <input value={expect} onChange={(e) => setExpect(e.target.value)} placeholder="clothing" />
        </label>
        <button className="sm" onClick={add}>+ Case</button>
      </div>

      {/* cases */}
      {!!cases.length && (
        <div className="table-wrap" style={{ marginTop: 10 }}>
          <table>
            <thead>
              <tr><th>Input</th><th>Expected</th></tr>
            </thead>
            <tbody>
              {cases.map((c) => (
                <tr key={c.id}>
                  <td className="mono" style={{ fontSize: 11 }}>{c.input}</td>
                  <td className="mono" style={{ fontSize: 11 }}>{c.expect || <span className="m">any</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* model selection + run */}
      <div style={{ marginTop: 12 }}>
        <div className="m" style={{ marginBottom: 6 }}>Models to compare (none = current):</div>
        <div className="chip-row">
          {models.map((m) => (
            <button
              key={m.id}
              className={'sm' + (chosen.includes(m.id) ? ' primary' : '')}
              onClick={() => toggle(m.id)}
            >
              {name(m)}
            </button>
          ))}
        </div>
        <div className="row" style={{ marginTop: 10 }}>
          <button className="ai" disabled={running || !cases.length} onClick={run}>
            {running ? 'Running…' : `Run evals${chosen.length ? ` × ${chosen.length} model(s)` : ''}`}
          </button>
        </div>
      </div>

      {/* results, side by side */}
      {!!results.length && (
        <div className="eval-results">
          {results.map((r) => (
            <div key={r.model} className="eval-col">
              <div className="eval-head">
                <b>{modelLabel(models, r.model)}</b>
                <span className={'badge ' + (r.passed === r.total ? 'pass' : 'fail')}>
                  {r.passed}/{r.total} pass
                </span>
                {r.varied > 0 && <span className="badge enum" title="run-to-run variance">{r.varied} varied</span>}
              </div>
              {r.cases.map((c, i) => (
                <div key={i} className={'eval-case' + (c.pass ? '' : ' bad')}>
                  <span className="ic">{c.pass ? '✓' : '✗'}</span>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <span className="mono">{c.input}</span>
                    <div className="m">→ {c.run1.join(', ') || '∅'}{c.varied ? ` / ${c.run2.join(', ') || '∅'}` : ''}</div>
                  </div>
                  {c.varied && <span className="badge enum" title="differed between runs">≠</span>}
                </div>
              ))}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function modelLabel(models: Model[], id: string): string {
  if (id === 'current') return 'Current model'
  const m = models.find((x) => x.id === id)
  return m?.label ?? m?.modelName ?? id
}
