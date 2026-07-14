/**
 * Graph Explorer — full knowledge graph browser.
 *
 * Loads the entire graph via `/api/cognitive/full-graph` (chunks hidden
 * by default). Provides:
 *   • Search to find & highlight nodes
 *   • Click node to focus (dims non-neighbors)
 *   • Toggle chunk visibility
 *   • Node detail panel on selection
 *   • Depth/limit controls
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Card,
  Empty,
  Space,
  Tag,
  Button,
  Switch,
  Input,
  Tooltip,
  Descriptions,
  message,
  theme,
} from 'antd';
import {
  ReloadOutlined,
  ClusterOutlined,
  SearchOutlined,
  ExpandOutlined,
  NodeIndexOutlined,
} from '@ant-design/icons';
import { GraphView, type GraphNode, type SubgraphPayload } from './GraphView';

export function GraphExplorerView() {
  const { token } = theme.useToken();

  const [graph, setGraph] = useState<SubgraphPayload | null>(null);
  const [loading, setLoading] = useState(false);
  const [dormant, setDormant] = useState(false);
  const [showChunks, setShowChunks] = useState(false);
  const [connectedOnly, setConnectedOnly] = useState(true);
  const [search, setSearch] = useState('');
  const [focusNode, setFocusNode] = useState<GraphNode | null>(null);

  const loadGraph = useCallback(
    async (chunks: boolean, connected: boolean = connectedOnly) => {
      setLoading(true);
      try {
        const params = new URLSearchParams({
          node_limit: '500',
          edge_limit: '2000',
          include_chunks: chunks ? 'true' : 'false',
          connected_only: connected ? 'true' : 'false',
        });
        const r = await fetch(`/api/cognitive/full-graph?${params}`);
        if (r.status === 503) {
          setDormant(true);
          return;
        }
        if (!r.ok) throw new Error(await r.text());
        setGraph(await r.json());
      } catch (e: any) {
        message.error(`Load failed: ${e?.message ?? e}`);
      } finally {
        setLoading(false);
      }
    },
    [connectedOnly],
  );

  useEffect(() => {
    loadGraph(showChunks, connectedOnly);
  }, [loadGraph, showChunks, connectedOnly]);

  const handleNodeClick = useCallback((n: GraphNode) => {
    setFocusNode(prev => (prev?.id === n.id ? null : n));
  }, []);

  // Stats
  const stats = useMemo(() => {
    if (!graph) return null;
    const kinds: Record<string, number> = {};
    for (const n of graph.nodes) {
      kinds[n.kind] = (kinds[n.kind] ?? 0) + 1;
    }
    return { nodes: graph.nodes.length, edges: graph.edges.length, kinds };
  }, [graph]);

  // Focus node neighbor info
  const focusEdges = useMemo(() => {
    if (!focusNode || !graph) return [];
    return graph.edges.filter(
      e => e.src === focusNode.id || e.dst === focusNode.id,
    );
  }, [focusNode, graph]);

  const focusNeighborNodes = useMemo(() => {
    if (!focusNode || !graph) return [];
    const ids = new Set<string>();
    for (const e of focusEdges) {
      ids.add(e.src === focusNode.id ? e.dst : e.src);
    }
    const nodeMap = new Map(graph.nodes.map(n => [n.id, n]));
    return Array.from(ids)
      .map(id => nodeMap.get(id))
      .filter(Boolean) as GraphNode[];
  }, [focusNode, focusEdges, graph]);

  if (dormant) {
    return (
      <Empty
        description="Cognitive memory is dormant — configure an embedding provider"
        style={{ marginTop: 60 }}
      />
    );
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 0 }}>
      {/* Header bar */}
      <Card
        size="small"
        style={{ borderRadius: '8px 8px 0 0', borderBottom: 'none' }}
        bodyStyle={{ padding: '8px 16px' }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 12,
            flexWrap: 'wrap',
          }}
        >
          <Space size={4}>
            <ClusterOutlined style={{ fontSize: 16, color: token.colorPrimary }} />
            <span style={{ fontWeight: 600, fontSize: 14 }}>Knowledge</span>
          </Space>

          {stats && (
            <Space size={8} style={{ fontSize: 12 }}>
              <Tag color="blue">{stats.nodes} nodes</Tag>
              <Tag color="cyan">{stats.edges} edges</Tag>
              {Object.entries(stats.kinds).map(([k, n]) => (
                <span key={k} style={{ opacity: 0.7 }}>
                  {n} {k}
                </span>
              ))}
            </Space>
          )}

          <div style={{ flex: 1 }} />

          <Input
            prefix={<SearchOutlined style={{ opacity: 0.4 }} />}
            placeholder="Search nodes…"
            size="small"
            value={search}
            onChange={e => setSearch(e.target.value)}
            allowClear
            style={{ width: 200 }}
          />

          <Tooltip title="Only show nodes that have connections">
            <Space size={4}>
              <span style={{ fontSize: 11, color: token.colorTextSecondary }}>
                Connected
              </span>
              <Switch
                size="small"
                checked={connectedOnly}
                onChange={v => setConnectedOnly(v)}
              />
            </Space>
          </Tooltip>

          <Tooltip title="Show chunk nodes (text fragments)">
            <Space size={4}>
              <span style={{ fontSize: 11, color: token.colorTextSecondary }}>
                Chunks
              </span>
              <Switch
                size="small"
                checked={showChunks}
                onChange={v => setShowChunks(v)}
              />
            </Space>
          </Tooltip>

          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={loading}
            onClick={() => loadGraph(showChunks, connectedOnly)}
          >
            Refresh
          </Button>
        </div>
      </Card>

      {/* Graph canvas + optional detail panel */}
      <div style={{ display: 'flex', borderRadius: '0 0 8px 8px', overflow: 'hidden' }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          {graph && graph.nodes.length > 0 ? (
            <GraphView
              data={graph}
              height={640}
              onNodeClick={handleNodeClick}
              highlightId={focusNode?.id ?? null}
              searchText={search}
              showLegend
            />
          ) : (
            <div
              style={{
                height: 640,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
              }}
            >
              <Empty description="No knowledge yet — add memories or ingest documents" />
            </div>
          )}

          {graph?.truncated && (
            <div
              style={{
                padding: '4px 16px',
                fontSize: 11,
                opacity: 0.6,
                background: 'rgba(0,0,0,0.02)',
              }}
            >
              Graph truncated — showing top {graph.nodes.length} nodes by recency
            </div>
          )}
        </div>

        {/* Detail panel */}
        {focusNode && (
          <div
            style={{
              width: 300,
              borderLeft: `1px solid ${token.colorBorderSecondary}`,
              padding: 16,
              overflowY: 'auto',
              maxHeight: 680,
              background: token.colorBgContainer,
            }}
          >
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                marginBottom: 12,
              }}
            >
              <Tag
                color={
                  focusNode.kind === 'entity'
                    ? 'blue'
                    : focusNode.kind === 'summary'
                      ? 'green'
                      : focusNode.kind === 'custom'
                        ? 'gold'
                        : 'default'
                }
              >
                {focusNode.kind}
              </Tag>
              <Button
                size="small"
                type="text"
                onClick={() => setFocusNode(null)}
              >
                ✕
              </Button>
            </div>

            <div style={{ fontWeight: 600, fontSize: 15, marginBottom: 8 }}>
              {focusNode.name || focusNode.id.slice(0, 16)}
            </div>

            {focusNode.summary && (
              <div
                style={{
                  fontSize: 12,
                  lineHeight: 1.6,
                  opacity: 0.8,
                  marginBottom: 16,
                  maxHeight: 120,
                  overflowY: 'auto',
                }}
              >
                {focusNode.summary}
              </div>
            )}

            <div
              style={{
                fontSize: 11,
                color: token.colorTextSecondary,
                marginBottom: 8,
              }}
            >
              <NodeIndexOutlined /> {focusEdges.length} connections
            </div>

            {/* Edges list */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
              {focusEdges.slice(0, 30).map((e, i) => {
                const otherId =
                  e.src === focusNode.id ? e.dst : e.src;
                const otherNode = graph?.nodes.find(
                  n => n.id === otherId,
                );
                const isOutgoing = e.src === focusNode.id;
                return (
                  <div
                    key={i}
                    style={{
                      padding: '4px 8px',
                      borderRadius: 6,
                      background: 'rgba(91,191,232,0.06)',
                      fontSize: 11,
                      cursor: 'pointer',
                      display: 'flex',
                      flexDirection: 'column',
                      gap: 2,
                    }}
                    onClick={() => {
                      if (otherNode) handleNodeClick(otherNode);
                    }}
                  >
                    <div>
                      <span style={{ opacity: 0.5 }}>
                        {isOutgoing ? '→' : '←'}
                      </span>{' '}
                      <span
                        style={{
                          color: '#5BBFE8',
                          fontWeight: 500,
                        }}
                      >
                        {e.predicate}
                      </span>
                    </div>
                    <div style={{ opacity: 0.7, paddingLeft: 14 }}>
                      {otherNode?.name || otherId.slice(0, 12)}
                    </div>
                  </div>
                );
              })}
              {focusEdges.length > 30 && (
                <div style={{ opacity: 0.5, fontSize: 10, textAlign: 'center' }}>
                  +{focusEdges.length - 30} more
                </div>
              )}
              {focusEdges.length === 0 && (
                <div style={{ opacity: 0.4, fontSize: 11 }}>
                  No connections
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Footer */}
      <div
        style={{
          padding: '6px 16px',
          fontSize: 11,
          opacity: 0.5,
        }}
      >
        Drag node = move · Drag canvas = pan · Wheel = zoom · Click node = focus · Click again = unfocus
      </div>
    </div>
  );
}
