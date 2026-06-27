import { useEffect, useState } from 'react';
import { App as AntApp, AutoComplete, Button, ConfigProvider, Input, Layout, Segmented, Space, Tag, Typography, theme } from 'antd';
import { BookOutlined, DeploymentUnitOutlined, FilterOutlined, FolderOpenOutlined, PartitionOutlined, ReloadOutlined, SettingOutlined } from '@ant-design/icons';
import { api, type RootInfo, type Status } from './api';
import { WikiView } from './components/WikiView';
import { CodeView } from './components/CodeView';
import { GraphView } from './components/GraphView';
import { SettingsDrawer } from './components/SettingsDrawer';

const { Header, Content } = Layout;
const { Title, Text } = Typography;

type Tab = 'wiki' | 'code' | 'graph';

/** Compact relative time, e.g. "5m", "3h", "2d" — empty for falsy/zero. */
function timeAgo(unixSecs: number): string {
  if (!unixSecs) return '';
  const diff = Math.max(0, Math.floor(Date.now() / 1000) - unixSecs);
  if (diff < 60) return 'vừa xong';
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}
type Mode = 'dark' | 'light';

function detectInitialMode(): Mode {
  try {
    const saved = localStorage.getItem('theme');
    if (saved === 'dark' || saved === 'light') return saved;
  } catch { /* ignore */ }
  if (typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches) {
    return 'dark';
  }
  return 'light';
}

export default function App() {
  const [mode, setMode] = useState<Mode>(detectInitialMode);

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const d = e.data;
      if (!d || typeof d !== 'object') return;
      const t = d.theme ?? d.env?.theme;
      if ((d.type === 'senclaw:init' || d.type === 'senclaw:theme') && (t === 'dark' || t === 'light')) {
        setMode(t);
      }
    };
    window.addEventListener('message', onMessage);
    try { window.parent?.postMessage({ type: 'senclaw:ready' }, '*'); } catch { /* ignore */ }
    return () => window.removeEventListener('message', onMessage);
  }, []);

  const isDark = mode === 'dark';

  return (
    <ConfigProvider
      theme={{
        algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: { colorPrimary: '#2563eb', borderRadius: 8 },
      }}
    >
      <AntApp>
        <Shell isDark={isDark} />
      </AntApp>
    </ConfigProvider>
  );
}

function Shell({ isDark }: { isDark: boolean }) {
  const { token } = theme.useToken();
  const { message, modal } = AntApp.useApp();
  const [tab, setTab] = useState<Tab>('wiki');
  const [repoPath, setRepoPath] = useState('');
  const [status, setStatus] = useState<Status | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const [focus, setFocus] = useState<string | undefined>();
  const [recents, setRecents] = useState<RootInfo[]>([]);
  const [exclude, setExclude] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);

  const openGraph = (name: string) => { setFocus(name); setTab('graph'); };

  useEffect(() => {
    document.body.style.background = token.colorBgLayout;
  }, [token.colorBgLayout]);

  const refreshStatus = async () => {
    try {
      const s = await api.status();
      setStatus(s);
      if (s.root) setRepoPath((p) => p || s.root!);
      if (s.exclude?.length) setExclude((e) => e || s.exclude!.join(', '));
    } catch { /* ignore */ }
  };
  const loadRecents = async () => {
    try { setRecents(await api.recents()); } catch { /* ignore */ }
  };
  useEffect(() => { void refreshStatus(); void loadRecents(); }, []);

  const runIndex = async (path: string) => {
    setRepoPath(path);
    setIndexing(true);
    try {
      const ex = exclude.split(',').map((x) => x.trim()).filter(Boolean);
      const r = await api.index(path, ex);
      message.success(`Đã index ${r.indexed} files · ${r.symbols} symbols${r.excluded ? ` · bỏ qua ${r.excluded}` : ''}`);
      await refreshStatus();
      await loadRecents();
      setReloadKey((k) => k + 1);
    } catch (e) {
      message.error(`Index lỗi: ${(e as Error).message}`);
    } finally {
      setIndexing(false);
    }
  };

  const doIndex = async (override?: string) => {
    const path = (override ?? repoPath).trim();
    if (!path) { message.warning('Nhập đường dẫn repo'); return; }
    // Confirm before re-indexing a repo that already has an index — re-indexing
    // rescans every file and rewrites the symbol graph for this folder.
    const prev = recents.find((r) => r.path === path);
    const alreadyIndexed = path === status?.root || !!prev;
    if (alreadyIndexed) {
      const info = prev ? `${prev.files} files · ${prev.symbols} symbols, lần cuối ${timeAgo(prev.last_indexed)} trước` : 'đã được index';
      modal.confirm({
        title: 'Index lại repo này?',
        icon: <ReloadOutlined style={{ color: token.colorPrimary }} />,
        content: (
          <div>
            <div style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12.5, wordBreak: 'break-all', marginBottom: 6 }}>{path}</div>
            <Text type="secondary" style={{ fontSize: 12 }}>{info}. Sẽ quét lại toàn bộ file và cập nhật symbol/graph.</Text>
          </div>
        ),
        okText: 'Index lại',
        cancelText: 'Huỷ',
        onOk: () => runIndex(path),
      });
      return;
    }
    await runIndex(path);
  };

  // AutoComplete options: previously-indexed roots, newest first.
  const recentOptions = recents.map((r) => ({
    value: r.path,
    label: (
      <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12, alignItems: 'baseline' }}>
        <span style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
          <FolderOpenOutlined style={{ color: token.colorPrimary, flexShrink: 0 }} />
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontFamily: 'ui-monospace, monospace', fontSize: 12.5 }}>
            {r.path}
          </span>
        </span>
        <Text type="secondary" style={{ fontSize: 11, flexShrink: 0 }}>
          {r.files} files · {r.symbols} sym · {timeAgo(r.last_indexed)}
        </Text>
      </div>
    ),
  }));

  const s = status?.stats;
  const indexed = !!status?.root;

  return (
    <Layout style={{ height: '100vh', background: token.colorBgLayout }}>
      <Header
        style={{
          display: 'flex', alignItems: 'center', gap: 16, flexWrap: 'wrap',
          background: token.colorBgContainer,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          paddingInline: 20, height: 'auto', minHeight: 58, lineHeight: 'normal', paddingBlock: 10,
        }}
      >
        <Title level={5} style={{ margin: 0, display: 'flex', alignItems: 'center', gap: 8 }}>
          <BookOutlined style={{ color: token.colorPrimary }} /> DeepWiki
        </Title>
        <Segmented<Tab>
          value={tab}
          onChange={setTab}
          options={[
            { label: 'Wiki', value: 'wiki', icon: <BookOutlined /> },
            { label: 'Code', value: 'code', icon: <DeploymentUnitOutlined /> },
            { label: 'Graph', value: 'graph', icon: <PartitionOutlined /> },
          ]}
        />
        <Space.Compact style={{ flex: 1, minWidth: 280 }}>
          <AutoComplete
            style={{ flex: 1 }}
            value={repoPath}
            options={recentOptions}
            popupMatchSelectWidth={460}
            onChange={(v) => setRepoPath(v)}
            onSelect={(v) => { void doIndex(v); }}
            filterOption={(input, option) => {
              // Show the whole list when the field is empty or still holds the
              // active root; only filter once the user types a different query.
              if (!input || input === (status?.root ?? '')) return true;
              return String(option?.value ?? '').toLowerCase().includes(input.toLowerCase());
            }}
            notFoundContent={recents.length ? 'Không khớp folder đã index' : null}
          >
            <Input
              prefix={<FolderOpenOutlined style={{ color: token.colorTextTertiary }} />}
              placeholder="Chọn folder đã index, hoặc nhập đường dẫn tuyệt đối…"
              onPressEnter={() => doIndex()}
              allowClear
            />
          </AutoComplete>
          <Button type="primary" icon={<ReloadOutlined />} loading={indexing} onClick={() => doIndex()}>
            Index
          </Button>
        </Space.Compact>
        <Input
          value={exclude}
          onChange={(e) => setExclude(e.target.value)}
          onPressEnter={() => doIndex()}
          prefix={<FilterOutlined style={{ color: token.colorTextTertiary }} />}
          placeholder="Loại trừ path: release, *.test.ts"
          allowClear
          style={{ width: 240 }}
          title="Bỏ qua khi index (ngoài mặc định: node_modules, dist, *.min.js…). Phân tách bằng dấu phẩy."
        />
        {indexed ? (
          <Space size={4} wrap>
            <Tag color="blue">{status!.root!.split('/').pop()}</Tag>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {s?.files ?? 0} files · {s?.symbols ?? 0} symbols · {s?.edges ?? 0} edges · {status?.pages ?? 0} pages
            </Text>
          </Space>
        ) : (
          <Text type="secondary" style={{ fontSize: 12 }}>chưa index</Text>
        )}
        <Button
          icon={<SettingOutlined />}
          onClick={() => setSettingsOpen(true)}
          style={{ marginLeft: 'auto' }}
          title="Cài đặt DeepWiki (loại trừ, LLM)"
        />
      </Header>
      <Content style={{ minHeight: 0, padding: 16, background: token.colorBgLayout }}>
        <div
          style={{
            height: '100%', minHeight: 0, overflow: 'hidden',
            background: token.colorBgContainer,
            border: `1px solid ${token.colorBorderSecondary}`,
            borderRadius: token.borderRadiusLG,
          }}
        >
          {tab === 'wiki' && <WikiView reloadKey={reloadKey} indexed={indexed} isDark={isDark} onOpenGraph={openGraph} />}
          {tab === 'code' && <CodeView reloadKey={reloadKey} indexed={indexed} isDark={isDark} onOpenGraph={openGraph} />}
          {tab === 'graph' && <GraphView reloadKey={reloadKey} indexed={indexed} isDark={isDark} focus={focus} onFocusChange={setFocus} />}
        </div>
      </Content>
      <SettingsDrawer
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        onSaved={() => { void refreshStatus(); }}
      />
    </Layout>
  );
}
