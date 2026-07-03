import { useState } from 'react';
import { theme } from 'antd';

interface Props {
  path: string;
  label?: string;
}

export function CopyPathButton({ path, label }: Props) {
  const { token } = theme.useToken();
  const [copied, setCopied] = useState(false);
  const onCopy = async () => {
    try {
      await navigator.clipboard.writeText(path);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // Fallback: select the text.
      const ta = document.createElement('textarea');
      ta.value = path;
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand('copy'); setCopied(true); setTimeout(() => setCopied(false), 1500); } catch { /* ignore */ }
      document.body.removeChild(ta);
    }
  };
  return (
    <button
      type="button"
      onClick={onCopy}
      title={`Copy: ${path}`}
      className="text-[10px] px-1.5 py-0.5 rounded border transition-colors"
      style={{
        background: copied ? token.colorSuccessBg : token.colorFillQuaternary,
        borderColor: copied ? token.colorSuccessBorder : token.colorBorderSecondary,
        color: copied ? token.colorSuccess : token.colorTextSecondary,
      }}
      onMouseEnter={(e) => {
        if (copied) return;
        e.currentTarget.style.color = token.colorPrimary;
        e.currentTarget.style.borderColor = token.colorPrimary;
      }}
      onMouseLeave={(e) => {
        if (copied) return;
        e.currentTarget.style.color = token.colorTextSecondary;
        e.currentTarget.style.borderColor = token.colorBorderSecondary;
      }}
    >
      {copied ? '✓ copied' : (label ?? '⧉ copy path')}
    </button>
  );
}
