// AIP Assist — a sidebar you can open from any tab.
//
// The same question gets a different answer depending on where you are: the
// current tab (and the source you have open) is sent along as session context
// and re-ranks retrieval server-side. Answers carry numbered citations back to
// the metadata chunk they came from, and each citation can jump you to the tab
// that chunk is about.
//
// It reads metadata only — never your data. That boundary is stated in the UI
// rather than buried, because a sidebar that silently could read row values
// would be a very different thing to leave open.

import { useEffect, useRef, useState } from 'react'
import { api } from './api'
import type { AssistResult } from './api'

type Props = {
  pid: number
  tab: string
  /** Logical name of the source the user has selected, when there is one. */
  source?: string
  open: boolean
  onClose: () => void
  onGoTo: (tab: string) => void
}

type Turn = { question: string; result?: AssistResult; error?: string }

const STARTERS: Record<string, string[]> = {
  studio: ['Which file formats can I load?', 'What does Auto-build actually do?'],
  sources: ['Which columns are candidate keys?', 'What does the profiled role of a column mean?'],
  tbox: ['What classes are defined in this project?', 'When should I use a reification class?'],
  mapping: ['How does my mapping mint subject IRIs?', 'What object forms does the DSL support?'],
  explore: ['Which predicates does my data actually use?', 'How is the default graph put together?'],
  competency: ['What are competency questions for?', 'Which of my questions have no SPARQL yet?'],
  validate: ['Which SHACL constraints are set?', 'Why is validation closed-world?'],
  governance: ['Where did my triples come from?', 'What does materialization add?'],
}

export function AssistSidebar({ pid, tab, source, open, onClose, onGoTo }: Props) {
  const [question, setQuestion] = useState('')
  const [turns, setTurns] = useState<Turn[]>([])
  const [busy, setBusy] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (open) inputRef.current?.focus()
  }, [open])

  // A new project is a new metadata universe — do not carry answers across.
  useEffect(() => {
    setTurns([])
  }, [pid])

  useEffect(() => {
    bodyRef.current?.scrollTo({ top: bodyRef.current.scrollHeight, behavior: 'smooth' })
  }, [turns, busy])

  async function ask(q: string) {
    const text = q.trim()
    if (!text || busy) return
    setQuestion('')
    setBusy(true)
    const turn: Turn = { question: text }
    setTurns((t) => [...t, turn])
    try {
      const result = await api.assist(pid, text, { tab, source })
      setTurns((t) => t.map((x) => (x === turn ? { ...x, result } : x)))
    } catch (e) {
      setTurns((t) => t.map((x) => (x === turn ? { ...x, error: (e as Error).message } : x)))
    } finally {
      setBusy(false)
    }
  }

  if (!open) return null
  const starters = STARTERS[tab] ?? STARTERS.studio

  return (
    <aside className="assist" aria-label="AIP Assist">
      <header>
        <div style={{ flex: 1 }}>
          <b>◆ AIP Assist</b>
          <div className="m">
            reading <span className="mono">{tab}</span>
            {source ? (
              <>
                {' · '}
                <span className="mono">{source}</span>
              </>
            ) : null}
          </div>
        </div>
        <button className="ghost sm" onClick={onClose} title="Close">
          ✕
        </button>
      </header>

      <div className="assist-body" ref={bodyRef}>
        {turns.length === 0 && (
          <>
            <div className="notice">
              Ask about the platform or about this project's <b>metadata</b> — sources, columns, classes,
              properties, the mapping, constraints, lineage. Answers change with the tab you are on.
            </div>
            <p className="hint" style={{ margin: '12px 0 6px' }}>
              Assist cannot see your data — no cell values, no samples. For counts and values use{' '}
              <b>Ask the graph</b> on the Studio tab.
            </p>
            <div className="chip-row">
              {starters.map((s) => (
                <button key={s} className="sm" onClick={() => ask(s)}>
                  {s}
                </button>
              ))}
            </div>
          </>
        )}

        {turns.map((t, i) => (
          <div key={i} className="turn">
            <div className="q">{t.question}</div>
            {t.error && <div className="notice warn">{t.error}</div>}
            {!t.result && !t.error && <div className="m">thinking…</div>}
            {t.result && (
              <>
                {t.result.dataQuestion && (
                  <div className="notice warn">
                    That asks for actual values. Assist only indexes metadata — use <b>Ask the graph</b> on
                    Studio, which queries the data.
                  </div>
                )}
                <div className="a">{t.result.answer}</div>
                {t.result.citations.length > 0 && (
                  <div className="cites">
                    {t.result.citations.map((c) => (
                      <button
                        key={c.n}
                        className="cite"
                        title={`${c.kind} · ${c.reason}`}
                        onClick={() => c.tab && onGoTo(c.tab)}
                      >
                        <span className="n">{c.n}</span> {c.title}
                      </button>
                    ))}
                  </div>
                )}
                <div className="m" style={{ marginTop: 6 }}>
                  {t.result.context}
                  {t.result.model ? ` · ${t.result.model}` : ''}
                </div>
              </>
            )}
          </div>
        ))}
      </div>

      <footer>
        <input
          ref={inputRef}
          value={question}
          placeholder="Ask about this project or the platform…"
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') ask(question)
          }}
        />
        <button className="ai" disabled={busy || !question.trim()} onClick={() => ask(question)}>
          {busy ? '…' : '↵'}
        </button>
      </footer>
    </aside>
  )
}
