import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import mermaid from 'mermaid';

let mmdSeq = 0;

/** Render a ```mermaid block into an SVG diagram (theme-aware). */
function Mermaid({ code }: { code: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    const light = document.documentElement.getAttribute('data-theme') === 'light';
    mermaid.initialize({ startOnLoad: false, securityLevel: 'loose', theme: light ? 'default' : 'dark' });
    const id = `mmd-${++mmdSeq}`;
    mermaid.render(id, code)
      .then(({ svg }) => { if (!cancelled && ref.current) { ref.current.innerHTML = svg; setErr(null); } })
      .catch((e) => { if (!cancelled) setErr(String(e?.message ?? e)); });
    return () => { cancelled = true; };
  }, [code]);
  if (err) return <pre className="mmd-err">Mermaid: {err}{'\n\n'}{code}</pre>;
  return <div className="mermaid-block" ref={ref} />;
}

export function MarkdownView({ text }: { text: string }) {
  return (
    <div className="md md-doc">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code(props) {
            const { className, children } = props as { className?: string; children?: React.ReactNode };
            const m = /language-(\w+)/.exec(className ?? '');
            const raw = String(children ?? '').replace(/\n$/, '');
            if (m?.[1] === 'mermaid') return <Mermaid code={raw} />;
            if (!m && !raw.includes('\n')) return <code className={className}>{children}</code>;
            return (
              <SyntaxHighlighter language={m?.[1] ?? 'text'} style={oneDark}
                customStyle={{ margin: '10px 0', fontSize: 13, borderRadius: 8, background: '#1b1b1b' }}>
                {raw}
              </SyntaxHighlighter>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
