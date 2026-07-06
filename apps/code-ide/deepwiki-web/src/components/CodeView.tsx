import { useEffect, useState } from 'react';
import { Button, Card, Empty, Input, List, Select, Space, Spin, Tag, Typography, theme, App as AntApp } from 'antd';
import { CodeOutlined, FolderOutlined, PartitionOutlined } from '@ant-design/icons';
import { api, type CallLink, type Exploration, type Investigation, type Sym, type SymbolResult } from '../api';
import { CodeBlock } from './CodeBlock';
import { OverviewGraph } from './OverviewGraph';
import { langFromPath } from '../lib';

const { Text, Title } = Typography;
const { Search } = Input;

type Snippet = { path: string; start_line: number; end_line: number; code: string; name?: string; kind?: string; signature?: string };

interface Props {
  reloadKey: number;
  indexed: boolean;
  isDark: boolean;
  onOpenGraph: (name: string) => void;
}

const mono = { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 12 } as const;

function SymbolCard({ s, onClick }: { s: Sym; onClick?: () => void }) {
  return (
    <Card size="small" hoverable={!!onClick} onClick={onClick} style={{ marginBottom: 8 }} styles={{ body: { padding: 10 } }}>
      <Space size={6} wrap>
        <Text strong>{s.name}</Text>
        <Tag color="purple">{s.kind}</Tag>
        {s.parent ? <Tag>in {s.parent}</Tag> : null}
      </Space>
      <div><Text type="secondary" style={mono}>{s.path}:{s.start_line}</Text></div>
      {s.signature ? <div><Text type="secondary" style={mono}>{s.signature}</Text></div> : null}
      {s.doc ? <div><Text style={{ color: '#3fb950', fontSize: 12 }}>{s.doc}</Text></div> : null}
    </Card>
  );
}

export function CodeView({ reloadKey, indexed, isDark, onOpenGraph }: Props) {
  const { token } = theme.useToken();
  const { message } = AntApp.useApp();
  const [results, setResults] = useState<Sym[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [detail, setDetail] = useState<SymbolResult | null>(null);
  const [explore, setExplore] = useState<Exploration | null>(null);
  const [source, setSource] = useState<Snippet | null>(null);
  const [graph, setGraph] = useState<Investigation | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [folder, setFolder] = useState<string | undefined>(undefined);
  const [folders, setFolders] = useState<string[]>([]);

  // reset when re-indexed
  useEffect(() => { setResults(null); setDetail(null); setExplore(null); setSource(null); setGraph(null); }, [reloadKey]);

  // folder options from the indexed file tree
  useEffect(() => {
    if (!indexed) { setFolders([]); return; }
    api.files().then((fs) => {
      const set = new Set<string>();
      for (const f of fs) {
        const parts = f.path.split('/');
        parts.pop();
        let pre = '';
        for (const p of parts) { pre = pre ? `${pre}/${p}` : p; set.add(pre); }
      }
      setFolders([...set].sort());
    }).catch(() => {});
  }, [indexed, reloadKey]);

  const doSearch = async (q: string) => {
    if (!q.trim()) return;
    setSearching(true);
    try {
      setResults(await api.search(q.trim(), 40, folder));
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setSearching(false);
    }
  };

  const loadSymbol = async (name: string) => {
    setLoadingDetail(true);
    setDetail(null);
    setExplore(null);
    setSource(null);
    setGraph(null);
    try {
      const [d, ex, snip, inv] = await Promise.all([
        api.symbol(name),
        api.explore(name, 4),
        api.snippet({ name, context: 0 }).catch(() => null),
        api.investigate(name, 2).catch(() => null),
      ]);
      setDetail(d);
      setExplore(ex);
      setSource(snip);
      setGraph(inv);
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setLoadingDetail(false);
    }
  };

  const linkList = (title: string, items: CallLink[]) => (
    <>
      <Title level={5} style={{ fontSize: 13, textTransform: 'uppercase', letterSpacing: '.04em', color: token.colorTextSecondary, margin: '16px 0 6px' }}>
        {title}
      </Title>
      {items.length ? (
        <List
          size="small"
          dataSource={items}
          renderItem={(c) => {
            const ext = c.path === '<external>' || c.kind === 'external';
            return (
              <List.Item
                style={{ cursor: ext ? 'default' : 'pointer', paddingInline: 8 }}
                onClick={() => !ext && loadSymbol(c.name)}
              >
                <Text style={{ color: ext ? token.colorWarning : token.colorPrimary }}>{c.name}</Text>
                <Text type="secondary" style={mono}>{ext ? 'external' : `${c.path}:${c.start_line}`}</Text>
              </List.Item>
            );
          }}
        />
      ) : (
        <Text type="secondary" style={{ fontSize: 12 }}>none</Text>
      )}
    </>
  );

  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1.2fr', height: '100%', minHeight: 0 }}>
      {/* Search column */}
      <div style={{ overflow: 'auto', padding: 12, borderRight: `1px solid ${token.colorBorderSecondary}` }}>
        <Select
          allowClear
          showSearch
          placeholder="Mọi folder"
          value={folder}
          onChange={(v) => setFolder(v)}
          options={folders.map((f) => ({ label: f, value: f }))}
          suffixIcon={<FolderOutlined />}
          disabled={!indexed}
          style={{ width: '100%', marginBottom: 8 }}
        />
        <Search
          placeholder="Tìm symbol (tên, signature, doc)…"
          enterButton
          loading={searching}
          onSearch={doSearch}
          disabled={!indexed}
          style={{ marginBottom: 10 }}
        />
        {results === null ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Index repo rồi tìm một symbol.</Text>} />
        ) : results.length ? (
          results.map((s) => <SymbolCard key={s.id} s={s} onClick={() => loadSymbol(s.name)} />)
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Không tìm thấy symbol.</Text>} />
        )}
      </div>

      {/* Detail column */}
      <div style={{ overflow: 'auto', padding: 16 }}>
        {loadingDetail ? (
          <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
        ) : detail ? (
          <>
            <Space align="baseline" wrap>
              <Title level={4} style={{ margin: 0 }}>{detail.name}</Title>
              <Tag>{detail.definitions.length} def · {detail.callers.length} callers · {detail.callees.length} callees</Tag>
              <Button size="small" icon={<PartitionOutlined />} onClick={() => onOpenGraph(detail.name)}>Mở tab Graph</Button>
            </Space>

            {(graph?.nodes?.length ?? 0) > 1 ? (
              <Card
                size="small"
                title={<span style={{ fontSize: 13 }}><PartitionOutlined /> Đồ thị luồng (click node để đi tiếp)</span>}
                style={{ marginTop: 14 }}
                styles={{ body: { padding: 8 } }}
              >
                <OverviewGraph graph={{ nodes: graph!.nodes, edges: graph!.edges }} isDark={isDark} onPick={loadSymbol} />
              </Card>
            ) : null}

            <Title level={5} style={{ fontSize: 13, textTransform: 'uppercase', letterSpacing: '.04em', color: token.colorTextSecondary, margin: '16px 0 6px' }}>
              Definitions
            </Title>
            {detail.definitions.length
              ? detail.definitions.map((s) => <SymbolCard key={s.id} s={s} />)
              : <Text type="secondary" style={{ fontSize: 12 }}>Không có định nghĩa (có thể là external).</Text>}

            {source && source.code ? (
              <>
                <Title level={5} style={{ fontSize: 13, textTransform: 'uppercase', letterSpacing: '.04em', color: token.colorTextSecondary, margin: '16px 0 6px' }}>
                  <CodeOutlined /> Source <Text type="secondary" style={{ ...mono, textTransform: 'none', letterSpacing: 0 }}>{source.path}:{source.start_line}-{source.end_line}</Text>
                </Title>
                <Card size="small" styles={{ body: { padding: 8 } }}>
                  <CodeBlock code={source.code} lang={langFromPath(source.path)} startLine={source.start_line} isDark={isDark} />
                </Card>
              </>
            ) : null}
            {linkList('Callers', detail.callers)}
            {linkList('Callees', detail.callees)}
            {linkList('Blast radius (transitive callers)', explore?.blast_radius ?? [])}
          </>
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Chọn một symbol để xem call graph và blast radius.</Text>} />
        )}
      </div>
    </div>
  );
}
