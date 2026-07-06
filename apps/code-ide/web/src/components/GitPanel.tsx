import { useCallback, useEffect, useMemo, useState } from 'react';
import { DiffEditor } from '@monaco-editor/react';
import { api, type GitCommit } from '../api';
import { basename, fileIcon, langFromPath, timeAgo } from '../lib';

interface Props { rootName: string | null; monacoTheme: string; onOpenFile: (path: string, line?: number) => void }
type Tab = 'changes' | 'graph';
type ViewMode = 'tree' | 'list';

interface Change { path: string; code: string; staged: boolean }

/** Split porcelain status into staged / unstaged change lists. */
function splitChanges(files: Record<string, string>): { staged: Change[]; unstaged: Change[] } {
  const staged: Change[] = []; const unstaged: Change[] = [];
  for (const [path, code] of Object.entries(files)) {
    const x = code[0] ?? ' ', y = code[1] ?? ' ';
    if (code === '??') { unstaged.push({ path, code: '?', staged: false }); continue; }
    if (x !== ' ' && x !== '?') staged.push({ path, code: x, staged: true });
    if (y !== ' ') unstaged.push({ path, code: y, staged: false });
  }
  const sort = (a: Change, b: Change) => a.path.localeCompare(b.path);
  return { staged: staged.sort(sort), unstaged: unstaged.sort(sort) };
}
const CODE_LABEL: Record<string, string> = { M: 'M', A: 'A', D: 'D', R: 'R', C: 'C', '?': 'U' };
const CODE_COLOR: Record<string, string> = { M: 'var(--modified)', A: 'var(--added)', '?': 'var(--added)', D: 'var(--danger)', R: 'var(--focus)' };

export function GitPanel({ rootName, monacoTheme, onOpenFile }: Props) {
  const [tab, setTab] = useState<Tab>('changes');
  const [view, setView] = useState<ViewMode>(() => (localStorage.getItem('code-ide-git-view') as ViewMode) || 'tree');
  const [menu, setMenu] = useState(false);
  const [branch, setBranch] = useState('');
  const [files, setFiles] = useState<Record<string, string>>({});
  const [msg, setMsg] = useState('');
  const [busy, setBusy] = useState(false);
  const [sel, setSel] = useState<Change | null>(null);
  const [diff, setDiff] = useState<{ original: string; modified: string } | null>(null);
  const [commits, setCommits] = useState<GitCommit[]>([]);

  const setViewMode = (v: ViewMode) => { setView(v); localStorage.setItem('code-ide-git-view', v); setMenu(false); };

  const refresh = useCallback(() => {
    api.git.status().then((r) => setFiles(r.files)).catch(() => setFiles({}));
    api.git.head().then((r) => setBranch(r.branch)).catch(() => setBranch(''));
  }, []);
  useEffect(() => { refresh(); }, [refresh]);
  useEffect(() => { if (tab === 'graph') api.git.log(150).then((r) => setCommits(r.commits)).catch(() => setCommits([])); }, [tab]);

  const { staged, unstaged } = useMemo(() => splitChanges(files), [files]);
  const total = staged.length + unstaged.length;

  function openDiff(c: Change) {
    setSel(c); setDiff(null);
    api.git.filediff(c.path, c.staged).then((r) => setDiff({ original: r.original, modified: r.modified })).catch(() => setDiff({ original: '', modified: '' }));
  }
  async function stage(paths: string[]) { await api.git.stage(paths).catch(() => {}); refresh(); }
  async function unstage(paths: string[]) { await api.git.unstage(paths).catch(() => {}); refresh(); }
  async function discard(c: Change) {
    if (!confirm(`Bỏ thay đổi ở ${c.path}? (không hoàn tác được)`)) return;
    await api.git.discard([c.path]).catch(() => {}); refresh(); if (sel?.path === c.path) setSel(null);
  }
  async function commit() {
    if (!msg.trim() || busy) return;
    setBusy(true);
    try { await api.git.commit(msg.trim()); setMsg(''); refresh(); } catch (e) { alert('Commit lỗi: ' + (e as Error).message); }
    setBusy(false);
  }

  const rowProps = (c: Change) => ({
    active: sel?.path === c.path && sel?.staged === c.staged,
    onOpen: () => openDiff(c),
    onAction: () => (c.staged ? unstage([c.path]) : stage([c.path])),
    onDiscard: c.staged ? undefined : () => discard(c),
  });

  return (
    <div className="git">
      <div className="git-head">
        <span className="git-title">Source Control</span>
        <div className="git-head-actions">
          <button className="git-hbtn" title="Làm mới" onClick={refresh}>↻</button>
          <div className="git-menu-wrap">
            <button className="git-hbtn" title="Xem" onClick={() => setMenu((m) => !m)}>⋯</button>
            {menu && (
              <>
                <div className="git-menu-scrim" onClick={() => setMenu(false)} />
                <div className="git-menu">
                  <button className={view === 'tree' ? 'on' : ''} onClick={() => setViewMode('tree')}>{view === 'tree' ? '✓' : ''} Xem dạng cây</button>
                  <button className={view === 'list' ? 'on' : ''} onClick={() => setViewMode('list')}>{view === 'list' ? '✓' : ''} Xem dạng danh sách</button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      <div className="git-subhead">
        <span className="git-branch" title="Nhánh hiện tại">⑂ {branch || 'main'}</span>
        <div className="git-tabs">
          <button className={tab === 'changes' ? 'active' : ''} onClick={() => setTab('changes')}>Thay đổi{total > 0 ? ` · ${total}` : ''}</button>
          <button className={tab === 'graph' ? 'active' : ''} onClick={() => setTab('graph')}>Lịch sử</button>
        </div>
        <span className="git-repo">{rootName}</span>
      </div>

      {tab === 'changes' ? (
        <div className="git-body">
          <div className="git-left">
            <div className="git-commit">
              <textarea value={msg} onChange={(e) => setMsg(e.target.value)} placeholder={`Message (⌘/Ctrl+Enter để commit lên ${branch || 'main'})`} rows={2}
                onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) commit(); }} />
              <button className="btn git-commit-btn" disabled={busy || !msg.trim() || staged.length === 0} onClick={commit}>{busy ? '…' : `✓ Commit${staged.length ? ` (${staged.length})` : ''}`}</button>
            </div>

            <div className="git-scroll">
              {staged.length > 0 && (
                <ChangeGroup label="Đã stage" count={staged.length} view={view}
                  changes={staged} rowProps={rowProps}
                  headAction={{ icon: '－', title: 'Unstage tất cả', run: () => unstage(staged.map((c) => c.path)) }} />
              )}
              {total === 0 ? (
                <div className="git-clean">✓ Không có thay đổi.</div>
              ) : (
                <ChangeGroup label="Thay đổi" count={unstaged.length} view={view}
                  changes={unstaged} rowProps={rowProps}
                  headAction={unstaged.length > 0 ? { icon: '＋', title: 'Stage tất cả', run: () => stage(unstaged.map((c) => c.path)) } : undefined} />
              )}
            </div>
          </div>

          <div className="git-diff">
            {sel ? (
              diff == null ? <div className="dw-hint" style={{ padding: 20 }}><span className="spin">◐</span> tải diff…</div> : (
                <>
                  <div className="git-diff-head">
                    <span className="git-code" style={{ color: CODE_COLOR[sel.code] ?? 'var(--fg-mute)' }}>{CODE_LABEL[sel.code] ?? sel.code}</span>
                    <span>{sel.path}</span>
                    <button className="btn ghost" onClick={() => onOpenFile(sel.path)}>&lt;/&gt; Mở file</button>
                  </div>
                  <div className="git-diff-body">
                    <DiffEditor theme={monacoTheme} language={langFromPath(sel.path)} original={diff.original} modified={diff.modified}
                      options={{ readOnly: true, renderSideBySide: true, fontSize: 12, minimap: { enabled: false }, scrollBeyondLastLine: false, automaticLayout: true }} />
                  </div>
                </>
              )
            ) : <div className="git-diff-empty">Chọn một file thay đổi để xem diff.</div>}
          </div>
        </div>
      ) : (
        <GitGraph commits={commits} />
      )}
    </div>
  );
}

/* ---------------- change group (tree | list) ---------------- */

interface RowProps { active: boolean; onOpen: () => void; onAction: () => void; onDiscard?: () => void }

function ChangeGroup({ label, count, view, changes, rowProps, headAction }: {
  label: string; count: number; view: ViewMode; changes: Change[];
  rowProps: (c: Change) => RowProps; headAction?: { icon: string; title: string; run: () => void };
}) {
  const [open, setOpen] = useState(true);
  return (
    <div className="git-group">
      <div className="git-grp-head" onClick={() => setOpen((o) => !o)}>
        <span className={`git-grp-chev${open ? ' open' : ''}`}>▸</span>
        <span className="git-grp-label">{label}</span>
        {headAction && <button className="git-grp-btn" title={headAction.title} onClick={(e) => { e.stopPropagation(); headAction.run(); }}>{headAction.icon}</button>}
        <span className="git-grp-count">{count}</span>
      </div>
      {open && (view === 'tree'
        ? <ChangeTree changes={changes} rowProps={rowProps} />
        : changes.map((c) => { const p = c.path.replace(/\/$/, ''); return <ChangeRow key={c.path + String(c.staged)} c={c} depth={0} label={basename(p)} dir={p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : ''} {...rowProps(c)} />; })
      )}
    </div>
  );
}

/* ---- tree model with VSCode-style compact folders ---- */
interface TNode { name: string; path: string; dir: boolean; change?: Change; children: TNode[] }

function buildTree(changes: Change[]): TNode {
  const root: TNode = { name: '', path: '', dir: true, children: [] };
  for (const c of changes) {
    const parts = c.path.split('/').filter(Boolean); // untracked dirs come as "foo/" → drop empty tail
    let node = root;
    for (let i = 0; i < parts.length; i++) {
      const last = i === parts.length - 1;
      const seg = parts[i];
      const p = parts.slice(0, i + 1).join('/');
      let child = node.children.find((n) => n.name === seg && n.dir === !last);
      if (!child) { child = { name: seg, path: p, dir: !last, change: last ? c : undefined, children: [] }; node.children.push(child); }
      node = child;
    }
  }
  const sortRec = (n: TNode) => {
    n.children.sort((a, b) => (a.dir === b.dir ? a.name.localeCompare(b.name) : a.dir ? -1 : 1));
    n.children.forEach(sortRec);
  };
  sortRec(root);
  // compact single-child folder chains (e.g. src / utils → "src/utils")
  const compact = (n: TNode) => {
    for (const child of n.children) compact(child);
    while (n.dir && n.children.length === 1 && n.children[0].dir) {
      const only = n.children[0];
      n.name = n.name ? `${n.name}/${only.name}` : only.name;
      n.path = only.path;
      n.children = only.children;
    }
  };
  root.children.forEach(compact);
  return root;
}

function ChangeTree({ changes, rowProps }: { changes: Change[]; rowProps: (c: Change) => RowProps }) {
  const tree = useMemo(() => buildTree(changes), [changes]);
  return <>{tree.children.map((n) => <TreeNodeRow key={n.path} node={n} depth={0} rowProps={rowProps} />)}</>;
}

function TreeNodeRow({ node, depth, rowProps }: { node: TNode; depth: number; rowProps: (c: Change) => RowProps }) {
  const [open, setOpen] = useState(true);
  if (!node.dir && node.change) {
    return <ChangeRow c={node.change} depth={depth} label={node.name} dir="" {...rowProps(node.change)} />;
  }
  return (
    <>
      <div className="git-trow git-tfolder" style={{ paddingLeft: 8 + depth * 14 }} onClick={() => setOpen((o) => !o)}>
        <span className={`git-tchev${open ? ' open' : ''}`}>▸</span>
        <span className="git-tfico">{open ? '📂' : '📁'}</span>
        <span className="git-tname">{node.name}</span>
      </div>
      {open && node.children.map((n) => <TreeNodeRow key={n.path} node={n} depth={depth + 1} rowProps={rowProps} />)}
    </>
  );
}

function ChangeRow({ c, depth, label, dir, active, onOpen, onAction, onDiscard }: {
  c: Change; depth: number; label: string; dir: string; active: boolean; onOpen: () => void; onAction: () => void; onDiscard?: () => void;
}) {
  return (
    <div className={`git-trow git-file${active ? ' active' : ''}`} style={{ paddingLeft: 8 + depth * 14 + (depth > 0 ? 14 : 0) }} onClick={onOpen} title={c.path}>
      <span className="git-tfico">{fileIcon(label)}</span>
      <span className="git-tname">{label}</span>
      {dir && <span className="git-tdir">{dir}</span>}
      <span className="git-actions">
        {onDiscard && <button title="Bỏ thay đổi" onClick={(e) => { e.stopPropagation(); onDiscard(); }}>↩</button>}
        <button title={c.staged ? 'Unstage' : 'Stage'} onClick={(e) => { e.stopPropagation(); onAction(); }}>{c.staged ? '－' : '＋'}</button>
      </span>
      <span className="git-code" style={{ color: CODE_COLOR[c.code] ?? 'var(--fg-mute)' }}>{CODE_LABEL[c.code] ?? c.code}</span>
    </div>
  );
}

/* ---------------- commit graph (lanes) ---------------- */
const LANE_COLORS = ['#4a9eff', '#5ec269', '#e2c08d', '#d1737a', '#a97bd6', '#4ec9b0', '#e0843c', '#c586c0'];
interface Row { c: GitCommit; lane: number; lanesAfter: (string | null)[]; branches: { from: number; to: number }[] }

function computeRows(commits: GitCommit[]): { rows: Row[]; maxLanes: number } {
  const rows: Row[] = [];
  let lanes: (string | null)[] = [];
  let maxLanes = 1;
  for (const c of commits) {
    let lane = lanes.findIndex((h) => h === c.hash);
    if (lane === -1) { lane = lanes.findIndex((h) => h == null); if (lane === -1) { lane = lanes.length; lanes.push(null); } }
    // merges: other lanes waiting for this commit collapse into `lane`
    lanes = lanes.map((h) => (h === c.hash ? null : h));
    const branches: { from: number; to: number }[] = [];
    if (c.parents.length > 0) {
      lanes[lane] = c.parents[0];
      for (let k = 1; k < c.parents.length; k++) {
        let pl = lanes.findIndex((h) => h == null);
        if (pl === -1) { pl = lanes.length; lanes.push(null); }
        lanes[pl] = c.parents[k];
        branches.push({ from: lane, to: pl });
      }
    } else {
      lanes[lane] = null;
    }
    while (lanes.length > 1 && lanes[lanes.length - 1] == null) lanes.pop();
    maxLanes = Math.max(maxLanes, lanes.length, lane + 1);
    rows.push({ c, lane, lanesAfter: [...lanes], branches });
  }
  return { rows, maxLanes };
}

function GitGraph({ commits }: { commits: GitCommit[] }) {
  const { rows, maxLanes } = useMemo(() => computeRows(commits), [commits]);
  const RH = 30, LW = 16;
  const gw = Math.min(maxLanes, 8) * LW + 8;
  if (commits.length === 0) return <div className="git-diff-empty">Chưa có commit.</div>;
  return (
    <div className="git-graph">
      {rows.map((r) => {
        const color = LANE_COLORS[r.lane % LANE_COLORS.length];
        const cx = r.lane * LW + 8;
        const activeCount = Math.max(r.lanesAfter.length, r.lane + 1);
        return (
          <div className="gg-row" key={r.c.hash} style={{ height: RH }}>
            <svg width={gw} height={RH} className="gg-svg">
              {/* vertical lanes continuing through this row */}
              {Array.from({ length: Math.min(activeCount, 8) }).map((_, li) => (
                <line key={li} x1={li * LW + 8} y1={0} x2={li * LW + 8} y2={RH} stroke={LANE_COLORS[li % LANE_COLORS.length]} strokeWidth={1.4} opacity={r.lanesAfter[li] || li === r.lane ? 0.55 : 0} />
              ))}
              {/* branch-out links */}
              {r.branches.map((b, bi) => (
                <line key={`b${bi}`} x1={cx} y1={RH / 2} x2={b.to * LW + 8} y2={RH} stroke={LANE_COLORS[b.to % LANE_COLORS.length]} strokeWidth={1.4} opacity={0.6} />
              ))}
              <circle cx={cx} cy={RH / 2} r={4.5} fill={color} stroke="var(--bg)" strokeWidth={1.5} />
            </svg>
            <div className="gg-info">
              {r.c.refs.map((ref, ri) => <span key={ri} className={`gg-ref${ref.startsWith('tag:') ? ' tag' : ''}`}>{ref.replace('tag: ', '')}</span>)}
              <span className="gg-subject" title={r.c.subject}>{r.c.subject}</span>
              <span className="gg-meta">{r.c.author} · {timeAgo(r.c.time)} · {r.c.hash.slice(0, 7)}</span>
            </div>
          </div>
        );
      })}
    </div>
  );
}
