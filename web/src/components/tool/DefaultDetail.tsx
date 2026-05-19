import { theme } from 'antd';
import type { ToolMessage } from '../../types';

/** Generic fallback used for any tool we don't have a custom renderer for.
 *  Preserves the original ToolGroupCard behaviour: a scrollable JSON dump. */
export function DefaultDetail({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const formatted =
    message.content == null
      ? ''
      : typeof message.content === 'string'
        ? message.content
        : safeStringify(message.content);
  return (
    <pre
      style={{
        margin: 0,
        padding: 8,
        background: token.colorBgLayout,
        border: `1px solid ${token.colorBorderSecondary}`,
        borderRadius: 4,
        fontSize: 11,
        maxHeight: 280,
        overflow: 'auto',
        whiteSpace: 'pre-wrap',
        wordBreak: 'break-word',
      }}
    >
      {formatted}
    </pre>
  );
}

function safeStringify(v: unknown): string {
  try {
    return JSON.stringify(v, null, 2);
  } catch {
    return String(v);
  }
}
