/**
 * WikiEditor — Markdown editor component
 *
 * Controlled <textarea>; swap implementations here only later if needed.
 */

import { theme } from 'antd';

interface Props {
  content: string;
  onChange: (val: string) => void;
}

export function WikiEditor({ content, onChange }: Props) {
  const { token } = theme.useToken();
  return (
    <textarea
      value={content}
      onChange={e => onChange(e.target.value)}
      className="w-full h-full resize-none outline-none font-mono text-sm leading-relaxed p-4"
      style={{
        backgroundColor: token.colorBgContainer,
        color: token.colorText,
      }}
      spellCheck={false}
      placeholder="# Title&#10;&#10;Enter Markdown here..."
    />
  );
}
