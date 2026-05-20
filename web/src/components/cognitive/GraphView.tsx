/*
 * Force-directed graph visualization for cognitive memory.
 *
 * No external graph library. ~200 LOC total:
 *   • Fruchterman-Reingold force simulation (repulsion + spring + cooling)
 *   • SVG render with pan/zoom
 *   • Color by node kind, stroke thickness by edge strength,
 *     stroke color by edge tier
 *
 * Why hand-rolled: cytoscape/d3 add ~200 KB to the bundle for a single
 * page. We have <200 nodes at the limit; an in-browser sim that runs
 * for ~200 ticks on mount renders comfortably under 16 ms.
 */

import React, { useEffect, useMemo, useRef, useState } from 'react';

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
  chunk: '#9CA3AF',
  summary: '#10B981',
  custom: '#F59E0B',
};

// L1Working / L2Episodic / L3Semantic
const TIER_COLORS = ['#6B7280', '#3B82F6', '#10B981'];

interface Props {
  data: SubgraphPayload | null;
  width?: number;
  height?: number;
  /** Click handler to re-seed the graph at a clicked node. */
  onNodeClick?: (node: GraphNode) => void;
  /** Highlight a node (e.g. current seed) by ID. */
  highlightId?: string | null;
}

interface SimNode extends GraphNode {
  x: number;
  y: number;
  vx: number;
  vy: number;
}

// =====================================================================
// Force simulation — Fruchterman-Reingold variant
// =====================================================================

function runForceLayout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  width: number,
  height: number,
  iterations = 250,
): SimNode[] {
  if (nodes.length === 0) return [];
  const k = Math.sqrt((width * height) / Math.max(nodes.length, 1)) * 0.7;
  // Random init in a small central square — deterministic seed for reproducibility.
  let seed = 1;
  const rng = () => {
    seed = (seed * 9301 + 49297) % 233280;
    return seed / 233280;
  };
  const sim: SimNode[] = nodes.map(n => ({
    ...n,
    x: width / 2 + (rng() - 0.5) * width * 0.4,
    y: height / 2 + (rng() - 0.5) * height * 0.4,
    vx: 0,
    vy: 0,
  }));
  const idx = new Map(sim.map((n, i) => [n.id, i]));

  // Resolve edges to indices once.
  const e2 = edges
    .map(e => ({ s: idx.get(e.src) ?? -1, t: idx.get(e.dst) ?? -1, w: e.strength }))
    .filter(e => e.s >= 0 && e.t >= 0 && e.s !== e.t);

  let temperature = width * 0.1;
  const cooling = temperature / iterations;

  for (let it = 0; it < iterations; it++) {
    // Repulsion (O(n²) — fine for n ≤ 200)
    for (let i = 0; i < sim.length; i++) {
      sim[i].vx = 0;
      sim[i].vy = 0;
      for (let j = 0; j < sim.length; j++) {
        if (i === j) continue;
        const dx = sim[i].x - sim[j].x;
        const dy = sim[i].y - sim[j].y;
        const d2 = dx * dx + dy * dy + 0.01;
        const d = Math.sqrt(d2);
        const force = (k * k) / d;
        sim[i].vx += (dx / d) * force;
        sim[i].vy += (dy / d) * force;
      }
    }

    // Attraction (along edges)
    for (const e of e2) {
      const a = sim[e.s];
      const b = sim[e.t];
      const dx = a.x - b.x;
      const dy = a.y - b.y;
      const d = Math.sqrt(dx * dx + dy * dy) + 0.01;
      // Stronger edges pull harder.
      const force = ((d * d) / k) * Math.max(0.2, Math.min(1.5, e.w));
      const fx = (dx / d) * force;
      const fy = (dy / d) * force;
      a.vx -= fx;
      a.vy -= fy;
      b.vx += fx;
      b.vy += fy;
    }

    // Apply with temperature cap, keep inside bounds.
    for (const n of sim) {
      const v = Math.sqrt(n.vx * n.vx + n.vy * n.vy) + 0.01;
      n.x += (n.vx / v) * Math.min(v, temperature);
      n.y += (n.vy / v) * Math.min(v, temperature);
      n.x = Math.max(20, Math.min(width - 20, n.x));
      n.y = Math.max(20, Math.min(height - 20, n.y));
    }
    temperature -= cooling;
  }

  return sim;
}

// =====================================================================
// Component
// =====================================================================

export function GraphView({
  data,
  width = 720,
  height = 480,
  onNodeClick,
  highlightId = null,
}: Props) {
  // Pan + zoom state.
  const [tx, setTx] = useState(0);
  const [ty, setTy] = useState(0);
  const [scale, setScale] = useState(1);
  const dragRef = useRef<{ x: number; y: number } | null>(null);

  const sim = useMemo(() => {
    if (!data) return [] as SimNode[];
    return runForceLayout(data.nodes, data.edges, width, height);
  }, [data, width, height]);

  const posById = useMemo(() => new Map(sim.map(n => [n.id, n])), [sim]);

  if (!data || data.nodes.length === 0) {
    return (
      <div
        style={{
          width,
          height,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          opacity: 0.5,
        }}
      >
        No graph data — pick a seed.
      </div>
    );
  }

  const onWheel = (e: React.WheelEvent<SVGSVGElement>) => {
    e.preventDefault();
    const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
    setScale(s => Math.max(0.3, Math.min(4, s * factor)));
  };

  const onMouseDown = (e: React.MouseEvent<SVGSVGElement>) => {
    dragRef.current = { x: e.clientX - tx, y: e.clientY - ty };
  };
  const onMouseMove = (e: React.MouseEvent<SVGSVGElement>) => {
    if (!dragRef.current) return;
    setTx(e.clientX - dragRef.current.x);
    setTy(e.clientY - dragRef.current.y);
  };
  const onMouseUp = () => {
    dragRef.current = null;
  };

  return (
    <svg
      width={width}
      height={height}
      onWheel={onWheel}
      onMouseDown={onMouseDown}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      onMouseLeave={onMouseUp}
      style={{ background: 'rgba(0,0,0,0.05)', cursor: dragRef.current ? 'grabbing' : 'grab' }}
    >
      <g transform={`translate(${tx},${ty}) scale(${scale})`}>
        {data.edges.map((e, i) => {
          const a = posById.get(e.src);
          const b = posById.get(e.dst);
          if (!a || !b) return null;
          return (
            <line
              key={i}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
              stroke={TIER_COLORS[e.tier] ?? '#888'}
              strokeWidth={Math.max(0.5, Math.min(4, e.strength * 3))}
              strokeOpacity={0.6}
            />
          );
        })}
        {sim.map(n => {
          const fill = KIND_COLORS[n.kind] ?? '#888';
          const isHi = n.id === highlightId;
          const r = isHi ? 10 : 6;
          return (
            <g
              key={n.id}
              transform={`translate(${n.x},${n.y})`}
              style={{ cursor: 'pointer' }}
              onClick={ev => {
                ev.stopPropagation();
                onNodeClick?.(n);
              }}
            >
              <circle
                r={r}
                fill={fill}
                stroke={isHi ? '#fff' : 'rgba(0,0,0,0.4)'}
                strokeWidth={isHi ? 2 : 1}
              />
              {/* Label — only render when zoomed in enough or highlighted */}
              {(isHi || scale > 0.8) && (
                <text
                  x={r + 3}
                  y={3}
                  fontSize={11}
                  fill="currentColor"
                  style={{ pointerEvents: 'none' }}
                >
                  {n.name || (n.summary ? n.summary.slice(0, 24) + '…' : 'chunk')}
                </text>
              )}
            </g>
          );
        })}
      </g>
    </svg>
  );
}
