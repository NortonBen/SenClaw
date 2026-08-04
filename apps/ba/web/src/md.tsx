/** Render markdown tài liệu BA: marked → DOMPurify → thay code fence mermaid
 * bằng SVG render sống. Sơ đồ hỏng hiển thị lỗi + giữ nguyên code để sửa. */
import { useEffect, useRef } from 'react'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import mermaid from 'mermaid'

mermaid.initialize({ startOnLoad: false, theme: 'neutral', securityLevel: 'strict' })

let seq = 0

export default function MarkdownView({ md }: { md: string }) {
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = ref.current
    if (!el) return
    const raw = marked.parse(md ?? '', { async: false }) as string
    el.innerHTML = DOMPurify.sanitize(raw, { ADD_ATTR: ['class'] })
    // Mermaid: tìm <pre><code class="language-mermaid">
    const blocks = el.querySelectorAll('code.language-mermaid')
    blocks.forEach((code) => {
      const pre = code.parentElement
      if (!pre) return
      const src = code.textContent ?? ''
      const box = document.createElement('div')
      box.className = 'mermaid-box'
      pre.replaceWith(box)
      const id = `mmd-${++seq}`
      mermaid
        .render(id, src)
        .then(({ svg }) => {
          box.innerHTML = DOMPurify.sanitize(svg, {
            USE_PROFILES: { svg: true, svgFilters: true },
            ADD_TAGS: ['foreignObject'],
            ADD_ATTR: ['dominant-baseline'],
          })
        })
        .catch((e) => {
          // Node lỗi mermaid v11 chèn vào body — dọn rồi hiện code gốc.
          document.getElementById(`d${id}`)?.remove()
          box.className = ''
          box.innerHTML = ''
          const err = document.createElement('div')
          err.className = 'mermaid-err'
          err.textContent = `⚠ sơ đồ mermaid lỗi cú pháp: ${String(e?.message ?? e).slice(0, 160)}`
          const preEl = document.createElement('pre')
          const codeEl = document.createElement('code')
          codeEl.textContent = src
          preEl.appendChild(codeEl)
          box.appendChild(err)
          box.appendChild(preEl)
        })
    })
  }, [md])

  return <div className="md-view" ref={ref} />
}
