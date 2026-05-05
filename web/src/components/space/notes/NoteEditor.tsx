import React, { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import {
  Button, Typography, theme, Tooltip, message, Segmented, Select,
} from 'antd';
import {
  SaveOutlined, ArrowLeftOutlined, BoldOutlined, ItalicOutlined,
  OrderedListOutlined, UnorderedListOutlined, CodeOutlined, LinkOutlined,
  EyeOutlined, EditOutlined, ColumnWidthOutlined,
} from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { SpaceNote, UseSpaceHook } from '../../../hooks/useSpace';

const { Text } = Typography;

const TAG_COLORS: Record<string, string> = {
  todo: '#1677ff', idea: '#722ed1', meeting: '#fa8c16',
  important: '#f5222d', personal: '#52c41a', work: '#13c2c2',
  note: '#8c8c8c', research: '#eb2f96', finance: '#faad14',
};

type EditorMode = 'edit' | 'preview' | 'split';

interface Props {
  hook: UseSpaceHook;
  note: SpaceNote | null;
  isNew: boolean;
  onBack: () => void;
  onSaved: (note: SpaceNote) => void;
}

// ─── Format helpers ───────────────────────────────────────────────────────────

interface FormatAction {
  before: string;
  after?: string;
  block?: boolean;
  linePrefix?: string;
}

function applyFormat(
  textarea: HTMLTextAreaElement,
  body: string,
  action: FormatAction,
  setBody: (v: string) => void,
) {
  const { selectionStart: start, selectionEnd: end } = textarea;
  const selected = body.slice(start, end);

  if (action.linePrefix) {
    // Line-level toggle (heading, list item)
    const lineStart = body.lastIndexOf('\n', start - 1) + 1;
    const lineEnd = body.indexOf('\n', end);
    const actualEnd = lineEnd === -1 ? body.length : lineEnd;
    const line = body.slice(lineStart, actualEnd);

    let newLine: string;
    if (line.startsWith(action.linePrefix)) {
      newLine = line.slice(action.linePrefix.length);
    } else {
      newLine = action.linePrefix + line;
    }

    const next = body.slice(0, lineStart) + newLine + body.slice(actualEnd);
    setBody(next);
    setTimeout(() => {
      const delta = newLine.length - line.length;
      textarea.setSelectionRange(start + delta, end + delta);
      textarea.focus();
    }, 0);
    return;
  }

  // Inline wrap
  const before = action.before;
  const after = action.after ?? before;
  let next: string;
  let newStart: number;
  let newEnd: number;

  if (selected) {
    next = body.slice(0, start) + before + selected + after + body.slice(end);
    newStart = start + before.length;
    newEnd = end + before.length;
  } else {
    const placeholder = action.block ? '\n' : 'văn bản';
    next = body.slice(0, start) + before + placeholder + after + body.slice(end);
    newStart = start + before.length;
    newEnd = newStart + placeholder.length;
  }

  setBody(next);
  setTimeout(() => {
    textarea.setSelectionRange(newStart, newEnd);
    textarea.focus();
  }, 0);
}

// ─── Markdown preview styles ──────────────────────────────────────────────────

const mdComponents = (token: ReturnType<typeof theme.useToken>['token']) => ({
  h1: ({ children }: any) => (
    <h1 style={{ color: token.colorText, borderBottom: `1px solid ${token.colorBorderSecondary}`, paddingBottom: 6, marginBottom: 12, fontSize: 22, fontWeight: 700 }}>{children}</h1>
  ),
  h2: ({ children }: any) => (
    <h2 style={{ color: token.colorText, fontSize: 18, fontWeight: 700, marginTop: 20, marginBottom: 8 }}>{children}</h2>
  ),
  h3: ({ children }: any) => (
    <h3 style={{ color: token.colorText, fontSize: 15, fontWeight: 600, marginTop: 16, marginBottom: 6 }}>{children}</h3>
  ),
  p: ({ children }: any) => (
    <p style={{ color: token.colorText, marginBottom: 10, lineHeight: 1.75 }}>{children}</p>
  ),
  strong: ({ children }: any) => (
    <strong style={{ color: token.colorText, fontWeight: 700 }}>{children}</strong>
  ),
  em: ({ children }: any) => (
    <em style={{ color: token.colorTextSecondary }}>{children}</em>
  ),
  ul: ({ children }: any) => (
    <ul style={{ color: token.colorText, paddingLeft: 20, marginBottom: 10 }}>{children}</ul>
  ),
  ol: ({ children }: any) => (
    <ol style={{ color: token.colorText, paddingLeft: 20, marginBottom: 10 }}>{children}</ol>
  ),
  li: ({ children }: any) => (
    <li style={{ marginBottom: 4, lineHeight: 1.7 }}>{children}</li>
  ),
  code: ({ inline, children }: any) => inline ? (
    <code style={{
      background: token.colorFillSecondary,
      color: token.colorError,
      padding: '1px 5px',
      borderRadius: 4,
      fontSize: '0.88em',
      fontFamily: 'monospace',
    }}>{children}</code>
  ) : (
    <pre style={{
      background: token.colorFillSecondary,
      border: `1px solid ${token.colorBorderSecondary}`,
      borderRadius: 6,
      padding: '12px 16px',
      overflowX: 'auto',
      marginBottom: 12,
    }}>
      <code style={{ fontFamily: 'monospace', fontSize: 13, color: token.colorText }}>{children}</code>
    </pre>
  ),
  blockquote: ({ children }: any) => (
    <blockquote style={{
      borderLeft: `3px solid ${token.colorPrimary}`,
      marginLeft: 0,
      paddingLeft: 14,
      color: token.colorTextSecondary,
      fontStyle: 'italic',
      marginBottom: 10,
    }}>{children}</blockquote>
  ),
  a: ({ href, children }: any) => (
    <a href={href} target="_blank" rel="noreferrer" style={{ color: token.colorPrimary }}>{children}</a>
  ),
  hr: () => <hr style={{ border: 'none', borderTop: `1px solid ${token.colorBorderSecondary}`, margin: '16px 0' }} />,
  table: ({ children }: any) => (
    <table style={{ borderCollapse: 'collapse', width: '100%', marginBottom: 12 }}>{children}</table>
  ),
  th: ({ children }: any) => (
    <th style={{ border: `1px solid ${token.colorBorderSecondary}`, padding: '6px 10px', background: token.colorFillSecondary, color: token.colorText, fontWeight: 600 }}>{children}</th>
  ),
  td: ({ children }: any) => (
    <td style={{ border: `1px solid ${token.colorBorderSecondary}`, padding: '6px 10px', color: token.colorText }}>{children}</td>
  ),
});

// ─── Component ────────────────────────────────────────────────────────────────

export function NoteEditor({ hook, note, isNew, onBack, onSaved }: Props) {
  const { token } = theme.useToken();
  const [title, setTitle] = useState(note?.title ?? '');
  const [body, setBody] = useState(note?.body ?? '');
  const [tags, setTags] = useState<string[]>(Array.isArray(note?.tags) ? note!.tags : []);
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [mode, setMode] = useState<EditorMode>('split');

  // Collect all unique tags across existing notes as suggestions
  const tagSuggestions = useMemo(() => {
    const set = new Set<string>();
    for (const n of hook.notes) {
      if (Array.isArray(n.tags)) n.tags.forEach(t => set.add(t));
    }
    return Array.from(set).sort();
  }, [hook.notes]);

  const titleRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    setTitle(note?.title ?? '');
    setBody(note?.body ?? '');
    setTags(Array.isArray(note?.tags) ? note!.tags : []);
    setDirty(false);
    if (isNew) setTimeout(() => titleRef.current?.focus(), 50);
  }, [note?.id, isNew]);

  const handleSave = async () => {
    if (!title.trim() && !body.trim()) {
      message.warning('Vui lòng nhập tiêu đề hoặc nội dung');
      return;
    }
    setSaving(true);
    try {
      if (isNew) {
        const created = await hook.createNote(title || '(Không tiêu đề)', body, tags);
        if (created) { onSaved(created); message.success('Đã tạo ghi chú'); }
      } else if (note) {
        await hook.updateNote(note.id, { title, body, tags });
        onSaved({ ...note, title, body, tags });
        message.success('Đã lưu');
      }
      setDirty(false);
    } finally {
      setSaving(false);
    }
  };

  const handleTagsChange = (newTags: string[]) => {
    setTags(newTags.map(t => t.trim().toLowerCase()).filter(Boolean));
    setDirty(true);
  };

  const format = useCallback((action: FormatAction) => {
    const ta = textareaRef.current;
    if (!ta) return;
    applyFormat(ta, body, action, (next) => { setBody(next); setDirty(true); });
  }, [body]);

  const toolbar: { icon: React.ReactNode; title: string; action: FormatAction }[] = [
    { icon: <BoldOutlined />, title: 'Bold (⌘B)', action: { before: '**', after: '**' } },
    { icon: <ItalicOutlined />, title: 'Italic (⌘I)', action: { before: '_', after: '_' } },
    { icon: <span className="text-xs font-bold leading-none">H1</span>, title: 'Heading 1', action: { linePrefix: '# ' } },
    { icon: <span className="text-xs font-bold leading-none">H2</span>, title: 'Heading 2', action: { linePrefix: '## ' } },
    { icon: <span className="text-xs font-bold leading-none">H3</span>, title: 'Heading 3', action: { linePrefix: '### ' } },
    { icon: <UnorderedListOutlined />, title: 'Bullet list', action: { linePrefix: '- ' } },
    { icon: <OrderedListOutlined />, title: 'Numbered list', action: { linePrefix: '1. ' } },
    { icon: <CodeOutlined />, title: 'Inline code', action: { before: '`', after: '`' } },
    { icon: <span className="text-xs font-mono leading-none">```</span>, title: 'Code block', action: { before: '```\n', after: '\n```', block: true } },
    { icon: <LinkOutlined />, title: 'Link', action: { before: '[', after: '](url)' } },
    { icon: <span className="text-xs font-bold leading-none">❝</span>, title: 'Blockquote', action: { linePrefix: '> ' } },
  ];

  return (
    <div className="flex flex-col h-full" style={{ minWidth: 0 }}>
      {/* ── Header ─────────────────────────────────────────────────────────── */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Tooltip title="Quay lại">
          <Button type="text" size="small" icon={<ArrowLeftOutlined />} onClick={onBack} />
        </Tooltip>
        <span className="flex-1 text-sm font-medium truncate" style={{ color: token.colorTextSecondary }}>
          {isNew ? 'Ghi chú mới' : 'Chỉnh sửa ghi chú'}
        </span>

        {/* View mode toggle */}
        <Segmented
          size="small"
          value={mode}
          onChange={v => setMode(v as EditorMode)}
          options={[
            { value: 'edit', icon: <EditOutlined />, title: 'Soạn thảo' },
            { value: 'split', icon: <ColumnWidthOutlined />, title: 'Chia đôi' },
            { value: 'preview', icon: <EyeOutlined />, title: 'Xem trước' },
          ]}
        />

        <Button
          type="primary"
          size="small"
          icon={<SaveOutlined />}
          loading={saving}
          disabled={!dirty && !isNew}
          onClick={handleSave}
        >
          Lưu
        </Button>
      </div>

      {/* ── Tags bar ───────────────────────────────────────────────────────── */}
      <div
        className="flex items-center gap-2 px-4 py-1.5 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Select
          mode="tags"
          size="small"
          value={tags}
          onChange={handleTagsChange}
          placeholder="Thêm tag... (nhập hoặc chọn gợi ý)"
          style={{ flex: 1 }}
          bordered={false}
          suffixIcon={null}
          tokenSeparators={[',', ' ']}
          options={tagSuggestions.map(t => ({
            value: t,
            label: (
              <span style={{ color: TAG_COLORS[t] ?? token.colorTextSecondary }}>
                # {t}
              </span>
            ),
          }))}
          tagRender={({ label, value, closable, onClose }) => (
            <span
              className="inline-flex items-center gap-1 rounded px-2 py-0.5 text-xs mr-1"
              style={{
                background: (TAG_COLORS[value] ?? token.colorPrimary) + '20',
                border: `1px solid ${(TAG_COLORS[value] ?? token.colorPrimary) + '50'}`,
                color: TAG_COLORS[value] ?? token.colorPrimary,
              }}
            >
              {value}
              {closable && (
                <span
                  onClick={onClose}
                  style={{ cursor: 'pointer', marginLeft: 2, opacity: 0.7 }}
                >
                  ×
                </span>
              )}
            </span>
          )}
        />
      </div>

      {/* ── Title ──────────────────────────────────────────────────────────── */}
      <input
        ref={titleRef}
        value={title}
        onChange={e => { setTitle(e.target.value); setDirty(true); }}
        placeholder="Tiêu đề..."
        onKeyDown={e => e.key === 'Enter' && e.preventDefault()}
        className="w-full px-4 py-3 text-xl font-bold outline-none bg-transparent border-b"
        style={{ borderColor: token.colorBorderSecondary, color: token.colorText }}
      />

      {/* ── Format toolbar (edit / split mode) ────────────────────────────── */}
      {mode !== 'preview' && (
        <div
          className="flex flex-wrap items-center gap-0.5 px-3 py-1.5 border-b flex-shrink-0"
          style={{ borderColor: token.colorBorderSecondary, background: token.colorFillQuaternary }}
        >
          {toolbar.map((item, i) => (
            <Tooltip key={i} title={item.title} mouseEnterDelay={0.6}>
              <button
                type="button"
                onClick={() => format(item.action)}
                className="flex items-center justify-center rounded transition-colors"
                style={{
                  width: 28,
                  height: 26,
                  border: 'none',
                  background: 'transparent',
                  cursor: 'pointer',
                  color: token.colorTextSecondary,
                }}
                onMouseEnter={e => (e.currentTarget.style.background = token.colorFillSecondary)}
                onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
              >
                {item.icon}
              </button>
            </Tooltip>
          ))}
          <div className="flex-1" />
          <Text type="secondary" style={{ fontSize: 10 }}>⌘S lưu · Tab thụt lề</Text>
        </div>
      )}

      {/* ── Editor / Preview pane ──────────────────────────────────────────── */}
      <div className="flex flex-1 min-h-0 overflow-hidden">

        {/* Edit pane */}
        {(mode === 'edit' || mode === 'split') && (
          <div
            className="flex flex-col"
            style={{ width: mode === 'split' ? '50%' : '100%', borderRight: mode === 'split' ? `1px solid ${token.colorBorderSecondary}` : 'none' }}
          >
            <textarea
              ref={textareaRef}
              value={body}
              onChange={e => { setBody(e.target.value); setDirty(true); }}
              placeholder="Nội dung Markdown..."
              className="flex-1 w-full px-4 py-3 resize-none outline-none bg-transparent font-mono text-sm"
              style={{
                color: token.colorText,
                lineHeight: 1.7,
                fontFamily: '"JetBrains Mono", "Fira Code", "Cascadia Code", monospace',
              }}
              onKeyDown={e => {
                if (e.key === 'Tab') {
                  e.preventDefault();
                  const el = e.target as HTMLTextAreaElement;
                  const s = el.selectionStart;
                  const en = el.selectionEnd;
                  const next = body.slice(0, s) + '  ' + body.slice(en);
                  setBody(next);
                  setDirty(true);
                  setTimeout(() => el.setSelectionRange(s + 2, s + 2), 0);
                }
                if ((e.ctrlKey || e.metaKey) && e.key === 's') {
                  e.preventDefault();
                  handleSave();
                }
                if ((e.ctrlKey || e.metaKey) && e.key === 'b') {
                  e.preventDefault();
                  format({ before: '**', after: '**' });
                }
                if ((e.ctrlKey || e.metaKey) && e.key === 'i') {
                  e.preventDefault();
                  format({ before: '_', after: '_' });
                }
              }}
            />
          </div>
        )}

        {/* Preview pane */}
        {(mode === 'preview' || mode === 'split') && (
          <div
            className="flex-1 overflow-y-auto px-6 py-4"
            style={{ width: mode === 'split' ? '50%' : '100%' }}
          >
            {body.trim() ? (
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={mdComponents(token) as any}
              >
                {body}
              </ReactMarkdown>
            ) : (
              <span style={{ color: token.colorTextQuaternary, fontSize: 14 }}>
                Preview sẽ hiện ở đây...
              </span>
            )}
          </div>
        )}
      </div>

      {/* ── Footer ─────────────────────────────────────────────────────────── */}
      <div
        className="px-4 py-1.5 flex items-center justify-between border-t flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Text type="secondary" style={{ fontSize: 11 }}>
          {body.length} ký tự · {body.split('\n').length} dòng
        </Text>
        {dirty && <Text type="warning" style={{ fontSize: 11 }}>● Chưa lưu</Text>}
      </div>
    </div>
  );
}
