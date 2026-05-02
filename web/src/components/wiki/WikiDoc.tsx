/**
 * WikiDoc — document detail page
 * Toggle view (Markdown render) vs edit (WikiEditor)
 */

import { useEffect, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github.css';
import { theme, Button, Tag, Typography, Space, Divider } from 'antd';
import { ArrowLeftOutlined, EditOutlined, SaveOutlined, CloseOutlined, CaretRightOutlined, CaretDownOutlined } from '@ant-design/icons';
import type { WikiDoc as WikiDocType } from '../../hooks/useWiki';
import { WikiEditor } from './WikiEditor';

const { Text } = Typography;

interface Props {
  path: string;
  doc: WikiDocType | null;
  loading: boolean;
  onBack: () => void;
  onLoad: (path: string) => void;
  onSave: (path: string, content: string) => Promise<void>;
  onRefresh: (path: string) => void;
}

function relativeTime(iso: string): string {
  if (!iso) return '';
  const diff = Date.now() - new Date(iso).getTime();
  const m = Math.floor(diff / 60000);
  if (m < 1) return 'just now';
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

export function WikiDoc({ path, doc, loading, onBack, onLoad, onSave, onRefresh }: Props) {
  const { token } = theme.useToken();
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState('');
  const [saving, setSaving] = useState(false);
  const [showHistory, setShowHistory] = useState(false);

  useEffect(() => {
    onLoad(path);
    setEditing(false);
    setShowHistory(false);
  }, [path, onLoad]);

  const handleEdit = () => {
    if (!doc) return;
    setEditContent(doc.content);
    setEditing(true);
  };

  const handleSave = async () => {
    setSaving(true);
    try {
      await onSave(path, editContent);
      setEditing(false);
      onRefresh(path);
    } finally {
      setSaving(false);
    }
  };

  const handleCancel = () => {
    setEditing(false);
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden" style={{ background: token.colorBgContainer }}>
      {/* Toolbar */}
      <div 
        className="flex items-center gap-3 px-6 py-3 flex-shrink-0"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}`, background: token.colorBgContainer }}
      >
        <Button 
          type="text" 
          icon={<ArrowLeftOutlined />} 
          onClick={onBack} 
          size="small"
          style={{ color: token.colorTextSecondary }}
        >
          Back
        </Button>

        <Divider type="vertical" style={{ margin: 0, borderColor: token.colorBorderSecondary }} />
        <Text type="secondary" ellipsis className="flex-1 font-mono text-xs">{path}</Text>

        {!editing && (
          <Button type="default" size="small" icon={<EditOutlined />} onClick={handleEdit}>
            Edit
          </Button>
        )}
        {editing && (
          <Space>
            <Button size="small" icon={<CloseOutlined />} onClick={handleCancel}>
              Cancel
            </Button>
            <Button size="small" type="primary" icon={<SaveOutlined />} loading={saving} onClick={handleSave}>
              Save
            </Button>
          </Space>
        )}
      </div>

      {loading && (
        <div className="flex-1 flex items-center justify-center">
          <Text type="secondary">Loading...</Text>
        </div>
      )}

      {!loading && doc && !editing && (
        <div className="flex-1 overflow-y-auto">
          <div className="max-w-3xl mx-auto px-8 py-6">
            {/* Meta */}
            {(doc.frontmatter.tags.length > 0 || doc.frontmatter.updated) && (
              <div className="flex items-center gap-3 mb-6 flex-wrap">
                <div className="flex gap-1.5 flex-wrap">
                  {doc.frontmatter.tags.map(t => <Tag color="orange" key={t}>{t}</Tag>)}
                </div>
                {doc.frontmatter.updated && (
                  <Text type="secondary" className="text-xs ml-auto">
                    {relativeTime(doc.frontmatter.updated)}
                  </Text>
                )}
              </div>
            )}

            {/* Markdown content */}
            <div 
              className="prose prose-sm max-w-none prose-headings:font-semibold"
              style={{
                color: token.colorText,
                '--tw-prose-body': token.colorText,
                '--tw-prose-headings': token.colorText,
                '--tw-prose-links': token.colorPrimary,
                '--tw-prose-bold': token.colorText,
                '--tw-prose-counters': token.colorTextSecondary,
                '--tw-prose-bullets': token.colorTextSecondary,
                '--tw-prose-hr': token.colorBorderSecondary,
                '--tw-prose-quotes': token.colorTextSecondary,
                '--tw-prose-quote-borders': token.colorBorderSecondary,
                '--tw-prose-captions': token.colorTextSecondary,
                '--tw-prose-code': token.colorWarning,
                '--tw-prose-pre-code': token.colorText,
                '--tw-prose-pre-bg': token.colorFillAlter,
                '--tw-prose-th-borders': token.colorBorderSecondary,
                '--tw-prose-td-borders': token.colorBorderSecondary,
              } as React.CSSProperties}
            >
              <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                {/* Strip frontmatter from display */}
                {doc.content.startsWith('---')
                  ? doc.content.replace(/^---[\s\S]*?\n---\n?/, '')
                  : doc.content}
              </ReactMarkdown>
            </div>

            {/* History */}
            {doc.gitLog.length > 0 && (
              <div className="mt-8 pt-4" style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}>
                <Button 
                  type="text" 
                  size="small" 
                  icon={showHistory ? <CaretDownOutlined /> : <CaretRightOutlined />}
                  onClick={() => setShowHistory(h => !h)}
                  style={{ color: token.colorTextSecondary, padding: 0 }}
                >
                  History ({doc.gitLog.length})
                </Button>
                {showHistory && (
                  <div className="mt-2 space-y-1">
                    {doc.gitLog.map(c => (
                      <div key={c.hash} className="flex items-center gap-3 text-xs" style={{ color: token.colorTextSecondary }}>
                        <Text type="secondary" className="font-mono text-[10px]">{c.hash.slice(0, 7)}</Text>
                        <Text className="flex-1 truncate" style={{ color: token.colorTextSecondary }}>{c.message}</Text>
                        <Text type="secondary" className="flex-shrink-0">{new Date(c.date).toLocaleDateString('zh-CN')}</Text>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {!loading && doc && editing && (
        <div className="flex-1 flex flex-col overflow-hidden">
          <WikiEditor content={editContent} onChange={setEditContent} />
        </div>
      )}
    </div>
  );
}
