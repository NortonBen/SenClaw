/**
 * WikiEditor — Markdown editor component
 *
 * Current implementation: controlled <textarea>, wrapped as a component for easy future replacement.
 */

interface Props {
  content: string;
  onChange: (val: string) => void;
}

export function WikiEditor({ content, onChange }: Props) {
  return (
    <textarea
      value={content}
      onChange={e => onChange(e.target.value)}
      className="w-full h-full resize-none outline-none font-mono text-sm text-gray-800 leading-relaxed p-4 bg-white"
      spellCheck={false}
      placeholder="# Title&#10;&#10;Write Markdown content here..."
    />
  );
}
