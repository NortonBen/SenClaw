import { useState } from 'react';
import { theme } from 'antd';
import { BulbFilled } from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';

/**
 * Compact "thinking" indicator — visually matches `ToolGroupCard` so a
 * conversation reads as one consistent timeline:
 *
 *   ● Read 3 files, ran 1 command  ›
 *   ◐ think                         ›
 *   ● cog add                       ›
 *
 * Collapsed (default) = single inline row (icon + "think" + chevron) at the
 * same font size / spacing as a tool group. Expanded = indented italic body
 * with the same left-border treatment ToolGroupCard uses.
 *
 * Shared between the chat view (`MessageBubble`) and the code view (`CodeView`)
 * so reasoning is split out of the visible answer identically in both places.
 */
export function ReasoningCollapsible({
  markdown,
  isDarkMode,
  embedded: _embedded,
}: {
  markdown: string;
  isDarkMode: boolean;
  embedded?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const { token } = theme.useToken();

  return (
    <div style={{ margin: '4px 0', padding: 0, background: 'transparent' }}>
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        aria-expanded={open}
        title={open ? 'Thu gọn phần suy luận' : 'Mở xem tiến trình tư duy'}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          background: 'transparent',
          border: 'none',
          padding: '4px 0',
          cursor: 'pointer',
          color: token.colorTextSecondary,
          fontSize: 13,
          textAlign: 'left',
          width: '100%',
        }}
      >
        <BulbFilled style={{ color: token.colorInfo, fontSize: 11 }} />
        <span style={{ color: 'inherit' }}>think</span>
        <span style={{ color: token.colorTextQuaternary }}>{open ? '▾' : '›'}</span>
      </button>

      {open && (
        <div
          style={{
            marginTop: 6,
            marginLeft: 18,
            paddingLeft: 12,
            borderLeft: `2px solid ${token.colorBorderSecondary}`,
          }}
        >
          <div
            className={`prose prose-sm max-w-none italic opacity-95 ${isDarkMode ? 'prose-invert' : ''}`}
            style={{ color: token.colorTextSecondary, fontSize: 12 }}
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
              {markdown}
            </ReactMarkdown>
          </div>
        </div>
      )}
    </div>
  );
}
