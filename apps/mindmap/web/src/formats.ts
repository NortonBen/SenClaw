// Import/export mind maps in standard formats: native JSON, Markdown outline,
// OPML, and FreeMind/Freeplane (.mm). All parsing is client-side.
import type { ImportNode, Layout, TreeNode } from './api'

export type Format = 'json' | 'markdown' | 'opml' | 'freemind'

export interface Parsed {
  title: string
  layout?: Layout
  children: ImportNode[]
}

// ---------- helpers ----------
function xmlEscape(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function stripTree(n: TreeNode): ImportNode {
  const out: ImportNode = { text: n.text }
  if (n.note) out.note = n.note
  if (n.color) out.color = n.color
  if (n.shape) out.shape = n.shape
  if (n.fill) out.fill = n.fill
  if (n.icon) out.icon = n.icon
  if (n.children.length) out.children = n.children.map(stripTree)
  return out
}

// ---------- export ----------
export function exportMap(tree: TreeNode, layout: Layout, fmt: Format): { text: string; ext: string; mime: string } {
  switch (fmt) {
    case 'json':
      return {
        text: JSON.stringify(
          { app: 'senclaw-mindmap', version: 1, title: tree.text, layout, tree: stripTree(tree) },
          null,
          2,
        ),
        ext: 'json',
        mime: 'application/json',
      }
    case 'markdown':
      return { text: toMarkdown(tree), ext: 'md', mime: 'text/markdown' }
    case 'opml':
      return { text: toOpml(tree), ext: 'opml', mime: 'text/x-opml' }
    case 'freemind':
      return { text: toFreeMind(tree), ext: 'mm', mime: 'application/x-freemind' }
  }
}

function toMarkdown(root: TreeNode): string {
  let out = `# ${root.text}\n\n`
  const walk = (n: TreeNode, depth: number) => {
    const pad = '  '.repeat(depth)
    const ic = n.icon ? `${n.icon} ` : ''
    out += `${pad}- ${ic}${n.text}${n.note ? ` — ${n.note}` : ''}\n`
    for (const c of n.children) walk(c, depth + 1)
  }
  for (const c of root.children) walk(c, 0)
  return out
}

function toOpml(root: TreeNode): string {
  const node = (n: TreeNode, depth: number): string => {
    const pad = '  '.repeat(depth)
    const attrs = `text="${xmlEscape((n.icon ? n.icon + ' ' : '') + n.text)}"${
      n.note ? ` _note="${xmlEscape(n.note)}"` : ''
    }`
    if (n.children.length === 0) return `${pad}<outline ${attrs}/>\n`
    return `${pad}<outline ${attrs}>\n${n.children.map((c) => node(c, depth + 1)).join('')}${pad}</outline>\n`
  }
  return (
    `<?xml version="1.0" encoding="UTF-8"?>\n<opml version="2.0">\n` +
    `  <head><title>${xmlEscape(root.text)}</title></head>\n  <body>\n` +
    node(root, 2) +
    `  </body>\n</opml>\n`
  )
}

function toFreeMind(root: TreeNode): string {
  const node = (n: TreeNode, depth: number): string => {
    const pad = '  '.repeat(depth)
    const attrs = `TEXT="${xmlEscape((n.icon ? n.icon + ' ' : '') + n.text)}"${
      n.color ? ` COLOR="${n.color}"` : ''
    }`
    if (n.children.length === 0) return `${pad}<node ${attrs}/>\n`
    return `${pad}<node ${attrs}>\n${n.children.map((c) => node(c, depth + 1)).join('')}${pad}</node>\n`
  }
  return `<map version="1.0.1">\n${node(root, 0)}</map>\n`
}

// ---------- import ----------
export function detectFormat(filename: string, content: string): Format {
  const ext = filename.split('.').pop()?.toLowerCase() ?? ''
  if (ext === 'json') return 'json'
  if (ext === 'opml') return 'opml'
  if (ext === 'mm') return 'freemind'
  if (ext === 'md' || ext === 'markdown' || ext === 'txt') return 'markdown'
  // sniff
  const t = content.trimStart()
  if (t.startsWith('{') || t.startsWith('[')) return 'json'
  if (t.includes('<opml')) return 'opml'
  if (t.includes('<map') && t.includes('<node')) return 'freemind'
  return 'markdown'
}

export function parseImport(filename: string, content: string): Parsed {
  const fmt = detectFormat(filename, content)
  const fallbackTitle = filename.replace(/\.[^.]+$/, '') || 'Sơ đồ nhập'
  switch (fmt) {
    case 'json':
      return parseJson(content, fallbackTitle)
    case 'opml':
      return parseXml(content, fallbackTitle, 'opml')
    case 'freemind':
      return parseXml(content, fallbackTitle, 'freemind')
    default:
      return parseMarkdown(content, fallbackTitle)
  }
}

function parseJson(content: string, fallbackTitle: string): Parsed {
  const data = JSON.parse(content)
  const asNode = (v: unknown): ImportNode | null => {
    if (!v || typeof v !== 'object') return null
    const o = v as Record<string, unknown>
    const text = typeof o.text === 'string' ? o.text : typeof o.title === 'string' ? o.title : ''
    const kids = Array.isArray(o.children) ? (o.children.map(asNode).filter(Boolean) as ImportNode[]) : []
    return { text, note: typeof o.note === 'string' ? o.note : undefined, children: kids }
  }
  if (data && typeof data === 'object' && 'tree' in data) {
    const root = asNode((data as { tree: unknown }).tree)
    return {
      title: (data as { title?: string }).title || root?.text || fallbackTitle,
      layout: (data as { layout?: Layout }).layout,
      children: root?.children ?? [],
    }
  }
  if (Array.isArray(data)) return { title: fallbackTitle, children: data.map(asNode).filter(Boolean) as ImportNode[] }
  const root = asNode(data)
  return { title: root?.text || fallbackTitle, children: root?.children ?? [] }
}

function parseMarkdown(content: string, fallbackTitle: string): Parsed {
  const entries: { depth: number; text: string }[] = []
  let base = 0
  for (const raw of content.split('\n')) {
    const line = raw.replace(/\r$/, '')
    if (!line.trim()) continue
    const h = line.match(/^(#{1,6})\s+(.*)$/)
    if (h) {
      base = h[1].length
      entries.push({ depth: base, text: h[2].trim() })
      continue
    }
    const b = line.match(/^(\s*)(?:[-*+]|\d+[.)])\s+(.*)$/)
    if (b) {
      const indent = b[1].replace(/\t/g, '  ').length
      entries.push({ depth: base + 1 + Math.floor(indent / 2), text: cleanInline(b[2].trim()) })
    }
  }
  return fromDepthEntries(entries, fallbackTitle)
}

function cleanInline(s: string): string {
  return s
    .replace(/\*\*(.*?)\*\*/g, '$1')
    .replace(/`([^`]*)`/g, '$1')
    .replace(/^\[[ xX]\]\s*/, '')
    .trim()
}

function fromDepthEntries(entries: { depth: number; text: string }[], fallbackTitle: string): Parsed {
  const roots: ImportNode[] = []
  const stack: { depth: number; node: ImportNode }[] = []
  for (const e of entries) {
    const node: ImportNode = { text: e.text, children: [] }
    while (stack.length && stack[stack.length - 1].depth >= e.depth) stack.pop()
    if (stack.length) stack[stack.length - 1].node.children!.push(node)
    else roots.push(node)
    stack.push({ depth: e.depth, node })
  }
  if (roots.length === 1) return { title: roots[0].text || fallbackTitle, children: roots[0].children ?? [] }
  return { title: fallbackTitle, children: roots }
}

function parseXml(content: string, fallbackTitle: string, kind: 'opml' | 'freemind'): Parsed {
  const doc = new DOMParser().parseFromString(content, 'text/xml')
  if (doc.querySelector('parsererror')) throw new Error('File XML không hợp lệ')

  if (kind === 'opml') {
    const title = doc.querySelector('head > title')?.textContent?.trim() || fallbackTitle
    const body = doc.querySelector('body')
    const conv = (el: Element): ImportNode => ({
      text: (el.getAttribute('text') ?? el.getAttribute('title') ?? '').trim(),
      note: el.getAttribute('_note') ?? undefined,
      children: childrenByTag(el, 'outline').map(conv),
    })
    const tops = body ? childrenByTag(body, 'outline').map(conv) : []
    if (tops.length === 1) return { title: tops[0].text || title, children: tops[0].children ?? [] }
    return { title, children: tops }
  }

  // freemind
  const mapEl = doc.querySelector('map')
  const rootNode = mapEl ? childrenByTag(mapEl, 'node')[0] : undefined
  if (!rootNode) return { title: fallbackTitle, children: [] }
  const conv = (el: Element): ImportNode => ({
    text: (el.getAttribute('TEXT') ?? '').trim(),
    color: el.getAttribute('COLOR') ?? undefined,
    children: childrenByTag(el, 'node').map(conv),
  })
  const root = conv(rootNode)
  return { title: root.text || fallbackTitle, children: root.children ?? [] }
}

function childrenByTag(parent: Element, tag: string): Element[] {
  return Array.from(parent.children).filter((c) => c.tagName.toLowerCase() === tag)
}

/** Trigger a browser download of `text`. */
export function download(filename: string, text: string, mime: string) {
  const blob = new Blob([text], { type: mime })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
