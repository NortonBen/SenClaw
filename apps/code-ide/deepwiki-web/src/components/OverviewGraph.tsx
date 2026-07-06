import { useState, type MouseEvent } from 'react';
import { Button, Spin, Tag, Typography, theme } from 'antd';
import { CloseOutlined, PartitionOutlined } from '@ant-design/icons';
import { api, type InvestigationGraph } from '../api';
import { CodeBlock } from './CodeBlock';
import { kindColor, langFromPath, truncate } from '../lib';

const { Text } = Typography;

interface Props {
  graph: InvestigationGraph;
  isDark: boolean;
  onPick: (name: string) => void;
}

type Snip = { path: string; start_line: number; end_line: number; code: string; name?: string; kind?: string; signature?: string };

const NODE_W = 176;
const NODE_H = 46;
const COL_GAP = 196; // horizontal spacing between nodes within a depth level
const ROW_GAP = 116; // vertical spacing between depth levels (top → down)
const PAD = 28;
const CAP_W = 78; // left gutter for the per-level captions

/** Vertical bezier: bottom of the upper node → top of the lower node. */
function edge(x1: number, y1: number, x2: number, y2: number): string {
  const my = (y1 + y2) / 2;
  return `M${x1},${y1} C${x1},${my} ${x2},${my} ${x2},${y2}`;
}

function base(path: string): string {
  return path === '<external>' ? 'external' : path.split('/').pop() ?? path;
}

/**
 * Overview graph: a multi-hop call-flow subgraph laid out in columns by relative
 * depth. Clicking a node opens a popover with its source code (linked to that
 * point in the graph) + a button to re-centre the graph there.
 */
export function OverviewGraph({ graph, isDark, onPick }: Props) {
  const { token } = theme.useToken();
  const [popup, setPopup] = useState<{ name: string; x: number; y: number } | null>(null);
  const [snip, setSnip] = useState<Snip | null>(null);
  const [loadingSnip, setLoadingSnip] = useState(false);

  const nodes = graph?.nodes ?? [];
  if (nodes.length < 2) return null;

  // Top → down: callers (most-negative depth) at the top, focus in the middle,
  // callees flowing downward. Each depth is a horizontal row.
  const depths = [...new Set(nodes.map((n) => n.depth))].sort((a, b) => a - b);
  const rowOf = new Map(depths.map((d, i) => [d, i]));
  const rows = depths.length;
  const maxCols = Math.max(...depths.map((d) => nodes.filter((n) => n.depth === d).length));

  const W = CAP_W + PAD * 2 + (maxCols - 1) * COL_GAP + NODE_W;
  const H = PAD * 2 + (rows - 1) * ROW_GAP + NODE_H;
  const midX = CAP_W + (W - CAP_W) / 2;

  type P = { x: number; y: number; external: boolean; kind: string; path: string; line: number; depth: number };
  const pos = new Map<string, P>();
  for (const d of depths) {
    const y = PAD + rowOf.get(d)! * ROW_GAP + NODE_H / 2;
    const group = nodes.filter((n) => n.depth === d);
    group.forEach((n, i) => {
      const x = midX - ((group.length - 1) * COL_GAP) / 2 + i * COL_GAP;
      pos.set(n.id, { x, y, external: n.external, kind: n.kind, path: n.path, line: n.line, depth: n.depth });
    });
  }

  const caption = (d: number) => (d < 0 ? `callers ${-d}` : d === 0 ? 'focus' : `callees ${d}`);

  const openCode = (name: string, e: MouseEvent) => {
    e.stopPropagation();
    const x = Math.min(e.clientX + 8, window.innerWidth - 460);
    const y = Math.min(e.clientY + 8, window.innerHeight - 380);
    setPopup({ name, x: Math.max(8, x), y: Math.max(8, y) });
    setSnip(null);
    setLoadingSnip(true);
    api.snippet({ name, context: 2 })
      .then((s) => setSnip(s as Snip))
      .catch(() => setSnip(null))
      .finally(() => setLoadingSnip(false));
  };
  const close = () => { setPopup(null); setSnip(null); };

  return (
    <>
      <div style={{ overflow: 'auto', maxHeight: 600 }}>
      <svg viewBox={`0 0 ${W} ${H}`} width={W} height={H} style={{ display: 'block', maxWidth: 'none' }}>
        <defs>
          <marker id="ovarrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
            <path d="M0,0 L10,5 L0,10 z" fill={token.colorTextTertiary} />
          </marker>
        </defs>

        {depths.map((d) => (
          <text key={`cap${d}`} x={CAP_W - 10} y={PAD + rowOf.get(d)! * ROW_GAP + NODE_H / 2 + 4}
            textAnchor="end" fontSize={11} fill={token.colorTextTertiary}>
            {caption(d)}
          </text>
        ))}

        {graph.edges.map((e, i) => {
          const a = pos.get(e.from);
          const b = pos.get(e.to);
          if (!a || !b) return null;
          const down = b.y >= a.y;
          const y1 = a.y + (down ? NODE_H / 2 : -NODE_H / 2);
          const y2 = b.y + (down ? -NODE_H / 2 - 5 : NODE_H / 2 + 5);
          return <path key={i} d={edge(a.x, y1, b.x, y2)} fill="none" stroke={token.colorTextQuaternary} strokeWidth={1.4} markerEnd="url(#ovarrow)" />;
        })}

        {[...pos.entries()].map(([id, p]) => {
          const center = p.depth === 0;
          const accent = p.external ? token.colorTextTertiary : kindColor(p.kind);
          const clickable = !p.external;
          return (
            <g key={id}
              transform={`translate(${p.x - NODE_W / 2}, ${p.y - NODE_H / 2})`}
              style={{ cursor: clickable ? 'pointer' : 'default' }}
              onClick={(e) => clickable && openCode(id, e)}
            >
              <title>{p.external ? `${id} (external)` : `${id} — ${p.path}:${p.line} (bấm để xem code)`}</title>
              <rect width={NODE_W} height={NODE_H} rx={7}
                fill={center ? token.colorPrimaryBg : token.colorFillTertiary}
                stroke={popup?.name === id ? token.colorPrimary : center ? token.colorPrimary : token.colorBorderSecondary}
                strokeWidth={popup?.name === id || center ? 2 : 1} />
              <rect width={4} height={NODE_H} rx={2} fill={accent} />
              <rect x={NODE_W - 12 - kindLabel(p.kind, p.external).length * 6.4} y={6}
                width={kindLabel(p.kind, p.external).length * 6.4 + 8} height={14} rx={7}
                fill={token.colorFillSecondary} />
              <text x={NODE_W - 8} y={16} textAnchor="end" fontSize={9.5} fill={token.colorTextSecondary}>
                {kindLabel(p.kind, p.external)}
              </text>
              <text x={12} y={21} fontSize={13} fontWeight={center ? 700 : 600}
                fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
                fill={p.external ? token.colorTextTertiary : token.colorText}>
                {truncate(id, 18)}
              </text>
              <text x={12} y={36} fontSize={10.5} fill={token.colorTextTertiary}
                fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace">
                {p.external ? 'external' : `${truncate(base(p.path), 18)}:${p.line}`}
              </text>
            </g>
          );
        })}
      </svg>
      </div>

      {popup ? (
        <>
          <div onClick={close} style={{ position: 'fixed', inset: 0, zIndex: 1000 }} />
          <div
            style={{
              position: 'fixed', left: popup.x, top: popup.y, zIndex: 1001, width: 440, maxWidth: '92vw',
              background: token.colorBgElevated, border: `1px solid ${token.colorBorder}`,
              borderRadius: 10, boxShadow: token.boxShadowSecondary, overflow: 'hidden',
            }}
            onClick={(e) => e.stopPropagation()}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 10px', borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
              <Text strong style={{ fontFamily: 'ui-monospace, monospace', fontSize: 13 }}>{popup.name}</Text>
              {snip?.kind ? <Tag color="purple" style={{ margin: 0 }}>{snip.kind}</Tag> : null}
              <Text type="secondary" style={{ fontSize: 11, marginLeft: 'auto', fontFamily: 'ui-monospace, monospace' }}>
                {snip ? `${snip.path}:${snip.start_line}` : ''}
              </Text>
              <CloseOutlined onClick={close} style={{ cursor: 'pointer', color: token.colorTextTertiary }} />
            </div>
            <div style={{ maxHeight: 300, overflow: 'auto', padding: 6 }}>
              {loadingSnip ? (
                <div style={{ textAlign: 'center', padding: 24 }}><Spin /></div>
              ) : snip?.code ? (
                <CodeBlock code={snip.code} lang={langFromPath(snip.path)} startLine={snip.start_line} isDark={isDark} />
              ) : (
                <Text type="secondary" style={{ fontSize: 12, padding: 12, display: 'block' }}>Không lấy được mã nguồn (có thể là symbol ngoài).</Text>
              )}
            </div>
            <div style={{ padding: '8px 10px', borderTop: `1px solid ${token.colorBorderSecondary}`, textAlign: 'right' }}>
              <Button size="small" icon={<PartitionOutlined />} onClick={() => { onPick(popup.name); close(); }}>
                Đặt làm tâm graph
              </Button>
            </div>
          </div>
        </>
      ) : null}
    </>
  );
}

function kindLabel(kind: string, external: boolean): string {
  if (external) return 'ext';
  return kind.length > 9 ? kind.slice(0, 9) : kind;
}
