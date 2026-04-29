/**
 * WikiEditor — Markdown editor component
 *
 * Controlled <textarea>; swap implementations here only later if needed.
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
      placeholder="# Title&#10;&#10;Enter Markdown here..."
    />
  );
}
