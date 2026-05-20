import { Space, Tag, Typography, theme } from 'antd';
import type { ToolMessage } from '../../types';
import { DefaultDetail } from './DefaultDetail';

const { Text } = Typography;
const MAX_DIFF_LINES = 60;
const MAX_WRITE_LINES = 40;

/** Detail view for Edit / NotebookEdit / Write and their MCP equivalents.
 *  Senclaw's Edit/Write tools emit `content.diff` as a unified-diff string
 *  (`make_unified_diff`). We parse it line-by-line and color `+`/`-` lines,
 *  matching the claude-code chat-inline diff rendering.
 *
 *  Falls back to `oldString`/`newString` (less-structured payload from MCP
 *  edit tools) and finally to a plain JSON dump. */
export function EditDetail({ message }: { message: ToolMessage }) {
  const { token } = theme.useToken();
  const c = (message.content ?? {}) as Record<string, unknown>;

  const path =
    typeof c.path === 'string' ? c.path :
    typeof c.filePath === 'string' ? c.filePath : undefined;
  const diffStr = typeof c.diff === 'string' ? c.diff : undefined;
  const oldString = typeof c.oldString === 'string' ? c.oldString : undefined;
  const newString = typeof c.newString === 'string' ? c.newString : undefined;
  const replacements = typeof c.replacements === 'number' ? c.replacements : undefined;
  const size = typeof c.size === 'number' ? c.size : undefined;

  // --- Path 1: unified-diff string (senclaw native Edit/Write output) ---
  if (diffStr) {
    const lines = diffStr.split('\n');
    const counts = lines.reduce((acc, l) => {
      if (l.startsWith('+++') || l.startsWith('---')) return acc;
      if (l.startsWith('+')) acc.plus++;
      else if (l.startsWith('-')) acc.minus++;
      return acc;
    }, { plus: 0, minus: 0 });

    const shown = lines.slice(0, MAX_DIFF_LINES);
    const extra = lines.length - shown.length;

    return (
      <Space direction="vertical" size={4} style={{ width: '100%' }}>
        <Space size={6} wrap>
          {path && (
            <code style={{ background: token.colorBgLayout, padding: '2px 6px', borderRadius: 3, fontSize: 11 }}>
              {path}
            </code>
          )}
          <Tag color="success">+{counts.plus}</Tag>
          <Tag color="error">−{counts.minus}</Tag>
          {replacements !== undefined && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              {replacements} replacement{replacements === 1 ? '' : 's'}
            </Text>
          )}
          {size !== undefined && (
            <Text type="secondary" style={{ fontSize: 11 }}>{size} bytes</Text>
          )}
        </Space>
        <DiffBlock lines={shown} extraCount={extra} />
      </Space>
    );
  }

  // --- Path 2: oldString / newString (less-structured) ---
  if (oldString !== undefined || newString !== undefined) {
    // Write-style (no oldString) — preview of new content.
    if (oldString === undefined && newString !== undefined) {
      const wlines = newString.split('\n');
      const wshown = wlines.slice(0, MAX_WRITE_LINES);
      const wextra = wlines.length - wshown.length;
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
              fontSize: 11, maxHeight: 320, overflow: 'auto', whiteSpace: 'pre-wrap',
            }}
          >
            {wshown.join('\n')}
            {wextra > 0 && `\n… ${wextra} more lines`}
          </pre>
        </Space>
      );
    }

    // Edit-style: synthesise a pseudo-diff from old/new strings.
    const oldLines = (oldString ?? '').split('\n');
    const newLines = (newString ?? '').split('\n');
    const pseudo = [
      ...oldLines.map(l => `-${l}`),
      ...newLines.map(l => `+${l}`),
    ];
    const pshown = pseudo.slice(0, MAX_DIFF_LINES);
    const pextra = pseudo.length - pshown.length;
    return (
      <Space direction="vertical" size={4} style={{ width: '100%' }}>
        {path && (
          <code style={{ background: token.colorBgLayout, padding: '2px 6px', borderRadius: 3, fontSize: 11 }}>
            {path}
          </code>
        )}
        <Space size={6}>
          <Tag color="success">+{newLines.length}</Tag>
          <Tag color="error">−{oldLines.length}</Tag>
        </Space>
        <DiffBlock lines={pshown} extraCount={pextra} />
      </Space>
    );
  }

  // --- Path 3: nothing recognisable — JSON dump ---
  return <DefaultDetail message={message} />;
}

/** Render an array of unified-diff lines with claude-code-style coloring. */
function DiffBlock({ lines, extraCount }: { lines: string[]; extraCount: number }) {
  const { token } = theme.useToken();
  return (
    <div
      style={{
        padding: 8,
        background: token.colorBgLayout,
        border: `1px solid ${token.colorBorderSecondary}`,
        borderRadius: 4,
        fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
        fontSize: 11,
        lineHeight: 1.5,
        maxHeight: 380,
        overflow: 'auto',
        whiteSpace: 'pre',
      }}
    >
      {lines.map((l, i) => {
        let bg = 'transparent';
        let color = token.colorText;
        if (l.startsWith('+++') || l.startsWith('---')) {
          color = token.colorTextTertiary;
        } else if (l.startsWith('@@')) {
          color = token.colorPrimary;
          bg = `${token.colorPrimary}10`;
        } else if (l.startsWith('+')) {
          color = token.colorSuccessText;
          bg = `${token.colorSuccess}15`;
        } else if (l.startsWith('-')) {
          color = token.colorErrorText;
          bg = `${token.colorError}15`;
        }
        return (
          <div key={i} style={{ background: bg, color, padding: '0 4px' }}>
            {l || ' '}
          </div>
        );
      })}
      {extraCount > 0 && (
        <div style={{ color: token.colorTextQuaternary, padding: '4px' }}>
          … {extraCount} more lines
        </div>
      )}
    </div>
  );
}
