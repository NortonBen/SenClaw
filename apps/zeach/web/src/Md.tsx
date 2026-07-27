import type { CSSProperties } from 'react'
import { theme } from 'antd'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'

/** Themed Markdown renderer for research reports. Colors come from AntD tokens
 *  so it tracks light/dark automatically. react-markdown escapes HTML, so the
 *  LLM/web-derived report body cannot inject markup. */
export default function Md({ children }: { children: string }) {
  const { token } = theme.useToken()
  const vars = {
    '--md-border': token.colorBorderSecondary,
    '--md-accent': token.colorPrimary,
    '--md-muted': token.colorTextSecondary,
    '--md-code-bg': token.colorFillSecondary,
    '--md-text': token.colorText,
  } as CSSProperties
  return (
    <div className="md" style={vars}>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
    </div>
  )
}
