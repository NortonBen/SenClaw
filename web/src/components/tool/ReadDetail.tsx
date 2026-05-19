import { Space, Tag, Typography, theme } from 'antd';
import type { ToolMessage } from '../../types';
import { DefaultDetail } from './DefaultDetail';

const { Text } = Typography;

/** Detail view for the `Read` (and MCP `*_read_file`) tools.
 *  Shows path + line range when the content carries them, else falls back
 *  to the generic JSON dump. */
export function ReadDetail({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const c = (message.content ?? {}) as Record<string, unknown>;
  const path = typeof c.path === 'string' ? c.path : typeof c.filePath === 'string' ? c.filePath : undefined;
  const lineStart = typeof c.lineStart === 'number' ? c.lineStart : undefined;
  const lineEnd = typeof c.lineEnd === 'number' ? c.lineEnd : undefined;
  const totalLines = typeof c.totalLines === 'number' ? c.totalLines : undefined;

  // Nothing structured we can use — fall through.
  if (!path && lineStart === undefined) return <DefaultDetail message={message} />;

  return (
    <Space direction="vertical" size={4} style={{ width: '100%' }}>
      {path && (
        <code style={{ background: token.colorBgLayout, padding: '2px 6px', borderRadius: 3, fontSize: 11 }}>
          {path}
        </code>
      )}
      <Space size={6} wrap>
        {lineStart !== undefined && lineEnd !== undefined && (
          <Tag color="blue">Lines {lineStart}–{lineEnd}</Tag>
        )}
        {totalLines !== undefined && (
          <Text type="secondary" style={{ fontSize: 11 }}>of {totalLines} total</Text>
        )}
      </Space>
    </Space>
  );
}
