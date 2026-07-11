import { useCallback, useEffect, useRef, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import {
  api,
  outline,
  type ChatMsg,
  type ChatSession,
  type Layout,
  type MapMeta,
  type ModelInfo,
  type RestoreNode,
  type TemplateInfo,
  type TreeNode,
} from './api'
import MindmapCanvas, { type StylePatch } from './components/MindmapCanvas'
import ChatPanel from './components/ChatPanel'
import { layout as computeLayout } from './lib'
import { exportMap, parseImport, download, type Format } from './formats'

type Theme = 'light' | 'dark'

interface Settings {
  defaultLayout: Layout
  fullLabels: boolean
  showCount: boolean
}

const DEFAULT_SETTINGS: Settings = { defaultLayout: 'mindmap', fullLabels: false, showCount: false }

function loadSettings(): Settings {
  try {
    const raw = localStorage.getItem('mm-settings')
    if (raw) return { ...DEFAULT_SETTINGS, ...JSON.parse(raw) }
  } catch {
    /* ignore */
  }
  return DEFAULT_SETTINGS
}

const EXPORT_FORMATS: { id: Format; label: string; ext: string }[] = [
  { id: 'json', label: 'JSON (gốc)', ext: '.json' },
  { id: 'markdown', label: 'Markdown', ext: '.md' },
  { id: 'opml', label: 'OPML', ext: '.opml' },
  { id: 'freemind', label: 'FreeMind', ext: '.mm' },
]

/** Initial theme: senclaw's shared `theme` key, then our own, then OS preference. */
function detectTheme(): Theme {
  try {
    const shared = localStorage.getItem('theme')
    if (shared === 'dark' || shared === 'light') return shared
    const own = localStorage.getItem('mm-theme')
    if (own === 'dark' || own === 'light') return own
  } catch {
    /* ignore */
  }
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

const LAYOUTS: { id: Layout; label: string; icon: string }[] = [
  { id: 'mindmap', label: 'Sơ đồ tư duy', icon: '🧠' },
  { id: 'org', label: 'Sơ đồ tổ chức', icon: '🏛️' },
  { id: 'outline', label: 'Danh sách', icon: '☰' },
  { id: 'right', label: 'Cây ngang', icon: '🌿' },
]

function findNode(n: TreeNode | null, id: number): TreeNode | null {
  if (!n) return null
  if (n.id === id) return n
  for (const c of n.children) {
    const f = findNode(c, id)
    if (f) return f
  }
  return null
}

/** Flatten a tree into the flat node list used by the restore (undo/redo) API. */
function flattenTree(root: TreeNode): RestoreNode[] {
  const out: RestoreNode[] = []
  const walk = (n: TreeNode, parentId: number | null, ord: number) => {
    out.push({
      id: n.id,
      parent_id: parentId,
      text: n.text,
      note: n.note,
      color: n.color,
      shape: n.shape,
      fill: n.fill,
      icon: n.icon,
      pos_x: n.pos_x,
      pos_y: n.pos_y,
      collapsed: n.collapsed,
      ord,
    })
    n.children.forEach((c, i) => walk(c, n.id, i))
  }
  walk(root, null, 0)
  return out
}

interface Snapshot {
  tree: TreeNode
  layout: Layout
}

/** Return a new tree with the given nodes' positions applied (immutable). */
function applyPositions(root: TreeNode, items: { id: number; x: number; y: number }[]): TreeNode {
  const map = new Map(items.map((i) => [i.id, i]))
  const walk = (n: TreeNode): TreeNode => {
    const p = map.get(n.id)
    const next: TreeNode = p ? { ...n, pos_x: p.x, pos_y: p.y } : n
    if (n.children.length === 0) return next === n ? n : next
    return { ...next, children: n.children.map(walk) }
  }
  return walk(root)
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(detectTheme)
  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
    try {
      localStorage.setItem('mm-theme', theme)
    } catch {
      /* ignore */
    }
  }, [theme])

  // Follow the SenClaw desktop/host theme via postMessage (like the other apps).
  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const d = e.data
      if (!d || typeof d !== 'object') return
      const t = d.theme ?? d.env?.theme
      if ((d.type === 'senclaw:init' || d.type === 'senclaw:theme') && (t === 'dark' || t === 'light')) {
        setTheme(t)
      }
    }
    window.addEventListener('message', onMessage)
    try {
      window.parent?.postMessage({ type: 'senclaw:ready' }, '*')
    } catch {
      /* ignore */
    }
    return () => window.removeEventListener('message', onMessage)
  }, [])

  const [maps, setMaps] = useState<MapMeta[]>([])
  const [mapId, setMapId] = useState<number | null>(null)
  const [tree, setTree] = useState<TreeNode | null>(null)
  const [mapLayout, setMapLayout] = useState<Layout>('mindmap')

  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [editingId, setEditingId] = useState<number | null>(null)
  const [generatingId, setGeneratingId] = useState<number | null>(null)

  const [chat, setChat] = useState<ChatMsg[]>([])
  const [chatBusy, setChatBusy] = useState(false)
  const [showChat, setShowChat] = useState(true)
  const [sessions, setSessions] = useState<ChatSession[]>([])
  const [activeSession, setActiveSession] = useState<number | null>(null)
  const [importing, setImporting] = useState(false)

  const [models, setModels] = useState<ModelInfo[]>([])
  const [activeModel, setActiveModel] = useState<string | null>(null)
  const [llmOk, setLlmOk] = useState<boolean | null>(null)
  const [llmModel, setLlmModel] = useState<string | null>(null)

  const [newMap, setNewMap] = useState(false)
  const [showTemplates, setShowTemplates] = useState(false)
  const [templates, setTemplates] = useState<TemplateInfo[]>([])
  const [genModal, setGenModal] = useState<{ nodeId: number; text: string } | null>(null)
  const [toast, setToast] = useState<{ msg: string; err?: boolean } | null>(null)

  const [settings, setSettings] = useState<Settings>(loadSettings)
  useEffect(() => {
    try {
      localStorage.setItem('mm-settings', JSON.stringify(settings))
    } catch {
      /* ignore */
    }
  }, [settings])
  const [dragMode, setDragMode] = useState(false)
  const [showSettings, setShowSettings] = useState(false)
  const [showIO, setShowIO] = useState(false)
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const importRef = useRef<HTMLInputElement>(null)

  // Undo/redo — snapshot-based history of the current map's tree + layout.
  const [undoStack, setUndoStack] = useState<Snapshot[]>([])
  const [redoStack, setRedoStack] = useState<Snapshot[]>([])
  const treeRef = useRef<TreeNode | null>(null)
  treeRef.current = tree
  const layoutRef = useRef<Layout>('mindmap')
  layoutRef.current = mapLayout

  // Node notes, right-click menu, and chat context pins.
  const [ctxMenu, setCtxMenu] = useState<{ nodeId: number; x: number; y: number } | null>(null)
  const [noteEdit, setNoteEdit] = useState<{ nodeId: number; text: string; note: string } | null>(null)
  const [noteView, setNoteView] = useState<{ nodeId: number; text: string; note: string } | null>(null)
  const [notingId, setNotingId] = useState<number | null>(null)
  const [pins, setPins] = useState<{ id: number; text: string; note: string }[]>([])

  const flash = useCallback((msg: string, err = false) => {
    setToast({ msg, err })
    setTimeout(() => setToast(null), 2600)
  }, [])

  const loadMaps = useCallback(async () => {
    try {
      setMaps(await api.maps())
    } catch (e) {
      flash(String(e), true)
    }
  }, [flash])

  const refreshTree = useCallback(async (id: number) => {
    const { tree } = await api.getMap(id)
    setTree(tree)
  }, [])

  const openSession = useCallback(
    async (sid: number) => {
      try {
        const rows = await api.messages(sid)
        setActiveSession(sid)
        setChat(
          rows.map((r) => ({
            role: r.role === 'user' ? 'user' : 'assistant',
            content: r.content,
            model: r.model ?? undefined,
          })),
        )
      } catch (e) {
        flash(String(e), true)
      }
    },
    [flash],
  )

  const loadSessions = useCallback(
    async (mid: number, openFirst = true) => {
      try {
        const list = await api.sessions(mid)
        setSessions(list)
        if (openFirst) {
          if (list.length > 0) await openSession(list[0].id)
          else {
            setActiveSession(null)
            setChat([])
          }
        }
        return list
      } catch {
        return []
      }
    },
    [openSession],
  )

  const openMap = useCallback(
    async (id: number) => {
      try {
        const { meta, tree } = await api.getMap(id)
        setMapId(id)
        setTree(tree)
        setMapLayout(meta.layout)
        setSelectedId(tree?.id ?? null)
        setEditingId(null)
        setUndoStack([])
        setRedoStack([])
        setPins([])
        setCtxMenu(null)
        await loadSessions(id)
      } catch (e) {
        flash(String(e), true)
      }
    },
    [flash, loadSessions],
  )

  useEffect(() => {
    loadMaps()
    api.templates().then(setTemplates).catch(() => {})
    api.llmInfo().then((r) => {
      setLlmOk(r.ok)
      setLlmModel(r.model ?? null)
    })
    api.models().then((r) => {
      setModels(r.configs)
      setActiveModel(r.activeId)
    })
  }, [loadMaps])

  // ---- map ops ----
  const createMap = async (
    title: string,
    description: string,
    layout: Layout,
    withAi: boolean,
    instruction: string,
  ) => {
    try {
      const { id, rootId } = await api.createMap(title, description, layout)
      await loadMaps()
      await openMap(id)
      if (withAi) {
        setGeneratingId(rootId)
        try {
          const r = await api.generate(rootId, { topic: title, instruction })
          flash(`Đã sinh ${r.added} nút · ${r.model}`)
        } catch (e) {
          flash(String(e), true)
        } finally {
          setGeneratingId(null)
          await refreshTree(id)
        }
      }
    } catch (e) {
      flash(String(e), true)
    }
  }

  const deleteMap = async (id: number) => {
    if (!confirm('Xoá mindmap này?')) return
    await api.deleteMap(id)
    if (mapId === id) {
      setMapId(null)
      setTree(null)
      setSelectedId(null)
      setSessions([])
      setActiveSession(null)
      setChat([])
    }
    await loadMaps()
  }

  const createFromTemplate = async (templateId: string) => {
    try {
      const { id } = await api.createFromTemplate(templateId)
      await loadMaps()
      await openMap(id)
      setShowTemplates(false)
    } catch (e) {
      flash(String(e), true)
    }
  }

  const changeLayout = async (layout: Layout) => {
    if (mapId == null) return
    pushHistory()
    setMapLayout(layout)
    try {
      await api.setLayout(mapId, layout)
      await loadMaps()
    } catch (e) {
      flash(String(e), true)
    }
  }

  const styleNode = async (id: number, patch: StylePatch) => {
    if (mapId == null) return
    pushHistory()
    try {
      await api.updateNode(id, patch)
      await refreshTree(mapId)
    } catch (e) {
      flash(String(e), true)
    }
  }

  // ---- undo / redo ----
  const pushHistory = useCallback(() => {
    const t = treeRef.current
    if (!t) return
    setUndoStack((s) => [...s.slice(-49), { tree: t, layout: layoutRef.current }])
    setRedoStack([])
  }, [])

  const restoreSnapshot = useCallback(
    async (snap: Snapshot) => {
      if (mapId == null) return
      try {
        await api.restoreMap(mapId, snap.layout, flattenTree(snap.tree))
        setMapLayout(snap.layout)
        await refreshTree(mapId)
        await loadMaps()
      } catch (e) {
        flash(String(e), true)
      }
    },
    [mapId, refreshTree, loadMaps, flash],
  )

  const undo = useCallback(async () => {
    if (mapId == null || treeRef.current == null || undoStack.length === 0) return
    const prev = undoStack[undoStack.length - 1]
    setUndoStack((u) => u.slice(0, -1))
    setRedoStack((r) => [...r, { tree: treeRef.current!, layout: layoutRef.current }])
    await restoreSnapshot(prev)
  }, [mapId, undoStack, restoreSnapshot])

  const redo = useCallback(async () => {
    if (mapId == null || treeRef.current == null || redoStack.length === 0) return
    const next = redoStack[redoStack.length - 1]
    setRedoStack((r) => r.slice(0, -1))
    setUndoStack((u) => [...u, { tree: treeRef.current!, layout: layoutRef.current }])
    await restoreSnapshot(next)
  }, [mapId, redoStack, restoreSnapshot])

  // ---- notes ----
  const openNote = (nodeId: number) => {
    const node = findNode(tree, nodeId)
    if (!node) return
    setNoteEdit({ nodeId, text: node.text, note: node.note })
    setCtxMenu(null)
  }

  const expandNote = (nodeId: number) => {
    const node = findNode(tree, nodeId)
    if (!node) return
    setNoteView({ nodeId, text: node.text, note: node.note })
  }

  const saveNote = async (nodeId: number, note: string) => {
    if (mapId == null) return
    pushHistory()
    setNoteEdit(null)
    try {
      await api.updateNode(nodeId, { note })
      await refreshTree(mapId)
    } catch (e) {
      flash(String(e), true)
    }
  }

  const removeNote = async (nodeId: number) => {
    if (mapId == null) return
    const node = findNode(tree, nodeId)
    if (!node || !node.note) return
    pushHistory()
    try {
      await api.updateNode(nodeId, { note: '' })
      await refreshTree(mapId)
      flash('Đã xoá ghi chú')
    } catch (e) {
      flash(String(e), true)
    }
  }

  const aiWriteNote = async (nodeId: number) => {
    if (mapId == null) return
    setCtxMenu(null)
    pushHistory()
    setNotingId(nodeId)
    try {
      const r = await api.aiNote(nodeId)
      flash(`AI đã viết ghi chú · ${r.model}`)
      // If the note editor is open on this node, show the generated text.
      setNoteEdit((prev) => (prev && prev.nodeId === nodeId ? { ...prev, note: r.note } : prev))
    } catch (e) {
      flash(String(e), true)
    } finally {
      setNotingId(null)
      await refreshTree(mapId)
    }
  }

  // ---- chat context pins ----
  const pinNode = (nodeId: number) => {
    const node = findNode(tree, nodeId)
    if (!node) return
    setPins((ps) => (ps.some((p) => p.id === nodeId) ? ps : [...ps, { id: nodeId, text: node.text, note: node.note }]))
    setShowChat(true)
    setCtxMenu(null)
  }
  const unpin = (id: number) => setPins((ps) => ps.filter((p) => p.id !== id))
  const clearPins = () => setPins([])

  // Save an AI answer as the note of the selected node (or the first pinned node).
  const saveAiAsNote = async (textVal: string) => {
    const target = selectedId ?? pins[0]?.id ?? null
    if (target == null) {
      flash('Chọn một nút để lưu ghi chú', true)
      return
    }
    await saveNote(target, textVal.trim())
    const node = findNode(tree, target)
    flash(`Đã lưu ghi chú cho "${node?.text ?? 'nút'}"`)
  }

  // ---- free-drag / positions ----
  const toggleDragMode = async () => {
    if (mapId == null || !tree) return
    if (!dragMode) {
      // Entering free mode: if the map has no saved positions yet, bake the
      // current auto-layout so every node has a stable, draggable position.
      const anyPos = (function has(n: TreeNode): boolean {
        return n.pos_x != null || n.children.some(has)
      })(tree)
      if (!anyPos) {
        const l = computeLayout(tree, mapLayout)
        const items = l.nodes.map((n) => ({ id: n.id, x: n.x, y: n.y }))
        try {
          await api.savePositions(items)
          await refreshTree(mapId)
        } catch (e) {
          flash(String(e), true)
          return
        }
      }
      setDragMode(true)
      flash('Đã bật kéo thả — kéo để đổi vị trí nút')
    } else {
      setDragMode(false)
    }
  }

  const onMovePositions = async (items: { id: number; x: number; y: number }[]) => {
    if (mapId == null) return
    pushHistory()
    // Optimistic local update so the drag sticks without a full reload flash.
    setTree((t) => (t ? applyPositions(t, items) : t))
    try {
      await api.savePositions(items)
    } catch (e) {
      flash(String(e), true)
    }
  }

  const autoSort = async () => {
    if (mapId == null) return
    pushHistory()
    try {
      await api.clearPositions(mapId)
      await refreshTree(mapId)
      setDragMode(false)
      flash('Đã tự động sắp xếp lại')
    } catch (e) {
      flash(String(e), true)
    }
  }

  // ---- import / export ----
  const doImport = async (file: File) => {
    try {
      const content = await file.text()
      const parsed = parseImport(file.name, content)
      if (parsed.children.length === 0) {
        flash('Không đọc được nút nào từ file', true)
        return
      }
      const layout = parsed.layout ?? settings.defaultLayout
      const { id } = await api.importMap(parsed.title, layout, parsed.children)
      await loadMaps()
      await openMap(id)
      setShowIO(false)
      flash(`Đã nhập "${parsed.title}"`)
    } catch (e) {
      flash(`Nhập thất bại: ${e}`, true)
    }
  }

  const doExport = (fmt: Format) => {
    if (!tree) return
    const { text, ext, mime } = exportMap(tree, mapLayout, fmt)
    const base = (tree.text || 'mindmap').replace(/[^\p{L}\p{N}_-]+/gu, '_').slice(0, 60)
    download(`${base}.${ext}`, text, mime)
  }

  // ---- node ops ----
  const addChild = async (parentId: number, edit = true) => {
    if (mapId == null) return
    pushHistory()
    const { id } = await api.addNode(parentId, 'Ý mới')
    // Expand parent if collapsed so the new child is visible.
    const parent = findNode(tree, parentId)
    if (parent?.collapsed) await api.updateNode(parentId, { collapsed: false })
    await refreshTree(mapId)
    setSelectedId(id)
    if (edit) setEditingId(id)
    await loadMaps()
  }

  const addSibling = async (nodeId: number) => {
    const node = findNode(tree, nodeId)
    if (!node || mapId == null) return
    // Find parent id.
    const parentId = parentOf(tree, nodeId)
    if (parentId == null) return addChild(nodeId) // root → add child instead
    pushHistory()
    const { id } = await api.addNode(parentId, 'Ý mới')
    await refreshTree(mapId)
    setSelectedId(id)
    setEditingId(id)
    await loadMaps()
  }

  const commitEdit = async (id: number, text: string) => {
    setEditingId(null)
    const node = findNode(tree, id)
    const t = text.trim()
    if (!node || !t || t === node.text) return
    if (mapId == null) return
    pushHistory()
    await api.updateNode(id, { text: t })
    await refreshTree(mapId)
    await loadMaps()
  }

  const toggleCollapse = async (id: number) => {
    const node = findNode(tree, id)
    if (!node || mapId == null) return
    await api.updateNode(id, { collapsed: !node.collapsed })
    await refreshTree(mapId)
  }

  const deleteNode = async (id: number) => {
    if (mapId == null) return
    const parentId = parentOf(tree, id)
    if (parentId == null) {
      flash('Không thể xoá nút gốc', true)
      return
    }
    pushHistory()
    await api.deleteNode(id)
    await refreshTree(mapId)
    setSelectedId(parentId)
    await loadMaps()
  }

  const quickGenerate = async (id: number) => {
    if (mapId == null) return
    const node = findNode(tree, id)
    if (!node) return
    pushHistory()
    setGeneratingId(id)
    try {
      const r = await api.generate(id, { topic: node.text })
      if (node.collapsed) await api.updateNode(id, { collapsed: false })
      flash(`Đã sinh ${r.added} nút · ${r.model}`)
    } catch (e) {
      flash(String(e), true)
    } finally {
      setGeneratingId(null)
      await refreshTree(mapId)
      await loadMaps()
    }
  }

  const generateWith = async (id: number, instruction: string, replace: boolean) => {
    setGenModal(null)
    if (mapId == null) return
    const node = findNode(tree, id)
    if (!node) return
    pushHistory()
    setGeneratingId(id)
    try {
      const r = await api.generate(id, { topic: node.text, instruction, replace })
      if (node.collapsed) await api.updateNode(id, { collapsed: false })
      flash(`Đã sinh ${r.added} nút · ${r.model}`)
    } catch (e) {
      flash(String(e), true)
    } finally {
      setGeneratingId(null)
      await refreshTree(mapId)
      await loadMaps()
    }
  }

  // ---- chat ----
  const ensureSession = async (firstText?: string): Promise<number | null> => {
    if (activeSession != null) return activeSession
    if (mapId == null) return null
    const title = firstText ? firstText.slice(0, 32) : 'Hội thoại mới'
    const { id } = await api.createSession(mapId, title)
    setActiveSession(id)
    await loadSessions(mapId, false)
    return id
  }

  // Build the chat grounding context: pinned nodes (with notes) then the outline.
  const chatContext = (): string | null => {
    let ctx = ''
    if (pins.length) {
      ctx += 'Các nút đang ghim làm ngữ cảnh:\n'
      for (const p of pins) ctx += `- ${p.text}${p.note ? `: ${p.note}` : ''}\n`
      ctx += '\n'
    }
    if (tree) ctx += `Sơ đồ hiện tại:\n${outline(tree)}`
    return ctx.trim() ? ctx : null
  }

  const sendChat = async (text: string) => {
    if (mapId == null) return
    const sid = await ensureSession(text)
    if (sid == null) return
    setChat((c) => [...c, { role: 'user', content: text }])
    setChatBusy(true)
    const t0 = performance.now()
    try {
      const r = await api.chatSend(sid, text, chatContext())
      setChat((c) => [
        ...c,
        { role: 'assistant', content: r.text, model: r.model, ms: performance.now() - t0 },
      ])
      await loadSessions(mapId, false)
    } catch (e) {
      setChat((c) => [...c, { role: 'assistant', content: `⚠️ ${e}` }])
    } finally {
      setChatBusy(false)
    }
  }

  const newSession = async () => {
    if (mapId == null) return
    try {
      const { id } = await api.createSession(mapId)
      await loadSessions(mapId, false)
      setActiveSession(id)
      setChat([])
    } catch (e) {
      flash(String(e), true)
    }
  }

  const renameSession = async (id: number, title: string) => {
    await api.renameSession(id, title)
    if (mapId != null) await loadSessions(mapId, false)
  }

  const deleteSession = async (id: number) => {
    await api.deleteSession(id)
    if (mapId == null) return
    const list = await loadSessions(mapId, false)
    if (id === activeSession) {
      if (list.length > 0) await openSession(list[0].id)
      else {
        setActiveSession(null)
        setChat([])
      }
    }
  }

  // Attach a file → OCR/extract text → generate a NEW map from its content.
  const attachFile = async (file: File) => {
    setImporting(true)
    try {
      const { text, name, chars, ocr } = await api.importFile(file)
      if (!text.trim()) {
        flash('File không có nội dung văn bản', true)
        return
      }
      const base = name.replace(/\.[^.]+$/, '')
      const { id, rootId } = await api.createMap(base || 'Từ file')
      await loadMaps()
      await openMap(id)
      setGeneratingId(rootId)
      try {
        const r = await api.generate(rootId, { source: text, topic: base })
        flash(`${ocr ? 'OCR' : 'Đọc'} ${chars} ký tự → ${r.added} nút · ${r.model}`)
      } finally {
        setGeneratingId(null)
        await refreshTree(id)
        await loadMaps()
      }
    } catch (e) {
      flash(String(e), true)
    } finally {
      setImporting(false)
    }
  }

  // Turn a chat answer into nodes under the current map's root.
  const generateFromText = async (text: string) => {
    if (mapId == null || !tree) return
    const rootId = tree.id
    pushHistory()
    setGeneratingId(rootId)
    try {
      const r = await api.generate(rootId, { source: text, topic: tree.text })
      if (tree.collapsed) await api.updateNode(rootId, { collapsed: false })
      flash(`Đã tạo ${r.added} nút từ câu trả lời · ${r.model}`)
    } catch (e) {
      flash(String(e), true)
    } finally {
      setGeneratingId(null)
      await refreshTree(mapId)
      await loadMaps()
    }
  }

  // ---- keyboard shortcuts ----
  const selRef = useRef<number | null>(null)
  selRef.current = selectedId
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) return
      const id = selRef.current
      if (id == null) return
      if (e.key === 'Tab') {
        e.preventDefault()
        addChild(id)
      } else if (e.key === 'Enter') {
        e.preventDefault()
        addSibling(id)
      } else if (e.key === 'F2') {
        e.preventDefault()
        setEditingId(id)
      } else if (e.key === 'Delete' || e.key === 'Backspace') {
        e.preventDefault()
        deleteNode(id)
      } else if (e.key === 'Escape') {
        setSelectedId(null)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tree, mapId])

  // Undo/redo shortcuts: Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z (or Ctrl+Y).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) return
      if (!(e.metaKey || e.ctrlKey)) return
      const k = e.key.toLowerCase()
      if (k === 'z' && !e.shiftKey) {
        e.preventDefault()
        undo()
      } else if ((k === 'z' && e.shiftKey) || k === 'y') {
        e.preventDefault()
        redo()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [undo, redo])

  return (
    <div className="app">
      <div className="topbar">
        <button
          className="icon-btn hamburger"
          title="Danh sách map"
          onClick={() => setSidebarOpen((v) => !v)}
        >
          ☰
        </button>
        <div className="brand">
          <span className="logo">🧠</span> <span className="brand-tx">SenClaw Mindmap</span>
        </div>
        {mapId != null && (
          <div className="seg" title="Kiểu bố cục">
            {LAYOUTS.map((l) => (
              <button
                key={l.id}
                className={mapLayout === l.id ? 'on' : ''}
                onClick={() => changeLayout(l.id)}
                title={l.label}
              >
                <span className="seg-ic">{l.icon}</span>
                <span className="seg-tx">{l.label}</span>
              </button>
            ))}
          </div>
        )}
        {mapId != null && (
          <div className="tb-tools">
            <button
              className="icon-btn"
              title="Hoàn tác (Ctrl/⌘+Z)"
              onClick={undo}
              disabled={undoStack.length === 0}
            >
              ↶
            </button>
            <button
              className="icon-btn"
              title="Làm lại (Ctrl/⌘+Shift+Z)"
              onClick={redo}
              disabled={redoStack.length === 0}
            >
              ↷
            </button>
            <button
              className={`icon-btn${dragMode ? ' on' : ''}`}
              title={dragMode ? 'Đang bật kéo thả — bấm để khoá' : 'Mở khoá để kéo thả vị trí nút'}
              onClick={toggleDragMode}
            >
              {dragMode ? '🔓' : '🔒'}
            </button>
            <button className="icon-btn" title="Tự động sắp xếp lại" onClick={autoSort}>
              ↻
            </button>
          </div>
        )}
        <div className="spacer" />
        <select
          className="btn model-select"
          value={activeModel ?? ''}
          onChange={async (e) => {
            const id = e.target.value
            setActiveModel(id)
            try {
              await api.setModel(id)
              flash('Đã đổi model')
            } catch (err) {
              flash(String(err), true)
            }
          }}
          title="Model AI đang dùng"
        >
          {models.length === 0 && <option value="">(chưa có model)</option>}
          {models.map((m) => (
            <option key={m.id} value={m.id}>
              {m.modelName ?? m.id}
            </option>
          ))}
        </select>
        <span className="pill" title={llmModel ?? ''}>
          <span className={`dot${llmOk ? '' : ' off'}`} />
          {llmOk ? llmModel ?? 'LLM sẵn sàng' : 'LLM chưa cấu hình'}
        </span>
        <button className="icon-btn" title="Nhập / Xuất mind map" onClick={() => setShowIO(true)}>
          📤
        </button>
        <button className="icon-btn" title="Cài đặt" onClick={() => setShowSettings(true)}>
          ⚙️
        </button>
        {!showChat && (
          <button className="icon-btn" title="Hiện trợ lý AI" onClick={() => setShowChat(true)}>
            💬
          </button>
        )}
        <button
          className="icon-btn"
          title="Sáng / tối"
          onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
        >
          {theme === 'dark' ? '☀️' : '🌙'}
        </button>
      </div>

      {sidebarOpen && <div className="sidebar-backdrop" onClick={() => setSidebarOpen(false)} />}
      <div className={`sidebar${sidebarOpen ? ' open' : ''}`}>
        <div className="head">
          <h3>Mind maps</h3>
          <button className="icon-btn close-sidebar" onClick={() => setSidebarOpen(false)}>
            ✕
          </button>
        </div>
        <div style={{ padding: '0 8px 8px', display: 'flex', gap: 6 }}>
          <button className="btn primary" style={{ flex: 1 }} onClick={() => setNewMap(true)}>
            ＋ Map mới
          </button>
          <button className="btn" onClick={() => setShowTemplates(true)} title="Chọn từ mẫu">
            📄 Mẫu
          </button>
        </div>
        <div className="maps">
          {maps.length === 0 && (
            <div style={{ color: 'var(--muted)', fontSize: 12, padding: '8px 10px' }}>
              Chưa có map nào.
            </div>
          )}
          {maps.map((m) => (
            <div
              key={m.id}
              className={`map-item${m.id === mapId ? ' active' : ''}`}
              onClick={() => {
                openMap(m.id)
                setSidebarOpen(false)
              }}
            >
              <div className="title">{m.title}</div>
              <div className="meta">{m.node_count} nút</div>
              <button
                className="del"
                title="Xoá"
                onClick={(e) => {
                  e.stopPropagation()
                  deleteMap(m.id)
                }}
              >
                🗑
              </button>
            </div>
          ))}
        </div>
      </div>

      <div className={`main${showChat ? '' : ' no-chat'}`}>
        <MindmapCanvas
          root={tree}
          style={mapLayout}
          selectedId={selectedId}
          editingId={editingId}
          generatingId={generatingId ?? notingId}
          dragEnabled={dragMode}
          fullLabels={settings.fullLabels}
          showCount={settings.showCount}
          onSelect={setSelectedId}
          onStartEdit={setEditingId}
          onCommitEdit={commitEdit}
          onCancelEdit={() => setEditingId(null)}
          onToggleCollapse={toggleCollapse}
          onAddChild={(id) => addChild(id)}
          onAddSibling={addSibling}
          onDelete={deleteNode}
          onStyle={styleNode}
          onNote={openNote}
          onExpandNote={expandNote}
          onContextMenu={(id, x, y) => setCtxMenu({ nodeId: id, x, y })}
          onMove={onMovePositions}
          onGenerate={(id) => {
            const node = findNode(tree, id)
            if (node) setGenModal({ nodeId: id, text: node.text })
          }}
        />
        {showChat && (
          <ChatPanel
            messages={chat}
            sessions={sessions}
            activeSession={activeSession}
            busy={chatBusy}
            importing={importing}
            hasMap={mapId != null}
            pins={pins}
            onSend={sendChat}
            onUnpin={unpin}
            onClearPins={clearPins}
            onNewSession={newSession}
            onSwitchSession={openSession}
            onRenameSession={renameSession}
            onDeleteSession={deleteSession}
            onAttach={attachFile}
            onGenerateFromText={generateFromText}
            onSaveNote={saveAiAsNote}
            onClose={() => setShowChat(false)}
          />
        )}
      </div>

      {newMap && (
        <NewMapModal
          defaultLayout={settings.defaultLayout}
          onClose={() => setNewMap(false)}
          onCreate={createMap}
        />
      )}
      {showTemplates && (
        <TemplateGallery
          templates={templates}
          onClose={() => setShowTemplates(false)}
          onPick={createFromTemplate}
        />
      )}
      {showSettings && (
        <SettingsModal
          settings={settings}
          onClose={() => setShowSettings(false)}
          onChange={(patch) => setSettings((s) => ({ ...s, ...patch }))}
        />
      )}
      {showIO && (
        <ImportExportModal
          hasMap={tree != null}
          onClose={() => setShowIO(false)}
          onImportClick={() => importRef.current?.click()}
          onExport={doExport}
        />
      )}
      <input
        ref={importRef}
        type="file"
        style={{ display: 'none' }}
        accept=".json,.md,.markdown,.txt,.opml,.mm"
        onChange={(e) => {
          const f = e.target.files?.[0]
          if (f) doImport(f)
          e.target.value = ''
        }}
      />
      {genModal && (
        <GenerateModal
          text={genModal.text}
          onClose={() => setGenModal(null)}
          onQuick={() => {
            setGenModal(null)
            quickGenerate(genModal.nodeId)
          }}
          onGenerate={(instruction, replace) => generateWith(genModal.nodeId, instruction, replace)}
        />
      )}
      {ctxMenu &&
        (() => {
          const node = findNode(tree, ctxMenu.nodeId)
          if (!node) return null
          const isRoot = tree?.id === node.id
          return (
            <NodeContextMenu
              x={ctxMenu.x}
              y={ctxMenu.y}
              hasNote={!!node.note}
              isRoot={isRoot}
              onClose={() => setCtxMenu(null)}
              items={[
                { icon: '✎', label: 'Sửa tên', onClick: () => setEditingId(node.id) },
                { icon: '📝', label: node.note ? 'Sửa ghi chú' : 'Thêm ghi chú', onClick: () => openNote(node.id) },
                { icon: '✨', label: 'AI viết ghi chú', onClick: () => aiWriteNote(node.id) },
                ...(node.note
                  ? [{ icon: '🧹', label: 'Xoá ghi chú', onClick: () => removeNote(node.id), danger: true as const }]
                  : []),
                { sep: true as const },
                { icon: '📌', label: 'Ghim vào hỏi AI', onClick: () => pinNode(node.id) },
                { icon: '✨', label: 'AI mở rộng nhánh', onClick: () => setGenModal({ nodeId: node.id, text: node.text }) },
                { icon: '＋', label: 'Thêm nhánh con', onClick: () => addChild(node.id) },
                ...(!isRoot
                  ? [
                      { sep: true as const },
                      { icon: '🗑', label: 'Xoá nút', onClick: () => deleteNode(node.id), danger: true as const },
                    ]
                  : []),
              ]}
            />
          )
        })()}
      {noteEdit && (
        <NoteEditorModal
          nodeText={noteEdit.text}
          note={noteEdit.note}
          busy={notingId === noteEdit.nodeId}
          onClose={() => setNoteEdit(null)}
          onSave={(n) => saveNote(noteEdit.nodeId, n)}
          onAiWrite={() => aiWriteNote(noteEdit.nodeId)}
        />
      )}
      {noteView && (
        <NoteViewDialog
          nodeText={noteView.text}
          note={noteView.note}
          onClose={() => setNoteView(null)}
          onEdit={() => {
            setNoteView(null)
            openNote(noteView.nodeId)
          }}
        />
      )}
      {toast && <div className={`toast${toast.err ? ' err' : ''}`}>{toast.msg}</div>}
    </div>
  )
}

function parentOf(root: TreeNode | null, id: number): number | null {
  if (!root) return null
  const walk = (n: TreeNode): number | null => {
    for (const c of n.children) {
      if (c.id === id) return n.id
      const f = walk(c)
      if (f != null) return f
    }
    return null
  }
  return walk(root)
}

function NewMapModal({
  defaultLayout,
  onClose,
  onCreate,
}: {
  defaultLayout: Layout
  onClose: () => void
  onCreate: (
    title: string,
    description: string,
    layout: Layout,
    withAi: boolean,
    instruction: string,
  ) => void
}) {
  const [title, setTitle] = useState('')
  const [desc, setDesc] = useState('')
  const [layout, setLayout] = useState<Layout>(defaultLayout)
  const [withAi, setWithAi] = useState(true)
  const [instruction, setInstruction] = useState('')
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => ref.current?.focus(), [])
  const submit = () => {
    if (!title.trim()) return
    onCreate(title.trim(), desc.trim(), layout, withAi, instruction.trim())
    onClose()
  }
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>🧠 Tạo mindmap mới</h3>
        <label>Chủ đề trung tâm</label>
        <input
          ref={ref}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="VD: Chiến lược marketing 2026"
          onKeyDown={(e) => e.key === 'Enter' && submit()}
        />
        <label>Kiểu bố cục</label>
        <div className="layout-choices">
          {LAYOUTS.map((l) => (
            <button
              key={l.id}
              className={`layout-choice${layout === l.id ? ' on' : ''}`}
              onClick={() => setLayout(l.id)}
            >
              <span style={{ fontSize: 18 }}>{l.icon}</span>
              {l.label}
            </button>
          ))}
        </div>
        <label>Mô tả (tuỳ chọn)</label>
        <input value={desc} onChange={(e) => setDesc(e.target.value)} />
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 14 }}>
          <input
            type="checkbox"
            checked={withAi}
            onChange={(e) => setWithAi(e.target.checked)}
            style={{ width: 'auto' }}
          />
          ✨ Dùng AI sinh nội dung ban đầu
        </label>
        {withAi && (
          <>
            <label>Hướng dẫn thêm cho AI (tuỳ chọn)</label>
            <input
              value={instruction}
              onChange={(e) => setInstruction(e.target.value)}
              placeholder="VD: tập trung vào kênh digital, 5 nhánh"
            />
          </>
        )}
        <div className="row">
          <button className="btn" onClick={onClose}>
            Huỷ
          </button>
          <button className="btn primary" onClick={submit} disabled={!title.trim()}>
            Tạo
          </button>
        </div>
      </div>
    </div>
  )
}

function TemplateGallery({
  templates,
  onClose,
  onPick,
}: {
  templates: TemplateInfo[]
  onClose: () => void
  onPick: (id: string) => void
}) {
  const cats = Array.from(new Set(templates.map((t) => t.category)))
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <h3>📄 Chọn mẫu có sẵn</h3>
        <div className="tpl-scroll">
          {cats.map((cat) => (
            <div key={cat}>
              <div className="tpl-cat">{cat}</div>
              <div className="tpl-grid">
                {templates
                  .filter((t) => t.category === cat)
                  .map((t) => (
                    <button key={t.id} className="tpl-card" onClick={() => onPick(t.id)}>
                      <div className="tpl-icon">{t.icon}</div>
                      <div className="tpl-name">{t.name}</div>
                      <div className="tpl-desc">{t.description}</div>
                      <div className="tpl-layout">
                        {LAYOUTS.find((l) => l.id === t.layout)?.icon}{' '}
                        {LAYOUTS.find((l) => l.id === t.layout)?.label}
                      </div>
                    </button>
                  ))}
              </div>
            </div>
          ))}
        </div>
        <div className="row">
          <button className="btn" onClick={onClose}>
            Đóng
          </button>
        </div>
      </div>
    </div>
  )
}

function GenerateModal({
  text,
  onClose,
  onQuick,
  onGenerate,
}: {
  text: string
  onClose: () => void
  onQuick: () => void
  onGenerate: (instruction: string, replace: boolean) => void
}) {
  const [instruction, setInstruction] = useState('')
  const [replace, setReplace] = useState(false)
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>✨ AI mở rộng: “{text}”</h3>
        <label>Hướng dẫn (tuỳ chọn)</label>
        <input
          autoFocus
          value={instruction}
          onChange={(e) => setInstruction(e.target.value)}
          placeholder="VD: liệt kê rủi ro, hoặc 4 nhánh chính"
          onKeyDown={(e) => e.key === 'Enter' && onGenerate(instruction.trim(), replace)}
        />
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 14 }}>
          <input
            type="checkbox"
            checked={replace}
            onChange={(e) => setReplace(e.target.checked)}
            style={{ width: 'auto' }}
          />
          Thay thế các nhánh con hiện có
        </label>
        <div className="row">
          <button className="btn" onClick={onQuick}>
            Sinh nhanh
          </button>
          <button className="btn primary" onClick={() => onGenerate(instruction.trim(), replace)}>
            Sinh
          </button>
        </div>
      </div>
    </div>
  )
}

function SettingsModal({
  settings,
  onClose,
  onChange,
}: {
  settings: Settings
  onClose: () => void
  onChange: (patch: Partial<Settings>) => void
}) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>⚙️ Cài đặt hiển thị</h3>

        <label>Bố cục mặc định cho map mới</label>
        <div className="layout-choices">
          {LAYOUTS.map((l) => (
            <button
              key={l.id}
              className={`layout-choice${settings.defaultLayout === l.id ? ' on' : ''}`}
              onClick={() => onChange({ defaultLayout: l.id })}
            >
              <span style={{ fontSize: 18 }}>{l.icon}</span>
              {l.label}
            </button>
          ))}
        </div>

        <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 16 }}>
          <input
            type="checkbox"
            checked={settings.fullLabels}
            onChange={(e) => onChange({ fullLabels: e.target.checked })}
            style={{ width: 'auto' }}
          />
          Hiện đầy đủ nội dung nút (không rút gọn “…”)
        </label>
        <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 10 }}>
          <input
            type="checkbox"
            checked={settings.showCount}
            onChange={(e) => onChange({ showCount: e.target.checked })}
            style={{ width: 'auto' }}
          />
          Hiện số lượng nhánh con trên mỗi nút
        </label>

        <div className="row">
          <button className="btn primary" onClick={onClose}>
            Xong
          </button>
        </div>
      </div>
    </div>
  )
}

function ImportExportModal({
  hasMap,
  onClose,
  onImportClick,
  onExport,
}: {
  hasMap: boolean
  onClose: () => void
  onImportClick: () => void
  onExport: (fmt: Format) => void
}) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>📤 Nhập / Xuất mind map</h3>

        <label>Nhập từ file</label>
        <button className="btn block" onClick={onImportClick}>
          📥 Chọn file… (JSON, Markdown, OPML, FreeMind .mm)
        </button>
        <div style={{ fontSize: 11.5, color: 'var(--muted)', marginTop: 6 }}>
          Hỗ trợ các định dạng sơ đồ tư duy tiêu chuẩn. File sẽ được tạo thành một map mới.
        </div>

        <label style={{ marginTop: 16 }}>Xuất map hiện tại</label>
        {!hasMap && (
          <div style={{ fontSize: 12, color: 'var(--muted)' }}>Chọn một map để xuất.</div>
        )}
        {hasMap && (
          <div className="export-grid">
            {EXPORT_FORMATS.map((f) => (
              <button key={f.id} className="btn" onClick={() => onExport(f.id)}>
                ⬇ {f.label}
                <span style={{ color: 'var(--muted)', marginLeft: 4 }}>{f.ext}</span>
              </button>
            ))}
          </div>
        )}

        <div className="row">
          <button className="btn" onClick={onClose}>
            Đóng
          </button>
        </div>
      </div>
    </div>
  )
}

type MenuItem =
  | { icon: string; label: string; onClick: () => void; danger?: boolean }
  | { sep: true }

function NodeContextMenu({
  x,
  y,
  onClose,
  items,
}: {
  x: number
  y: number
  hasNote: boolean
  isRoot: boolean
  onClose: () => void
  items: MenuItem[]
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('pointerdown', onDown, true)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('pointerdown', onDown, true)
      window.removeEventListener('keydown', onKey)
    }
  }, [onClose])
  const left = Math.min(x, window.innerWidth - 210)
  const top = Math.min(y, window.innerHeight - items.length * 34 - 12)
  return (
    <div className="ctx-menu" ref={ref} style={{ left, top }}>
      {items.map((it, i) =>
        'sep' in it ? (
          <div key={i} className="ctx-sep" />
        ) : (
          <button
            key={i}
            className={`ctx-item${it.danger ? ' danger' : ''}`}
            onClick={() => {
              it.onClick()
              onClose()
            }}
          >
            <span className="ctx-ic">{it.icon}</span>
            {it.label}
          </button>
        ),
      )}
    </div>
  )
}

function NoteEditorModal({
  nodeText,
  note,
  busy,
  onClose,
  onSave,
  onAiWrite,
}: {
  nodeText: string
  note: string
  busy: boolean
  onClose: () => void
  onSave: (note: string) => void
  onAiWrite: () => void
}) {
  const [val, setVal] = useState(note)
  useEffect(() => setVal(note), [note])
  const ref = useRef<HTMLTextAreaElement>(null)
  useEffect(() => ref.current?.focus(), [])
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h3>📝 Ghi chú: “{nodeText}”</h3>
        <textarea
          ref={ref}
          value={val}
          onChange={(e) => setVal(e.target.value)}
          placeholder="Nhập ghi chú cho nút này…"
          style={{ width: '100%', minHeight: 130, resize: 'vertical' }}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) onSave(val.trim())
          }}
        />
        <div className="row" style={{ justifyContent: 'space-between' }}>
          <button className="btn" onClick={onAiWrite} disabled={busy}>
            {busy ? <span className="spin" /> : '✨'} AI viết giúp
          </button>
          <div style={{ display: 'flex', gap: 8 }}>
            <button className="btn" onClick={onClose}>
              Huỷ
            </button>
            <button className="btn primary" onClick={() => onSave(val.trim())}>
              Lưu
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

function NoteViewDialog({
  nodeText,
  note,
  onClose,
  onEdit,
}: {
  nodeText: string
  note: string
  onClose: () => void
  onEdit: () => void
}) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" onClick={(e) => e.stopPropagation()}>
        <h3>📝 {nodeText}</h3>
        <div className="note-view markdown">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{note}</ReactMarkdown>
        </div>
        <div className="row">
          <button className="btn" onClick={onEdit}>
            ✎ Sửa
          </button>
          <button className="btn primary" onClick={onClose}>
            Đóng
          </button>
        </div>
      </div>
    </div>
  )
}
