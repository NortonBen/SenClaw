import { useEffect, useMemo, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { api, type Codemap, type DwEdge } from '../api';
import { dirColor, kindColor, timeAgo } from '../lib';

/* ---------------- Sidebar: list + new ---------------- */
export function CodemapSidebar({ codemaps, activeId, busy, onGenerate, onSelect, onDelete }: {
  codemaps: Codemap[];
  activeId: string | null;
  busy: boolean;
  onGenerate: (start: string) => void;
  onSelect: (cm: Codemap) => void;
  onDelete: (id: string) => void;
}) {
  const [start, setStart] = useState('');
  function go() { const s = start.trim(); if (s && !busy) { onGenerate(s); setStart(''); } }
  return (
    <div className="cm-side">
      <div className="cm-side-title">🗺 Codemaps</div>
      <div className="cm-new">
        <input
          value={start}
          onChange={(e) => setStart(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter') go(); }}
          placeholder="Điểm bắt đầu cho codemap mới…"
        />
        <button className="btn" onClick={go} disabled={busy || !start.trim()}>{busy ? '…' : 'Tạo'}</button>
      </div>
      <div className="cm-list">
        <div className="cm-list-head">CODEMAP CỦA BẠN</div>
        {codemaps.length === 0 && <div className="cm-empty">Chưa có codemap. Nhập điểm bắt đầu ở trên để agent điều tra codebase.</div>}
        {busy && <div className="cm-item generating"><span className="spin">◐</span> Đang tạo codemap… <span className="cm-exploring">Đang khám phá</span></div>}
        {codemaps.map((cm) => (
          <div key={cm.id} className={`cm-item${activeId === cm.id ? ' active' : ''}`} onClick={() => onSelect(cm)}>
            <div className="cm-item-title">{cm.title}</div>
            <div className="cm-item-desc">{cm.narrative.replace(/[#*`>]/g, '').slice(0, 90)}…</div>
            <div className="cm-item-meta">
              <span>{timeAgo(Math.floor(cm.at / 1000))} · {cm.focus ?? cm.start}</span>
              <button className="cm-del" onClick={(e) => { e.stopPropagation(); onDelete(cm.id); }} title="Xoá">🗑</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ---------------- Main: generating / walkthrough ---------------- */
export function CodemapMain({ codemap, busy, busyStart, onOpenFile, onGenerate }: {
  codemap: Codemap | null;
  busy: boolean;
  busyStart: string;
  onOpenFile: (path: string, line?: number) => void;
  onGenerate: (start: string) => void;
}) {
  const [mode, setMode] = useState<'map' | 'graph'>('map');
  const header = (
    <div className="cm-modebar">
      <div className="cm-modes">
        <button className={mode === 'map' ? 'active' : ''} onClick={() => setMode('map')}>🗺 Bản đồ</button>
        <button className={mode === 'graph' ? 'active' : ''} onClick={() => setMode('graph')}>🕸 Graph toàn repo</button>
      </div>
    </div>
  );
  if (mode === 'graph') {
    // "Call-graph chi tiết" from a node → generate a codemap and switch to Bản đồ.
    const onCodemap = (name: string) => { setMode('map'); onGenerate(name); };
    return <div className="cm-main">{header}<GraphView onOpenFile={onOpenFile} onCodemap={onCodemap} /></div>;
  }
  return <CodemapBody codemap={codemap} busy={busy} busyStart={busyStart} onOpenFile={onOpenFile} header={header} />;
}

function CodemapBody({ codemap, busy, busyStart, onOpenFile, header }: {
  codemap: Codemap | null; busy: boolean; busyStart: string;
  onOpenFile: (path: string, line?: number) => void; header: React.ReactNode;
}) {
  if (busy) {
    return (
      <div className="cm-main">
        {header}
        <div className="cm-state">
          <div className="cm-spinner"><span className="spin">◐</span></div>
          <div className="cm-loading-title">Đang tạo codemap cho “{busyStart}”</div>
          <div className="cm-loading-steps">
            <div>• Điều tra call-graph quanh điểm bắt đầu…</div>
            <div>• Đọc các file liên quan…</div>
            <div>• Viết walkthrough có trích nguồn…</div>
          </div>
        </div>
      </div>
    );
  }
  if (!codemap) {
    return (
      <div className="cm-main">
        {header}
        <div className="cm-state">
          <div style={{ fontSize: 44, opacity: 0.35 }}>🗺</div>
          <p>Codemap là bản đồ dẫn đường qua codebase từ một điểm bắt đầu —<br />agent lần theo call-graph, đọc code, và viết walkthrough có trích nguồn.</p>
          <p style={{ color: 'var(--fg-mute)' }}>Nhập điểm bắt đầu ở thanh bên để tạo, hoặc xem <b>🕸 Graph toàn repo</b>.</p>
        </div>
      </div>
    );
  }

  // Group nodes into caller / focus / callee lanes.
  const byDepth = new Map<number, string[]>();
  codemap.nodes.forEach((n) => { const a = byDepth.get(n.depth) ?? []; a.push(n.id); byDepth.set(n.depth, a); });
  const depths = [...byDepth.keys()].sort((a, b) => a - b);
  const locOf = (id: string) => codemap.matches.find((m) => m.name === id || id.endsWith(`::${m.name}`));
  const label = (d: number) => (d < 0 ? `Callers L${-d}` : d === 0 ? 'Điểm bắt đầu' : `Callees L${d}`);

  return (
    <div className="cm-main">
      {header}
      <div className="cm-header">
        <h1>{codemap.title}</h1>
        <div className="cm-sub">bắt đầu từ <b>{codemap.focus ?? codemap.start}</b></div>
      </div>

      {depths.length > 0 && (
        <div className="cm-flow">
          {depths.map((d) => (
            <div key={d} className="cm-lane">
              <div className="cm-lane-label">{label(d)}</div>
              <div className="cm-lane-nodes">
                {(byDepth.get(d) ?? []).map((id, i) => {
                  const loc = locOf(id);
                  return (
                    <button key={i} className={`cm-node${loc ? '' : ' ext'}`} disabled={!loc}
                      onClick={() => loc && onOpenFile(loc.path, loc.start_line)}
                      title={loc ? `${loc.path}:${loc.start_line}` : 'ngoài repo'}>{id}</button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}

      <div className="cm-narrative md">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{codemap.narrative}</ReactMarkdown>
      </div>

      {codemap.matches.length > 0 && (
        <div className="cm-files">
          <div className="cm-files-head">FILE LIÊN QUAN</div>
          {codemap.matches.slice(0, 16).map((m, i) => (
            <div key={i} className="cm-file" onClick={() => onOpenFile(m.path, m.start_line)}>
              <span className="cm-file-kind">{m.kind}</span>
              <span className="cm-file-name">{m.name}</span>
              <span className="cm-file-loc">{m.path}:{m.start_line}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/* ---------------- Whole-repo graph (file / symbol), searchable ---------------- */
interface GNode { id: string; label: string; path: string; line?: number; sub?: string; sym?: number; kind?: string }

/** Deterministic force layout (no animation) → node-id → {x,y}. */
function layout(ids: string[], edges: DwEdge[], W: number, H: number) {
  const N = ids.length;
  const pos = ids.map((_, i) => ({
    x: W / 2 + Math.cos((i / Math.max(N, 1)) * 2 * Math.PI) * Math.min(W, H) * 0.36 + (i % 5),
    y: H / 2 + Math.sin((i / Math.max(N, 1)) * 2 * Math.PI) * Math.min(W, H) * 0.36 + (i % 3),
  }));
  const idx = new Map(ids.map((id, i) => [id, i]));
  const k = Math.sqrt((W * H) / Math.max(N, 1)) * 0.9;
  for (let it = 0; it < 260; it++) {
    const temp = (1 - it / 260) * (W * 0.05);
    const disp = pos.map(() => ({ x: 0, y: 0 }));
    for (let i = 0; i < N; i++) for (let j = i + 1; j < N; j++) {
      const dx = pos[i].x - pos[j].x, dy = pos[i].y - pos[j].y; const d = Math.hypot(dx, dy) || 0.01;
      const f = (k * k) / d; disp[i].x += (dx / d) * f; disp[i].y += (dy / d) * f; disp[j].x -= (dx / d) * f; disp[j].y -= (dy / d) * f;
    }
    for (const e of edges) {
      const a = idx.get(e.from), b = idx.get(e.to); if (a == null || b == null) continue;
      const dx = pos[a].x - pos[b].x, dy = pos[a].y - pos[b].y; const d = Math.hypot(dx, dy) || 0.01;
      const f = (d * d) / k; disp[a].x -= (dx / d) * f; disp[a].y -= (dy / d) * f; disp[b].x += (dx / d) * f; disp[b].y += (dy / d) * f;
    }
    for (let i = 0; i < N; i++) {
      disp[i].x += (W / 2 - pos[i].x) * 0.02 * k; disp[i].y += (H / 2 - pos[i].y) * 0.02 * k;
      const dl = Math.hypot(disp[i].x, disp[i].y) || 0.01;
      pos[i].x += (disp[i].x / dl) * Math.min(dl, temp); pos[i].y += (disp[i].y / dl) * Math.min(dl, temp);
      pos[i].x = Math.max(30, Math.min(W - 30, pos[i].x)); pos[i].y = Math.max(30, Math.min(H - 30, pos[i].y));
    }
  }
  return new Map(ids.map((id, i) => [id, pos[i]]));
}

interface OutlineSym { name: string; kind: string; start_line: number }
function GraphView({ onOpenFile, onCodemap }: {
  onOpenFile: (path: string, line?: number) => void;
  onCodemap: (name: string) => void;
}) {
  const [kind, setKind] = useState<'file' | 'function' | 'symbol'>('file');
  const [nodes, setNodes] = useState<GNode[]>([]);
  const [edges, setEdges] = useState<DwEdge[]>([]);
  const [q, setQ] = useState('');
  const [busy, setBusy] = useState(true);
  const [sel, setSel] = useState<GNode | null>(null);
  const [snip, setSnip] = useState<string | null>(null);
  const [outline, setOutline] = useState<OutlineSym[] | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [folder, setFolder] = useState('');

  useEffect(() => {
    setBusy(true); setSel(null); setFocusId(null); setFolder('');
    const load = kind === 'file' ? api.dw.fileGraph() : api.dw.symbolGraph();
    load.then((g) => {
      if (kind === 'file') {
        setNodes((g.nodes as { path: string; loc: number; symbols: number }[]).map((n) => ({ id: n.path, label: n.path.split('/').pop() ?? n.path, path: n.path, sym: n.symbols, sub: `${n.symbols} sym` })));
        setEdges(g.edges);
      } else {
        let sn = g.nodes as { name: string; kind: string; path: string; line: number }[];
        if (kind === 'function') sn = sn.filter((n) => n.kind === 'function' || n.kind === 'method');
        const ids = new Set(sn.map((n) => n.name));
        setNodes(sn.map((n) => ({ id: n.name, label: n.name, path: n.path, line: n.line, kind: n.kind, sub: n.kind })));
        setEdges(g.edges.filter((e) => ids.has(e.from) && ids.has(e.to)));
      }
    }).catch(() => { setNodes([]); setEdges([]); }).finally(() => setBusy(false));
  }, [kind]);

  function selectNode(n: GNode) {
    setSel(n); setSnip(null); setOutline(null);
    if (kind === 'file') {
      // Show the file's symbol outline (like DeepWiki's file-node popup).
      api.dw.fileOutline(n.path).then((r) => setOutline(r.outline ?? [])).catch(() => setOutline([]));
    } else {
      api.dw.snippet(n.label).then((s) => setSnip(s.code ?? null)).catch(() => setSnip(null));
    }
  }

  const W = 900, H = 620;
  const svgRef = useRef<SVGSVGElement>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const [vb, setVb] = useState({ x: 0, y: 0, w: W, h: H });
  const drag = useRef<{ x: number; y: number; vx: number; vy: number } | null>(null);

  // Reset the viewport when the graph data changes.
  useEffect(() => { setVb({ x: 0, y: 0, w: W, h: H }); }, [kind, focusId, nodes.length]);

  // Non-passive wheel → zoom toward the cursor (page won't scroll).
  useEffect(() => {
    const el = canvasRef.current; if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const svg = svgRef.current; if (!svg) return;
      const r = svg.getBoundingClientRect();
      const mx = (e.clientX - r.left) / r.width, my = (e.clientY - r.top) / r.height;
      const f = e.deltaY > 0 ? 1.12 : 0.893;
      setVb((v) => {
        const nw = Math.max(120, Math.min(W * 3.5, v.w * f)); const nh = nw * (H / W);
        return { x: v.x + mx * v.w - mx * nw, y: v.y + my * v.h - my * nh, w: nw, h: nh };
      });
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  function onDown(e: React.MouseEvent) { drag.current = { x: e.clientX, y: e.clientY, vx: vb.x, vy: vb.y }; }
  function onMove(e: React.MouseEvent) {
    if (!drag.current) return;
    const svg = svgRef.current; if (!svg) return;
    const r = svg.getBoundingClientRect();
    const dx = ((e.clientX - drag.current.x) / r.width) * vb.w;
    const dy = ((e.clientY - drag.current.y) / r.height) * vb.h;
    setVb((v) => ({ ...v, x: drag.current!.vx - dx, y: drag.current!.vy - dy }));
  }
  function onUp() { drag.current = null; }
  function zoom(f: number) { setVb((v) => { const nw = Math.max(120, Math.min(W * 3.5, v.w * f)); return { x: v.x + (v.w - nw) / 2, y: v.y + (v.h - nw * (H / W)) / 2, w: nw, h: nw * (H / W) }; }); }

  // Distinct folders (from node paths) for the path filter.
  const folders = useMemo(() => {
    const set = new Set<string>();
    nodes.forEach((n) => { const d = n.path.split('/').slice(0, -1).join('/'); if (d) set.add(d); });
    return [...set].sort();
  }, [nodes]);

  const view = useMemo(() => {
    let ns = nodes, es = edges;
    if (folder) {
      ns = ns.filter((n) => n.path === folder || n.path.startsWith(folder + '/'));
      const ids = new Set(ns.map((n) => n.id));
      es = es.filter((e) => ids.has(e.from) && ids.has(e.to));
    }
    if (focusId) {
      const near = new Set<string>([focusId]);
      es.forEach((e) => { if (e.from === focusId) near.add(e.to); if (e.to === focusId) near.add(e.from); });
      ns = ns.filter((n) => near.has(n.id));
      es = es.filter((e) => near.has(e.from) && near.has(e.to));
    }
    const deg = new Map<string, number>();
    es.forEach((e) => { deg.set(e.from, (deg.get(e.from) ?? 0) + 1); deg.set(e.to, (deg.get(e.to) ?? 0) + 1); });
    const top = [...ns].sort((a, b) => (deg.get(b.id) ?? 0) - (deg.get(a.id) ?? 0)).slice(0, focusId ? 40 : 70);
    const ids = new Set(top.map((n) => n.id));
    const es2 = es.filter((e) => ids.has(e.from) && ids.has(e.to));
    const pos = layout(top.map((n) => n.id), es2, W, H);
    return { top, es: es2, pos };
  }, [nodes, edges, focusId, folder]);

  const match = (n: GNode) => q.trim().length > 0 && (n.label.toLowerCase().includes(q.toLowerCase()) || n.path.toLowerCase().includes(q.toLowerCase()));
  // Colour by directory (file graph) or by symbol kind (function/symbol graph);
  // size file nodes by symbol count — matching the DeepWiki graph.
  const nodeColor = (n?: GNode) => (!n ? '#64748b' : kind === 'file' ? dirColor(n.path) : kindColor(n.kind ?? ''));
  const nodeR = (n?: GNode) => (!n ? 6 : kind === 'file' ? 8 + Math.sqrt(n.sym ?? 0) * 2.5 : 6.5);

  return (
    <div className="gv">
      <div className="gv-bar">
        <div className="gv-kind">
          <button className={kind === 'file' ? 'active' : ''} onClick={() => setKind('file')}>Theo file</button>
          <button className={kind === 'function' ? 'active' : ''} onClick={() => setKind('function')}>Theo function</button>
          <button className={kind === 'symbol' ? 'active' : ''} onClick={() => setKind('symbol')}>Theo symbol</button>
        </div>
        <input className="gv-search" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Tìm node — click node để xem code…" />
        <select className="gv-folder" value={folder} onChange={(e) => setFolder(e.target.value)} title="Lọc theo thư mục">
          <option value="">📁 Mọi folder</option>
          {folders.map((f) => <option key={f} value={f}>{f}</option>)}
        </select>
        {focusId && <button className="gv-unfocus" onClick={() => setFocusId(null)}>✕ Bỏ tập trung</button>}
        <span className="gv-stat">{view.top.length} nodes · {view.es.length} cạnh</span>
      </div>
      <div className="gv-canvas" ref={canvasRef}>
        {busy ? <div className="dw-hint" style={{ padding: 20 }}><span className="spin">◐</span> đang dựng graph…</div> : (
          <svg ref={svgRef} viewBox={`${vb.x} ${vb.y} ${vb.w} ${vb.h}`} className="gv-svg"
            onMouseDown={onDown} onMouseMove={onMove} onMouseUp={onUp} onMouseLeave={onUp}
            style={{ cursor: drag.current ? 'grabbing' : 'grab' }}>
            <defs>
              <marker id="gvarrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 z" className="gv-arrow" />
              </marker>
            </defs>
            {view.es.map((e, i) => {
              const a = view.pos.get(e.from), b = view.pos.get(e.to); if (!a || !b) return null;
              const na = view.top.find((n) => n.id === e.from), nb = view.top.find((n) => n.id === e.to);
              const ra = nodeR(na), rb = nodeR(nb);
              const dx = b.x - a.x, dy = b.y - a.y, d = Math.hypot(dx, dy) || 1;
              return (
                <line key={i} className="gv-edge"
                  x1={a.x + (dx / d) * ra} y1={a.y + (dy / d) * ra}
                  x2={b.x - (dx / d) * (rb + 4)} y2={b.y - (dy / d) * (rb + 4)}
                  strokeWidth={0.6 + Math.min((e.weight ?? 1) / 3, 1) * 2} markerEnd="url(#gvarrow)" />
              );
            })}
            {view.top.map((n) => {
              const p = view.pos.get(n.id); if (!p) return null;
              const hot = match(n); const picked = sel?.id === n.id;
              const r = nodeR(n); const color = hot ? '#e2c08d' : nodeColor(n);
              return (
                <g key={n.id} className="gv-node" transform={`translate(${p.x},${p.y})`} onMouseDown={(e) => e.stopPropagation()} onClick={() => selectNode(n)} style={{ cursor: 'pointer' }}>
                  <circle r={r} fill={color} fillOpacity={picked ? 1 : 0.85} className="gv-circle" strokeWidth={picked || hot ? 2 : 1.5} />
                  <text className="gv-label" y={r + 12} textAnchor="middle" fontSize={11}>{n.label}</text>
                  {n.sub && <text className="gv-sub" y={r + 23} textAnchor="middle" fontSize={9}>{n.sub}</text>}
                </g>
              );
            })}
          </svg>
        )}
        {!busy && (
          <div className="gv-zoom">
            <button onClick={() => zoom(0.8)} title="Phóng to">＋</button>
            <button onClick={() => zoom(1.25)} title="Thu nhỏ">－</button>
            <button onClick={() => setVb({ x: 0, y: 0, w: W, h: H })} title="Đặt lại">⛶</button>
          </div>
        )}

        {sel && (
          <div className="gv-pop" onMouseDown={(e) => e.stopPropagation()}>
            <div className="gv-pop-head">
              <span className="gv-pop-name">{sel.label}</span>
              {sel.sub && <span className="gv-pop-kind">{sel.sub}</span>}
              <span className="gv-pop-loc">{sel.path}{sel.line ? `:${sel.line}` : ''}</span>
              <button className="gv-pop-x" onClick={() => setSel(null)}>×</button>
            </div>
            {kind === 'file' ? (
              <div className="gv-pop-outline">
                {outline == null ? <div className="dw-hint" style={{ padding: 10 }}><span className="spin">◐</span> tải outline…</div>
                  : outline.length === 0 ? <div className="dw-hint" style={{ padding: 10 }}>Không có symbol.</div>
                  : outline.map((s, i) => (
                    <div key={i} className="gv-out-row" onClick={() => onOpenFile(sel.path, s.start_line)}>
                      <span className="gv-out-name">{s.name}</span>
                      <span className="gv-out-kind">{s.kind}</span>
                      <span className="gv-out-line">:{s.start_line}</span>
                    </div>
                  ))}
              </div>
            ) : (
              <div className="gv-pop-code">
                {snip == null ? <div className="dw-hint" style={{ padding: 10 }}><span className="spin">◐</span> tải code…</div> : (
                  <SyntaxHighlighter language="text" style={oneDark} customStyle={{ margin: 0, fontSize: 11.5, background: '#1b1b1b', maxHeight: 220 }}>
                    {snip}
                  </SyntaxHighlighter>
                )}
              </div>
            )}
            <div className="gv-pop-actions">
              <button className="btn ghost" onClick={() => setFocusId(sel.id)}>◈ Tập trung</button>
              {kind !== 'file' && <button className="btn ghost" onClick={() => onCodemap(sel.label)}>🕸 Call-graph chi tiết</button>}
              <button className="btn" onClick={() => onOpenFile(sel.path, sel.line)}>&lt;/&gt; Xem code</button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
