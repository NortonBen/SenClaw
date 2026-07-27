// Minimal markdown renderer — headings, lists, fenced/inline code, bold,
// italic, rules. Deliberately no dependency: rule docs are short and trusted.

import type { ReactNode } from 'react'

const INLINE = /(`[^`]+`)|(\*\*[^*]+\*\*)|(\*[^*\n]+\*)/g

function inline(src: string, key: string): ReactNode[] {
  const out: ReactNode[] = []
  let last = 0
  let i = 0
  INLINE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = INLINE.exec(src)) !== null) {
    if (m.index > last) out.push(src.slice(last, m.index))
    const t = m[0]
    const k = `${key}-${i++}`
    if (t.startsWith('`')) out.push(<code key={k}>{t.slice(1, -1)}</code>)
    else if (t.startsWith('**')) out.push(<strong key={k}>{t.slice(2, -2)}</strong>)
    else out.push(<em key={k}>{t.slice(1, -1)}</em>)
    last = m.index + t.length
  }
  if (last < src.length) out.push(src.slice(last))
  return out
}

export default function Markdown({ text }: { text: string }) {
  const source = (text ?? '').replace(/\r\n/g, '\n')
  if (!source.trim()) {
    return <div style={{ opacity: 0.6, fontSize: 13 }}>Node này chưa có tài liệu.</div>
  }

  const lines = source.split('\n')
  const blocks: ReactNode[] = []
  let list: string[] = []
  let ordered = false
  let para: string[] = []
  let code: string[] | null = null
  let codeLang = ''
  let n = 0

  const flushList = () => {
    if (list.length === 0) return
    const items = list.map((l, i) => <li key={i}>{inline(l, `li${n}-${i}`)}</li>)
    blocks.push(
      ordered ? <ol key={`b${n++}`}>{items}</ol> : <ul key={`b${n++}`}>{items}</ul>,
    )
    list = []
  }
  const flushPara = () => {
    if (para.length === 0) return
    const body = para.join(' ')
    blocks.push(<p key={`b${n++}`}>{inline(body, `p${n}`)}</p>)
    para = []
  }
  const flushAll = () => {
    flushList()
    flushPara()
  }

  for (const raw of lines) {
    const line = raw.replace(/\s+$/, '')

    if (line.trim().startsWith('```')) {
      if (code === null) {
        flushAll()
        code = []
        codeLang = line.trim().slice(3).trim()
      } else {
        blocks.push(
          <pre key={`b${n++}`} data-lang={codeLang}>
            <code>{code.join('\n')}</code>
          </pre>,
        )
        code = null
      }
      continue
    }
    if (code !== null) {
      code.push(raw)
      continue
    }

    if (line.trim() === '') {
      flushAll()
      continue
    }
    if (/^ {0,3}(-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
      flushAll()
      blocks.push(<hr key={`b${n++}`} />)
      continue
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(line)
    if (heading) {
      flushAll()
      const level = heading[1].length
      const content = inline(heading[2], `h${n}`)
      const Tag = (['h1', 'h2', 'h3', 'h4'] as const)[level - 1]
      blocks.push(<Tag key={`b${n++}`}>{content}</Tag>)
      continue
    }

    const bullet = /^\s*[-*+]\s+(.*)$/.exec(line)
    if (bullet) {
      flushPara()
      if (ordered && list.length) flushList()
      ordered = false
      list.push(bullet[1])
      continue
    }
    const num = /^\s*\d+[.)]\s+(.*)$/.exec(line)
    if (num) {
      flushPara()
      if (!ordered && list.length) flushList()
      ordered = true
      list.push(num[1])
      continue
    }

    flushList()
    para.push(line.trim())
  }

  if (code !== null) {
    blocks.push(
      <pre key={`b${n++}`}>
        <code>{code.join('\n')}</code>
      </pre>,
    )
  }
  flushAll()

  return <div className="md-body">{blocks}</div>
}
