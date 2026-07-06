import { useCallback, useEffect, useRef, useState } from 'react';
import { api, type ChatMsg, type Codemap, type Conversation, type ModelInfo, type Pin, type PlanStep, type Recent, type RunMode, type SearchHit, type TreeEntry } from './api';
import { basename, dirname, timeAgo } from './lib';
import { Explorer } from './components/Explorer';
import { EditorPane, type Tab } from './components/EditorPane';
import { ChatPanel } from './components/ChatPanel';
import { TerminalPanel } from './components/TerminalPanel';
import { InputModal, type ModalSpec } from './components/InputModal';
import { DiffModal, type DiffSpec } from './components/DiffModal';
import { FolderBrowser } from './components/FolderBrowser';
import { DeepWikiPanel, DeepWikiHistorySidebar, type DwView } from './components/DeepWikiPanel';
import { CodemapSidebar, CodemapMain } from './components/Codemap';
import { GitPanel } from './components/GitPanel';

type Nav = 'explorer' | 'search' | 'deepwiki' | 'codemap' | 'git';

function loadCodemaps(): Codemap[] {
  try { return JSON.parse(localStorage.getItem('dw-codemaps-v2') || '[]'); } catch { return []; }
}

function loadConvs(): Conversation[] {
  try { return JSON.parse(localStorage.getItem('code-ide-convs') || '[]'); } catch { return []; }
}

export default function App() {
  const [hasRoot, setHasRoot] = useState(false);
  const [rootName, setRootName] = useState<string | null>(null);
  const [rootPath, setRootPath] = useState<string | null>(null);
  const [roots, setRoots] = useState<TreeEntry[]>([]);
  const [recents, setRecents] = useState<Recent[]>([]);
  const [gitFiles, setGitFiles] = useState<Record<string, string>>({});
  const [refreshKey, setRefreshKey] = useState(0);

  const [tabs, setTabs] = useState<Tab[]>([]);
  const [activePath, setActivePath] = useState<string | null>(null);
  const [reveal, setReveal] = useState<{ path: string; line: number; nonce: number }>();

  const [nav, setNav] = useState<Nav>('explorer');
  const [dwView, setDwView] = useState<DwView>(null);
  // Codemaps (Devin-style, own section)
  const [codemaps, setCodemaps] = useState<Codemap[]>(loadCodemaps);
  const [activeCm, setActiveCm] = useState<Codemap | null>(null);
  const [cmBusy, setCmBusy] = useState(false);
  const [cmStart, setCmStart] = useState('');
  const [pins, setPins] = useState<Pin[]>([]);
  const [messages, setMessages] = useState<ChatMsg[]>([]);
  const messagesRef = useRef<ChatMsg[]>([]);
  messagesRef.current = messages;
  const [sending, setSending] = useState(false);
  const [model, setModel] = useState<string | null>(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);
  const [mode, setMode] = useState<RunMode>('chat');

  const [sidebarW, setSidebarW] = useState(260);
  const [chatW, setChatW] = useState(380);
  const [showTerm, setShowTerm] = useState(false);
  const [termHeight, setTermHeight] = useState(240);
  const [fitKey, setFitKey] = useState(0);
  const [modal, setModal] = useState<ModalSpec | null>(null);
  const [diff, setDiff] = useState<DiffSpec | null>(null);
  const [browseRoot, setBrowseRoot] = useState(false);
  const [chatCollapsed, setChatCollapsed] = useState(false);
  const [conversations, setConversations] = useState<Conversation[]>(loadConvs);
  const [theme, setTheme] = useState<'dark' | 'light'>(() => (localStorage.getItem('code-ide-theme') === 'light' ? 'light' : 'dark'));

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    try { localStorage.setItem('code-ide-theme', theme); } catch { /* ignore */ }
  }, [theme]);
  const monacoTheme = theme === 'light' ? 'light' : 'vs-dark';

  // ---- boot ----
  useEffect(() => {
    (async () => {
      const st = await api.status().catch(() => null);
      if (st?.hasRoot) {
        setHasRoot(true);
        setRootName(st.name);
        setRootPath(st.root);
        setRoots(await api.tree('').catch(() => []));
        loadGit();
      } else {
        setRecents(await api.recents().catch(() => []));
      }
      api.llmInfo().then((i) => setModel(i.ok ? i.model ?? null : null)).catch(() => {});
      api.models().then((r) => { setModels(r.configs); setActiveModelId(r.activeId); }).catch(() => {});
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const loadGit = useCallback(() => {
    api.gitStatus().then((r) => setGitFiles(r.files)).catch(() => setGitFiles({}));
  }, []);

  // ---- filesystem change stream ----
  useEffect(() => {
    if (!hasRoot) return;
    const es = new EventSource('/api/events');
    es.addEventListener('fs', () => {
      setRefreshKey((k) => k + 1);
      loadGit();
    });
    return () => es.close();
  }, [hasRoot, loadGit]);

  async function openFolder(path: string) {
    try {
      const r = await api.open(path);
      setHasRoot(true);
      setRootName(r.name);
      setRootPath(r.root);
      setRoots(r.tree);
      setTabs([]);
      setActivePath(null);
      loadGit();
      // Auto-index the workspace into the in-process DeepWiki (background).
      api.deepwikiIndex(r.root);
    } catch (e) {
      alert('Không mở được thư mục: ' + (e as Error).message);
    }
  }

  async function openFile(path: string, line?: number) {
    setNav('explorer'); // opening a file returns to the editor view

    const existing = tabs.find((t) => t.path === path);
    if (existing) {
      setActivePath(path);
      if (line) setReveal({ path, line, nonce: Date.now() });
      return;
    }
    try {
      const f = await api.file(path);
      const tab: Tab = {
        path,
        content: f.content,
        lang: f.lang,
        dirty: false,
        readOnly: f.binary || f.too_large,
        note: f.binary ? 'file nhị phân' : f.too_large ? 'file quá lớn' : undefined,
      };
      setTabs((ts) => [...ts, tab]);
      setActivePath(path);
      if (line) setReveal({ path, line, nonce: Date.now() });
    } catch (e) {
      alert('Không mở được file: ' + (e as Error).message);
    }
  }

  function onEditorChange(path: string, value: string) {
    setTabs((ts) => ts.map((t) => (t.path === path ? { ...t, content: value, dirty: true } : t)));
  }

  function closeTab(path: string) {
    const t = tabs.find((x) => x.path === path);
    if (t?.dirty && !confirm(`Đóng ${basename(path)} mà chưa lưu?`)) return;
    setTabs((ts) => {
      const next = ts.filter((x) => x.path !== path);
      if (activePath === path) setActivePath(next.length ? next[next.length - 1].path : null);
      return next;
    });
  }

  async function saveActive() {
    const t = tabs.find((x) => x.path === activePath);
    if (!t || !t.dirty) return;
    try {
      await api.save(t.path, t.content);
      setTabs((ts) => ts.map((x) => (x.path === t.path ? { ...x, dirty: false } : x)));
      loadGit();
    } catch (e) {
      alert('Lưu thất bại: ' + (e as Error).message);
    }
  }

  // ---- chat ----
  async function sendChat(textInput: string, extraPins: Pin[] = []) {
    const allPins = extraPins.length ? [...pins, ...extraPins] : pins;
    if (extraPins.length) setPins(allPins);
    const next = [...messages, { role: 'user' as const, content: textInput }];
    setMessages(next);
    setSending(true);
    const started = performance.now();
    try {
      const r = await api.chat(next, allPins, activePath, mode);
      const secs = Math.max(1, Math.round((performance.now() - started) / 1000));
      // In Plan mode, parse the response into an approvable step timeline.
      const steps = mode === 'plan' ? parsePlan(r.text) : undefined;
      setMessages([...next, { role: 'assistant', content: r.text, ms: secs, steps }]);
      if (r.model) setModel(r.model);
    } catch (e) {
      setMessages([...next, { role: 'assistant', content: '⚠️ ' + (e as Error).message }]);
    } finally {
      setSending(false);
    }
  }

  // ---- conversations (past chats) ----
  function persistConvs(next: Conversation[]) {
    setConversations(next);
    try { localStorage.setItem('code-ide-convs', JSON.stringify(next)); } catch { /* ignore */ }
  }
  function newChat() {
    if (messages.length) {
      const title = (messages.find((m) => m.role === 'user')?.content ?? 'Hội thoại').slice(0, 80);
      const conv: Conversation = { id: String(performance.now()), title, messages, at: Date.now() };
      persistConvs([conv, ...conversations].slice(0, 60));
    }
    setMessages([]);
    setPins([]);
  }
  function loadConversation(id: string) {
    const c = conversations.find((x) => x.id === id);
    if (c) setMessages(c.messages);
  }
  function deleteConversation(id: string) {
    persistConvs(conversations.filter((c) => c.id !== id));
  }
  function clearAllConversations() {
    if (confirm('Xoá tất cả lịch sử hội thoại?')) persistConvs([]);
  }

  async function selectModel(id: string) {
    setActiveModelId(id);
    const info = models.find((m) => m.id === id);
    if (info?.modelName) setModel(info.modelName);
    try {
      await api.setModel(id);
    } catch (e) {
      alert('Không đổi được model: ' + (e as Error).message);
    }
  }

  // ---- Devin-style Plan → Approve → Execute ----
  // Extract numbered/bulleted steps from a plan response.
  function parsePlan(text: string): PlanStep[] | undefined {
    const steps: PlanStep[] = [];
    for (const raw of text.split('\n')) {
      const m = raw.match(/^\s*(?:\d+[.)]|[-*])\s+(.+)/);
      if (m) {
        const title = m[1].replace(/\*\*/g, '').replace(/`/g, '').trim();
        if (title.length > 4) steps.push({ title, status: 'pending' });
      }
    }
    return steps.length >= 2 ? steps.slice(0, 8) : undefined;
  }

  function updateStep(msgIdx: number, stepIdx: number, patch: Partial<PlanStep>) {
    setMessages((ms) =>
      ms.map((m, mi) =>
        mi === msgIdx && m.steps
          ? { ...m, steps: m.steps.map((s, si) => (si === stepIdx ? { ...s, ...patch } : s)) }
          : m,
      ),
    );
  }

  // Execute a parsed plan step-by-step, each step producing applyable code.
  async function executePlan(msgIdx: number) {
    const msg = messagesRef.current[msgIdx];
    if (!msg?.steps || sending) return;
    setMessages((ms) => ms.map((m, mi) => (mi === msgIdx ? { ...m, executing: true } : m)));
    for (let i = 0; i < msg.steps.length; i++) {
      updateStep(msgIdx, i, { status: 'running', result: undefined });
      const prompt = `Bối cảnh — kế hoạch:\n${msg.content}\n\nThực hiện BƯỚC ${i + 1}: "${msg.steps[i].title}".\n` +
        'Trả về code hoàn chỉnh cho các file cần đổi, mỗi block có dòng đầu `// file: <đường dẫn>`. Giải thích ngắn gọn.';
      try {
        const r = await api.chat([{ role: 'user', content: prompt }], pins, activePath, 'agent');
        updateStep(msgIdx, i, { status: 'done', result: r.text });
      } catch (e) {
        updateStep(msgIdx, i, { status: 'error', result: (e as Error).message });
      }
    }
    setMessages((ms) => ms.map((m, mi) => (mi === msgIdx ? { ...m, executing: false } : m)));
  }

  // Right-click "Hỏi AI về đoạn chọn": pin it and auto-ask.
  function askAboutSelection(pin: Pin) {
    if (sending) return;
    const loc = pin.end_line ? `${basename(pin.path)}:${pin.start_line}-${pin.end_line}` : basename(pin.path);
    sendChat(`Giải thích đoạn code đã ghim (${loc}): nó làm gì và có vấn đề gì không?`, [pin]);
  }

  // Open a diff preview of an AI-proposed edit (does not write yet).
  async function applyCode(code: string, target: string | null) {
    // Strip a leading `// file:` hint line if present.
    const body = /^(?:\/\/|#|<!--)\s*file:/i.test(code.split('\n', 1)[0] ?? '')
      ? code.split('\n').slice(1).join('\n')
      : code;
    const path = target ?? activePath;
    if (!path) { alert('Không rõ file đích. Mở một file trước, hoặc ghi `// file: path` ở đầu code block.'); return; }
    let original = tabs.find((t) => t.path === path)?.content;
    if (original === undefined) {
      try { original = (await api.file(path)).content; } catch { original = ''; }
    }
    setDiff({ path, original: original ?? '', modified: body });
  }

  // Write the currently-previewed diff to disk.
  async function commitDiff() {
    if (!diff) return;
    const { path, modified } = diff;
    setDiff(null);
    try {
      await api.save(path, modified);
      const open = tabs.find((t) => t.path === path);
      if (open) {
        setTabs((ts) => ts.map((t) => (t.path === path ? { ...t, content: modified, dirty: false } : t)));
        setActivePath(path);
      } else {
        await openFile(path);
      }
      loadGit();
      setRefreshKey((k) => k + 1);
    } catch (e) {
      alert('Ghi thất bại: ' + (e as Error).message);
    }
  }

  // ---- chat / whole-file context ----
  function addFileToChat() {
    const t = tabs.find((x) => x.path === activePath);
    if (!t || t.readOnly) return;
    const lines = t.content.split('\n').length;
    setPins((ps) =>
      ps.some((p) => p.path === t.path && p.start_line === 1 && p.end_line === lines)
        ? ps
        : [...ps, { path: t.path, start_line: 1, end_line: lines, code: t.content, lang: t.lang }],
    );
  }

  // Pin a file by path (for the chat `@`-mention picker).
  async function addFileByPath(path: string) {
    try {
      const f = await api.file(path);
      if (f.binary || f.too_large) return;
      const lines = f.content.split('\n').length;
      setPins((ps) => (ps.some((p) => p.path === path && p.start_line === 1) ? ps : [...ps, { path, start_line: 1, end_line: lines, code: f.content, lang: f.lang }]));
    } catch { /* ignore */ }
  }

  function toggleTerminal() {
    setShowTerm((v) => !v);
    setFitKey((k) => k + 1);
  }

  function openDeepwiki() {
    setNav('deepwiki');
    if (rootPath) api.deepwikiIndex(rootPath);
  }

  // Generate a Codemap: investigate the call-graph + write a grounded narrative.
  async function generateCodemap(start: string) {
    setNav('codemap');
    setCmBusy(true);
    setCmStart(start);
    setActiveCm(null);
    try {
      const inv = await api.dw.investigate(start, 2).catch(() => null);
      const ask = await api.dw.ask(start);
      const cm: Codemap = {
        id: String(performance.now()),
        start,
        title: inv?.focus ? `${inv.focus}` : start,
        narrative: ask.answer,
        matches: inv?.matches?.length ? inv.matches : ask.matches,
        nodes: inv?.nodes ?? [],
        focus: inv?.focus ?? null,
        at: Date.now(),
      };
      const next = [cm, ...codemaps].slice(0, 20);
      setCodemaps(next);
      try { localStorage.setItem('dw-codemaps-v2', JSON.stringify(next)); } catch { /* ignore */ }
      setActiveCm(cm);
    } catch (e) {
      alert('Tạo codemap lỗi: ' + (e as Error).message);
    }
    setCmBusy(false);
  }

  function deleteCodemap(id: string) {
    const next = codemaps.filter((c) => c.id !== id);
    setCodemaps(next);
    try { localStorage.setItem('dw-codemaps-v2', JSON.stringify(next)); } catch { /* ignore */ }
    if (activeCm?.id === id) setActiveCm(null);
  }

  // ---- resizers ----
  function startDrag(kind: 'sidebar' | 'chat') {
    return (e: React.MouseEvent) => {
      e.preventDefault();
      const move = (ev: MouseEvent) => {
        if (kind === 'sidebar') setSidebarW(Math.max(150, Math.min(560, ev.clientX - 48)));
        else setChatW(Math.max(260, Math.min(700, window.innerWidth - ev.clientX)));
      };
      const up = () => {
        window.removeEventListener('mousemove', move);
        window.removeEventListener('mouseup', up);
        document.body.style.cursor = '';
        setFitKey((k) => k + 1);
      };
      document.body.style.cursor = 'col-resize';
      window.addEventListener('mousemove', move);
      window.addEventListener('mouseup', up);
    };
  }

  function startTermDrag(e: React.MouseEvent) {
    e.preventDefault();
    const move = (ev: MouseEvent) =>
      setTermHeight(Math.max(120, Math.min(window.innerHeight - 160, window.innerHeight - ev.clientY - 22)));
    const up = () => {
      window.removeEventListener('mousemove', move);
      window.removeEventListener('mouseup', up);
      document.body.style.cursor = '';
    };
    document.body.style.cursor = 'row-resize';
    window.addEventListener('mousemove', move);
    window.addEventListener('mouseup', up);
  }

  if (!hasRoot) return <Welcome recents={recents} onOpen={openFolder} />;

  const activeTab = tabs.find((t) => t.path === activePath) ?? null;

  return (
    <div className="ide">
      <div className="ide-main">
      <div className="activity">
        <button className={nav === 'explorer' ? 'active' : ''} data-tip="Explorer" onClick={() => setNav('explorer')}>🗂</button>
        <button className={nav === 'search' ? 'active' : ''} data-tip="Tìm kiếm" onClick={() => setNav('search')}>🔍</button>
        <button className={nav === 'git' ? 'active' : ''} data-tip="Source Control" onClick={() => setNav('git')}>
          <svg width="19" height="19" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.35" strokeLinecap="round" strokeLinejoin="round">
            <circle cx="4.5" cy="3" r="1.7" />
            <circle cx="4.5" cy="13" r="1.7" />
            <circle cx="11.5" cy="5" r="1.7" />
            <path d="M4.5 4.7 V11.3" />
            <path d="M4.5 8 C4.5 5.8 6.5 5 9.8 5" />
          </svg>
        </button>
        <button className={nav === 'deepwiki' ? 'active' : ''} data-tip="DeepWiki — trí tuệ mã & wiki" onClick={openDeepwiki}>📖</button>
        <button className={nav === 'codemap' ? 'active' : ''} data-tip="Codemaps — bản đồ codebase" onClick={() => setNav('codemap')}>🗺</button>
        <div className="spacer" />
        <button data-tip={theme === 'dark' ? 'Chuyển sáng' : 'Chuyển tối'} onClick={() => setTheme((t) => (t === 'dark' ? 'light' : 'dark'))}>{theme === 'dark' ? '☀️' : '🌙'}</button>
        <button data-tip="Mở / đổi thư mục" onClick={() => setBrowseRoot(true)}>📁</button>
      </div>

      {nav !== 'git' && (
        <>
          <div className="sidebar" style={{ width: sidebarW }}>
            {nav === 'explorer' && (
              <>
                <div className="sidebar-head">
                  <span>{rootName ?? 'Explorer'}</span>
                  <div className="actions">
                    <button data-tip="File mới" onClick={() => newEntry(false)}>＋</button>
                    <button data-tip="Thư mục mới" onClick={() => newEntry(true)}>📁</button>
                    <button data-tip="Làm mới" onClick={async () => { setRoots(await api.tree('')); setRefreshKey((k) => k + 1); loadGit(); }}>⟳</button>
                  </div>
                </div>
                <div className="sidebar-body">
                  <Explorer roots={roots} activePath={activePath} gitFiles={gitFiles} onOpen={(p) => openFile(p)} refreshKey={refreshKey} />
                </div>
              </>
            )}
            {nav === 'search' && <SearchView onOpen={openFile} />}
            {nav === 'deepwiki' && <DeepWikiHistorySidebar view={dwView} onView={setDwView} />}
            {nav === 'codemap' && (
              <CodemapSidebar
                codemaps={codemaps}
                activeId={activeCm?.id ?? null}
                busy={cmBusy}
                onGenerate={generateCodemap}
                onSelect={setActiveCm}
                onDelete={deleteCodemap}
              />
            )}
          </div>
          <div className="resizer" onMouseDown={startDrag('sidebar')} />
        </>
      )}

      {nav === 'deepwiki' ? (
        <DeepWikiPanel rootPath={rootPath} onOpenFile={openFile} view={dwView} onView={setDwView} />
      ) : nav === 'codemap' ? (
        <CodemapMain codemap={activeCm} busy={cmBusy} busyStart={cmStart} onOpenFile={openFile} onGenerate={generateCodemap} />
      ) : nav === 'git' ? (
        <GitPanel rootName={rootName} monacoTheme={monacoTheme} onOpenFile={openFile} />
      ) : (
      <>
      <EditorPane
        tabs={tabs}
        activePath={activePath}
        onSelect={setActivePath}
        onClose={closeTab}
        onChange={onEditorChange}
        onPin={(p) => setPins((ps) => [...ps, p])}
        onAsk={askAboutSelection}
        onSave={saveActive}
        onAddFile={addFileToChat}
        onToggleTerminal={toggleTerminal}
        monacoTheme={monacoTheme}
        reveal={reveal}
        terminal={
          showTerm ? (
            <TerminalPanel
              height={termHeight}
              fitKey={fitKey}
              onClose={() => setShowTerm(false)}
              onResizeStart={startTermDrag}
            />
          ) : null
        }
      />

      {!chatCollapsed && (
        <>
          <div className="resizer" onMouseDown={startDrag('chat')} />
          <div style={{ width: chatW, display: 'flex', flexShrink: 0 }}>
            <ChatPanel
              messages={messages}
              pins={pins}
              sending={sending}
              model={model}
              models={models}
              activeModelId={activeModelId}
              onSelectModel={selectModel}
              mode={mode}
              onSelectMode={setMode}
              onSend={sendChat}
              onRemovePin={(i) => setPins((ps) => ps.filter((_, k) => k !== i))}
              onClearPins={() => setPins([])}
              onClear={() => setMessages([])}
              onApply={applyCode}
              onAddFile={addFileToChat}
              onMentionFile={addFileByPath}
              onToggleTerminal={toggleTerminal}
              onExecutePlan={executePlan}
              rootName={rootName}
              conversations={conversations}
              onNewChat={newChat}
              onLoadConversation={loadConversation}
              onDeleteConversation={deleteConversation}
              onClearAllConversations={clearAllConversations}
              onCollapse={() => setChatCollapsed(true)}
            />
          </div>
        </>
      )}
      </>
      )}
      </div>

      <StatusBar rootPath={rootPath} activeTab={activeTab} pins={pins.length} model={model} onToggleTerminal={toggleTerminal} chatCollapsed={chatCollapsed} onOpenChat={() => setChatCollapsed(false)} />
      <InputModal spec={modal} onClose={() => setModal(null)} />
      <DiffModal spec={diff} monacoTheme={monacoTheme} onConfirm={commitDiff} onCancel={() => setDiff(null)} />
      {browseRoot && (
        <FolderBrowser
          startPath={rootPath}
          onPick={(p) => { setBrowseRoot(false); openFolder(p); }}
          onClose={() => setBrowseRoot(false)}
        />
      )}
    </div>
  );

  // create a new file/dir at workspace root via an in-app modal
  function newEntry(dir: boolean) {
    setModal({
      title: dir ? 'Thư mục mới' : 'File mới',
      placeholder: dir ? 'ten-thu-muc' : 'duong/dan/file.rs',
      okLabel: 'Tạo',
      onSubmit: async (name) => {
        try {
          await api.create(name, dir);
          setRoots(await api.tree(''));
          setRefreshKey((k) => k + 1);
          if (!dir) openFile(name);
        } catch (e) {
          alert('Tạo thất bại: ' + (e as Error).message);
        }
      },
    });
  }
}

function StatusBar({ rootPath, activeTab, pins, model, onToggleTerminal, chatCollapsed, onOpenChat }: {
  rootPath: string | null; activeTab: Tab | null; pins: number; model: string | null; onToggleTerminal: () => void;
  chatCollapsed: boolean; onOpenChat: () => void;
}) {
  return (
    <div className="statusbar">
      <span className="seg" style={{ cursor: 'pointer' }} onClick={onToggleTerminal} title="Bật/tắt terminal (Ctrl+`)">⌘ Terminal</span>
      {chatCollapsed && <span className="seg" style={{ cursor: 'pointer' }} onClick={onOpenChat} title="Mở lại AI Chat">💬 Chat</span>}
      <span className="seg">⑂ {rootPath ? basename(rootPath) : '—'}</span>
      {activeTab && <span className="seg">{activeTab.lang}{activeTab.dirty ? ' ●' : ''}</span>}
      {pins > 0 && <span className="seg">📌 {pins} pinned</span>}
      <div className="right">
        <span className="seg">{model ? `🤖 ${model}` : '🤖 —'}</span>
        <span className="seg">SenClaw Code</span>
      </div>
    </div>
  );
}

function SearchView({ onOpen }: { onOpen: (path: string, line?: number) => void }) {
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [busy, setBusy] = useState(false);
  const timer = useRef<number>(0);

  function run(value: string) {
    setQ(value);
    window.clearTimeout(timer.current);
    if (value.trim().length < 2) { setHits([]); return; }
    timer.current = window.setTimeout(async () => {
      setBusy(true);
      try { setHits(await api.search(value, 200)); } catch { setHits([]); } finally { setBusy(false); }
    }, 250);
  }

  return (
    <>
      <div className="sidebar-head"><span>Tìm kiếm</span>{busy && <span className="spin">◐</span>}</div>
      <div style={{ padding: '4px 10px 8px' }}>
        <input
          autoFocus
          value={q}
          onChange={(e) => run(e.target.value)}
          placeholder="Tìm trong workspace…"
          style={{ width: '100%', background: 'var(--bg-input)', border: '1px solid var(--border-strong)', borderRadius: 6, color: 'var(--fg)', padding: '6px 8px', outline: 'none', fontSize: 13 }}
        />
      </div>
      <div className="sidebar-body">
        {hits.map((h, i) => (
          <div key={i} className="tree-row" style={{ height: 'auto', padding: '3px 10px', display: 'block' }} onClick={() => onOpen(h.path, h.line)} title={`${h.path}:${h.line}`}>
            <div style={{ color: 'var(--focus)', fontSize: 11, overflow: 'hidden', textOverflow: 'ellipsis' }}>{h.path}:{h.line}</div>
            <div style={{ color: 'var(--fg-dim)', fontSize: 12, fontFamily: 'ui-monospace, Menlo, monospace', overflow: 'hidden', textOverflow: 'ellipsis' }}>{h.text}</div>
          </div>
        ))}
        {!busy && q.trim().length >= 2 && hits.length === 0 && (
          <div style={{ padding: 16, color: 'var(--fg-mute)', fontSize: 12 }}>Không có kết quả.</div>
        )}
      </div>
    </>
  );
}

function Welcome({ recents, onOpen }: { recents: Recent[]; onOpen: (path: string) => void }) {
  const [path, setPath] = useState('');
  const [browsing, setBrowsing] = useState(false);
  return (
    <div className="welcome">
      <div className="welcome-card">
        <h1>💻 SenClaw Code</h1>
        <p>Mở một thư mục trên máy để bắt đầu. Editor giống VSCode, chat AI tích hợp bên phải, ghim code theo dòng bằng <b>Cmd/Ctrl + L</b>.</p>
        <div className="open-row">
          <input
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="Đường dẫn tuyệt đối, ví dụ ~/Projects/my-app"
            onKeyDown={(e) => { if (e.key === 'Enter' && path.trim()) onOpen(path.trim()); }}
          />
          <button className="btn ghost" onClick={() => setBrowsing(true)}>Duyệt…</button>
          <button className="btn" disabled={!path.trim()} onClick={() => onOpen(path.trim())}>Mở</button>
        </div>
        {browsing && (
          <FolderBrowser
            startPath={recents[0]?.path ?? null}
            onPick={(p) => { setBrowsing(false); onOpen(p); }}
            onClose={() => setBrowsing(false)}
          />
        )}
        {recents.length > 0 && (
          <div>
            <p style={{ marginBottom: 6 }}>Gần đây</p>
            <div className="recents">
              {recents.map((r) => (
                <div className="r" key={r.path} onClick={() => onOpen(r.path)}>
                  <span>📁</span>
                  <span className="rn">{r.name}</span>
                  <span className="rp">{dirname(r.path)}</span>
                  <span style={{ marginLeft: 'auto', color: 'var(--fg-mute)', fontSize: 11 }}>{timeAgo(r.openedAt)}</span>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
