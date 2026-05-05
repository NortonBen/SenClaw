import React, { useEffect, useRef, useState } from 'react';
import {
  Input, Button, Tag, Select, Space, Typography, theme, Tooltip, message,
} from 'antd';
import {
  SaveOutlined, PlusOutlined, CloseOutlined, ArrowLeftOutlined,
} from '@ant-design/icons';
import type { SpaceNote, UseSpaceHook } from '../../../hooks/useSpace';

const { TextArea } = Input;
const { Text } = Typography;

const PRESET_TAGS = ['todo', 'idea', 'meeting', 'important', 'personal', 'work'];
const TAG_COLORS: Record<string, string> = {
  todo: 'blue', idea: 'purple', meeting: 'orange',
  important: 'red', personal: 'green', work: 'cyan',
};

interface Props {
  hook: UseSpaceHook;
  note: SpaceNote | null;
  isNew: boolean;
  onBack: () => void;
  onSaved: (note: SpaceNote) => void;
}

export function NoteEditor({ hook, note, isNew, onBack, onSaved }: Props) {
  const { token } = theme.useToken();
  const [title, setTitle] = useState(note?.title ?? '');
  const [body, setBody] = useState(note?.body ?? '');
  const [tags, setTags] = useState<string[]>(
    Array.isArray(note?.tags) ? note.tags : []
  );
  const [saving, setSaving] = useState(false);
  const [dirty, setDirty] = useState(false);
  const titleRef = useRef<HTMLInputElement>(null);

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

  const toggleTag = (t: string) => {
    setTags(prev => prev.includes(t) ? prev.filter(x => x !== t) : [...prev, t]);
    setDirty(true);
  };

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Tooltip title="Quay lại">
          <Button type="text" size="small" icon={<ArrowLeftOutlined />} onClick={onBack} />
        </Tooltip>
        <span className="flex-1 text-sm font-medium" style={{ color: token.colorTextSecondary }}>
          {isNew ? 'Ghi chú mới' : 'Chỉnh sửa ghi chú'}
        </span>
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

      {/* Tags bar */}
      <div
        className="flex flex-wrap items-center gap-1.5 px-4 py-2 border-b"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        {PRESET_TAGS.map(t => {
          const active = tags.includes(t);
          return (
            <Tag
              key={t}
              color={active ? TAG_COLORS[t] : undefined}
              style={{ cursor: 'pointer', opacity: active ? 1 : 0.4 }}
              onClick={() => toggleTag(t)}
            >
              {t}
            </Tag>
          );
        })}
      </div>

      {/* Title */}
      <input
        ref={titleRef}
        value={title}
        onChange={e => { setTitle(e.target.value); setDirty(true); }}
        placeholder="Tiêu đề..."
        onKeyDown={e => e.key === 'Enter' && e.preventDefault()}
        className="w-full px-4 py-3 text-xl font-bold outline-none bg-transparent border-b"
        style={{
          borderColor: token.colorBorderSecondary,
          color: token.colorText,
        }}
      />

      {/* Body — Markdown textarea */}
      <TextArea
        value={body}
        onChange={e => { setBody(e.target.value); setDirty(true); }}
        placeholder="Nội dung (Markdown)..."
        autoSize={false}
        bordered={false}
        className="flex-1 px-4 py-3 resize-none font-mono text-sm"
        style={{
          height: '100%',
          color: token.colorText,
          background: 'transparent',
        }}
        onKeyDown={e => {
          // Tab → insert 2 spaces
          if (e.key === 'Tab') {
            e.preventDefault();
            const el = e.target as HTMLTextAreaElement;
            const start = el.selectionStart;
            const end = el.selectionEnd;
            const next = body.slice(0, start) + '  ' + body.slice(end);
            setBody(next);
            setDirty(true);
            setTimeout(() => el.setSelectionRange(start + 2, start + 2), 0);
          }
          // Ctrl/Cmd+S → save
          if ((e.ctrlKey || e.metaKey) && e.key === 's') {
            e.preventDefault();
            handleSave();
          }
        }}
      />

      {/* Footer hint */}
      <div
        className="px-4 py-1.5 flex justify-between border-t"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Text type="secondary" style={{ fontSize: 11 }}>Markdown · Tab=indent · ⌘S=lưu</Text>
        {dirty && <Text type="warning" style={{ fontSize: 11 }}>● Chưa lưu</Text>}
      </div>
    </div>
  );
}
