import type { Claim, Contradiction, Evidence } from './api'

/**
 * Claims with confidence chips.
 *
 * Two things this component must never do: present a `disputed` claim as if it
 * were settled, and let "confidence" read as "probability of being true". The
 * note is rendered inline rather than tucked into a tooltip for that reason.
 */
export default function Claims({
  claims,
  contradictions,
  evidence,
  note,
}: {
  claims: Claim[]
  contradictions: Contradiction[]
  evidence: Evidence[]
  note?: string
}) {
  const byId = new Map(evidence.map((e, i) => [e.id, { e, n: i + 1 }]))
  const claimText = (id: string) => claims.find((c) => c.id === id)?.text ?? id

  const cite = (ids: string[]) =>
    ids
      .map((id) => byId.get(id))
      .filter((x): x is { e: Evidence; n: number } => !!x)
      .map(({ e, n }) => (
        <a
          key={e.id}
          className="cite"
          href={e.url ?? undefined}
          target="_blank"
          rel="noreferrer"
          title={`${e.title}${e.domain ? ` · ${e.domain}` : ''}`}
        >
          [{n}]
        </a>
      ))

  return (
    <div className="panel">
      <h2>Khẳng định rút ra</h2>

      {contradictions.length > 0 && (
        // Contradictions come FIRST and are never resolved by picking a side —
        // naming a disagreement is more useful than hiding it.
        <div className="contradictions">
          <h3>⚠ Các nguồn mâu thuẫn nhau</h3>
          {contradictions.map((c) => (
            <div className="tmpl" key={c.id}>
              <div>{c.summary}</div>
              <div className="why">· {claimText(c.claim_a)}</div>
              <div className="why">· {claimText(c.claim_b)}</div>
            </div>
          ))}
        </div>
      )}

      {claims.map((c) => (
        <div className="claim" key={c.id}>
          <div className="claim-head">
            <span className={`tier ${c.tier}`}>{c.tier_label}</span>
            {c.high_stakes && (
              <span className="tag" title="Sai ở loại khẳng định này thì tốn kém — số liệu, pháp lý, y tế, tài chính hoặc quy kết phát ngôn">
                hệ trọng
              </span>
            )}
            <span className="why">
              {c.independent_count} nguồn độc lập · đồng thuận{' '}
              {(c.agreement * 100).toFixed(0)}%
            </span>
          </div>
          <div className="claim-text">{c.text}</div>
          <div className="prov">
            {c.supports.length > 0 && <span>ủng hộ {cite(c.supports)}</span>}
            {c.refutes.length > 0 && <span>· phản bác {cite(c.refutes)}</span>}
            {c.dropped_citations.length > 0 && (
              // A model citing evidence that does not exist must leave a trace,
              // not quietly look better-sourced than it is.
              <span className="dropped" title={c.dropped_citations.join(', ')}>
                · {c.dropped_citations.length} trích dẫn không tồn tại đã bị bỏ
              </span>
            )}
          </div>
        </div>
      ))}

      {note && <p className="why" style={{ marginTop: 12 }}>{note}</p>}
    </div>
  )
}
