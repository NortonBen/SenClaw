import { useMemo, useState, type MouseEvent } from 'react';
import { List, Spin, Tag, Typography, theme } from 'antd';
import { CloseOutlined, FileTextOutlined } from '@ant-design/icons';
import { api, type FileGraphData, type Sym } from '../api';
import { dirColor, forceLayout } from '../lib';

const { Text } = Typography;

interface Props {
  graph: FileGraphData;
  onPickSymbol: (name: string) => void;
}

const W = 1200;
const H = 760;

function base(p: string): string {
  return p.split('/').pop() ?? p;
}

/**
 * Whole-codebase graph: every file is a node (sized by symbol count, coloured by
 * directory), edges are cross-file calls. Click a file to see its symbols, then
 * click a symbol to drill into its per-symbol call graph.
 */
export function FileGraph({ graph, onPickSymbol }: Props) {
  const { token } = theme.useToken();
  const nodes = graph?.nodes ?? [];
  const edges = graph?.edges ?? [];
  const pos = useMemo(() => forceLayout(nodes.map((n) => n.path), edges), [nodes, edges]);

  const [popup, setPopup] = useState<{ path: string; x: number; y: number } | null>(null);
  const [syms, setSyms] = useState<Sym[] | null>(null);
  const [loading, setLoading] = useState(false);

  if (nodes.length === 0) return null;
  const maxW = Math.max(1, ...edges.map((e) => e.weight));

  const openFile = (path: string, e: MouseEvent) => {
    e.stopPropagation();
    const x = Math.min(e.clientX + 8, window.innerWidth - 380);
    const y = Math.min(e.clientY + 8, window.innerHeight - 360);
    setPopup({ path, x: Math.max(8, x), y: Math.max(8, y) });
    setSyms(null);
    setLoading(true);
    api.fileOutline(path)
      .then((d) => setSyms(d.outline))
      .catch(() => setSyms([]))
      .finally(() => setLoading(false));
  };
  const close = () => { setPopup(null); setSyms(null); };

  return (
    <>
      <div style={{ overflow: 'auto', maxHeight: 640 }}>
        <svg viewBox={`0 0 ${W} ${H}`} width={W} height={H} style={{ display: 'block', maxWidth: 'none' }}>
          <defs>
            <marker id="fgarrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
              <path d="M0,0 L10,5 L0,10 z" fill={token.colorTextQuaternary} />
            </marker>
          </defs>

          {edges.map((e, i) => {
            const a = pos.get(e.from), b = pos.get(e.to);
            if (!a || !b) return null;
            const dx = b.x - a.x, dy = b.y - a.y;
            const d = Math.hypot(dx, dy) || 1;
            const ra = 8 + Math.sqrt((nodes.find((n) => n.path === e.from)?.symbols ?? 0)) * 2.6;
            const rb = 8 + Math.sqrt((nodes.find((n) => n.path === e.to)?.symbols ?? 0)) * 2.6;
            return (
              <line key={i}
                x1={a.x + (dx / d) * ra} y1={a.y + (dy / d) * ra}
                x2={b.x - (dx / d) * (rb + 4)} y2={b.y - (dy / d) * (rb + 4)}
                stroke={token.colorTextQuaternary} strokeWidth={0.6 + (e.weight / maxW) * 2.4}
                strokeOpacity={0.55} markerEnd="url(#fgarrow)" />
            );
          })}

          {nodes.map((n) => {
            const p = pos.get(n.path);
            if (!p) return null;
            const r = 8 + Math.sqrt(n.symbols) * 2.6;
            const sel = popup?.path === n.path;
            return (
              <g key={n.path} transform={`translate(${p.x},${p.y})`} style={{ cursor: 'pointer' }} onClick={(e) => openFile(n.path, e)}>
                <title>{`${n.path} · ${n.symbols} symbols · ${n.loc} dòng`}</title>
                <circle r={r} fill={dirColor(n.path)} fillOpacity={sel ? 1 : 0.82}
                  stroke={sel ? token.colorText : token.colorBgContainer} strokeWidth={sel ? 2 : 1.5} />
                <text y={r + 13} textAnchor="middle" fontSize={11.5}
                  fontFamily="ui-monospace, SFMono-Regular, Menlo, monospace"
                  fill={token.colorText}>{base(n.path)}</text>
                <text y={r + 25} textAnchor="middle" fontSize={9.5} fill={token.colorTextTertiary}>
                  {n.symbols} sym
                </text>
              </g>
            );
          })}
        </svg>
      </div>

      {popup ? (
        <>
          <div onClick={close} style={{ position: 'fixed', inset: 0, zIndex: 1000 }} />
          <div onClick={(e) => e.stopPropagation()}
            style={{
              position: 'fixed', left: popup.x, top: popup.y, zIndex: 1001, width: 360, maxWidth: '92vw',
              background: token.colorBgElevated, border: `1px solid ${token.colorBorder}`,
              borderRadius: 10, boxShadow: token.boxShadowSecondary, overflow: 'hidden',
            }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '8px 10px', borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
              <FileTextOutlined style={{ color: token.colorPrimary }} />
              <Text strong style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12.5, flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{popup.path}</Text>
              <CloseOutlined onClick={close} style={{ cursor: 'pointer', color: token.colorTextTertiary }} />
            </div>
            <div style={{ maxHeight: 300, overflow: 'auto' }}>
              {loading ? (
                <div style={{ textAlign: 'center', padding: 24 }}><Spin /></div>
              ) : syms && syms.length ? (
                <List
                  size="small"
                  dataSource={syms}
                  renderItem={(sym) => (
                    <List.Item style={{ cursor: 'pointer', paddingInline: 12 }} onClick={() => { onPickSymbol(sym.name); close(); }}>
                      <div style={{ width: '100%' }}>
                        <Text strong style={{ fontSize: 13 }}>{sym.name}</Text> <Tag style={{ margin: 0 }}>{sym.kind}</Tag>
                        <Text type="secondary" style={{ fontSize: 11, marginLeft: 6 }}>:{sym.start_line}</Text>
                      </div>
                    </List.Item>
                  )}
                />
              ) : (
                <Text type="secondary" style={{ fontSize: 12, padding: 12, display: 'block' }}>Không có symbol.</Text>
              )}
            </div>
          </div>
        </>
      ) : null}
    </>
  );
}
