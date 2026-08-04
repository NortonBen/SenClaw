import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Layout, Card, Row, Col, Input, Button, Select, Table, Tag, Statistic,
  Switch, Space, Empty, Alert, Upload, message, theme,
} from 'antd';
import { CloudUploadOutlined } from '@ant-design/icons';
import { AppLayout } from '../components/AppLayout';
import { GraphView, type SubgraphPayload } from '../components/cognitive/GraphView';
import {
  CognitiveSidebar,
  type CognitiveSection,
} from '../components/cognitive/CognitiveSidebar';
import { DataPointsView } from '../components/cognitive/DataPointsView';
import { GraphExplorerView } from '../components/cognitive/GraphExplorerView';

const { Content } = Layout;

// =====================================================================
// Wire types — mirror Rust `gateway::ui_server::cognitive` exactly.
// =====================================================================

interface NodeView {
  id: string;
  kind: string;
  name: string;
  summary: string;
  salience: number;
  mention_count: number;
  created_at: number;
  last_seen_at: number;
}

interface HitView {
  node: NodeView;
  score: number;
  path_len: number;
}

interface StatsResponse {
  edges: number;
  nodes_total: number;
  nodes_by_kind: [string, number][];
}

interface DecayRunRow {
  run_at: number;
  edges_scanned: number;
  edges_pruned: number;
  edges_promoted: number;
  duration_ms: number;
}

type SearchMode = 'graph' | 'chunks' | 'triplet' | 'spreading' | 'fts' | 'hybrid';

const MODE_OPTIONS: { value: SearchMode; label: string }[] = [
  { value: 'graph', label: 'GraphCompletion' },
  { value: 'hybrid', label: 'Hybrid (vec+FTS)' },
  { value: 'fts', label: 'FTS (keyword)' },
  { value: 'chunks', label: 'Chunks' },
  { value: 'triplet', label: 'Triplet' },
  { value: 'spreading', label: 'Spreading' },
];

interface RecallSource {
  index: number;
  id: string;
  kind: string;
  name: string;
  summary: string;
  score: number;
}

interface RecallResponse {
  answer: string;
  grounded: boolean;
  note?: string;
  model?: string;
  sources: RecallSource[];
}

interface IngestResult {
  filename?: string | null;
  chunks_added: number;
  chunks_deduped: number;
  entities_added: number;
  edges_added: number;
  llm_skipped: boolean;
}

// =====================================================================
// Page
// =====================================================================

export function CognitivePage() {
  const { token } = theme.useToken();

  // Section navigation lives in the page so refreshing within a section
  // doesn't reset which view the user picked. Default = search/graph view.
  const [section, setSection] = useState<CognitiveSection>('search');
  // Bumped on any memory mutation (forget) so the sidebar re-fetches stats.
  const [refreshKey, setRefreshKey] = useState(0);
  const bumpRefresh = useCallback(() => setRefreshKey(k => k + 1), []);

  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [decayRuns, setDecayRuns] = useState<DecayRunRow[]>([]);
  const [query, setQuery] = useState('');
  const [mode, setMode] = useState<SearchMode>('graph');
  const [rerank, setRerank] = useState(false);
  const [limit, setLimit] = useState(10);
  const [hits, setHits] = useState<HitView[]>([]);
  const [loading, setLoading] = useState(false);
  // 503 dormant flag — when the daemon hasn't booted the cognitive system,
  // we want one obvious banner rather than 5 individual errors.
  const [dormant, setDormant] = useState(false);

  // Graph view state
  const [seedId, setSeedId] = useState<string | null>(null);
  const [graphHops, setGraphHops] = useState(2);
  const [graph, setGraph] = useState<SubgraphPayload | null>(null);
  const [graphLoading, setGraphLoading] = useState(false);

  // Recall (LLM synthesis) state
  const [recallQuery, setRecallQuery] = useState('');
  const [recallMode, setRecallMode] = useState<SearchMode>('hybrid');
  const [recallLoading, setRecallLoading] = useState(false);
  const [recall, setRecall] = useState<RecallResponse | null>(null);

  // Ingest (add text / upload file) state
  const [ingestText, setIngestText] = useState('');
  const [ingestTags, setIngestTags] = useState('');
  const [ingestLoading, setIngestLoading] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [ingestResult, setIngestResult] = useState<IngestResult | null>(null);

  const loadSubgraph = useCallback(
    async (seed: string, hops: number) => {
      setGraphLoading(true);
      try {
        const r = await fetch(
          `/api/cognitive/subgraph?seed=${encodeURIComponent(seed)}&hops=${hops}&limit=100`,
        );
        if (r.status === 503) {
          setDormant(true);
          return;
        }
        if (r.status === 404) {
          message.warning('seed node no longer exists');
          setGraph(null);
          return;
        }
        if (!r.ok) throw new Error(await r.text());
        setGraph(await r.json());
      } catch (e) {
        message.error(`subgraph failed: ${e}`);
      } finally {
        setGraphLoading(false);
      }
    },
    [],
  );

  // Whenever the seed or hops change, refresh the graph.
  useEffect(() => {
    if (seedId) loadSubgraph(seedId, graphHops);
  }, [seedId, graphHops, loadSubgraph]);

  const loadStats = useCallback(async () => {
    try {
      const r = await fetch('/api/cognitive/stats');
      if (r.status === 503) {
        setDormant(true);
        return;
      }
      setDormant(false);
      if (!r.ok) throw new Error(await r.text());
      setStats(await r.json());
    } catch (e) {
      console.warn('cog stats:', e);
    }
  }, []);

  const loadDecay = useCallback(async () => {
    try {
      const r = await fetch('/api/cognitive/decay-log?limit=20');
      if (!r.ok) return;
      const body = await r.json();
      setDecayRuns(body.runs ?? []);
    } catch (e) {
      console.warn('cog decay-log:', e);
    }
  }, []);

  useEffect(() => {
    loadStats();
    loadDecay();
  }, [loadStats, loadDecay]);

  const runSearch = useCallback(async () => {
    if (!query.trim()) return;
    setLoading(true);
    try {
      const r = await fetch('/api/cognitive/search', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query, mode, limit, hops: 2, rerank }),
      });
      if (r.status === 503) {
        setDormant(true);
        return;
      }
      if (!r.ok) throw new Error(await r.text());
      const body = await r.json();
      setHits(body.hits ?? []);
    } catch (e) {
      message.error(`search failed: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [query, mode, limit, rerank]);

  const forgetNode = useCallback(async (id: string) => {
    try {
      const r = await fetch(`/api/cognitive/node/${id}`, { method: 'DELETE' });
      if (!r.ok) throw new Error(await r.text());
      message.success('forgotten');
      setHits(hs => hs.filter(h => h.node.id !== id));
      loadStats();
      bumpRefresh();
    } catch (e) {
      message.error(`forget failed: ${e}`);
    }
  }, [loadStats, bumpRefresh]);

  const runRecall = useCallback(async () => {
    if (!recallQuery.trim()) return;
    setRecallLoading(true);
    try {
      const r = await fetch('/api/cognitive/recall', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query: recallQuery, mode: recallMode, limit: 6, hops: 2 }),
      });
      if (r.status === 503) {
        setDormant(true);
        return;
      }
      if (!r.ok) throw new Error(await r.text());
      setRecall(await r.json());
    } catch (e) {
      message.error(`recall failed: ${e}`);
    } finally {
      setRecallLoading(false);
    }
  }, [recallQuery, recallMode]);

  const tagList = useCallback(
    () => ingestTags.split(',').map(s => s.trim()).filter(Boolean),
    [ingestTags],
  );

  const addText = useCallback(async () => {
    if (!ingestText.trim()) return;
    setIngestLoading(true);
    try {
      const r = await fetch('/api/cognitive/add', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text: ingestText, tags: tagList() }),
      });
      if (r.status === 503) {
        setDormant(true);
        return;
      }
      if (!r.ok) throw new Error(await r.text());
      const body = (await r.json()) as IngestResult;
      setIngestResult(body);
      setIngestText('');
      message.success(`added ${body.chunks_added} chunks, ${body.entities_added} entities`);
      loadStats();
      bumpRefresh();
    } catch (e) {
      message.error(`add failed: ${e}`);
    } finally {
      setIngestLoading(false);
    }
  }, [ingestText, tagList, loadStats, bumpRefresh]);

  const uploadFile = useCallback(async (file: File) => {
    setUploading(true);
    try {
      const fd = new FormData();
      fd.append('file', file);
      const tags = tagList().join(',');
      if (tags) fd.append('tags', tags);
      const r = await fetch('/api/cognitive/upload', { method: 'POST', body: fd });
      if (r.status === 503) {
        setDormant(true);
        return;
      }
      if (!r.ok) throw new Error(await r.text());
      const body = (await r.json()) as IngestResult;
      setIngestResult(body);
      message.success(`${file.name}: ${body.chunks_added} chunks, ${body.entities_added} entities`);
      loadStats();
      bumpRefresh();
    } catch (e) {
      message.error(`upload failed: ${e}`);
    } finally {
      setUploading(false);
    }
  }, [tagList, loadStats, bumpRefresh]);

  const hitColumns = useMemo(() => [
    {
      title: 'Score',
      dataIndex: ['score'],
      width: 80,
      render: (s: number) => s.toFixed(3),
    },
    {
      title: 'Kind',
      dataIndex: ['node', 'kind'],
      width: 90,
      render: (k: string) => <Tag>{k}</Tag>,
    },
    {
      title: 'Name / Summary',
      render: (_: unknown, h: HitView) => (
        <div>
          {h.node.name && <strong>{h.node.name}</strong>}
          {h.node.summary && (
            <div style={{ opacity: 0.7, fontSize: 12 }}>
              {h.node.summary.length > 240
                ? h.node.summary.slice(0, 240) + '…'
                : h.node.summary}
            </div>
          )}
        </div>
      ),
    },
    {
      title: 'Hops',
      dataIndex: ['path_len'],
      width: 70,
    },
    {
      title: '',
      width: 150,
      render: (_: unknown, h: HitView) => (
        <Space size="small">
          <Button size="small" onClick={() => setSeedId(h.node.id)}>
            graph
          </Button>
          <Button size="small" danger onClick={() => forgetNode(h.node.id)}>
            forget
          </Button>
        </Space>
      ),
    },
  ], [forgetNode]);

  const decayColumns = useMemo(() => [
    {
      title: 'When',
      dataIndex: 'run_at',
      render: (ts: number) => new Date(ts * 1000).toLocaleString(),
    },
    { title: 'Scanned', dataIndex: 'edges_scanned', width: 100 },
    { title: 'Archived', dataIndex: 'edges_pruned', width: 100 },
    { title: 'Promoted', dataIndex: 'edges_promoted', width: 100 },
    { title: 'ms', dataIndex: 'duration_ms', width: 80 },
  ], []);

  // Each section is its own JSX so the active one drops straight into
  // Content without an outer Row driving N hidden cards.
  const searchSection = (
    <Row gutter={[16, 16]}>
      <Col span={24}>
        <Card title="Stats" size="small">
          <Row gutter={16}>
            <Col span={6}>
              <Statistic title="Edges" value={stats?.edges ?? 0} />
            </Col>
            <Col span={6}>
              <Statistic title="Nodes" value={stats?.nodes_total ?? 0} />
            </Col>
            <Col span={12}>
              <Space wrap>
                {(stats?.nodes_by_kind ?? []).map(([k, n]) => (
                  <Tag key={k}>{k}: {n}</Tag>
                ))}
              </Space>
            </Col>
          </Row>
        </Card>
      </Col>

      <Col span={24}>
        <Card title="Search" size="small">
              <Space.Compact style={{ width: '100%' }}>
                <Input
                  placeholder="query…"
                  value={query}
                  onChange={e => setQuery(e.target.value)}
                  onPressEnter={runSearch}
                />
                <Select
                  value={mode}
                  onChange={setMode}
                  style={{ width: 160 }}
                  options={MODE_OPTIONS}
                />
                <Select
                  value={limit}
                  onChange={setLimit}
                  style={{ width: 80 }}
                  options={[5, 10, 20, 50].map(n => ({ value: n, label: `${n}` }))}
                />
                <Button type="primary" loading={loading} onClick={runSearch}>
                  Search
                </Button>
              </Space.Compact>
              <div style={{ marginTop: 12 }}>
                <Space>
                  <span>Re-rank (LightGCN):</span>
                  <Switch checked={rerank} onChange={setRerank} />
                </Space>
              </div>
              <div style={{ marginTop: 16 }}>
                {hits.length === 0 ? (
                  <Empty
                    description={query ? 'No matches' : 'Run a search to see results'}
                  />
                ) : (
                  <Table
                    rowKey={(h: HitView) => h.node.id}
                    columns={hitColumns as any}
                    dataSource={hits}
                    pagination={false}
                    size="small"
                  />
                )}
              </div>
            </Card>
          </Col>

          <Col span={24}>
            <Card
              title="Graph"
              size="small"
              extra={
                <Space>
                  <span style={{ fontSize: 12, opacity: 0.7 }}>
                    seed:&nbsp;
                    {seedId
                      ? seedId.slice(0, 8) + '…'
                      : 'pick a result above'}
                  </span>
                  <Select
                    size="small"
                    value={graphHops}
                    onChange={setGraphHops}
                    options={[1, 2, 3].map(n => ({ value: n, label: `${n} hop${n > 1 ? 's' : ''}` }))}
                    style={{ width: 90 }}
                  />
                  <Button
                    size="small"
                    onClick={() => seedId && loadSubgraph(seedId, graphHops)}
                    loading={graphLoading}
                    disabled={!seedId}
                  >
                    Refresh
                  </Button>
                </Space>
              }
            >
              {!seedId ? (
                <Empty description="Click a result row's 'graph' button to visualize" />
              ) : (
                <>
                  <GraphView
                    data={graph}
                    height={520}
                    highlightId={seedId}
                    onNodeClick={n => setSeedId(n.id)}
                  />
                  {graph?.truncated && (
                    <div style={{ marginTop: 8, fontSize: 12, opacity: 0.7 }}>
                      Truncated at 100 nodes — click a node to re-center.
                    </div>
                  )}
                  <div style={{ marginTop: 8, fontSize: 12, opacity: 0.7 }}>
                    Drag = pan · Wheel = zoom · Click node = re-seed
                  </div>
                </>
              )}
            </Card>
          </Col>
        </Row>
  );

  const decaySection = (
    <Card
      title="Decay log"
      size="small"
      extra={<Button size="small" onClick={loadDecay}>Refresh</Button>}
    >
      {decayRuns.length === 0 ? (
        <Empty description="No decay sweeps recorded yet" />
      ) : (
        <Table
          rowKey="run_at"
          columns={decayColumns as any}
          dataSource={decayRuns}
          pagination={false}
          size="small"
        />
      )}
    </Card>
  );

  const datapointsSection = (
    <DataPointsView
      onMutated={() => {
        bumpRefresh();
        loadStats();
      }}
      onOpenInGraph={id => {
        setSeedId(id);
        setSection('search');
      }}
    />
  );

  const recallSection = (
    <Row gutter={[16, 16]}>
      <Col span={24}>
        <Card title="Recall — grounded answer" size="small">
          <Space.Compact style={{ width: '100%' }}>
            <Input
              placeholder="ask a question…"
              value={recallQuery}
              onChange={e => setRecallQuery(e.target.value)}
              onPressEnter={runRecall}
            />
            <Select
              value={recallMode}
              onChange={setRecallMode}
              style={{ width: 160 }}
              options={MODE_OPTIONS}
            />
            <Button type="primary" loading={recallLoading} onClick={runRecall}>
              Ask
            </Button>
          </Space.Compact>

          <div style={{ marginTop: 16 }}>
            {!recall ? (
              <Empty description="Ask a question to synthesize an answer from memory" />
            ) : (
              <>
                {recall.note && !recall.grounded && (
                  <Alert
                    type="info"
                    showIcon
                    style={{ marginBottom: 12 }}
                    message={recall.note}
                  />
                )}
                {recall.answer && (
                  <Card
                    size="small"
                    style={{ marginBottom: 12 }}
                    title={
                      <Space>
                        <span>Answer</span>
                        {recall.grounded && <Tag color="green">grounded</Tag>}
                        {recall.model && <Tag>{recall.model}</Tag>}
                      </Space>
                    }
                  >
                    <div style={{ whiteSpace: 'pre-wrap' }}>{recall.answer}</div>
                  </Card>
                )}
                {recall.sources?.length > 0 && (
                  <Card size="small" title={`Sources (${recall.sources.length})`}>
                    <Space direction="vertical" style={{ width: '100%' }} size={8}>
                      {recall.sources.map(s => (
                        <div key={s.id} style={{ display: 'flex', gap: 8 }}>
                          <Tag>{`[${s.index}]`}</Tag>
                          <div style={{ flex: 1 }}>
                            <Space size={4} wrap>
                              <Tag>{s.kind}</Tag>
                              {s.name && <strong>{s.name}</strong>}
                              <span style={{ opacity: 0.5, fontSize: 12 }}>
                                {s.score.toFixed(3)}
                              </span>
                            </Space>
                            {s.summary && (
                              <div style={{ opacity: 0.7, fontSize: 12 }}>{s.summary}</div>
                            )}
                            <Button
                              size="small"
                              type="link"
                              style={{ padding: 0 }}
                              onClick={() => {
                                setSeedId(s.id);
                                setSection('search');
                              }}
                            >
                              open in graph
                            </Button>
                          </div>
                        </div>
                      ))}
                    </Space>
                  </Card>
                )}
              </>
            )}
          </div>
        </Card>
      </Col>
    </Row>
  );

  const ingestSection = (
    <Row gutter={[16, 16]}>
      <Col span={24}>
        <Card title="Add knowledge" size="small">
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            <Input
              placeholder="tags (comma-separated, optional) — map to knowledge-base scopes"
              value={ingestTags}
              onChange={e => setIngestTags(e.target.value)}
            />
            <Input.TextArea
              placeholder="paste text to remember…"
              rows={6}
              value={ingestText}
              onChange={e => setIngestText(e.target.value)}
            />
            <Space wrap>
              <Button
                type="primary"
                loading={ingestLoading}
                onClick={addText}
                disabled={!ingestText.trim()}
              >
                Add text
              </Button>
              <Upload
                showUploadList={false}
                multiple
                beforeUpload={file => {
                  uploadFile(file as File);
                  return false; // prevent antd's default XHR upload
                }}
              >
                <Button icon={<CloudUploadOutlined />} loading={uploading}>
                  Upload file
                </Button>
              </Upload>
              <span style={{ fontSize: 12, opacity: 0.6 }}>
                txt · md · html · json · csv · yaml · ≤10MB
              </span>
            </Space>
            {ingestResult && (
              <Alert
                type="success"
                showIcon
                message={`Ingested${ingestResult.filename ? ' ' + ingestResult.filename : ''}`}
                description={
                  `chunks +${ingestResult.chunks_added} (deduped ${ingestResult.chunks_deduped}) · ` +
                  `entities +${ingestResult.entities_added} · edges +${ingestResult.edges_added}` +
                  (ingestResult.llm_skipped ? ' · LLM dormant (no triplets extracted)' : '')
                }
              />
            )}
          </Space>
        </Card>
      </Col>
    </Row>
  );

  return (
    <AppLayout
      sidebar={
        <CognitiveSidebar
          activeSection={section}
          onSelect={setSection}
          refreshKey={refreshKey}
        />
      }
    >
      <Content style={{ padding: 24, overflow: 'auto' }}>
        {dormant && (
          <Card style={{ marginBottom: 16, borderColor: token.colorWarning }}>
            <strong>Cognitive memory is dormant.</strong>{' '}
            Configure an embedding provider
            (<code>SENCLAW_MEMORY_EMBEDDING_PROVIDER=openai | ollama | local</code>)
            and restart the daemon to enable.
          </Card>
        )}

        {section === 'search' && searchSection}
        {section === 'recall' && recallSection}
        {section === 'ingest' && ingestSection}
        {section === 'explorer' && <GraphExplorerView />}
        {section === 'datapoints' && datapointsSection}
        {section === 'decay' && decaySection}
      </Content>
    </AppLayout>
  );
}
