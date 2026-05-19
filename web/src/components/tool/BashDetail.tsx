import { Space, Tag, Typography, theme } from 'antd';
import type { ToolMessage } from '../../types';
import { DefaultDetail } from './DefaultDetail';

const { Text } = Typography;
const MAX_STDOUT_LINES = 30;

/** Detail view for Bash. Splits command, stdout, stderr, exit code. */
export function BashDetail({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const c = (message.content ?? {}) as Record<string, unknown>;
  const command = typeof c.command === 'string' ? c.command : typeof c.cmd === 'string' ? c.cmd : undefined;
  const stdout = typeof c.stdout === 'string' ? c.stdout : typeof c.output === 'string' ? c.output : undefined;
  const stderr = typeof c.stderr === 'string' ? c.stderr : undefined;
  const exitCode = typeof c.exitCode === 'number' ? c.exitCode : undefined;

  if (command === undefined && stdout === undefined && stderr === undefined && exitCode === undefined) {
    return <DefaultDetail message={message} />;
  }

  const stdoutLines = stdout ? stdout.split('\n') : [];
  const stdoutShown = stdoutLines.slice(0, MAX_STDOUT_LINES);
  const stdoutExtra = stdoutLines.length - stdoutShown.length;

  return (
    <Space direction="vertical" size={6} style={{ width: '100%' }}>
      {command !== undefined && (
        <code
          style={{
            display: 'block', background: token.colorBgLayout, padding: '4px 8px',
            borderRadius: 4, fontSize: 11, whiteSpace: 'pre-wrap', wordBreak: 'break-all',
          }}
        >
          $ {command}
        </code>
      )}
      {exitCode !== undefined && (
        <Tag color={exitCode === 0 ? 'success' : 'error'}>exit {exitCode}</Tag>
      )}
      {stdoutShown.length > 0 && (
        <pre
          style={{
            margin: 0, padding: 8, background: token.colorBgLayout,
            border: `1px solid ${token.colorBorderSecondary}`, borderRadius: 4,
            fontSize: 11, maxHeight: 240, overflow: 'auto', whiteSpace: 'pre-wrap',
          }}
        >
          {stdoutShown.join('\n')}
          {stdoutExtra > 0 && `\n… ${stdoutExtra} more lines`}
        </pre>
      )}
      {(stderr || (!message.ok && !stderr)) && (
        <div
          style={{
            padding: 8,
            background: `${token.colorErrorBg}`,
            border: `1px solid ${token.colorErrorBorder}`,
            borderRadius: 4,
            color: token.colorError,
            fontSize: 11,
            fontFamily: 'monospace',
            whiteSpace: 'pre-wrap',
            wordBreak: 'break-word',
            maxHeight: 200,
            overflow: 'auto',
          }}
        >
          {stderr ?? <Text type="danger" style={{ fontSize: 11 }}>command failed</Text>}
        </div>
      )}
    </Space>
  );
}
