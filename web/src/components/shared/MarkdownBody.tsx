import React from 'react';
import { theme } from 'antd';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import rehypeKatex from 'rehype-katex';
import rehypeHighlight from 'rehype-highlight';
import 'katex/dist/katex.min.css';
import 'highlight.js/styles/github.css';

interface MarkdownBodyProps {
  content: string;
  /** Thu nhỏ font/spacing cho khung preview. */
  compact?: boolean;
}

export function MarkdownBody({ content, compact }: MarkdownBodyProps) {
  const { token } = theme.useToken();
  const baseFont = compact ? 12 : 13;
  const lineHeight = compact ? 1.65 : 1.75;

  return (
    <div
      className="task-result-markdown"
      style={{
        fontSize: baseFont,
        lineHeight,
        color: token.colorText,
        wordBreak: 'break-word',
      }}
    >
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[rehypeKatex, rehypeHighlight]}
        components={{
          h1: ({ children }) => (
            <h1
              style={{
                color: token.colorText,
                borderBottom: `1px solid ${token.colorBorderSecondary}`,
                paddingBottom: 6,
                marginBottom: 10,
                fontSize: compact ? 18 : 22,
                fontWeight: 700,
              }}
            >
              {children}
            </h1>
          ),
          h2: ({ children }) => (
            <h2
              style={{
                color: token.colorText,
                fontSize: compact ? 15 : 18,
                fontWeight: 700,
                marginTop: compact ? 12 : 16,
                marginBottom: 6,
              }}
            >
              {children}
            </h2>
          ),
          h3: ({ children }) => (
            <h3
              style={{
                color: token.colorText,
                fontSize: compact ? 13 : 15,
                fontWeight: 600,
                marginTop: compact ? 10 : 14,
                marginBottom: 4,
              }}
            >
              {children}
            </h3>
          ),
          p: ({ children }) => (
            <p style={{ color: token.colorText, marginBottom: compact ? 8 : 10 }}>{children}</p>
          ),
          ul: ({ children }) => (
            <ul style={{ color: token.colorText, paddingLeft: 20, marginBottom: compact ? 8 : 10 }}>
              {children}
            </ul>
          ),
          ol: ({ children }) => (
            <ol style={{ color: token.colorText, paddingLeft: 20, marginBottom: compact ? 8 : 10 }}>
              {children}
            </ol>
          ),
          li: ({ children }) => (
            <li style={{ marginBottom: 3, lineHeight }}>{children}</li>
          ),
          code: ({ className, children }) => {
            const inline = !className;
            return inline ? (
              <code
                style={{
                  background: token.colorFillSecondary,
                  color: token.colorError,
                  padding: '1px 5px',
                  borderRadius: 4,
                  fontSize: '0.9em',
                  fontFamily: 'monospace',
                }}
              >
                {children}
              </code>
            ) : (
              <code
                className={className}
                style={{
                  fontFamily: 'monospace',
                  fontSize: compact ? 11 : 12,
                }}
              >
                {children}
              </code>
            );
          },
          pre: ({ children }) => (
            <pre
              style={{
                background: token.colorFillSecondary,
                border: `1px solid ${token.colorBorderSecondary}`,
                borderRadius: 6,
                padding: compact ? '8px 10px' : '10px 14px',
                overflowX: 'auto',
                marginBottom: compact ? 8 : 12,
              }}
            >
              {children}
            </pre>
          ),
          blockquote: ({ children }) => (
            <blockquote
              style={{
                borderLeft: `3px solid ${token.colorPrimary}`,
                marginLeft: 0,
                paddingLeft: 12,
                color: token.colorTextSecondary,
                marginBottom: compact ? 8 : 10,
              }}
            >
              {children}
            </blockquote>
          ),
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer" style={{ color: token.colorPrimary }}>
              {children}
            </a>
          ),
          hr: () => (
            <hr
              style={{
                border: 'none',
                borderTop: `1px solid ${token.colorBorderSecondary}`,
                margin: compact ? '10px 0' : '14px 0',
              }}
            />
          ),
          table: ({ children }) => (
            <div style={{ overflowX: 'auto', marginBottom: compact ? 8 : 12 }}>
              <table style={{ borderCollapse: 'collapse', width: '100%' }}>{children}</table>
            </div>
          ),
          th: ({ children }) => (
            <th
              style={{
                border: `1px solid ${token.colorBorderSecondary}`,
                padding: '5px 8px',
                background: token.colorFillSecondary,
                fontWeight: 600,
              }}
            >
              {children}
            </th>
          ),
          td: ({ children }) => (
            <td
              style={{
                border: `1px solid ${token.colorBorderSecondary}`,
                padding: '5px 8px',
              }}
            >
              {children}
            </td>
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
