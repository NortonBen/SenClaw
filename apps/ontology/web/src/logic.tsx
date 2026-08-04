// [8] AIP Logic — LLM functions that propose typed edits, reviewed by a human.
//
// The tab has two halves: define/run functions on the left, review the proposal
// queue on the right. Nothing an LLM produces touches the graph until it is
// approved here — and every action shown has already been type-checked against
// the ontology, so an invalid one is flagged, not silently applied.

import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import type { LogicFunction, Proposal, RunReport } from './api'
import { EvalsPanel } from './evals'

type Props = { pid: number; notify: (m: string, err?: boolean) => void; onChanged: () => void }

export function LogicPanel({ pid, notify, onChanged }: Props) {
  const [fns, setFns] = useState<LogicFunction[]>([])
  const [counts, setCounts] = useState<Record<string, number>>({})
  const [proposals, setProposals] = useState<Proposal[]>([])
  const [filter, setFilter] = useState<'pending' | 'approved' | 'rejected' | 'invalid' | ''>('pending')
  const [busy, setBusy] = useState('')
  const [preview, setPreview] = useState<RunReport | null>(null)
  const [evalsFor, setEvalsFor] = useState<LogicFunction | null>(null)

  // create-function form
  const [name, setName] = useState('')
  const [kind, setKind] = useState<'extract' | 'classify' | 'resolve'>('extract')
  const [target, setTarget] = useState('')
  const [instruction, setInstruction] = useState('')

  const loadFns = useCallback(() => {
    api.listFunctions(pid).then((r) => {
      setFns(r.functions)
      setCounts(r.proposalCounts)
    }).catch((e) => notify((e as Error).message, true))
  }, [pid, notify])

  const loadProps = useCallback(() => {
    api.listProposals(pid, filter || undefined).then((r) => {
      setProposals(r.proposals)
      setCounts(r.counts)
    }).catch((e) => notify((e as Error).message, true))
  }, [pid, filter, notify])

  useEffect(() => {
    loadFns()
  }, [loadFns])
  useEffect(() => {
    loadProps()
  }, [loadProps])

  async function create() {
    if (!name.trim()) return notify('Name is required', true)
    // resolve is deterministic — the "instruction" is only an optional threshold.
    if (kind !== 'resolve' && !instruction.trim()) return notify('Instruction is required', true)
    if ((kind === 'classify' || kind === 'resolve') && !target.trim())
      return notify(kind === 'classify' ? 'Classify needs a target source' : 'Resolve needs a target class', true)
    try {
      await api.createFunction(pid, { name: name.trim(), kind, target: target.trim(), instruction: instruction.trim() })
      setName('')
      setInstruction('')
      setTarget('')
      loadFns()
      notify('Function created')
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  async function trial(fid: number) {
    setBusy(`trial-${fid}`)
    setPreview(null)
    try {
      setPreview(await api.trialFunction(pid, fid))
    } catch (e) {
      notify((e as Error).message, true)
    } finally {
      setBusy('')
    }
  }

  async function run(fid: number) {
    setBusy(`run-${fid}`)
    setPreview(null)
    try {
      const r = await api.runFunction(pid, fid)
      notify(
        `${r.proposed} proposal(s)${r.invalid ? `, ${r.invalid} blocked by the type checker` : ''}${
          r.applied ? `, ${r.applied} auto-applied` : ''
        }`,
      )
      setFilter('pending')
      loadFns()
      loadProps()
      if (r.applied) onChanged()
    } catch (e) {
      notify((e as Error).message, true)
    } finally {
      setBusy('')
    }
  }

  async function approve(ids?: number[]) {
    try {
      const r = await api.approveProposals(pid, ids)
      notify(`Applied ${r.applied} action(s) → ${r.triples} triples${r.staleRejected ? `, ${r.staleRejected} stale` : ''}`)
      loadProps()
      loadFns()
      onChanged()
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  async function reject(ids?: number[]) {
    try {
      await api.rejectProposals(pid, ids)
      loadProps()
      loadFns()
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  const pending = counts.pending ?? 0

  return (
    <div className="grid2">
      {/* ---------- left: functions ---------- */}
      <div>
        <div className="card">
          <h2>AIP Logic function</h2>
          <p className="hint">
            An LLM function that reads your data and proposes <b>typed actions</b> on the ontology — add an entity, set
            an attribute, link two entities. Every action is checked against the T-Box, so a made-up class or property is
            rejected before it can ever become a triple. Nothing is written until you approve it.
          </p>
          <label className="fld">
            <span>Name</span>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Classify products" />
          </label>
          <div className="row">
            <label className="fld" style={{ flex: 1 }}>
              <span>Kind</span>
              <select value={kind} onChange={(e) => setKind(e.target.value as 'extract' | 'classify' | 'resolve')}>
                <option value="extract">extract — pull facts from a text source</option>
                <option value="classify">classify — run over each row of a source</option>
                <option value="resolve">resolve — find duplicate entities (no LLM)</option>
              </select>
            </label>
            <label className="fld" style={{ flex: 1 }}>
              <span>{kind === 'classify' ? 'Source (rows)' : kind === 'resolve' ? 'Class (e.g. ex:Supplier)' : 'Text source (optional)'}</span>
              <input value={target} onChange={(e) => setTarget(e.target.value)} placeholder={kind === 'classify' ? 'products' : kind === 'resolve' ? 'ex:Supplier' : 'contract'} />
            </label>
          </div>
          <label className="fld">
            <span>{kind === 'resolve' ? 'Similarity threshold (optional, e.g. 0.9 or 90%)' : 'Instruction (plain language)'}</span>
            <textarea rows={kind === 'resolve' ? 1 : 3} value={instruction} onChange={(e) => setInstruction(e.target.value)} placeholder={kind === 'resolve' ? '0.85' : 'Set hasCategory to clothing or appliance based on the product name.'} />
          </label>
          <div className="row">
            <button className="primary" onClick={create}>Create function</button>
          </div>
        </div>

        <div className="card">
          <div className="row">
            <h2 style={{ flex: 1 }}>Functions</h2>
            <span className="pill">{fns.length}</span>
          </div>
          {!fns.length && <div className="empty">No functions yet.</div>}
          {fns.map((f) => (
            <div key={f.id} className="fn-row">
              <div style={{ flex: 1, minWidth: 0 }}>
                <span className="n">{f.name}</span>
                <span className={'badge ' + (f.kind === 'classify' ? 'relation' : f.kind === 'resolve' ? 'enum' : 'object')} style={{ marginLeft: 8 }}>{f.kind}</span>
                {f.target && <span className="m mono"> · {f.target}</span>}
                <div className="m">{f.instruction}</div>
              </div>
              <div className="chip-row">
                <button className="sm" disabled={busy === `trial-${f.id}`} onClick={() => trial(f.id)}>{busy === `trial-${f.id}` ? '…' : 'Trial'}</button>
                <button className="ai sm" disabled={busy === `run-${f.id}`} onClick={() => run(f.id)}>{busy === `run-${f.id}` ? '…' : 'Run'}</button>
                <button className="sm" onClick={() => setEvalsFor(f)}>Evals</button>
                <button className="sm danger" onClick={async () => { if (evalsFor?.id === f.id) setEvalsFor(null); await api.deleteFunction(pid, f.id); loadFns() }}>✕</button>
              </div>
            </div>
          ))}
        </div>

        {evalsFor && (
          <EvalsPanel pid={pid} fn={evalsFor} notify={notify} onClose={() => setEvalsFor(null)} />
        )}

        {preview && (
          <div className="card">
            <h2>Trial preview <span className="m">(nothing written)</span></h2>
            <div className="m" style={{ marginBottom: 8 }}>
              {preview.proposed} valid · {preview.invalid} blocked{preview.errors.length ? ` · ${preview.errors.length} error(s)` : ''}
            </div>
            {preview.preview.map((p, i) => (
              <div key={i} className={'prop-line' + (p.valid ? '' : ' bad')}>
                <span className="ic">{p.valid ? '✓' : '✗'}</span>
                <div style={{ flex: 1 }}>
                  <span className="mono">{p.summary}</span>
                  {!p.valid && <div className="m err-text">{p.invalidReason}</div>}
                  {p.rationale && <div className="m">{p.rationale}</div>}
                </div>
                <span className="conf">{Math.round(p.confidence * 100)}%</span>
              </div>
            ))}
            {!!preview.errors.length && <div className="notice warn">{preview.errors.join(' · ')}</div>}
          </div>
        )}
      </div>

      {/* ---------- right: proposal queue ---------- */}
      <div>
        <div className="card">
          <div className="row">
            <h2 style={{ flex: 1 }}>Proposal queue</h2>
            {(['pending', 'approved', 'rejected', 'invalid'] as const).map((st) => (
              <button
                key={st}
                className={'sm' + (filter === st ? ' primary' : '')}
                onClick={() => setFilter(st)}
              >
                {st} {counts[st] ? `(${counts[st]})` : ''}
              </button>
            ))}
          </div>
          <p className="hint">
            Every LLM edit lands here as a proposal — the “writes are proposals” rule. Approving applies the valid ones
            as one provenance batch you can later drop; the ontology is the contract the type checker enforced on the way
            in.
          </p>
          {filter === 'pending' && pending > 0 && (
            <div className="row" style={{ marginBottom: 10 }}>
              <button className="primary" onClick={() => approve()}>✓ Approve all {pending}</button>
              <button className="danger" onClick={() => reject()}>✕ Reject all</button>
            </div>
          )}
          {!proposals.length && <div className="empty">No {filter || ''} proposals.</div>}
          {proposals.map((p) => (
            <div key={p.id} className={'prop-line' + (p.valid ? '' : ' bad')}>
              <span className="ic">{p.status === 'approved' ? '●' : p.status === 'rejected' ? '–' : p.valid ? '○' : '✗'}</span>
              <div style={{ flex: 1, minWidth: 0 }}>
                <span className="mono">{p.summary}</span>
                {!p.valid && <div className="m err-text">{p.invalidReason}</div>}
                {p.rationale && <div className="m">{p.rationale}</div>}
              </div>
              <span className="conf">{Math.round(p.confidence * 100)}%</span>
              {p.status === 'pending' && p.valid && (
                <div className="chip-row">
                  <button className="sm primary" onClick={() => approve([p.id])}>✓</button>
                  <button className="sm danger" onClick={() => reject([p.id])}>✕</button>
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
