import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import {
  Card, Empty, Input, List, Spin, Tag, Tree, Typography, theme, App as AntApp,
} from 'antd';
import { FileTextOutlined, FolderOutlined } from '@ant-design/icons';
import type { DataNode } from 'antd/es/tree';
import ReactMarkdown from 'react-markdown';
import { api, type ContextResult, type Edge, type FileRec, type Sym, type WikiPage } from '../api';
import { CodeBlock } from './CodeBlock';
import { langFromPath } from '../lib';

const { Text } = Typography;
const { Search } = Input;

interface Props {
  reloadKey: number;
  indexed: boolean;
  isDark: boolean;
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

export function WikiView({ reloadKey, indexed, isDark }: Props) {
  const { token } = theme.useToken();
  const { message } = AntApp.useApp();
  const [pages, setPages] = useState<WikiPage[]>([]);
  const [files, setFiles] = useState<FileRec[]>([]);
  const [active, setActive] = useState<WikiPage | null>(null);
  const [activeFile, setActiveFile] = useState<FileView | null>(null);
  const [loadingPage, setLoadingPage] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [asking, setAsking] = useState(false);
  const [evidence, setEvidence] = useState<ContextResult | null>(null);

  const load = async () => {
    try {
      const [pg, fl] = await Promise.all([api.pages(), api.files().catch(() => [])]);
      setPages(pg);
      setFiles(fl);
    } catch { /* ignore */ }
  };
  useEffect(() => { void load(); }, [reloadKey]);

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
    if (!q.trim()) return;
    setAsking(true);
    setEvidence(null);
    try {
      setEvidence(await api.context(q.trim(), 4));
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setAsking(false);
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
        <Text type="secondary" style={{ fontSize: 11, fontWeight: 600, paddingInline: 8 }}>PAGES</Text>
        {pages.length ? (
          <Tree
            showIcon blockNode treeData={pageTree}
            selectedKeys={active ? [active.slug] : []}
            onSelect={(keys) => keys[0] && openPage(String(keys[0]))}
            style={{ background: 'transparent', marginTop: 4 }}
          />
        ) : (
          <div style={{ padding: 8 }}><Text type="secondary" style={{ fontSize: 12 }}>Chưa có trang nào.</Text></div>
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
      </div>

      {/* Content */}
      <div style={{ flex: 1, minWidth: 0, overflow: 'auto', padding: '20px 28px' }}>
        {loading ? (
          <div style={{ textAlign: 'center', padding: 40 }}><Spin /></div>
        ) : activeFile ? (
          <FileViewer fv={activeFile} isDark={isDark} token={token} />
        ) : active ? (
          <div className="md" style={mdVars}>
            <ReactMarkdown>{active.content}</ReactMarkdown>
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

        {/* Ask box */}
        <div style={{ borderTop: `1px solid ${token.colorBorderSecondary}`, marginTop: 28, paddingTop: 16 }}>
          <Search
            placeholder="Hỏi codebase… (có dẫn chứng từ source)"
            enterButton="Hỏi" loading={asking} onSearch={doAsk} disabled={!indexed}
          />
          {evidence ? (
            evidence.matches.length ? (
              <>
                <Text type="secondary" style={{ fontSize: 12, display: 'block', margin: '12px 0 4px' }}>
                  {evidence.matches.length} dẫn chứng · {evidence.callers.length} callers · {evidence.callees.length} callees.
                  Hỏi SenClaw (skill deepwiki-ask) để có câu trả lời tổng hợp.
                </Text>
                <List
                  size="small"
                  dataSource={evidence.matches.slice(0, 8)}
                  renderItem={(m) => (
                    <List.Item>
                      <div style={{ width: '100%' }}>
                        <Text strong>{m.name}</Text> <Tag>{m.kind}</Tag>
                        <div><Text type="secondary" style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12 }}>{m.path}:{m.start_line}</Text></div>
                        {m.signature ? <div><Text type="secondary" style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12 }}>{m.signature}</Text></div> : null}
                      </div>
                    </List.Item>
                  )}
                />
              </>
            ) : (
              <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 12 }}>Không tìm thấy symbol khớp. Thử từ khoá khác hoặc index repo trước.</Text>
            )
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
