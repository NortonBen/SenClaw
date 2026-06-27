import { useEffect, useMemo, useState, type CSSProperties, type MouseEvent } from 'react';
import {
  Button, Card, Empty, Input, List, Spin, Tag, Tree, Typography, theme, App as AntApp,
} from 'antd';
import { DeleteOutlined, FileAddOutlined, FileTextOutlined, FolderOutlined, HistoryOutlined, PartitionOutlined, RobotOutlined } from '@ant-design/icons';
import type { DataNode } from 'antd/es/tree';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { api, type AskHistoryItem, type AskResult, type Edge, type FileRec, type Sym, type WikiPage } from '../api';
import { CodeBlock } from './CodeBlock';
import { OverviewGraph } from './OverviewGraph';
import { langFromPath, timeAgo } from '../lib';

const { Text } = Typography;
const { Search } = Input;

interface Props {
  reloadKey: number;
  indexed: boolean;
  isDark: boolean;
  onOpenGraph: (name: string) => void;
}

interface FileView {
  path: string;
  outline: Sym[];
  imports: Edge[];
  code: string;
}

// ---- file-system tree from indexed file paths ----
interface Raw {
  name: string;
  path: string;
  isFile: boolean;
  loc?: number;
  count: number;
  children: Map<string, Raw>;
}

function buildRaw(files: FileRec[]): Map<string, Raw> {
  const rootChildren = new Map<string, Raw>();
  for (const f of files) {
    const parts = f.path.split('/').filter(Boolean);
    let children = rootChildren;
    let prefix = '';
    parts.forEach((part, i) => {
      prefix = prefix ? `${prefix}/${part}` : part;
      const isFile = i === parts.length - 1;
      let node = children.get(part);
      if (!node) {
        node = { name: part, path: prefix, isFile, loc: isFile ? f.loc : undefined, count: 0, children: new Map() };
        children.set(part, node);
      }
      if (!isFile) node.count += 1; // total files under this folder
      children = node.children;
    });
  }
  return rootChildren;
}

function toNodes(map: Map<string, Raw>): DataNode[] {
  return [...map.values()]
    .sort((a, b) => (a.isFile !== b.isFile ? (a.isFile ? 1 : -1) : a.name.localeCompare(b.name)))
    .map((n) =>
      n.isFile
        ? {
            key: n.path,
            isLeaf: true,
            icon: <FileTextOutlined />,
            title: (
              <span>
                {n.name} <Text type="secondary" style={{ fontSize: 11 }}>{n.loc} dòng</Text>
              </span>
            ),
          }
        : {
            key: `dir:${n.path}`,
            icon: <FolderOutlined />,
            title: (
              <span>
                {n.name} <Text type="secondary" style={{ fontSize: 11 }}>({n.count})</Text>
              </span>
            ),
            children: toNodes(n.children),
          },
    );
}

export function WikiView({ reloadKey, indexed, isDark, onOpenGraph }: Props) {
  const { token } = theme.useToken();
  const { message } = AntApp.useApp();
  const [pages, setPages] = useState<WikiPage[]>([]);
  const [files, setFiles] = useState<FileRec[]>([]);
  const [active, setActive] = useState<WikiPage | null>(null);
  const [activeFile, setActiveFile] = useState<FileView | null>(null);
  const [loadingPage, setLoadingPage] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [answer, setAnswer] = useState<AskResult | null>(null);
  const [history, setHistory] = useState<AskHistoryItem[]>([]);
  const [generating, setGenerating] = useState(false);

  const loadHistory = async () => {
    try { setHistory(await api.askHistory()); } catch { /* ignore */ }
  };
  const load = async () => {
    try {
      const [pg, fl] = await Promise.all([api.pages(), api.files().catch(() => [])]);
      setPages(pg);
      setFiles(fl);
      void loadHistory();
    } catch { /* ignore */ }
  };
  useEffect(() => { void load(); }, [reloadKey]);

  const openHistory = async (id: number) => {
    try {
      const r = await api.getAsk(id);
      setActive(null);
      setActiveFile(null);
      setAnswer(r);
    } catch (e) {
      message.error((e as Error).message);
    }
  };
  const genWiki = async () => {
    setGenerating(true);
    try {
      const r = await api.generateWiki();
      message.success(`Đã sinh ${r.created.length} trang: ${r.created.join(', ')}`);
      await load();
      if (r.created.includes('overview')) await openPage('overview');
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setGenerating(false);
    }
  };
  const removeHistory = async (id: number, e: MouseEvent) => {
    e.stopPropagation();
    try { await api.deleteAsk(id); await loadHistory(); } catch { /* ignore */ }
  };

  const pageTree: DataNode[] = useMemo(() => {
    const byParent = new Map<string | null, WikiPage[]>();
    for (const p of pages) {
      const k = p.parent ?? null;
      if (!byParent.has(k)) byParent.set(k, []);
      byParent.get(k)!.push(p);
    }
    const build = (parent: string | null): DataNode[] =>
      (byParent.get(parent) ?? []).map((p) => ({
        key: p.slug, title: p.title, icon: <FileTextOutlined />, children: build(p.slug),
      }));
    const roots = build(null);
    const known = new Set(pages.map((p) => p.slug));
    for (const p of pages) {
      if (p.parent && !known.has(p.parent)) roots.push({ key: p.slug, title: p.title, icon: <FileTextOutlined /> });
    }
    return roots;
  }, [pages]);

  const fileTree: DataNode[] = useMemo(() => toNodes(buildRaw(files)), [files]);

  const openPage = async (slug: string) => {
    setActiveFile(null);
    setLoadingPage(true);
    try {
      setActive(await api.getPage(slug));
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setLoadingPage(false);
    }
  };

  const openFile = async (path: string) => {
    setActive(null);
    setLoadingFile(true);
    try {
      const [fo, snip] = await Promise.all([
        api.fileOutline(path),
        api.snippet({ path, start: 1, end: 100000, context: 0 }).catch(() => null),
      ]);
      setActiveFile({ path, outline: fo.outline, imports: fo.imports, code: snip?.code ?? '' });
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setLoadingFile(false);
    }
  };

  const onStructureSelect = (keys: unknown[]) => {
    const key = keys[0] ? String(keys[0]) : '';
    if (!key) return;
    if (key.startsWith('dir:')) {
      // toggle the folder open/closed when its row is clicked
      setExpanded((prev) => (prev.includes(key) ? prev.filter((k) => k !== key) : [...prev, key]));
    } else {
      void openFile(key);
    }
  };

  const doAsk = async (q: string) => {
    if (!q.trim() || !indexed) return;
    setBusy(true);
    setAnswer(null);
    try {
      setAnswer(await api.ask(q.trim()));
      void loadHistory();
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const mdVars = {
    '--md-border': token.colorBorderSecondary,
    '--md-accent': token.colorPrimary,
    '--md-muted': token.colorTextSecondary,
    '--md-code-bg': token.colorFillSecondary,
  } as CSSProperties;

  const loading = loadingPage || loadingFile;

  return (
    <div style={{ display: 'flex', height: '100%', minHeight: 0 }}>
      {/* Sidebar */}
      <div
        style={{
          width: 270, flexShrink: 0, overflow: 'auto', padding: '12px 8px',
          borderRight: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingInline: 8 }}>
          <Text type="secondary" style={{ fontSize: 11, fontWeight: 600 }}>PAGES</Text>
          <Button
            type="link" size="small" icon={<FileAddOutlined />} loading={generating}
            disabled={!indexed} onClick={genWiki} style={{ fontSize: 12, padding: 0, height: 'auto' }}
            title="Sinh trang wiki (Tổng quan + Kiến trúc) bằng LLM"
          >
            Sinh wiki
          </Button>
        </div>
        {pages.length ? (
          <Tree
            showIcon blockNode treeData={pageTree}
            selectedKeys={active ? [active.slug] : []}
            onSelect={(keys) => keys[0] && openPage(String(keys[0]))}
            style={{ background: 'transparent', marginTop: 4 }}
          />
        ) : (
          <div style={{ padding: 8 }}><Text type="secondary" style={{ fontSize: 12 }}>Chưa có trang. Bấm <b>Sinh wiki</b> để tạo bằng AI.</Text></div>
        )}

        <Text type="secondary" style={{ fontSize: 11, fontWeight: 600, paddingInline: 8, display: 'block', marginTop: 16 }}>
          STRUCTURE {files.length ? <Text type="secondary" style={{ fontSize: 11 }}>· {files.length} files</Text> : null}
        </Text>
        {fileTree.length ? (
          <Tree
            showIcon blockNode
            treeData={fileTree}
            expandedKeys={expanded}
            onExpand={(keys) => setExpanded(keys.map(String))}
            selectedKeys={activeFile ? [activeFile.path] : []}
            onSelect={onStructureSelect}
            style={{ background: 'transparent', marginTop: 4 }}
          />
        ) : (
          <div style={{ padding: 8 }}><Text type="secondary" style={{ fontSize: 12 }}>Index một repo để xem cây mã nguồn.</Text></div>
        )}

        <Text type="secondary" style={{ fontSize: 11, fontWeight: 600, paddingInline: 8, display: 'block', marginTop: 16 }}>
          LỊCH SỬ HỎI AI {history.length ? <Text type="secondary" style={{ fontSize: 11 }}>· {history.length}</Text> : null}
        </Text>
        {history.length ? (
          <div style={{ marginTop: 4 }}>
            {history.map((h) => (
              <div
                key={h.id}
                onClick={() => openHistory(h.id)}
                style={{
                  display: 'flex', gap: 6, alignItems: 'flex-start', padding: '5px 8px',
                  borderRadius: 6, cursor: 'pointer',
                  background: answer?.id === h.id ? token.colorPrimaryBg : 'transparent',
                }}
              >
                <HistoryOutlined style={{ color: token.colorTextTertiary, marginTop: 3, flexShrink: 0, fontSize: 12 }} />
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ fontSize: 13, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{h.question}</div>
                  <Text type="secondary" style={{ fontSize: 11 }}>{timeAgo(h.created_at)}{h.focus ? ` · ${h.focus}` : ''}</Text>
                </div>
                <DeleteOutlined
                  onClick={(e) => removeHistory(h.id, e)}
                  style={{ color: token.colorTextTertiary, flexShrink: 0, marginTop: 3, fontSize: 12 }}
                />
              </div>
            ))}
          </div>
        ) : (
          <div style={{ padding: 8 }}><Text type="secondary" style={{ fontSize: 12 }}>Chưa có câu hỏi nào.</Text></div>
        )}
      </div>

      {/* Content */}
      <div style={{ flex: 1, minWidth: 0, overflow: 'auto', padding: '20px 28px' }}>
        {loading ? (
          <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
        ) : activeFile ? (
          <FileViewer fv={activeFile} isDark={isDark} token={token} />
        ) : active ? (
          <div className="md" style={mdVars}>
            <ReactMarkdown remarkPlugins={[remarkGfm]}>{active.content}</ReactMarkdown>
          </div>
        ) : (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Text type="secondary">
                {indexed
                  ? 'Chọn một trang, mở cây STRUCTURE để xem mã nguồn, hoặc dùng skill deepwiki-generate.'
                  : 'Index một repo (ô trên cùng), rồi duyệt cây mã nguồn hoặc hỏi codebase bên dưới.'}
              </Text>
            }
          />
        )}

        {/* Search / Ask box */}
        <div style={{ borderTop: `1px solid ${token.colorBorderSecondary}`, marginTop: 28, paddingTop: 16 }}>
          <div style={{ display: 'flex', gap: 10, alignItems: 'center', marginBottom: 8, flexWrap: 'wrap' }}>
            <Text strong style={{ fontSize: 13 }}><RobotOutlined /> Hỏi AI</Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              LLM của SenClaw điều tra sâu qua call-graph rồi trả lời (có trích nguồn) — lịch sử ở sidebar.
              Cần tìm/duyệt symbol thì dùng tab Code.
            </Text>
          </div>

          <Search
            placeholder="Hỏi AI về codebase… (vd: indexing hoạt động thế nào?)"
            enterButton="Hỏi AI"
            loading={busy}
            onSearch={doAsk}
            disabled={!indexed}
          />

          {answer ? (
            <div style={{ marginTop: 14 }}>
              <div style={{ display: 'flex', gap: 8, alignItems: 'center', marginBottom: 10, flexWrap: 'wrap' }}>
                {answer.model ? <Tag color="blue" icon={<RobotOutlined />}>{answer.model}</Tag> : null}
                {answer.focus ? (
                  <Button size="small" icon={<PartitionOutlined />} onClick={() => onOpenGraph(answer.focus!)}>
                    Xem luồng: {answer.focus}
                  </Button>
                ) : null}
              </div>
              <div className="md" style={mdVars}><ReactMarkdown remarkPlugins={[remarkGfm]}>{answer.answer}</ReactMarkdown></div>
              {(answer.graph?.nodes?.length ?? 0) > 1 ? (
                <Card
                  size="small"
                  title={<span style={{ fontSize: 13 }}><PartitionOutlined /> Graph tổng quan — luồng điều tra</span>}
                  style={{ marginTop: 14 }}
                  styles={{ body: { padding: 8 } }}
                >
                  <OverviewGraph graph={answer.graph} isDark={isDark} onPick={onOpenGraph} />
                </Card>
              ) : null}
              {answer.matches?.length ? (
                <>
                  <Text type="secondary" style={{ fontSize: 12, display: 'block', margin: '14px 0 4px' }}>Dẫn chứng:</Text>
                  <List
                    size="small"
                    dataSource={answer.matches.slice(0, 6)}
                    renderItem={(m) => (
                      <List.Item>
                        <div style={{ width: '100%' }}>
                          <Text strong>{m.name}</Text> <Tag>{m.kind}</Tag>
                          <Text type="secondary" style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12, marginLeft: 6 }}>{m.path}:{m.start_line}</Text>
                        </div>
                      </List.Item>
                    )}
                  />
                </>
              ) : null}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function FileViewer({ fv, isDark, token }: { fv: FileView; isDark: boolean; token: ReturnType<typeof theme.useToken>['token'] }) {
  const mono = { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 12 } as const;
  return (
    <div>
      <Typography.Title level={5} style={{ margin: '0 0 8px', ...mono }}>
        <FileTextOutlined style={{ color: token.colorPrimary, marginRight: 6 }} />{fv.path}
      </Typography.Title>
      {fv.outline.length ? (
        <Card size="small" title={<span style={{ fontSize: 13 }}>Outline · {fv.outline.length} symbols</span>} style={{ marginBottom: 12 }} styles={{ body: { padding: 8 } }}>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>
            {fv.outline.map((s) => (
              <Tag key={s.id} style={{ margin: 0 }}>
                <Text style={{ fontSize: 12 }}>{s.name}</Text>{' '}
                <Text type="secondary" style={{ fontSize: 11 }}>{s.kind}:{s.start_line}</Text>
              </Tag>
            ))}
          </div>
        </Card>
      ) : null}
      <Card size="small" styles={{ body: { padding: 8 } }}>
        <CodeBlock code={fv.code} lang={langFromPath(fv.path)} startLine={1} isDark={isDark} />
      </Card>
    </div>
  );
}
