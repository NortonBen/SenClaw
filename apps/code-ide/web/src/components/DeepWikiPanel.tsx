import { useCallback, useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { api, type DwAsk, type DwAskHistItem, type DwPage, type DwStatus } from '../api';
import { timeAgo } from '../lib';

/** What the DeepWiki reader is currently showing (driven by the sidebar). */
export type DwView = { kind: 'page'; slug: string } | { kind: 'ask'; q?: string } | null;

/* ================= Sidebar: New + wiki pages + Q&A history ================= */
export function DeepWikiHistorySidebar({ view, onView }: { view: DwView; onView: (v: DwView) => void }) {
  const [hist, setHist] = useState<DwAskHistItem[]>([]);
  const [pages, setPages] = useState<DwPage[]>([]);
  const loadPages = () => api.dw.pages().then(setPages).catch(() => setPages([]));
  const loadHist = () => api.dw.askHistory().then(setHist).catch(() => setHist([]));
  useEffect(() => { loadHist(); loadPages(); }, []);
  async function delAsk(id: number) {
    try { await api.dw.deleteAsk(id); } catch { /* ignore */ }
    loadHist();
  }
  const isPage = (slug: string) => view?.kind === 'page' && view.slug === slug;
  async function del(slug: string, title: string) {
    if (!confirm(`Xoá trang wiki “${title}”?`)) return;
    try { await api.dw.deletePage(slug); } catch { /* ignore */ }
    loadPages();
    if (isPage(slug)) onView(null);
  }
  return (
    <div className="dwh">
      <div className="dwh-title">📖 DeepWiki</div>
      <button className="dwh-new" onClick={() => onView({ kind: 'ask' })}>＋ Hỏi mới</button>

      <div className="dwh-sec">TRANG WIKI</div>
      {pages.length === 0 && <div className="dwh-empty">Chưa có trang wiki.</div>}
      {pages.map((p) => (
        <div key={p.slug} className={`dwh-row dwh-page${isPage(p.slug) ? ' active' : ''}`} style={{ paddingLeft: p.parent ? 26 : 12 }}
          onClick={() => onView({ kind: 'page', slug: p.slug })}>
          <span className="dwh-q">📄 {p.title}</span>
          <button className="dwh-del" title="Xoá trang" onClick={(e) => { e.stopPropagation(); del(p.slug, p.title); }}>🗑</button>
        </div>
      ))}

      <div className="dwh-sec">LỊCH SỬ HỎI AI · {hist.length}</div>
      {hist.length === 0 && <div className="dwh-empty">Chưa có câu hỏi nào.</div>}
      {hist.map((h) => (
        <div key={h.id} className="dwh-row dwh-page" title={h.question} onClick={() => onView({ kind: 'ask', q: h.question })}>
          <span className="dwh-q">🕑 {h.question}</span>
          {h.created_at ? <span className="dwh-time">{timeAgo(h.created_at)}</span> : null}
          <button className="dwh-del" title="Xoá lịch sử" onClick={(e) => { e.stopPropagation(); delAsk(h.id); }}>🗑</button>
        </div>
      ))}
    </div>
  );
}

/* ================= Center reader (no tabs) ================= */
interface Props { rootPath: string | null; onOpenFile: (path: string, line?: number) => void; view: DwView; onView: (v: DwView) => void }

export function DeepWikiPanel({ rootPath, onOpenFile, view, onView }: Props) {
  const [status, setStatus] = useState<DwStatus | null>(null);
  const [indexing, setIndexing] = useState(false);
  const [gen, setGen] = useState(false);
  const [genDialog, setGenDialog] = useState(false);
  const [genInstr, setGenInstr] = useState('');

  const loadStatus = useCallback(() => { api.dw.status().then(setStatus).catch(() => setStatus(null)); }, []);
  useEffect(() => { loadStatus(); }, [loadStatus]);

  async function reindex() {
    if (!rootPath) return;
    setIndexing(true);
    try { await api.dw.index(rootPath); } catch { /* ignore */ }
    setIndexing(false); loadStatus();
  }
  async function generate() {
    setGenDialog(false);
    setGen(true);
    try { const r = await api.dw.generateWiki(genInstr.trim()); loadStatus(); if (r.created[0]) onView({ kind: 'page', slug: r.created[0] }); }
    catch (e) { alert('Sinh wiki lỗi: ' + (e as Error).message); }
    setGen(false);
  }

  const s = status?.stats ?? {};
  const files = s.files ?? 0;

  return (
    <div className="dw">
      <div className="dw-head">
        <span className="dw-title">{view?.kind === 'ask' ? '💬 Hỏi DeepWiki' : '📖 DeepWiki'}</span>
        <span className="dw-stats">{files} files · {s.symbols ?? 0} symbols · {s.edges ?? 0} edges</span>
        <button className="btn ghost" onClick={() => setGenDialog(true)} disabled={gen || files === 0}>{gen ? '…' : '✨ Sinh wiki'}</button>
        <button className="btn" onClick={reindex} disabled={indexing || !rootPath}>{indexing ? <span className="spin">◐</span> : '⟳'} Index</button>
      </div>

      {files === 0 ? (
        <div className="dw-empty">
          <div style={{ fontSize: 40, opacity: 0.4 }}>📖</div>
          <p>Chưa index workspace. Bấm <b>Index</b> để DeepWiki quét mã nguồn (tree-sitter),<br />rồi đọc Wiki hoặc bấm <b>＋ Hỏi mới</b> ở thanh bên.</p>
          <button className="btn" onClick={reindex} disabled={indexing || !rootPath}>{indexing ? 'Đang index…' : 'Index ngay'}</button>
        </div>
      ) : (
        <div className="dw-body">
          {view?.kind === 'page' && <PageView slug={view.slug} onOpenFile={onOpenFile} />}
          {view?.kind === 'ask' && <AskView initialQ={view.q} onOpenFile={onOpenFile} />}
          {!view && (
            <div className="dw-blank">
              <div style={{ fontSize: 40, opacity: 0.35 }}>📖</div>
              <p>Chọn một trang wiki ở thanh bên, hoặc bấm <b>＋ Hỏi mới</b> để hỏi về codebase.</p>
            </div>
          )}
        </div>
      )}

      {genDialog && (
        <div className="modal-overlay" onMouseDown={() => setGenDialog(false)}>
          <div className="gen-card" onMouseDown={(e) => e.stopPropagation()}>
            <div className="gen-title">✨ Sinh wiki</div>
            <div className="gen-sub">Mô tả wiki bạn muốn — DeepWiki sẽ sinh nội dung theo yêu cầu (grounded trên code đã index). Để trống = mặc định (Tổng quan + Kiến trúc).</div>
            <textarea autoFocus value={genInstr} onChange={(e) => setGenInstr(e.target.value)}
              placeholder="Ví dụ: Tập trung vào REST API và luồng dữ liệu, viết cho dev mới, tiếng Việt, kèm ví dụ code…" />
            <div className="gen-presets">
              {['Cho người mới onboard', 'Tập trung API & luồng dữ liệu', 'Giải thích kiến trúc & module', 'Hướng dẫn deploy & cấu hình'].map((p) => (
                <button key={p} onClick={() => setGenInstr((v) => (v ? v + '. ' + p : p))}>{p}</button>
              ))}
            </div>
            <div className="gen-actions">
              <button className="btn ghost" onClick={() => setGenDialog(false)}>Huỷ</button>
              <button className="btn" onClick={generate}>✨ Sinh</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/* ================= Wiki page (content + On this page) ================= */
interface Heading { level: number; text: string; id: string }
function slug(t: string) { return t.toLowerCase().replace(/[^\w]+/g, '-').replace(/^-|-$/g, ''); }
function headings(md: string): Heading[] {
  const out: Heading[] = [];
  for (const l of md.split('\n')) { const m = l.match(/^(#{1,3})\s+(.+)/); if (m) out.push({ level: m[1].length, text: m[2].replace(/[*`]/g, '').trim(), id: slug(m[2]) }); }
  return out;
}
const SRC_RE = /([\w./-]+\.(?:rs|ts|tsx|js|jsx|mjs|cjs|py|go|java|rb|php|c|cc|cpp|h|hpp|cs|rst|md|toml|yaml|yml)):(\d+)(?:-\d+)?/g;
function sources(md: string): { path: string; line: number }[] {
  const seen = new Set<string>(); const out: { path: string; line: number }[] = []; let m: RegExpExecArray | null;
  while ((m = SRC_RE.exec(md))) { const k = `${m[1]}:${m[2]}`; if (!seen.has(k)) { seen.add(k); out.push({ path: m[1], line: +m[2] }); } }
  return out.slice(0, 12);
}

function PageView({ slug: slugId, onOpenFile }: { slug: string; onOpenFile: Props['onOpenFile'] }) {
  const [page, setPage] = useState<DwPage | null>(null);
  const mainRef = useRef<HTMLDivElement>(null);
  useEffect(() => { setPage(null); api.dw.page(slugId).then(setPage).catch(() => setPage(null)); }, [slugId]);
  function scrollTo(id: string) { mainRef.current?.querySelector(`#${CSS.escape(id)}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' }); }
  if (!page) return <div className="dw-hint" style={{ padding: 20 }}><span className="spin">◐</span> đang tải trang…</div>;
  const hs = headings(page.content ?? '');
  const srcs = sources(page.content ?? '');
  return (
    <div className="dw-wiki">
      <div className="dw-main dw-wiki-main" ref={mainRef}>
        <h1 className="dw-wiki-title">{page.title}</h1>
        {srcs.length > 0 && (
          <div className="dw-sources">
            <span className="dw-sources-label">Nguồn:</span>
            {srcs.map((sc, i) => <button key={i} className="dw-src-chip" onClick={() => onOpenFile(sc.path, sc.line)}>{sc.path.split('/').pop()}:{sc.line}</button>)}
          </div>
        )}
        <Markdown text={page.content ?? ''} withHeadingIds />
      </div>
      {hs.length > 0 && (
        <div className="dw-toc">
          <div className="dw-side-head"><span>TRÊN TRANG NÀY</span></div>
          {hs.map((h, i) => <div key={i} className={`dw-toc-row lvl${h.level}`} onClick={() => scrollTo(h.id)}>{h.text}</div>)}
        </div>
      )}
    </div>
  );
}

/* ================= Ask page ================= */
const BASE_SUGGEST: { icon: string; q: string }[] = [
  { icon: '🏛', q: 'Kiến trúc tổng thể của dự án này hoạt động thế nào?' },
  { icon: '🚪', q: 'Các entry point chính (điểm khởi đầu) nằm ở đâu?' },
  { icon: '🔄', q: 'Luồng xử lý một request/tác vụ chính đi qua những file nào?' },
  { icon: '🔥', q: 'Những hàm được gọi nhiều nhất và vai trò của chúng?' },
  { icon: '💾', q: 'Dữ liệu được lưu trữ và truy vấn như thế nào?' },
  { icon: '⚠️', q: 'Có điểm nào rủi ro, dễ lỗi hoặc khó bảo trì không?' },
];

function AskView({ initialQ, onOpenFile }: { initialQ?: string; onOpenFile: Props['onOpenFile'] }) {
  const [q, setQ] = useState(initialQ ?? '');
  const [busy, setBusy] = useState(false);
  const [res, setRes] = useState<DwAsk | null>(null);
  const [fileSuggest, setFileSuggest] = useState<{ icon: string; q: string }[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);

  const ask = useCallback(async (query: string) => {
    const t = query.trim(); if (!t) return;
    setBusy(true); setRes(null);
    try { setRes(await api.dw.ask(t)); } catch (e) { setRes({ question: t, answer: '⚠️ ' + (e as Error).message, model: null, matches: [] }); }
    setBusy(false);
  }, []);

  // Opening from history (initialQ) auto-runs; a fresh "New" focuses the box.
  useEffect(() => {
    setQ(initialQ ?? '');
    setRes(null);
    if (initialQ) ask(initialQ); else requestAnimationFrame(() => inputRef.current?.focus());
  }, [initialQ, ask]);

  // Derive a few suggestions from the biggest indexed files.
  useEffect(() => {
    api.dw.files().then((fs) => {
      const top = [...fs].sort((a, b) => b.loc - a.loc).slice(0, 4);
      setFileSuggest(top.map((f) => ({ icon: '📄', q: `File \`${f.path}\` làm gì và có vai trò gì trong dự án?` })));
    }).catch(() => setFileSuggest([]));
  }, []);

  const suggests = [...BASE_SUGGEST, ...fileSuggest];
  const showLanding = !busy && !res;

  return (
    <div className="dw-main dw-ask">
      <div className="dw-ask-bar">
        <input ref={inputRef} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={(e) => { if (e.key === 'Enter') ask(q); }}
          placeholder="Hỏi về codebase — điều tra qua call-graph, trả lời có trích nguồn…" />
        <button className="btn" onClick={() => ask(q)} disabled={busy || !q.trim()}>{busy ? '…' : 'Hỏi AI'}</button>
      </div>

      {showLanding && (
        <div className="dw-suggest">
          <div className="dw-suggest-head">💡 Gợi ý — bấm để hỏi</div>
          <div className="dw-suggest-grid">
            {suggests.map((s, i) => (
              <button key={i} className="dw-suggest-card" onClick={() => { setQ(s.q); ask(s.q); }}>
                <span className="dw-suggest-ico">{s.icon}</span>
                <span className="dw-suggest-q">{s.q}</span>
              </button>
            ))}
          </div>
        </div>
      )}

      {busy && <div className="dw-hint"><span className="spin">◐</span> đang điều tra call-graph…</div>}
      {res && (
        <div className="dw-answer">
          <Markdown text={res.answer} />
          {res.matches?.length > 0 && (
            <div className="dw-matches">
              <div className="dw-side-head" style={{ padding: '8px 0 4px' }}><span>NGUỒN</span></div>
              {res.matches.slice(0, 12).map((m, i) => (
                <div key={i} className="dw-sym-row" onClick={() => onOpenFile(m.path, m.start_line)}>
                  <span className="dw-sym-kind">{m.kind}</span>
                  <span className="dw-sym-name">{m.name}</span>
                  <span className="dw-sym-loc">{m.path}:{m.start_line}</span>
                </div>
              ))}
            </div>
          )}
          {res.model && (
            <div className="dw-answer-foot">
              <span className="dw-model">🤖 {res.model}</span>
              <button className="btn ghost" onClick={() => { setRes(null); requestAnimationFrame(() => inputRef.current?.focus()); }}>＋ Hỏi tiếp</button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/* ================= shared markdown ================= */
function Markdown({ text, withHeadingIds }: { text: string; withHeadingIds?: boolean }) {
  const hid = withHeadingIds
    ? {
        h1: (p: { children?: React.ReactNode }) => <h1 id={slug(String((p.children as [string]) ?? ''))}>{p.children}</h1>,
        h2: (p: { children?: React.ReactNode }) => <h2 id={slug(String((p.children as [string]) ?? ''))}>{p.children}</h2>,
        h3: (p: { children?: React.ReactNode }) => <h3 id={slug(String((p.children as [string]) ?? ''))}>{p.children}</h3>,
      }
    : {};
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          ...hid,
          code(props) {
            const { className, children } = props as { className?: string; children?: React.ReactNode };
            const match = /language-(\w+)/.exec(className ?? '');
            const raw = String(children ?? '').replace(/\n$/, '');
            if (!match && !raw.includes('\n')) return <code className={className}>{children}</code>;
            return (
              <SyntaxHighlighter language={match?.[1] ?? 'text'} style={oneDark} customStyle={{ margin: '8px 0', fontSize: 12, background: '#1b1b1b', borderRadius: 6 }}>
                {raw}
              </SyntaxHighlighter>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
