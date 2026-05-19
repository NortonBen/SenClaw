import { Space, Tag, Typography, theme } from 'antd';
import type { ToolMessage } from '../../types';
import { DefaultDetail } from './DefaultDetail';

const { Text } = Typography;
const MAX_MATCHES = 5;

interface GrepMatch {
  file?: unknown;
  line?: unknown;
  text?: unknown;
}

/** Detail view for Grep. Shows pattern + first N matches as "file:line". */
export function GrepDetail({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const c = (message.content ?? {}) as Record<string, unknown>;
  const pattern = typeof c.pattern === 'string' ? c.pattern : undefined;
  const matches = Array.isArray(c.matches) ? (c.matches as GrepMatch[]) : undefined;

  if (pattern === undefined && matches === undefined) return <DefaultDetail message={message} />;

  const shown = matches ? matches.slice(0, MAX_MATCHES) : [];
  const extra = matches ? Math.max(0, matches.length - shown.length) : 0;

  return (
    <Space direction="vertical" size={4} style={{ width: '100%' }}>
      {pattern !== undefined && (
        <code style={{ background: token.colorBgLayout, padding: '2px 6px', borderRadius: 3, fontSize: 11 }}>
          /{pattern}/
        </code>
      )}
      {matches && (
        <Tag color="blue">{matches.length} match{matches.length === 1 ? '' : 'es'}</Tag>
      )}
      {shown.length > 0 && (
        <div style={{ fontFamily: 'monospace', fontSize: 11 }}>
          {shown.map((m, i) => {
            const file = typeof m.file === 'string' ? m.file : '?';
            const line = typeof m.line === 'number' ? m.line : '?';
            const text = typeof m.text === 'string' ? m.text : '';
            return (
              <div key={i} style={{ color: token.colorTextSecondary }}>
                <span style={{ color: token.colorTextTertiary }}>{file}:{line}</span>
                {text && <span style={{ marginLeft: 8 }}>{text}</span>}
              </div>
            );
          })}
          {extra > 0 && (
            <Text type="secondary" style={{ fontSize: 11 }}>… {extra} more</Text>
          )}
        </div>
      )}
    </Space>
  );
}
