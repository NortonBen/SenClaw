import { Space, Typography, theme } from 'antd';
import type { ToolMessage } from '../../types';
import { DefaultDetail } from './DefaultDetail';

const { Text } = Typography;
const MAX_DIFF_LINES = 20;
const MAX_WRITE_LINES = 20;

/** Detail view for Edit / NotebookEdit / Write and their MCP equivalents.
 *  - Edit: simple line-by-line diff (no LCS) with red/green coloring.
 *  - Write: "New file" badge + capped preview of the new content. */
export function EditDetail({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const c = (message.content ?? {}) as Record<string, unknown>;
  const path =
    typeof c.filePath === 'string' ? c.filePath :
    typeof c.path === 'string' ? c.path : undefined;
  const oldString = typeof c.oldString === 'string' ? c.oldString : undefined;
  const newString = typeof c.newString === 'string' ? c.newString : undefined;

  // No structured fields — bail to JSON.
  if (oldString === undefined && newString === undefined) return <DefaultDetail message={message} />;

  // Write-style payload (no oldString) — render newString preview.
  if (oldString === undefined && newString !== undefined) {
    const lines = newString.split('\n');
    const shown = lines.slice(0, MAX_WRITE_LINES);
    const extra = lines.length - shown.length;
    return (
      <Space direction="vertical" size={4} style={{ width: '100%' }}>
        {path && (
          <code style={{ background: token.colorBgLayout, padding: '2px 6px', borderRadius: 3, fontSize: 11 }}>
            {path}
          </code>
        )}
        <Text type="secondary" style={{ fontSize: 11 }}>New file</Text>
        <pre
          style={{
            margin: 0, padding: 8, background: token.colorBgLayout,
            border: `1px solid ${token.colorBorderSecondary}`, borderRadius: 4,
            fontSize: 11, maxHeight: 280, overflow: 'auto', whiteSpace: 'pre-wrap',
          }}
        >
          {shown.join('\n')}
          {extra > 0 && `\n… ${extra} more lines`}
        </pre>
      </Space>
    );
  }

  // Edit-style: render minus block + plus block. No LCS — simple but readable.
  const oldLines = (oldString ?? '').split('\n');
  const newLines = (newString ?? '').split('\n');
  const total = oldLines.length + newLines.length;
  const oldShown = oldLines.slice(0, MAX_DIFF_LINES);
  const newShown = newLines.slice(0, Math.max(0, MAX_DIFF_LINES - oldShown.length));
  const truncated = total - oldShown.length - newShown.length;

  return (
    <Space direction="vertical" size={4} style={{ width: '100%' }}>
      {path && (
        <code style={{ background: token.colorBgLayout, padding: '2px 6px', borderRadius: 3, fontSize: 11 }}>
          {path}
        </code>
      )}
      <div
        style={{
          padding: 8,
          background: token.colorBgLayout,
          border: `1px solid ${token.colorBorderSecondary}`,
          borderRadius: 4,
          fontFamily: 'monospace',
          fontSize: 11,
          maxHeight: 280,
          overflow: 'auto',
          whiteSpace: 'pre',
        }}
      >
        {oldShown.map((l, i) => (
          <div key={`-${i}`} style={{ color: token.colorError }}>- {l}</div>
        ))}
        {newShown.map((l, i) => (
          <div key={`+${i}`} style={{ color: token.colorSuccess }}>+ {l}</div>
        ))}
        {truncated > 0 && (
          <div style={{ color: token.colorTextQuaternary }}>… {truncated} more lines</div>
        )}
      </div>
    </Space>
  );
}
