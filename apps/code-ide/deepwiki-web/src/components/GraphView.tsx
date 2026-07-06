import { useEffect, useMemo, useState } from 'react';
import { Empty, Input, Segmented, Select, Spin, Typography, theme, App as AntApp } from 'antd';
import { FolderOutlined, SearchOutlined } from '@ant-design/icons';
import { api, type FileGraphData, type Investigation, type SymbolGraphData } from '../api';
import { OverviewGraph } from './OverviewGraph';
import { FileGraph } from './FileGraph';
import { FunctionGraph } from './FunctionGraph';

const { Text } = Typography;
const { Search } = Input;

interface Props {
  reloadKey: number;
  indexed: boolean;
  isDark: boolean;
  focus?: string;
  onFocusChange: (name: string) => void;
}

type Side = 'both' | 'callers' | 'callees';
type Mode = 'file' | 'function' | 'symbol';

export function GraphView({ reloadKey, indexed, isDark, focus, onFocusChange }: Props) {
  const { message } = AntApp.useApp();
  const { token } = theme.useToken();
  const [mode, setMode] = useState<Mode>('file');
  const [name, setName] = useState(focus ?? '');
  const [side, setSide] = useState<Side>('both');
  const [depth, setDepth] = useState(20);
  const [folder, setFolder] = useState<string | undefined>(undefined);
  const [q, setQ] = useState('');
  const [inv, setInv] = useState<Investigation | null>(null);
  const [loading, setLoading] = useState(false);
  const [fg, setFg] = useState<FileGraphData | null>(null);
  const [sg, setSg] = useState<SymbolGraphData | null>(null);
  const [loadingWhole, setLoadingWhole] = useState(false);

  useEffect(() => { if (focus) { setName(focus); setMode('symbol'); } }, [focus]);

  // Whole-codebase graphs (file or function).
  useEffect(() => {
    if (!indexed || (mode !== 'file' && mode !== 'function')) return;
    let alive = true;
    setLoadingWhole(true);
    const p = mode === 'file' ? api.fileGraph() : api.symbolGraph();
    p.then((d) => { if (alive) (mode === 'file' ? setFg(d as FileGraphData) : setSg(d as SymbolGraphData)); })
      .catch(() => {})
      .finally(() => { if (alive) setLoadingWhole(false); });
    return () => { alive = false; };
  }, [mode, indexed, reloadKey]);

  // Per-symbol investigation.
  useEffect(() => {
    if (mode !== 'symbol' || !name.trim()) { setInv(null); return; }
    let alive = true;
    setLoading(true);
    api.investigate(name.trim(), depth)
      .then((d) => { if (alive) setInv(d); })
      .catch((e) => message.error((e as Error).message))
      .finally(() => { if (alive) setLoading(false); });
    return () => { alive = false; };
  }, [mode, name, depth, reloadKey, message]);

  const pick = (n: string) => { setName(n); setMode('symbol'); onFocusChange(n); };

  // Folder options from the active whole-codebase dataset.
  const folders = useMemo(() => {
    const paths = (mode === 'function' ? sg?.nodes : fg?.nodes)?.map((n) => n.path) ?? [];
    const set = new Set<string>();
    for (const path of paths) {
      const parts = path.split('/');
      parts.pop();
      let pre = '';
      for (const p of parts) { pre = pre ? `${pre}/${p}` : p; set.add(pre); }
    }
    return [...set].sort();
  }, [fg, sg, mode]);

  const inScope = (p: string) => !folder || p === folder || p.startsWith(`${folder}/`);

  const fgScoped = useMemo<FileGraphData | null>(() => {
    if (!fg) return null;
    let nodes = folder ? fg.nodes.filter((n) => inScope(n.path)) : fg.nodes;
    const ql = q.trim().toLowerCase();
    if (ql) {
      // Keep files matching the query plus their direct neighbours (related files).
      const matched = new Set(nodes.filter((n) => n.path.toLowerCase().includes(ql)).map((n) => n.path));
      const keep = new Set(matched);
      for (const e of fg.edges) {
        if (matched.has(e.from)) keep.add(e.to);
        if (matched.has(e.to)) keep.add(e.from);
      }
      nodes = nodes.filter((n) => keep.has(n.path));
    }
    const ids = new Set(nodes.map((n) => n.path));
    return { nodes, edges: fg.edges.filter((e) => ids.has(e.from) && ids.has(e.to)) };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fg, folder, q]);

  const sgScoped = useMemo<SymbolGraphData | null>(() => {
    if (!sg) return null;
    let nodes = folder ? sg.nodes.filter((n) => inScope(n.path)) : sg.nodes;
    const ql = q.trim().toLowerCase();
    if (ql) {
      // Keep functions matching the query plus their direct callers/callees.
      const matched = new Set(nodes.filter((n) => n.name.toLowerCase().includes(ql)).map((n) => n.name));
      const keep = new Set(matched);
      for (const e of sg.edges) {
        if (matched.has(e.from)) keep.add(e.to);
        if (matched.has(e.to)) keep.add(e.from);
      }
      nodes = nodes.filter((n) => keep.has(n.name));
    }
    const ids = new Set(nodes.map((n) => n.name));
    return { nodes, edges: sg.edges.filter((e) => ids.has(e.from) && ids.has(e.to)) };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sg, folder, q]);

  const filtered = useMemo(() => {
    if (!inv) return { nodes: [], edges: [] };
    const nodes = inv.nodes.filter((n) =>
      side === 'both' ? true : side === 'callers' ? n.depth <= 0 : n.depth >= 0);
    const ids = new Set(nodes.map((n) => n.id));
    return { nodes, edges: inv.edges.filter((e) => ids.has(e.from) && ids.has(e.to)) };
  }, [inv, side]);

  const whole = mode === 'function' ? sgScoped : fgScoped;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
      <div style={{ padding: 12, borderBottom: `1px solid ${token.colorBorderSecondary}`, display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
        <Segmented<Mode>
          value={mode}
          onChange={setMode}
          options={[
            { label: 'Theo file', value: 'file' },
            { label: 'Theo function', value: 'function' },
            { label: 'Theo symbol', value: 'symbol' },
          ]}
        />
        {mode === 'symbol' ? (
          <>
            <Search key={name} placeholder="Symbol để điều tra call-graph…" enterButton defaultValue={name}
              loading={loading} onSearch={(v) => pick(v.trim())} disabled={!indexed} style={{ maxWidth: 280 }} />
            <Segmented value={side} onChange={(v) => setSide(v as Side)}
              options={[{ label: 'Cả hai', value: 'both' }, { label: 'Callers', value: 'callers' }, { label: 'Callees', value: 'callees' }]} />
            <Segmented value={depth} onChange={(v) => setDepth(v as number)}
              options={[1, 2, 3, 5, 10, 20].map((d) => ({ label: `Sâu ${d}`, value: d }))} />
            {inv?.focus ? (
              <Text type="secondary" style={{ fontSize: 12 }}>
                {inv.callers.length} callers · {inv.callees.length} callees · {filtered.nodes.length} nodes
              </Text>
            ) : null}
          </>
        ) : (
          <>
            <Input allowClear value={q} onChange={(e) => setQ(e.target.value)}
              prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
              placeholder={mode === 'file' ? 'Lọc tên file (giữ file liên quan)…' : 'Lọc tên function (giữ hàm liên quan)…'}
              style={{ flex: 1, minWidth: 200, maxWidth: 360 }} />
            <Text type="secondary" style={{ fontSize: 12, whiteSpace: 'nowrap' }}>
              {whole ? `${whole.nodes.length} ${mode === 'file' ? 'files' : 'functions'} · ${whole.edges.length} liên kết` : ''}
            </Text>
            <Select allowClear showSearch placeholder="Mọi folder" value={folder} onChange={(v) => setFolder(v)}
              options={folders.map((f) => ({ label: f, value: f }))} suffixIcon={<FolderOutlined />} style={{ width: 200 }} />
          </>
        )}
      </div>

      <div style={{ flex: 1, minHeight: 0, overflow: 'auto', padding: 16 }}>
        {!indexed ? (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Index một repo để xem đồ thị.</Text>} />
        ) : mode === 'symbol' ? (
          loading ? (
            <div style={{ textAlign: 'center', padding: 60 }}><Spin /></div>
          ) : !name ? (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Nhập một symbol để điều tra call-graph.</Text>} />
          ) : !inv?.focus ? (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Không tìm thấy “{name}”.</Text>} />
          ) : filtered.nodes.length < 2 ? (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Không có quan hệ gọi hàm cho “{inv.focus}”.</Text>} />
          ) : (
            <OverviewGraph graph={filtered} isDark={isDark} onPick={pick} />
          )
        ) : loadingWhole ? (
          <div style={{ textAlign: 'center', padding: 60 }}><Spin /></div>
        ) : whole && whole.nodes.length ? (
          mode === 'file'
            ? <FileGraph graph={fgScoped!} onPickSymbol={pick} />
            : <FunctionGraph graph={sgScoped!} isDark={isDark} onPick={pick} />
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description={<Text type="secondary">Chưa có dữ liệu đồ thị.</Text>} />
        )}
      </div>
    </div>
  );
}
