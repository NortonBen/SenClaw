import type { CSSProperties } from 'react'
import { theme } from 'antd'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

/** Fragment target a citation is rewritten to. Relative, so react-markdown's
 *  URL sanitiser keeps it (a custom `zeach-cite:` scheme would be stripped). */
const CITE_HREF = '#zeach-cite-'

/**
 * Turn every bare `[n]` in the body into a link the reader can open.
 *
 * Skipped deliberately:
 *  - `[title](url)` — a real markdown link; the negative lookahead on `(` keeps
 *    the "Nguồn dẫn" appendix (`1. [Tiêu đề](https://…)`) intact.
 *  - fenced code blocks — a `[0]` in a code sample is an array index, not a
 *    citation.
 */
function linkifyCitations(md: string): string {
  return md
    .split(/(```[\s\S]*?```|~~~[\s\S]*?~~~)/g)
    .map((part, i) =>
      i % 2 === 1 ? part : part.replace(/\[(\d{1,3})\](?!\()/g, `[[$1]](${CITE_HREF}$1)`),
    )
    .join('')
}

/** Themed Markdown renderer for research reports. Colors come from AntD tokens
 *  so it tracks light/dark automatically. react-markdown escapes HTML, so the
 *  LLM/web-derived report body cannot inject markup.
 *
 *  With `onCite`, citations become clickable: `[n]` opens the evidence dialog
 *  instead of navigating away from the report. */
export default function Md({
  children,
  onCite,
}: {
  children: string
  onCite?: (n: number) => void
}) {
  const { token } = theme.useToken()
  const vars = {
    '--md-border': token.colorBorderSecondary,
    '--md-accent': token.colorPrimary,
    '--md-muted': token.colorTextSecondary,
    '--md-code-bg': token.colorFillSecondary,
    '--md-text': token.colorText,
  } as CSSProperties
  const body = onCite ? linkifyCitations(children) : children
  return (
    <div className="md" style={vars}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Standalone trong trình duyệt thật: mở tab mới thay vì rời trang.
          // Trong desktop webview, hook openExternal bắt click trước.
          a: ({ node: _node, href, ...props }) => {
            const n = href?.startsWith(CITE_HREF)
              ? Number(href.slice(CITE_HREF.length))
              : undefined
            if (onCite && n && Number.isFinite(n)) {
              return (
                <a
                  {...props}
                  href={href}
                  style={{ fontWeight: 600, textDecoration: 'none' }}
                  onClick={(ev) => {
                    ev.preventDefault()
                    onCite(n)
                  }}
                />
              )
            }
            return <a {...props} href={href} target="_blank" rel="noreferrer noopener" />
          },
        }}
      >
        {body}
      </ReactMarkdown>
    </div>
  )
}
