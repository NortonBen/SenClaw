import React, { useEffect, useState } from 'react';
import {
  Input, Button, Tag, Empty, Spin, Tooltip, Popconfirm, Typography, theme,
} from 'antd';
import {
  PlusOutlined, SearchOutlined, DeleteOutlined, PushpinOutlined,
  PushpinFilled, TagOutlined,
} from '@ant-design/icons';
import type { SpaceNote, UseSpaceHook } from '../../../hooks/useSpace';

const { Text } = Typography;
const { Search } = Input;

const TAG_COLORS: Record<string, string> = {
  todo: 'blue',
  idea: 'purple',
  meeting: 'orange',
  important: 'red',
};

interface Props {
  hook: UseSpaceHook;
  selectedId: string | null;
  onSelect: (note: SpaceNote) => void;
  onNew: () => void;
}

export function NotesList({ hook, selectedId, onSelect, onNew }: Props) {
  const { token } = theme.useToken();
  const [searchQuery, setSearchQuery] = useState('');
  const [searchResults, setSearchResults] = useState<SpaceNote[] | null>(null);
  const [searching, setSearching] = useState(false);

  useEffect(() => {
    hook.loadNotes();
  }, []);

  const handleSearch = async (q: string) => {
    if (!q.trim()) { setSearchResults(null); return; }
    setSearching(true);
    const res = await hook.searchNotes(q);
    setSearchResults(res);
    setSearching(false);
  };

  const displayed = searchResults ?? hook.notes;

  const formatDate = (ms: number) => {
    const d = new Date(ms);
    const now = new Date();
    if (d.toDateString() === now.toDateString()) {
      return d.toLocaleTimeString('vi', { hour: '2-digit', minute: '2-digit' });
    }
    return d.toLocaleDateString('vi', { day: '2-digit', month: '2-digit' });
  };

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div className="p-3 flex gap-2 border-b" style={{ borderColor: token.colorBorderSecondary }}>
        <Search
          placeholder="Tìm ghi chú..."
          allowClear
          size="small"
          prefix={<SearchOutlined />}
          value={searchQuery}
          onChange={e => {
            setSearchQuery(e.target.value);
            if (!e.target.value) setSearchResults(null);
          }}
          onSearch={handleSearch}
          className="flex-1"
        />
        <Tooltip title="Tạo ghi chú mới">
          <Button type="primary" size="small" icon={<PlusOutlined />} onClick={onNew} />
        </Tooltip>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {(hook.notesLoading || searching) && (
          <div className="flex justify-center py-8"><Spin /></div>
        )}
        {!hook.notesLoading && !searching && displayed.length === 0 && (
          <Empty description="Chưa có ghi chú" className="py-8" />
        )}
        {displayed.map(note => {
          const active = note.id === selectedId;
          const tags: string[] = Array.isArray(note.tags)
            ? note.tags
            : JSON.parse(typeof note.tags === 'string' ? note.tags : '[]');

          return (
            <div
              key={note.id}
              onClick={() => onSelect(note)}
              className="px-3 py-2.5 border-b cursor-pointer group"
              style={{
                borderColor: token.colorBorderSecondary,
                background: active ? token.colorPrimaryBg : 'transparent',
              }}
            >
              <div className="flex items-start justify-between gap-1">
                <div className="flex items-center gap-1 min-w-0">
                  {note.pinned && (
                    <PushpinFilled style={{ color: token.colorPrimary, fontSize: 12, flexShrink: 0 }} />
                  )}
                  <Text
                    ellipsis
                    strong={active}
                    className="text-sm"
                    style={{ color: active ? token.colorPrimary : token.colorText }}
                  >
                    {note.title || '(Không tiêu đề)'}
                  </Text>
                </div>
                <Popconfirm
                  title="Xóa ghi chú này?"
                  onConfirm={e => { e?.stopPropagation(); hook.deleteNote(note.id); }}
                  onCancel={e => e?.stopPropagation()}
                  okText="Xóa"
                  cancelText="Hủy"
                >
                  <Button
                    type="text"
                    size="small"
                    danger
                    icon={<DeleteOutlined />}
                    className="opacity-0 group-hover:opacity-100 flex-shrink-0"
                    onClick={e => e.stopPropagation()}
                  />
                </Popconfirm>
              </div>

              <Text type="secondary" className="text-xs block mt-0.5" ellipsis>
                {note.body.slice(0, 80).replace(/[#*`]/g, '') || '—'}
              </Text>

              <div className="flex items-center justify-between mt-1">
                <div className="flex flex-wrap gap-1">
                  {tags.slice(0, 3).map(t => (
                    <Tag key={t} color={TAG_COLORS[t] ?? 'default'} style={{ fontSize: 10, padding: '0 4px', margin: 0 }}>
                      {t}
                    </Tag>
                  ))}
                </div>
                <Text type="secondary" style={{ fontSize: 10 }}>
                  {formatDate(note.updated_at)}
                </Text>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
