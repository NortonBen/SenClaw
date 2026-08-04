import { useEffect, useMemo, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import {
  App as AntApp, Button, Checkbox, Input, Modal, Select, Switch, Tag, Upload,
} from 'antd'
import {
  DeleteOutlined, FileTextOutlined, PlusOutlined, UploadOutlined,
} from '@ant-design/icons'
import { api } from './api'
import type { DocMeta, LlmProfile, Member, MinutesRow, Progress, ResultRow, ToolInfo } from './types'
import { HAT_COLORS, HAT_NAMES, splitHats } from './types'

// ---------------- Biên bản ----------------

export function MinutesPanel({ minutes }: { minutes: MinutesRow | null }) {
  if (!minutes) return <div className="panel-empty">Thư ký chưa ghi biên bản — chạy phiên để bắt đầu.</div>
  return (
    <div className="md-box">
      <div className="panel-note">Cập nhật sau vòng {minutes.round}</div>
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{minutes.content}</ReactMarkdown>
    </div>
  )
}

// ---------------- Kết quả ----------------

export function ResultPanel({
  result, status, onApprove, onReject,
}: {
  result: ResultRow | null
  status: string
  onApprove: () => void
  onReject: (feedback: string) => void
}) {
  const [feedback, setFeedback] = useState('')
  if (!result) {
    return (
      <div className="panel-empty">
        Chưa có kết quả. Manager sẽ đề nghị chốt khi thảo luận đủ theo yêu cầu — hoặc BOSS bấm “Chốt ngay”.
      </div>
    )
  }
  return (
    <div className="md-box">
      <div className={`result-status rs-${result.status}`}>
        {result.status === 'draft' && status === 'review' && 'DỰ THẢO — chờ BOSS nghiệm thu'}
        {result.status === 'approved' && '✅ ĐÃ DUYỆT'}
        {result.status === 'rejected' && `❌ Bị từ chối: ${result.feedback}`}
        {result.status === 'draft' && status !== 'review' && 'Bản nháp cũ'}
      </div>
      {status === 'review' && result.status === 'draft' && (
        <div className="review-actions">
          <Button type="primary" onClick={onApprove}>✅ Duyệt kết quả</Button>
          <div className="reject-row">
            <Input.TextArea
              autoSize={{ minRows: 1, maxRows: 3 }}
              value={feedback}
              placeholder="Góp ý bắt buộc khi từ chối…"
              onChange={(e) => setFeedback(e.target.value)}
            />
            <Button danger disabled={!feedback.trim()}
              onClick={() => { onReject(feedback.trim()); setFeedback('') }}>
              ❌ Từ chối
            </Button>
          </div>
        </div>
      )}
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{result.content}</ReactMarkdown>
    </div>
  )
}

// ---------------- Tiến độ ----------------

export function ProgressPanel({ progress }: { progress: Progress | null }) {
  if (!progress) return <div className="panel-empty">Chưa có dữ liệu.</div>
  const p = progress
  return (
    <div className="progress-box">
      <div className="score-row">
        <div className="score-num">{p.manager_score}<span>/100</span></div>
        <div className="score-meta">
          <div>Manager chấm so với yêu cầu BOSS</div>
          <div className="score-bar"><div style={{ width: `${Math.min(100, p.manager_score)}%` }} /></div>
          <div className="muted">Vòng {p.round}/{p.max_rounds} · trạng thái: {p.status}</div>
        </div>
      </div>
      {p.manager_missing?.length > 0 && (
        <div className="missing">
          <div className="panel-sub">Còn thiếu</div>
          <ul>{p.manager_missing.map((m, i) => <li key={i}>{m}</li>)}</ul>
        </div>
      )}
      <div className="panel-sub">Tham gia</div>
      <table className="ptable">
        <thead><tr><th>Thành viên</th><th>Phát biểu</th><th>Vòng cuối</th><th>Im lặng</th></tr></thead>
        <tbody>
          {p.participation.map((x) => (
            <tr key={x.member_id} className={x.silent_rounds >= 2 ? 'row-warn' : ''}>
              <td>{x.name}</td>
              <td>{x.message_count}</td>
              <td>{x.last_round || '—'}</td>
              <td>{x.silent_rounds >= 2 ? `⚠️ ${x.silent_rounds} vòng` : x.silent_rounds}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {p.open_opinions.length > 0 && (
        <>
          <div className="panel-sub">Luận điểm chưa ai phản hồi ({p.open_opinions.length})</div>
          <ul className="open-list">
            {p.open_opinions.slice(0, 6).map((o) => (
              <li key={o.id}>#{o.id} — {o.content.length > 110 ? o.content.slice(0, 110) + '…' : o.content}</li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}

// ---------------- Kho tài liệu ----------------

export function DocsPanel({
  discussionId, openDocId, onOpenedDoc,
}: {
  discussionId: number | null
  openDocId: number | null
  onOpenedDoc: () => void
}) {
  const { message, modal } = AntApp.useApp()
  const [docs, setDocs] = useState<DocMeta[]>([])
  const [q, setQ] = useState('')
  const [viewing, setViewing] = useState<{ id: number; title: string; content: string } | null>(null)
  const [pasting, setPasting] = useState(false)
  const [pTitle, setPTitle] = useState('')
  const [pContent, setPContent] = useState('')

  const load = async () => {
    const params = new URLSearchParams()
    if (q.trim()) params.set('q', q.trim())
    if (discussionId) params.set('discussion_id', String(discussionId))
    params.set('limit', '60')
    try {
      const r = await api.get<{ docs: DocMeta[] }>(`/docs?${params}`)
      setDocs(r.docs)
    } catch { /* im lặng */ }
  }
  useEffect(() => { void load() }, [discussionId, q]) // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (openDocId == null) return
    void (async () => {
      try {
        const r = await api.get<{ doc: { id: number; title: string; content: string } }>(`/docs/${openDocId}`)
        setViewing(r.doc)
      } catch { /* bỏ qua */ }
      onOpenedDoc()
    })()
  }, [openDocId]) // eslint-disable-line react-hooks/exhaustive-deps

  const upload = async (f: File) => {
    const form = new FormData()
    form.append('file', f)
    if (discussionId) form.append('discussion_id', String(discussionId))
    try {
      await api.upload('/docs/upload', form)
      message.success(`Đã nạp “${f.name}” vào kho`)
      await load()
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  const addText = async () => {
    try {
      await api.post('/docs/text', {
        title: pTitle.trim(), content: pContent.trim(),
        ...(discussionId ? { discussion_id: discussionId } : {}),
      })
      setPasting(false); setPTitle(''); setPContent('')
      await load()
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <div className="docs-box">
      <div className="docs-bar">
        <Input.Search allowClear placeholder="Tìm tài liệu (không dấu vẫn khớp)…"
          onSearch={(v) => setQ(v)} onChange={(e) => { if (!e.target.value) setQ('') }} />
        <Upload showUploadList={false} accept=".txt,.md,.markdown,.html,.htm,.pdf,.csv,.json"
          customRequest={({ file, onSuccess }) => { void upload(file as File).then(() => onSuccess?.(null)) }}>
          <Button icon={<UploadOutlined />}>Tải lên</Button>
        </Upload>
        <Button icon={<PlusOutlined />} onClick={() => setPasting(true)}>Dán văn bản</Button>
      </div>
      <div className="docs-list">
        {docs.length === 0 && <div className="panel-empty">Kho trống — tải tài liệu để cả đội cùng đọc và trích dẫn doc:&lt;id&gt;.</div>}
        {docs.map((d) => (
          <div key={d.id} className="doc-item" onClick={async () => {
            const r = await api.get<{ doc: { id: number; title: string; content: string } }>(`/docs/${d.id}`)
            setViewing(r.doc)
          }}>
            <div className="doc-title"><FileTextOutlined /> doc:{d.id} — {d.title}</div>
            <div className="doc-meta">{d.source} · {d.created_by} · {d.chars.toLocaleString('vi')} ký tự{d.discussion_id == null ? ' · kho chung' : ''}</div>
            <div className="doc-preview">{d.preview}</div>
            <Button className="doc-del" size="small" type="text" icon={<DeleteOutlined />}
              onClick={(e) => {
                e.stopPropagation()
                modal.confirm({
                  title: `Xoá tài liệu doc:${d.id}?`,
                  content: d.title,
                  okText: 'Xoá', okType: 'danger', cancelText: 'Huỷ',
                  onOk: () => api.del(`/docs/${d.id}`).then(load),
                })
              }} />
          </div>
        ))}
      </div>
      <Modal open={!!viewing} onCancel={() => setViewing(null)} footer={null} width={720}
        title={viewing ? `📄 doc:${viewing.id} — ${viewing.title}` : ''}>
        <pre className="doc-full">{viewing?.content}</pre>
      </Modal>
      <Modal open={pasting} onCancel={() => setPasting(false)} title="Thêm tài liệu văn bản"
        okText="Lưu vào kho" cancelText="Huỷ"
        okButtonProps={{ disabled: !pTitle.trim() || !pContent.trim() }}
        onOk={() => void addText()}>
        <div className="v-form">
          <Input value={pTitle} onChange={(e) => setPTitle(e.target.value)} placeholder="Tiêu đề" />
          <Input.TextArea rows={10} value={pContent} onChange={(e) => setPContent(e.target.value)} placeholder="Nội dung…" />
        </div>
      </Modal>
    </div>
  )
}

// ---------------- Đội (roster) ----------------

const HAT_OPTIONS = Object.entries(HAT_NAMES).map(([value, label]) => ({
  value,
  label: (
    <span>
      <span className="hat-dot" style={{ background: HAT_COLORS[value], marginRight: 6 }} />
      {label}
    </span>
  ),
}))

export function TeamPanel({ members, onChanged }: { members: Member[]; onChanged: () => void }) {
  const [editing, setEditing] = useState<Member | null>(null)
  const [adding, setAdding] = useState(false)
  const [memoryOf, setMemoryOf] = useState<Member | null>(null)
  const [profiles, setProfiles] = useState<LlmProfile[]>([])

  useEffect(() => {
    void api.get<{ profiles: LlmProfile[] }>('/llm-profiles').then((r) => setProfiles(r.profiles)).catch(() => {})
  }, [])

  const profileName = (id: string) => profiles.find((p) => p.id === id || p.name === id)?.name || id

  return (
    <div className="team-box">
      <div className="docs-bar">
        <div className="muted" style={{ flex: 1 }}>Bộ nhớ riêng + thinking của member tồn tại xuyên phiên.</div>
        <Button icon={<PlusOutlined />} onClick={() => setAdding(true)}>Thêm thành viên</Button>
      </div>
      {members.map((m) => (
        <div key={m.id} className={`team-item ${m.enabled ? '' : 'team-off'}`}>
          <span className="hat-stack">
            {splitHats(m.hat).map((h) => (
              <span key={h} className="hat-dot big" style={{ background: HAT_COLORS[h] || '#555' }} title={HAT_NAMES[h] || h} />
            ))}
            {splitHats(m.hat).length === 0 && <span className="hat-dot big" style={{ background: '#555' }} title="chưa chọn mũ" />}
          </span>
          <div className="team-main">
            <div className="team-name">
              {m.role === 'manager' && '🔵 '}
              {m.role === 'secretary' && '📝 '}
              {m.name}
              <span className="muted"> · {m.role}</span>
            </div>
            <div className="team-sub">{m.expertise}</div>
            <div className="team-sub muted">
              {m.use_tools ? (m.tools ? `${m.tools.length} tool giới hạn` : 'toàn bộ tool hệ thống') : 'không dùng tool (thuần suy luận)'}
              {m.model && (
                <Tag color="geekblue" style={{ marginLeft: 6 }} title={m.use_tools ? 'Member dùng tool hiện vẫn chạy model active của daemon' : `Chạy trên profile ${m.model}`}>
                  🧩 {profileName(m.model)}
                </Tag>
              )}
            </div>
          </div>
          <Button size="small" onClick={() => setMemoryOf(m)}>🧠 Bộ nhớ</Button>
          <Button size="small" onClick={() => setEditing(m)}>Sửa</Button>
          <Switch size="small" checked={m.enabled}
            onChange={(v) => void api.patch(`/members/${m.id}`, { enabled: v }).then(onChanged)} />
        </div>
      ))}
      {(editing || adding) && (
        <MemberDialog member={editing} profiles={profiles} onClose={() => { setEditing(null); setAdding(false) }}
          onSaved={() => { setEditing(null); setAdding(false); onChanged() }} />
      )}
      {memoryOf && <MemoryDialog member={memoryOf} onClose={() => setMemoryOf(null)} />}
    </div>
  )
}

function MemberDialog({
  member, profiles, onClose, onSaved,
}: {
  member: Member | null
  profiles: LlmProfile[]
  onClose: () => void
  onSaved: () => void
}) {
  const { message } = AntApp.useApp()
  const [name, setName] = useState(member?.name ?? '')
  const [role, setRole] = useState<Member['role']>(member?.role ?? 'member')
  const [expertise, setExpertise] = useState(member?.expertise ?? '')
  const [style, setStyle] = useState(member?.style ?? '')
  const [hats, setHats] = useState<string[]>(splitHats(member?.hat))
  const [model, setModel] = useState<string | undefined>(member?.model ?? undefined)
  const [useTools, setUseTools] = useState(member?.use_tools ?? true)
  const [restrict, setRestrict] = useState(Boolean(member?.tools))
  const [selected, setSelected] = useState<string[]>(member?.tools ?? [])
  const [catalog, setCatalog] = useState<ToolInfo[]>([])

  useEffect(() => {
    if (useTools && restrict && catalog.length === 0) {
      void api.get<{ tools: ToolInfo[] }>('/tools').then((r) => setCatalog(r.tools)).catch(() => {})
    }
  }, [useTools, restrict]) // eslint-disable-line react-hooks/exhaustive-deps

  const toolOptions = useMemo(
    () => catalog.map((t) => ({ value: t.full, label: t.full, title: t.description })),
    [catalog],
  )

  const save = async () => {
    const body: Record<string, unknown> = {
      name: name.trim(), expertise, style,
      hat: hats, // backend nhận mảng hoặc chuỗi phẩy
      model: model ?? null, // null = dùng model active của daemon
      use_tools: useTools,
      tools: useTools && restrict ? selected : null,
    }
    try {
      if (member) await api.patch(`/members/${member.id}`, body)
      else await api.post('/members', { ...body, role })
      onSaved()
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    }
  }

  return (
    <Modal open onCancel={onClose} width={640}
      title={member ? `Sửa: ${member.name}` : 'Thêm thành viên AI'}
      okText="Lưu" cancelText="Huỷ"
      okButtonProps={{ disabled: !name.trim() }}
      onOk={() => void save()}>
      <div className="v-form">
        <div className="form-grid">
          <label className="fld">Tên
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Hà • Dữ liệu" />
          </label>
          {!member ? (
            <label className="fld">Vai trò
              <Select value={role} onChange={setRole}
                options={[
                  { value: 'member', label: 'member — thảo luận' },
                  { value: 'manager', label: 'manager — điều phối' },
                  { value: 'secretary', label: 'secretary — thư ký' },
                ]} />
            </label>
          ) : <span />}
        </div>
        <label className="fld">Mũ thiên hướng (chọn được nhiều — mỗi phát biểu member dùng 1 mũ trong số này)
          <Select mode="multiple" allowClear value={hats} onChange={setHats}
            placeholder="Chưa chọn = member tự do chọn mũ" options={HAT_OPTIONS} />
        </label>
        <label className="fld">Model riêng (LLM profile — mỗi member một model để tranh luận chéo, VD 1 Gemini vs 1 Claude)
          <Select allowClear showSearch value={model} onChange={setModel}
            placeholder="Bỏ trống = model active của hệ thống"
            optionFilterProp="label"
            options={profiles.map((p) => ({
              value: p.id,
              label: `${p.name || p.model}${p.provider ? ` · ${p.provider}` : ''}${p.active ? ' (active)' : ''}`,
            }))} />
          {useTools && model && (
            <span className="muted" style={{ fontSize: 12 }}>
              ⚠ Member đang bật tool: daemon hiện chạy agent theo model active (chưa hỗ trợ per-run model) — model riêng chỉ áp dụng thật khi TẮT tool, hoặc sau khi daemon được vá.
            </span>
          )}
        </label>
        <label className="fld">Chuyên môn
          <Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={expertise}
            onChange={(e) => setExpertise(e.target.value)}
            placeholder="VD: Phân tích số liệu thị trường, đọc báo cáo tài chính…" />
        </label>
        <label className="fld">Phong cách
          <Input.TextArea autoSize={{ minRows: 2, maxRows: 5 }} value={style}
            onChange={(e) => setStyle(e.target.value)}
            placeholder="VD: Thẳng thắn, ngắn gọn, luôn đòi số liệu trước khi kết luận…" />
        </label>
        <Checkbox checked={useTools} onChange={(e) => setUseTools(e.target.checked)}>
          Dùng tool MCP (agent đầy đủ)
        </Checkbox>
        {useTools && (
          <Checkbox checked={restrict} onChange={(e) => setRestrict(e.target.checked)}>
            Giới hạn danh sách tool (bỏ chọn = toàn bộ tool hệ thống)
          </Checkbox>
        )}
        {useTools && restrict && (
          <Select mode="multiple" allowClear showSearch value={selected} onChange={setSelected}
            placeholder={catalog.length ? 'Chọn tool cho member này…' : 'Không lấy được danh mục tool từ daemon'}
            options={toolOptions} optionFilterProp="value"
            maxTagCount={6} styles={{ root: { width: '100%' } }} />
        )}
      </div>
    </Modal>
  )
}

function MemoryDialog({ member, onClose }: { member: Member; onClose: () => void }) {
  type Mem = { memory: { kind: string; content: string; created_at: number }[]; thinking: { round: number; content: string }[] }
  const [data, setData] = useState<Mem | null>(null)
  useEffect(() => {
    void api.get<Mem>(`/members/${member.id}/memory`).then(setData).catch(() => setData({ memory: [], thinking: [] }))
  }, [member.id]) // eslint-disable-line react-hooks/exhaustive-deps
  return (
    <Modal open onCancel={onClose} footer={null} width={640}
      title={<span>🧠 Bộ nhớ riêng — {member.name} {splitHats(member.hat).map((h) => (
        <Tag key={h} color={HAT_COLORS[h]} style={{ marginLeft: 4 }}>{HAT_NAMES[h]?.split('·')[0]}</Tag>
      ))}</span>}>
      {!data && <div className="panel-empty">Đang tải…</div>}
      {data && (
        <>
          <div className="panel-sub">Ghi nhớ ({data.memory.length})</div>
          {data.memory.length === 0 && <div className="muted">Chưa có — member tự ghi sau mỗi lượt.</div>}
          <ul className="mem-list">
            {data.memory.map((m, i) => <li key={i}><b>[{m.kind}]</b> {m.content}</li>)}
          </ul>
          <div className="panel-sub">Mạch suy nghĩ gần nhất</div>
          {data.thinking.length === 0 && <div className="muted">Chưa có (thinking lưu theo từng phiên).</div>}
          <ul className="mem-list">
            {data.thinking.map((t, i) => <li key={i}><b>vòng {t.round}:</b> {t.content}</li>)}
          </ul>
        </>
      )}
    </Modal>
  )
}
