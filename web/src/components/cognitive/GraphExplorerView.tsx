/**
 * Graph Explorer — free-form graph browsing without a search query.
 *
 * Two modes layered into one page:
 *
 *   1. **Default sample** — on mount we call `/api/cognitive/sample` which
 *      picks the K most-connected nodes and returns their merged k-hop
 *      neighbourhood. Lets the user see the "shape" of memory before
 *      knowing what to search for.
 *
 *   2. **Pick & expand** — the top-nodes list (left rail of the card) is
 *      a clickable chip list. Click toggles selection; once selected, the
 *      graph re-renders from those seeds (server-side merged BFS so the
 *      client doesn't have to union N subgraphs).
 *
 * Depth slider + node-limit selector let the user balance detail vs
 * performance. "Force load" pushes everything to max (20 seeds, 5 hops,
 * 500-node cap) — useful for small graphs where you want it all on screen.
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Card,
  Empty,
  Space,
  Tag,
  Button,
  Select,
  Slider,
  Tooltip,
  message,
  theme,
} from 'antd';
import {
  ThunderboltOutlined,
  ReloadOutlined,
  ClusterOutlined,
} from '@ant-design/icons';
import { GraphView, type SubgraphPayload } from './GraphView';

// ---------------------------------------------------------------------
// Wire types (mirror Rust shapes in gateway/ui_server/cognitive.rs)
// ---------------------------------------------------------------------

interface TopNodeView {
  node: {
    id: string;
    kind: string;
    name: string;
    summary: string;
  };
  degree: number;
}

// ---------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------

export function GraphExplorerView() {
  const { token } = theme.useToken();

  const [topNodes, setTopNodes] = useState<TopNodeView[]>([]);
  const [selectedSeeds, setSelectedSeeds] = useState<Set<string>>(new Set());
  const [hops, setHops] = useState<number>(2);
  const [nodeLimit, setNodeLimit] = useState<number>(150);
  const [graph, setGraph] = useState<SubgraphPayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [dormant, setDormant] = useState(false);

  /** Load the top-degree node list. Used for the picker rail. */
  const loadTopNodes = useCallback(async () => {
    try {
      const r = await fetch('/api/cognitive/top-nodes?limit=20');
      if (r.status === 503) {
        setDormant(true);
        return;
      }
      if (!r.ok) throw new Error(await r.text());
      const body = await r.json();
      setTopNodes(body.nodes ?? []);
    } catch (e: any) {
      message.error(`Top-nodes load failed: ${e?.message ?? e}`);
    }
  }, []);

  /** Default sample — top-K seeds merged BFS. Single round-trip. */
  const loadSample = useCallback(
    async (opts?: { seedCount?: number; hops?: number; limit?: number }) => {
      setLoading(true);
      try {
        const seedCount = opts?.seedCount ?? 5;
        const h = opts?.hops ?? hops;
        const l = opts?.limit ?? nodeLimit;
        const r = await fetch(
          `/api/cognitive/sample?seed_count=${seedCount}&hops=${h}&limit=${l}`,
        );
        if (r.status === 503) {
          setDormant(true);
          return;
        }
        if (!r.ok) throw new Error(await r.text());
        setGraph(await r.json());
      } catch (e: any) {
        message.error(`Sample failed: ${e?.message ?? e}`);
      } finally {
        setLoading(false);
      }
    },
    [hops, nodeLimit],
  );

  /**
   * Load subgraph(s) for explicitly-selected seeds. We union N calls
   * client-side because /subgraph is single-seed. Edges are dedup'd on
   * (src, dst, predicate); nodes on id.
   */
  const loadFromSelected = useCallback(async () => {
    if (selectedSeeds.size === 0) return;
    setLoading(true);
    try {
      const merged: SubgraphPayload = { nodes: [], edges: [], truncated: false };
      const seenN = new Set<string>();
      const seenE = new Set<string>();

      for (const seedId of selectedSeeds) {
        const r = await fetch(
          `/api/cognitive/subgraph?seed=${encodeURIComponent(seedId)}&hops=${hops}&limit=${nodeLimit}`,
        );
        if (r.status === 503) {
          setDormant(true);
          return;
        }
        if (r.status === 404) continue;
        if (!r.ok) throw new Error(await r.text());
        const part: SubgraphPayload = await r.json();
        for (const n of part.nodes) {
          if (!seenN.has(n.id)) {
            seenN.add(n.id);
            merged.nodes.push(n);
          }
        }
        for (const e of part.edges) {
          const k = `${e.src}|${e.dst}|${e.predicate}`;
          if (!seenE.has(k)) {
            seenE.add(k);
            merged.edges.push(e);
          }
        }
        if (part.truncated) merged.truncated = true;
        if (merged.nodes.length >= nodeLimit) {
          merged.truncated = true;
          break;
        }
      }

      setGraph(merged);
    } catch (e: any) {
      message.error(`Load failed: ${e?.message ?? e}`);
    } finally {
      setLoading(false);
    }
  }, [selectedSeeds, hops, nodeLimit]);

  /** Crank everything to max for small graphs. */
  const forceLoadAll = useCallback(() => {
    setSelectedSeeds(new Set());
    loadSample({ seedCount: 20, hops: 5, limit: 500 });
  }, [loadSample]);

  // Auto-load on mount.
  useEffect(() => {
    loadTopNodes();
    loadSample();
  }, [loadTopNodes, loadSample]);

  // Whenever the user changes seed selection, refresh the graph. Single
  // selection → /subgraph; empty selection → fall back to the sample.
  useEffect(() => {
    if (selectedSeeds.size > 0) {
      loadFromSelected();
    }
    // Empty selection is handled explicitly by the user (Reset button)
    // — we don't auto-refresh-to-sample here because that would race with
    // the slider's onChange-then-loadSample below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedSeeds]);

  const toggleSeed = useCallback((id: string) => {
    setSelectedSeeds(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const seedChips = useMemo(() => {
    if (topNodes.length === 0) return null;
    return (
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: 6,
          marginBottom: 12,
          maxHeight: 110,
          overflowY: 'auto',
        }}
      >
        {topNodes.map(t => {
          const active = selectedSeeds.has(t.node.id);
          const label = t.node.name || t.node.summary.slice(0, 30) || t.node.id.slice(0, 8);
          return (
            <Tooltip
              key={t.node.id}
              title={`${t.node.kind} · degree ${t.degree}${t.node.summary ? ` · ${t.node.summary.slice(0, 80)}` : ''}`}
            >
              <Tag.CheckableTag
                checked={active}
                onChange={() => toggleSeed(t.node.id)}
                style={{ fontSize: 11, padding: '2px 8px' }}
              >
                {label}
                <span style={{ marginLeft: 4, opacity: 0.6 }}>·{t.degree}</span>
              </Tag.CheckableTag>
            </Tooltip>
          );
        })}
      </div>
    );
  }, [topNodes, selectedSeeds, toggleSeed]);

  if (dormant) {
    return (
      <Empty
        description="Cognitive memory is dormant — configure an embedding provider"
        style={{ marginTop: 60 }}
      />
    );
  }

  return (
    <Card
      title={
        <Space>
          <ClusterOutlined />
          <span>Graph explorer</span>
        </Space>
      }
      size="small"
      extra={
        <Space wrap>
          <Tooltip title="Number of hops to expand from each seed">
            <Space size={4}>
              <span style={{ fontSize: 11, color: token.colorTextSecondary }}>depth</span>
              <Slider
                min={1}
                max={5}
                value={hops}
                onChange={v => setHops(v as number)}
                onAfterChange={() => {
                  if (selectedSeeds.size > 0) loadFromSelected();
                  else loadSample();
                }}
                style={{ width: 100 }}
                marks={{ 1: '1', 3: '3', 5: '5' }}
              />
            </Space>
          </Tooltip>
          <Select
            size="small"
            value={nodeLimit}
            onChange={n => {
              setNodeLimit(n);
              if (selectedSeeds.size > 0) loadFromSelected();
              else loadSample({ limit: n });
            }}
            style={{ width: 110 }}
            options={[50, 100, 150, 250, 500].map(n => ({ value: n, label: `${n} nodes` }))}
          />
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={loading}
            onClick={() => (selectedSeeds.size > 0 ? loadFromSelected() : loadSample())}
          >
            Refresh
          </Button>
          <Tooltip title="Load the maximum sample (20 seeds · 5 hops · 500 nodes)">
            <Button
              size="small"
              type="primary"
              icon={<ThunderboltOutlined />}
              loading={loading}
              onClick={forceLoadAll}
            >
              Force load
            </Button>
          </Tooltip>
        </Space>
      }
    >
      <div style={{ marginBottom: 4, fontSize: 12, color: token.colorTextSecondary }}>
        Top nodes by connections{' '}
        {selectedSeeds.size > 0 && (
          <span>
            ·{' '}
            <Button
              type="link"
              size="small"
              style={{ padding: 0, fontSize: 12 }}
              onClick={() => {
                setSelectedSeeds(new Set());
                loadSample();
              }}
            >
              clear ({selectedSeeds.size} selected)
            </Button>
          </span>
        )}
      </div>
      {seedChips}

      {graph && graph.nodes.length > 0 ? (
        <>
          <GraphView
            data={graph}
            height={520}
            onNodeClick={n => toggleSeed(n.id)}
            highlightId={
              selectedSeeds.size === 1
                ? Array.from(selectedSeeds)[0]
                : null
            }
          />
          {graph.truncated && (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.7 }}>
              Truncated at {nodeLimit} nodes — increase the node-limit or use Force load.
            </div>
          )}
          <div style={{ marginTop: 8, fontSize: 12, opacity: 0.7 }}>
            {graph.nodes.length} nodes · {graph.edges.length} edges
            {' · '}drag = pan · wheel = zoom · click node = toggle seed
          </div>
        </>
      ) : (
        <Empty description="No graph data — try adding memories or use Force load." />
      )}
    </Card>
  );
}
