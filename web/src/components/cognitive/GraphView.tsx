/*
 * Force-directed knowledge graph visualization.
 *
 * Hand-rolled Fruchterman-Reingold layout + SVG render with pan/zoom.
 * Redesigned with yFiles-inspired styling:
 *   • Nodes sized by degree (connection count)
 *   • Colored rings with glow effect by node kind
 *   • Curved edges with predicate labels on hover
 *   • Focus mode: click a node to highlight its neighbors
 *   • Smooth zoom centered on pointer
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';

// =====================================================================
// Wire types (mirror the Rust SubgraphResponse)
// =====================================================================

export interface GraphNode {
  id: string;
  kind: string;
  name: string;
  summary: string;
}

export interface GraphEdge {
  src: string;
  dst: string;
  predicate: string;
  strength: number;
  tier: number;
  ltp_status: number;
  inferred?: boolean;
}

export interface SubgraphPayload {
  nodes: GraphNode[];
  edges: GraphEdge[];
  truncated: boolean;
}

// =====================================================================
// Visual settings
// =====================================================================

const KIND_COLORS: Record<string, string> = {
  entity: '#5BBFE8',
  chunk: '#6B7280',
  summary: '#10B981',
  custom: '#F59E0B',
};

const KIND_BG: Record<string, string> = {
  entity: 'rgba(91,191,232,0.15)',
  chunk: 'rgba(107,114,128,0.1)',
  summary: 'rgba(16,185,129,0.15)',
  custom: 'rgba(245,158,11,0.15)',
};

const TIER_COLORS = ['#6B7280', '#3B82F6', '#10B981'];
const TIER_LABELS = ['L1 working', 'L2 episodic', 'L3 semantic'];

interface Props {
  data: SubgraphPayload | null;
  width?: number;
  height?: number;
  onNodeClick?: (node: GraphNode) => void;
  highlightId?: string | null;
  showLegend?: boolean;
  /** Search text to highlight matching nodes */
  searchText?: string;
}

interface SimNode extends GraphNode {
  x: number;
  y: number;
  vx: number;
  vy: number;
  degree: number;
}

// =====================================================================
// Force simulation — Fruchterman-Reingold variant
// =====================================================================

function runForceLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  width: number,
  height: number,
  iterations = 300,
): SimNode[] {
  if (nodes.length === 0) return [];

  // Pre-compute degree for each node.
  const degreeMap = new Map<string, number>();
  for (const e of edges) {
    degreeMap.set(e.src, (degreeMap.get(e.src) ?? 0) + 1);
    degreeMap.set(e.dst, (degreeMap.get(e.dst) ?? 0) + 1);
  }

  const k = Math.sqrt((width * height) / Math.max(nodes.length, 1)) * 0.85;
  let seed = 42;
  const rng = () => {
    seed = (seed * 9301 + 49297) % 233280;
    return seed / 233280;
  };

  const sim: SimNode[] = nodes.map(n => ({
    ...n,
    x: width / 2 + (rng() - 0.5) * width * 0.5,
    y: height / 2 + (rng() - 0.5) * height * 0.5,
    vx: 0,
    vy: 0,
    degree: degreeMap.get(n.id) ?? 0,
  }));
  const idx = new Map(sim.map((n, i) => [n.id, i]));

  const e2 = edges
    .map(e => ({ s: idx.get(e.src) ?? -1, t: idx.get(e.dst) ?? -1, w: e.strength }))
    .filter(e => e.s >= 0 && e.t >= 0 && e.s !== e.t);

  let temperature = Math.min(width, height) * 0.15;
  const cooling = temperature / iterations;

  // Build adjacency lists for center-gravity clustering.
  const adj: number[][] = Array.from({ length: sim.length }, () => []);
  for (const e of e2) {
    adj[e.s].push(e.t);
    adj[e.t].push(e.s);
  }

  for (let it = 0; it < iterations; it++) {
    // Repulsion (O(n²))
    for (let i = 0; i < sim.length; i++) {
      sim[i].vx = 0;
      sim[i].vy = 0;
      for (let j = i + 1; j < sim.length; j++) {
        const dx = sim[i].x - sim[j].x;
        const dy = sim[i].y - sim[j].y;
        const d = Math.sqrt(dx * dx + dy * dy) + 0.1;
        const force = (k * k) / d;
        const fx = (dx / d) * force;
        const fy = (dy / d) * force;
        sim[i].vx += fx;
        sim[i].vy += fy;
        sim[j].vx -= fx;
        sim[j].vy -= fy;
      }
    }

    // Attraction (along edges)
    for (const e of e2) {
      const a = sim[e.s];
      const b = sim[e.t];
      const dx = a.x - b.x;
      const dy = a.y - b.y;
      const d = Math.sqrt(dx * dx + dy * dy) + 0.1;
      const force = ((d * d) / k) * Math.max(0.3, Math.min(2.0, e.w));
      const fx = (dx / d) * force;
      const fy = (dy / d) * force;
      a.vx -= fx;
      a.vy -= fy;
      b.vx += fx;
      b.vy += fy;
    }

    // Center gravity — pull isolated nodes toward center.
    const cx = width / 2;
    const cy = height / 2;
    for (const n of sim) {
      const grav = n.degree === 0 ? 0.05 : 0.01;
      n.vx += (cx - n.x) * grav;
      n.vy += (cy - n.y) * grav;
    }

    // Apply velocity, clamp to temperature.
    for (const n of sim) {
      const v = Math.sqrt(n.vx * n.vx + n.vy * n.vy) + 0.01;
      n.x += (n.vx / v) * Math.min(v, temperature);
      n.y += (n.vy / v) * Math.min(v, temperature);
      const pad = 30;
      n.x = Math.max(pad, Math.min(width - pad, n.x));
      n.y = Math.max(pad, Math.min(height - pad, n.y));
    }
    temperature -= cooling;
  }

  return sim;
}

function nodeRadius(degree: number, isHighlighted: boolean): number {
  const base = Math.max(4, Math.min(20, 4 + Math.sqrt(degree) * 2.5));
  return isHighlighted ? base + 3 : base;
}

// =====================================================================
// Component
// =====================================================================

export function GraphView({
  data,
  width: widthProp,
  height = 600,
  onNodeClick,
  highlightId = null,
  showLegend = true,
  searchText = '',
}: Props) {
  const [tx, setTx] = useState(0);
  const [ty, setTy] = useState(0);
  const [scale, setScale] = useState(1);
  const dragRef = useRef<{ x: number; y: number } | null>(null);
  const nodeDragRef = useRef<{ id: string; startX: number; startY: number; moved: boolean } | null>(null);
  const [hoveredNode, setHoveredNode] = useState<string | null>(null);
  const [hoveredEdge, setHoveredEdge] = useState<number | null>(null);

  const wrapperRef = useRef<HTMLDivElement>(null);
  const [measuredWidth, setMeasuredWidth] = useState<number>(widthProp ?? 720);

  useEffect(() => {
    if (widthProp != null) {
      setMeasuredWidth(widthProp);
      return;
    }
    if (!wrapperRef.current) return;
    const el = wrapperRef.current;
    const update = () => {
      const w = el.clientWidth;
      if (w > 0) setMeasuredWidth(prev => (Math.abs(prev - w) >= 4 ? w : prev));
    };
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    return () => ro.disconnect();
  }, [widthProp]);

  const width = measuredWidth;

  const [sim, setSim] = useState<SimNode[]>([]);
  useEffect(() => {
    if (!data) { setSim([]); return; }
    setSim(runForceLayout(data.nodes, data.edges, width, height));
  }, [data, width, height]);

  const posById = useMemo(() => new Map(sim.map(n => [n.id, n])), [sim]);

  // Build neighbor sets for focus highlighting.
  const neighborMap = useMemo(() => {
    const m = new Map<string, Set<string>>();
    if (!data) return m;
    for (const e of data.edges) {
      if (!m.has(e.src)) m.set(e.src, new Set());
      if (!m.has(e.dst)) m.set(e.dst, new Set());
      m.get(e.src)!.add(e.dst);
      m.get(e.dst)!.add(e.src);
    }
    return m;
  }, [data]);

  const focusId = highlightId ?? hoveredNode;
  const focusNeighbors = focusId ? neighborMap.get(focusId) : null;

  // Search matching
  const searchLower = searchText.toLowerCase().trim();
  const searchMatches = useMemo(() => {
    if (!searchLower || !data) return new Set<string>();
    return new Set(
      data.nodes
        .filter(
          n =>
            n.name.toLowerCase().includes(searchLower) ||
            n.summary.toLowerCase().includes(searchLower),
        )
        .map(n => n.id),
    );
  }, [data, searchLower]);

  const isNodeVisible = useCallback(
    (id: string) => {
      if (!focusId) return true;
      if (id === focusId) return true;
      return focusNeighbors?.has(id) ?? false;
    },
    [focusId, focusNeighbors],
  );

  if (!data || data.nodes.length === 0) {
    return (
      <div
        ref={wrapperRef}
        style={{
          width: '100%',
          height,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          opacity: 0.5,
        }}
      >
        No graph data
      </div>
    );
  }

  const onWheel = (e: React.WheelEvent<SVGSVGElement>) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
    setScale(s => Math.max(0.15, Math.min(6, s * factor)));
  };

  const onMouseDown = (e: React.MouseEvent<SVGSVGElement>) => {
    if (e.button !== 0) return;
    if (nodeDragRef.current) return;
    dragRef.current = { x: e.clientX - tx, y: e.clientY - ty };
  };
  const onMouseMove = (e: React.MouseEvent<SVGSVGElement>) => {
    if (nodeDragRef.current) {
      const nd = nodeDragRef.current;
      const svgEl = e.currentTarget;
      const rect = svgEl.getBoundingClientRect();
      const mx = (e.clientX - rect.left - tx) / scale;
      const my = (e.clientY - rect.top - ty) / scale;
      nd.moved = true;
      setSim(prev =>
        prev.map(n =>
          n.id === nd.id ? { ...n, x: mx, y: my } : n,
        ),
      );
      return;
    }
    if (!dragRef.current) return;
    setTx(e.clientX - dragRef.current.x);
    setTy(e.clientY - dragRef.current.y);
  };
  const onMouseUp = () => {
    nodeDragRef.current = null;
    dragRef.current = null;
  };

  return (
    <div ref={wrapperRef} style={{ width: '100%', lineHeight: 0, position: 'relative', overflow: 'hidden', borderRadius: 8 }}>
      {/* Legend */}
      {showLegend && (
        <div
          style={{
            position: 'absolute',
            bottom: 8,
            left: 8,
            background: 'rgba(0,0,0,0.55)',
            backdropFilter: 'blur(6px)',
            color: '#fff',
            borderRadius: 8,
            padding: '8px 10px',
            fontSize: 10,
            lineHeight: 1.6,
            pointerEvents: 'none',
            zIndex: 1,
          }}
        >
          {Object.entries(KIND_COLORS).map(([k, c]) => (
            <div key={k} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
              <span
                style={{
                  width: 10,
                  height: 10,
                  borderRadius: '50%',
                  background: c,
                  border: `2px solid ${c}`,
                  display: 'inline-block',
                  boxShadow: `0 0 4px ${c}`,
                }}
              />
              {k}
            </div>
          ))}
          <div style={{ opacity: 0.5, margin: '3px 0 1px', borderTop: '1px solid rgba(255,255,255,0.2)', paddingTop: 3 }}>edges</div>
          {TIER_LABELS.map((label, i) => (
            <div key={label} style={{ display: 'flex', alignItems: 'center', gap: 5 }}>
              <span
                style={{
                  width: 14,
                  borderTop: `2px solid ${TIER_COLORS[i]}`,
                  display: 'inline-block',
                }}
              />
              {label}
            </div>
          ))}
        </div>
      )}

      {/* Hovered node tooltip */}
      {hoveredNode && (() => {
        const n = posById.get(hoveredNode);
        if (!n) return null;
        const neighbors = neighborMap.get(n.id);
        const connCount = neighbors?.size ?? 0;
        return (
          <div
            style={{
              position: 'absolute',
              top: 8,
              right: 8,
              background: 'rgba(0,0,0,0.75)',
              backdropFilter: 'blur(8px)',
              color: '#fff',
              borderRadius: 10,
              padding: '10px 14px',
              fontSize: 12,
              lineHeight: 1.5,
              maxWidth: 320,
              zIndex: 2,
              borderLeft: `3px solid ${KIND_COLORS[n.kind] ?? '#888'}`,
            }}
          >
            <div style={{ fontWeight: 600, fontSize: 13 }}>
              {n.name || n.id.slice(0, 12)}
            </div>
            <div style={{ opacity: 0.6, fontSize: 10, marginBottom: 4 }}>
              {n.kind} · {connCount} connections
            </div>
            {n.summary && (
              <div style={{ opacity: 0.85, fontSize: 11 }}>
                {n.summary.length > 200 ? n.summary.slice(0, 200) + '…' : n.summary}
              </div>
            )}
          </div>
        );
      })()}

      {/* Hovered edge tooltip */}
      {hoveredEdge !== null && data.edges[hoveredEdge] && (() => {
        const e = data.edges[hoveredEdge];
        const srcN = posById.get(e.src);
        const dstN = posById.get(e.dst);
        return (
          <div
            style={{
              position: 'absolute',
              top: 8,
              right: 8,
              background: 'rgba(0,0,0,0.75)',
              backdropFilter: 'blur(8px)',
              color: '#fff',
              borderRadius: 10,
              padding: '10px 14px',
              fontSize: 12,
              lineHeight: 1.5,
              maxWidth: 320,
              zIndex: 2,
            }}
          >
            <div style={{ fontWeight: 600 }}>
              {srcN?.name || e.src.slice(0, 8)} → {dstN?.name || e.dst.slice(0, 8)}
            </div>
            <div style={{ color: TIER_COLORS[e.tier] ?? '#888', fontSize: 13 }}>
              {e.predicate}
            </div>
            <div style={{ opacity: 0.6, fontSize: 10 }}>
              strength {e.strength.toFixed(2)} · {TIER_LABELS[e.tier] ?? `tier ${e.tier}`}
              {e.inferred ? ' · inferred' : ''}
            </div>
          </div>
        );
      })()}

      <svg
        width="100%"
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="xMidYMid meet"
        onWheel={onWheel}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseUp}
        style={{
          background: 'transparent',
          cursor: dragRef.current ? 'grabbing' : 'grab',
          display: 'block',
          borderRadius: 8,
        }}
      >
        <defs>
          {/* Glow filter for highlighted nodes */}
          <filter id="glow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="3" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
          {/* Glow for search matches */}
          <filter id="searchGlow" x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur stdDeviation="5" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>
        <g transform={`translate(${tx},${ty}) scale(${scale})`}>
          {/* Edges */}
          {data.edges.map((e, i) => {
            const a = posById.get(e.src);
            const b = posById.get(e.dst);
            if (!a || !b) return null;
            const dimmed = focusId
              ? !(e.src === focusId || e.dst === focusId)
              : false;
            const isHov = hoveredEdge === i;

            // Curved edge: compute a small perpendicular offset.
            const mx = (a.x + b.x) / 2;
            const my = (a.y + b.y) / 2;
            const dx = b.x - a.x;
            const dy = b.y - a.y;
            const len = Math.sqrt(dx * dx + dy * dy) + 0.1;
            const curveOffset = Math.min(20, len * 0.08);
            const nx = -dy / len;
            const ny = dx / len;
            const cx_ = mx + nx * curveOffset;
            const cy_ = my + ny * curveOffset;

            return (
              <path
                key={i}
                d={`M ${a.x} ${a.y} Q ${cx_} ${cy_} ${b.x} ${b.y}`}
                fill="none"
                stroke={isHov ? '#fff' : (TIER_COLORS[e.tier] ?? '#555')}
                strokeWidth={isHov ? 2.5 : Math.max(0.5, Math.min(3, e.strength * 2.5))}
                strokeOpacity={dimmed ? 0.06 : (e.inferred ? 0.25 : 0.45)}
                strokeDasharray={e.inferred ? '4 3' : undefined}
                style={{ cursor: 'pointer', transition: 'stroke-opacity 0.2s' }}
                onMouseEnter={() => { setHoveredEdge(i); setHoveredNode(null); }}
                onMouseLeave={() => setHoveredEdge(null)}
              />
            );
          })}

          {/* Nodes */}
          {sim.map(n => {
            const color = KIND_COLORS[n.kind] ?? '#888';
            const bgColor = KIND_BG[n.kind] ?? 'rgba(128,128,128,0.1)';
            const isHi = n.id === focusId;
            const isSearch = searchLower && searchMatches.has(n.id);
            const visible = isNodeVisible(n.id);
            const dimmed = focusId ? !visible : false;
            const r = nodeRadius(n.degree, isHi);
            const label = n.name || (n.summary ? n.summary.slice(0, 20) : '');

            return (
              <g
                key={n.id}
                transform={`translate(${n.x},${n.y})`}
                style={{
                  cursor: nodeDragRef.current?.id === n.id ? 'grabbing' : 'pointer',
                  opacity: dimmed ? 0.08 : 1,
                  transition: 'opacity 0.2s',
                }}
                onMouseDown={ev => {
                  ev.stopPropagation();
                  if (ev.button !== 0) return;
                  nodeDragRef.current = { id: n.id, startX: ev.clientX, startY: ev.clientY, moved: false };
                }}
                onClick={ev => {
                  ev.stopPropagation();
                  if (nodeDragRef.current?.moved) return;
                  onNodeClick?.(n);
                }}
                onMouseEnter={() => { setHoveredNode(n.id); setHoveredEdge(null); }}
                onMouseLeave={() => setHoveredNode(null)}
              >
                {/* Outer glow ring */}
                {(isHi || isSearch) && (
                  <circle
                    r={r + 6}
                    fill="none"
                    stroke={isSearch ? '#FFD700' : color}
                    strokeWidth={2}
                    strokeOpacity={0.5}
                    filter={isSearch ? 'url(#searchGlow)' : 'url(#glow)'}
                  />
                )}
                {/* Background circle */}
                <circle r={r} fill={bgColor} />
                {/* Main circle */}
                <circle
                  r={r - 1}
                  fill={bgColor}
                  stroke={color}
                  strokeWidth={isHi ? 2.5 : 1.5}
                />
                {/* Inner dot */}
                <circle r={Math.max(2, r * 0.35)} fill={color} />
                {/* Label */}
                {label && (scale > 0.5 || isHi || isSearch) && (
                  <text
                    x={0}
                    y={r + 12}
                    fontSize={isHi ? 11 : 9}
                    fontWeight={isHi ? 600 : 400}
                    fill="currentColor"
                    textAnchor="middle"
                    style={{ pointerEvents: 'none', userSelect: 'none' }}
                  >
                    {label.length > 24 ? label.slice(0, 22) + '…' : label}
                  </text>
                )}
              </g>
            );
          })}
        </g>
      </svg>
    </div>
  );
}
