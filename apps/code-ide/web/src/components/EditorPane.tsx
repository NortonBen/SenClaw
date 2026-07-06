import { useEffect, useRef, useState } from 'react';
import Editor, { type OnMount } from '@monaco-editor/react';
import type { editor } from 'monaco-editor';
import type { Pin } from '../api';
import { basename, fileIcon } from '../lib';
import { MarkdownView } from './MarkdownView';

const IMG_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'svg', 'ico', 'avif'];
function isImage(path: string): boolean {
  return IMG_EXTS.includes(path.split('.').pop()?.toLowerCase() ?? '');
}
function isMarkdown(path: string): boolean {
  return ['md', 'markdown', 'mdx'].includes(path.split('.').pop()?.toLowerCase() ?? '');
}

export interface Tab {
  path: string;
  content: string;
  lang: string;
  dirty: boolean;
  readOnly: boolean;
  note?: string; // e.g. "binary" / "too large"
}

interface Props {
  tabs: Tab[];
  activePath: string | null;
  onSelect: (path: string) => void;
  onClose: (path: string) => void;
  onChange: (path: string, value: string) => void;
  onPin: (pin: Pin) => void;
  /** Pin the selection AND ask the AI about it (auto-sends a question). */
  onAsk: (pin: Pin) => void;
  onSave: () => void;
  /** Add the whole active file to the chat as context. */
  onAddFile: () => void;
  onToggleTerminal: () => void;
  /** Jump to a line in the active editor; bump `nonce` to re-trigger. */
  reveal?: { path: string; line: number; nonce: number };
  /** Terminal panel rendered below the editor (or null when hidden). */
  terminal?: React.ReactNode;
  monacoTheme: string;
}

export function EditorPane({
  tabs, activePath, onSelect, onClose, onChange, onPin, onAsk, onSave, onAddFile, onToggleTerminal, reveal, terminal, monacoTheme,
}: Props) {
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null);
  const [mdMode, setMdMode] = useState<'view' | 'edit'>('view');
  // Default markdown files to preview mode when switching to a new .md tab.
  useEffect(() => { if (activePath && isMarkdown(activePath)) setMdMode('view'); }, [activePath]);
  // Latest values captured for Monaco commands/actions (which bind once on mount).
  const ctx = useRef({ activePath, tabs, onPin, onAsk, onSave, onAddFile, onToggleTerminal });
  ctx.current = { activePath, tabs, onPin, onAsk, onSave, onAddFile, onToggleTerminal };

  const active = tabs.find((t) => t.path === activePath) ?? null;

  const handleMount: OnMount = (ed, monaco) => {
    editorRef.current = ed;

    // Build a Pin from the current selection (or the caret's line if empty).
    const selectionPin = (): Pin | null => {
      const e = editorRef.current;
      const { activePath: path, tabs: ts } = ctx.current;
      if (!e || !path) return null;
      const model = e.getModel();
      let sel = e.getSelection();
      if (!model || !sel) return null;
      if (sel.isEmpty()) {
        const ln = sel.startLineNumber;
        sel = sel.setStartPosition(ln, 1).setEndPosition(ln, model.getLineMaxColumn(ln));
      }
      const code = model.getValueInRange(sel);
      if (!code.trim()) return null;
      return {
        path,
        start_line: sel.startLineNumber,
        end_line: sel.endLineNumber,
        code,
        lang: ts.find((t) => t.path === path)?.lang,
      };
    };

    // ---- right-click context-menu actions (group "senclaw") ----
    ed.addAction({
      id: 'senclaw.pinSelection',
      label: '📌 Ghim đoạn chọn vào chat',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyL],
      contextMenuGroupId: 'senclaw',
      contextMenuOrder: 1,
      run: () => { const p = selectionPin(); if (p) ctx.current.onPin(p); },
    });
    ed.addAction({
      id: 'senclaw.askSelection',
      label: '💬 Hỏi AI về đoạn chọn',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyI],
      contextMenuGroupId: 'senclaw',
      contextMenuOrder: 2,
      run: () => { const p = selectionPin(); if (p) ctx.current.onAsk(p); },
    });
    ed.addAction({
      id: 'senclaw.addFile',
      label: '➕ Thêm cả file vào chat',
      keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyL],
      contextMenuGroupId: 'senclaw',
      contextMenuOrder: 3,
      run: () => ctx.current.onAddFile(),
    });

    // Cmd/Ctrl+S → save. Ctrl+` → toggle terminal. (No menu entry needed.)
    ed.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => ctx.current.onSave());
    ed.addCommand(monaco.KeyMod.WinCtrl | monaco.KeyCode.Backquote, () => ctx.current.onToggleTerminal());
  };

  // Reveal a requested line once the matching file is the active tab.
  useEffect(() => {
    const e = editorRef.current;
    if (!e || !reveal || reveal.path !== activePath) return;
    e.revealLineInCenter(reveal.line);
    e.setPosition({ lineNumber: reveal.line, column: 1 });
    e.focus();
  }, [reveal, activePath]);

  return (
    <div className="center">
      <div className="tabbar">
      <div className="tabs">
        {tabs.map((t) => (
          <div
            key={t.path}
            className={`tab${t.path === activePath ? ' active' : ''}`}
            onClick={() => onSelect(t.path)}
            title={t.path}
          >
            <span className="ico">{fileIcon(basename(t.path))}</span>
            <span className="name">{basename(t.path)}</span>
            {t.dirty ? (
              <span className="dot">●</span>
            ) : (
              <button className="x" onClick={(e) => { e.stopPropagation(); onClose(t.path); }}>×</button>
            )}
          </div>
        ))}
      </div>
        <div className="tab-actions">
          {active && isMarkdown(active.path) && !active.readOnly && (
            <div className="md-toggle">
              <button className={mdMode === 'view' ? 'active' : ''} onClick={() => setMdMode('view')}>👁 Xem</button>
              <button className={mdMode === 'edit' ? 'active' : ''} onClick={() => setMdMode('edit')}>✏️ Sửa</button>
            </div>
          )}
          {active && !active.readOnly && (
            <button data-tip="Thêm cả file vào chat (Cmd/Ctrl+Shift+L)" onClick={onAddFile}>＋ Chat</button>
          )}
          <button data-tip="Bật/tắt terminal (Ctrl+`)" onClick={onToggleTerminal}>⌘ Terminal</button>
        </div>
      </div>
      <div className="editor-wrap">
        {active ? (
          isImage(active.path) ? (
            <div className="img-view">
              <img src={`/api/raw?path=${encodeURIComponent(active.path)}`} alt={basename(active.path)} />
              <div className="img-cap">{basename(active.path)}</div>
            </div>
          ) : isMarkdown(active.path) && mdMode === 'view' && !active.readOnly ? (
            <div className="md-scroll"><MarkdownView text={active.content} /></div>
          ) : active.readOnly ? (
            <div style={{ padding: 24, color: 'var(--fg-mute)' }}>
              Không thể mở <b>{basename(active.path)}</b> — {active.note ?? 'không hỗ trợ'}.
            </div>
          ) : (
            <Editor
              theme={monacoTheme}
              path={active.path}
              language={active.lang}
              value={active.content}
              onChange={(v) => onChange(active.path, v ?? '')}
              onMount={handleMount}
              options={{
                fontSize: 13,
                minimap: { enabled: true },
                scrollBeyondLastLine: false,
                smoothScrolling: true,
                automaticLayout: true,
                tabSize: 2,
                renderWhitespace: 'selection',
                fontLigatures: true,
              }}
            />
          )
        ) : (
          <div style={{ display: 'flex', height: '100%', alignItems: 'center', justifyContent: 'center', color: 'var(--fg-mute)', flexDirection: 'column', gap: 8 }}>
            <div style={{ fontSize: 44, opacity: 0.4 }}>💻</div>
            <div>Chọn một file ở thanh bên để bắt đầu</div>
            <div style={{ fontSize: 12 }}>Bôi chọn code → <b>Cmd/Ctrl + L</b> để ghim vào chat</div>
          </div>
        )}
      </div>
      {terminal}
    </div>
  );
}
