import { useRef, useState } from 'react'
import { Button, Modal, Select, Space } from 'antd'
import { DeleteOutlined, EditOutlined, PlusOutlined } from '@ant-design/icons'
import { fmtDate, fmtDateTime, type Customer, type CustomerDetail, type CustomerInput } from '../api'
import { CHANNEL_KINDS, CHANNEL_META, ROLE_ORDER, channelMeta, roleMeta } from '../constants'
import { tk, type T } from '../i18n'
import { Avatar } from '../components/Avatar'
import { Field, fileToDataUrl } from '../components/Field'
import { FilterChips, RolePicker, TagChip } from '../components/chips'
import {
  AISection,
  ChannelsSection,
  InteractionsSection,
  NotesBox,
  OrganizationsSection,
  RelationshipsSection,
  SalesStateSection,
} from '../components/contact/ContactSections'
import { DealsSection, TasksSection } from '../components/contact/ContactDeals'

export function CustomerRow({
  c,
  selected,
  onPick,
  t,
}: {
  c: Customer
  selected: boolean
  onPick: () => void
  t: T
}) {
  const rm = roleMeta(c.role)
  return (
    <div className={'row' + (selected ? ' sel' : '')} onClick={onPick}>
      <Avatar name={c.name} url={c.avatar_url} size={40} />
      <div className="rowbody">
        <div className="line1">
          <span className="name">{c.name}</span>
          <span className="role-badge small" style={{ color: rm.color, borderColor: rm.color + '55' }}>
            {rm.icon} {rm.short}
          </span>
        </div>
        <div className="line2">{c.company || c.email || c.phone || '—'}</div>
        <div className="line3">
          {c.tags.slice(0, 3).map((tag) => (
            <span key={tag} className="minitag">
              #{tag}
            </span>
          ))}
          {c.interaction_count > 0 && (
            <span className="lastseen">
              {c.interaction_count} {t('interactionsCount')} · {fmtDate(c.last_interaction_at)}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

/// Two-column detail: left an inline-editable field stack, right notes + history.
export function CustomerDetailView({
  detail,
  t,
  onPatch,
  onDelete,
  onInteractionsChanged,
  onPickCustomer,
}: {
  detail: CustomerDetail
  t: T
  onPatch: (patch: CustomerInput & { change_note?: string }) => Promise<void>
  onDelete: () => Promise<void>
  onInteractionsChanged: () => Promise<void>
  onPickCustomer?: (id: number) => void
}) {
  const c = detail.customer
  const [editing, setEditing] = useState(false)

  return (
    <div>
      <div className="dhead">
        <Avatar name={c.name} url={c.avatar_url} size={72} />
        <div className="dheadmain">
          <div className="dtitle">
            <h1>{c.name}</h1>
            <RolePicker value={c.role} t={t} onChange={(role) => onPatch({ role })} />
          </div>
          <div className="dsub">
            {c.title && <span>{c.title}</span>}
            {c.title && c.company && <span className="dot">·</span>}
            {c.company && <span>{c.company}</span>}
          </div>
          <div className="dtags">
            {c.tags.map((tag) => (
              <TagChip key={tag} tag={tag} />
            ))}
          </div>
        </div>
        <Space>
          <Button icon={<EditOutlined />} onClick={() => setEditing((v) => !v)}>
            {editing ? t('doneLabel') : t('edit')}
          </Button>
          <Button danger icon={<DeleteOutlined />} onClick={onDelete}>
            {t('del')}
          </Button>
        </Space>
      </div>

      {editing ? (
        <EditForm customer={c} t={t} onPatch={onPatch} />
      ) : (
        <div className="detail-cols">
          <div className="detail-col-left">
            <div className="card field-stack">
              <ContactFieldStack c={c} t={t} />
              <OrganizationsSection customerId={c.id} t={t} />
              <SalesStateSection customerId={c.id} t={t} />
            </div>
            <ChannelsSection customerId={c.id} t={t} />
            <DealsSection customerId={c.id} t={t} />
            <TasksSection customerId={c.id} t={t} />
          </div>
          <div className="detail-col-right">
            <NotesBox value={c.notes} t={t} onSave={(notes) => onPatch({ notes })} />
            <AISection customerId={c.id} t={t} />
            <RelationshipsSection customer={c} t={t} onPickCustomer={onPickCustomer} />
            <InteractionsSection
              customerId={c.id}
              interactions={detail.interactions}
              t={t}
              onChanged={onInteractionsChanged}
            />
          </div>
        </div>
      )}
    </div>
  )
}

/// The read-only half of the left field stack — phones, email, the flat fields.
function ContactFieldStack({ c, t }: { c: Customer; t: T }) {
  const rows: Array<[string, React.ReactNode]> = []
  if (c.email) rows.push([`✉️ ${t('email')}`, <a href={`mailto:${c.email}`}>{c.email}</a>])
  if (c.phone) rows.push([`📞 ${t('phone')}`, <a href={`tel:${c.phone}`}>{c.phone}</a>])
  if (c.title) rows.push([`💼 ${t('jobTitle')}`, c.title])
  if (c.source) rows.push([`🔎 ${t('source')}`, c.source])
  if (c.address) rows.push([`📍 ${t('address')}`, c.address])
  if (c.birthday) rows.push([`🎂 ${t('birthday')}`, c.birthday])
  rows.push([`🕐 ${t('updatedAt')}`, fmtDateTime(c.updated_at)])
  return (
    <>
      {rows.map(([k, v]) => (
        <div className="field-stack-row" key={k}>
          <div className="fs-label">{k}</div>
          <div className="fs-value">{v}</div>
        </div>
      ))}
    </>
  )
}

function EditForm({
  customer,
  t,
  onPatch,
}: {
  customer: Customer
  t: T
  onPatch: (p: CustomerInput & { change_note?: string }) => Promise<void>
}) {
  const [form, setForm] = useState<CustomerInput>({
    name: customer.name,
    email: customer.email,
    phone: customer.phone,
    company: customer.company,
    title: customer.title,
    avatar_url: customer.avatar_url,
    notes: customer.notes,
    tags: customer.tags,
    source: customer.source,
    address: customer.address,
    birthday: customer.birthday,
    role: customer.role,
  })
  const [changeNote, setChangeNote] = useState('')
  const [busy, setBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  function set<K extends keyof CustomerInput>(k: K, v: CustomerInput[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }

  async function onAvatarFile(f: File | undefined) {
    if (!f) return
    if (f.size > 512 * 1024) {
      alert(t('avatarTooBig'))
      return
    }
    set('avatar_url', await fileToDataUrl(f))
  }

  async function save() {
    setBusy(true)
    try {
      await onPatch({ ...form, change_note: changeNote.trim() || undefined })
      setChangeNote('')
    } finally {
      setBusy(false)
    }
  }

  const tags = form.tags ?? []

  return (
    <div className="card">
      <div className="edit-grid">
        <Field label={t('name')}>
          <input className="plain-input" value={form.name ?? ''} onChange={(e) => set('name', e.target.value)} />
        </Field>
        <Field label={t('email')}>
          <input
            className="plain-input"
            value={form.email ?? ''}
            onChange={(e) => set('email', e.target.value)}
            placeholder="a@example.com"
          />
        </Field>
        <Field label={t('phone')}>
          <input className="plain-input" value={form.phone ?? ''} onChange={(e) => set('phone', e.target.value)} />
        </Field>
        <Field label={t('company')}>
          <input
            className="plain-input"
            value={form.company ?? ''}
            onChange={(e) => set('company', e.target.value)}
          />
        </Field>
        <Field label={t('jobTitle')}>
          <input className="plain-input" value={form.title ?? ''} onChange={(e) => set('title', e.target.value)} />
        </Field>
        <Field label={t('source')}>
          <input
            className="plain-input"
            value={form.source ?? ''}
            onChange={(e) => set('source', e.target.value)}
          />
        </Field>
        <Field label={t('address')}>
          <input
            className="plain-input"
            value={form.address ?? ''}
            onChange={(e) => set('address', e.target.value)}
          />
        </Field>
        <Field label={t('birthday')}>
          <input
            className="plain-input"
            value={form.birthday ?? ''}
            onChange={(e) => set('birthday', e.target.value)}
            placeholder="YYYY-MM-DD"
          />
        </Field>
        <Field label={t('avatarUrl')} full>
          <div className="avatar-row">
            <Avatar name={form.name ?? customer.name} url={form.avatar_url} size={48} />
            <input
              className="plain-input"
              value={form.avatar_url ?? ''}
              onChange={(e) => set('avatar_url', e.target.value)}
              placeholder="https://… / data:image/…"
            />
            <input
              ref={fileRef}
              type="file"
              accept="image/*"
              style={{ display: 'none' }}
              onChange={(e) => onAvatarFile(e.target.files?.[0])}
            />
            <Button size="small" onClick={() => fileRef.current?.click()}>
              {t('uploadImage')}
            </Button>
            {form.avatar_url && (
              <Button size="small" onClick={() => set('avatar_url', '')}>
                {t('del')}
              </Button>
            )}
          </div>
        </Field>
        <Field label={t('tags')} full>
          <Select
            mode="tags"
            value={tags}
            onChange={(v) => set('tags', v)}
            style={{ width: '100%' }}
            placeholder={t('tagsPh')}
            tokenSeparators={[',']}
          />
        </Field>
        <Field label={t('notes')} full>
          <textarea
            rows={4}
            value={form.notes ?? ''}
            onChange={(e) => set('notes', e.target.value)}
            placeholder={t('notesPh')}
          />
        </Field>
        <Field label={t('changeNote')} full>
          <input
            className="plain-input"
            value={changeNote}
            onChange={(e) => setChangeNote(e.target.value)}
            placeholder={t('changeNotePh')}
          />
        </Field>
      </div>
      <div className="formactions">
        <Button type="primary" loading={busy} onClick={save}>
          {busy ? t('saving') : t('saveWithLog')}
        </Button>
      </div>
    </div>
  )
}

export function NewCustomerModal({
  t,
  onClose,
  onCreate,
}: {
  t: T
  onClose: () => void
  onCreate: (
    c: CustomerInput,
    channels?: Array<{ kind: string; value: string; label?: string }>,
  ) => Promise<void>
}) {
  const [form, setForm] = useState<CustomerInput>({ name: '', role: 'lead', tags: [] })
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [channels, setChannels] = useState<Array<{ kind: string; value: string; label: string }>>([])
  const fileRef = useRef<HTMLInputElement>(null)

  const tags = form.tags ?? []

  function set<K extends keyof CustomerInput>(k: K, v: CustomerInput[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }

  async function onAvatarFile(f: File | undefined) {
    if (!f) return
    if (f.size > 512 * 1024) {
      alert(t('avatarTooBig'))
      return
    }
    set('avatar_url', await fileToDataUrl(f))
  }

  async function submit() {
    if (!form.name?.trim()) {
      setErr(t('nameRequired'))
      return
    }
    setBusy(true)
    setErr('')
    try {
      await onCreate({ ...form, name: form.name!.trim() }, channels.filter((c) => c.value.trim()))
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(false)
    }
  }

  function addChannel(kind = 'zalo') {
    setChannels((cs) => [...cs, { kind, value: '', label: '' }])
  }
  function patchChannel(i: number, patch: Partial<{ kind: string; value: string; label: string }>) {
    setChannels((cs) => cs.map((c, idx) => (idx === i ? { ...c, ...patch } : c)))
  }
  function removeChannel(i: number) {
    setChannels((cs) => cs.filter((_, idx) => idx !== i))
  }

  const previewName = form.name?.trim() || t('newCustomer')
  const quick: Array<[string, string]> = [
    ['phone', '📞'],
    ['zalo', '💬'],
    ['facebook', '📘'],
    ['linkedin', '💼'],
    ['instagram', '📷'],
    ['telegram', '✈️'],
    ['whatsapp', '📱'],
    ['website', '🌐'],
  ]

  return (
    <Modal
      open
      onCancel={onClose}
      title={t('addCustomer')}
      width={620}
      footer={[
        <Button key="cancel" onClick={onClose}>
          {t('cancel')}
        </Button>,
        <Button key="ok" type="primary" loading={busy} onClick={submit}>
          {t('createCustomer')}
        </Button>,
      ]}
    >
      <div className="modal-avatar">
        <Avatar name={previewName} url={form.avatar_url} size={80} />
        <div className="modal-avatar-actions">
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            style={{ display: 'none' }}
            onChange={(e) => onAvatarFile(e.target.files?.[0])}
          />
          <Button onClick={() => fileRef.current?.click()}>{t('pickImage')}</Button>
          <input
            className="plain-input"
            value={form.avatar_url ?? ''}
            onChange={(e) => set('avatar_url', e.target.value)}
            placeholder={t('orPasteUrl')}
          />
        </div>
      </div>
      <div className="edit-grid">
        <Field label={`${t('name')} *`}>
          <input
            className="plain-input"
            autoFocus
            value={form.name ?? ''}
            onChange={(e) => set('name', e.target.value)}
          />
        </Field>
        <Field label={t('role')}>
          <RolePicker value={form.role ?? 'lead'} t={t} onChange={(role) => set('role', role)} />
        </Field>
        <Field label={t('email')}>
          <input className="plain-input" value={form.email ?? ''} onChange={(e) => set('email', e.target.value)} />
        </Field>
        <Field label={t('phone')}>
          <input className="plain-input" value={form.phone ?? ''} onChange={(e) => set('phone', e.target.value)} />
        </Field>
        <Field label={t('company')}>
          <input
            className="plain-input"
            value={form.company ?? ''}
            onChange={(e) => set('company', e.target.value)}
          />
        </Field>
        <Field label={t('jobTitle')}>
          <input className="plain-input" value={form.title ?? ''} onChange={(e) => set('title', e.target.value)} />
        </Field>
        <Field label={t('source')}>
          <input
            className="plain-input"
            value={form.source ?? ''}
            onChange={(e) => set('source', e.target.value)}
          />
        </Field>
        <Field label={t('birthday')}>
          <input
            className="plain-input"
            value={form.birthday ?? ''}
            onChange={(e) => set('birthday', e.target.value)}
            placeholder="YYYY-MM-DD"
          />
        </Field>
        <Field label={t('tags')} full>
          <Select
            mode="tags"
            value={tags}
            onChange={(v) => set('tags', v)}
            style={{ width: '100%' }}
            placeholder={t('tagsPh')}
            tokenSeparators={[',']}
          />
        </Field>
        <Field label={t('notes')} full>
          <textarea
            rows={3}
            value={form.notes ?? ''}
            onChange={(e) => set('notes', e.target.value)}
            placeholder={t('notesPh')}
          />
        </Field>
        <Field label={t('otherChannels')} full>
          <div className="modal-channels">
            {channels.map((ch, i) => {
              const meta = channelMeta(ch.kind)
              return (
                <div key={i} className="channel-form" style={{ marginTop: 0 }}>
                  <Select
                    value={ch.kind}
                    onChange={(v) => patchChannel(i, { kind: v })}
                    style={{ minWidth: 160 }}
                    options={CHANNEL_KINDS.map((k) => ({
                      value: k,
                      label: (
                        <span>
                          {CHANNEL_META[k]!.icon} {tk(t, 'chKind', k)}
                        </span>
                      ),
                    }))}
                  />
                  <input
                    className="plain-input"
                    value={ch.value}
                    placeholder={meta.placeholder}
                    onChange={(e) => patchChannel(i, { value: e.target.value })}
                  />
                  <input
                    className="plain-input"
                    value={ch.label}
                    placeholder={t('channelNotePh')}
                    style={{ maxWidth: 200 }}
                    onChange={(e) => patchChannel(i, { label: e.target.value })}
                  />
                  <Button size="small" danger type="text" onClick={() => removeChannel(i)}>
                    ×
                  </Button>
                </div>
              )
            })}
            <Space wrap style={{ marginTop: channels.length ? 6 : 0 }}>
              {quick.map(([kind, icon]) => (
                <Button key={kind} size="small" icon={<PlusOutlined />} onClick={() => addChannel(kind)}>
                  {icon} {tk(t, 'chKind', kind)}
                </Button>
              ))}
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('zalo')}>
                {t('otherChannelBtn')}
              </Button>
            </Space>
          </div>
        </Field>
      </div>
      {err && <div className="err inline">{err}</div>}
    </Modal>
  )
}

/// The Contacts page: filter rail + list on the left, detail on the right.
export function ContactsPage({
  t,
  customers,
  allTags,
  q,
  setQ,
  tag,
  setTag,
  roleFilter,
  setRoleFilter,
  selectedId,
  setSelectedId,
  detail,
  onPatch,
  onDelete,
  refreshDetail,
  onOpenNew,
}: {
  t: T
  customers: Customer[]
  allTags: string[]
  q: string
  setQ: (v: string) => void
  tag: string | null
  setTag: (v: string | null) => void
  roleFilter: string | null
  setRoleFilter: (v: string | null) => void
  selectedId: number | null
  setSelectedId: (id: number) => void
  detail: CustomerDetail | null
  onPatch: (patch: CustomerInput & { change_note?: string }) => Promise<void>
  onDelete: () => Promise<void>
  refreshDetail: () => Promise<void>
  onOpenNew: () => void
}) {
  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="sidebar-actions">
          <Button type="primary" icon={<PlusOutlined />} block onClick={onOpenNew}>
            {t('newCustomer')}
          </Button>
        </div>
        <div className="search">
          <input
            type="search"
            placeholder={t('searchCustomers')}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </div>
        <FilterChips
          value={roleFilter}
          onChange={setRoleFilter}
          allLabel={t('allRoles')}
          options={ROLE_ORDER.map((r) => ({
            value: r,
            color: roleMeta(r).color,
            label: `${roleMeta(r).icon} ${tk(t, 'role', r)}`,
          }))}
        />
        {allTags.length > 0 && (
          <FilterChips
            value={tag}
            onChange={setTag}
            allLabel={t('allTags')}
            options={allTags.map((x) => ({ value: x, color: '#8e8e93', label: `#${x}` }))}
          />
        )}
        <div className="list">
          {customers.length === 0 && <div className="empty">{t('noCustomers')}</div>}
          {customers.map((c) => (
            <CustomerRow
              key={c.id}
              c={c}
              t={t}
              selected={c.id === selectedId}
              onPick={() => setSelectedId(c.id)}
            />
          ))}
        </div>
      </aside>

      <main className="detail">
        {detail ? (
          <CustomerDetailView
            detail={detail}
            t={t}
            onPatch={onPatch}
            onDelete={onDelete}
            onInteractionsChanged={refreshDetail}
            onPickCustomer={setSelectedId}
          />
        ) : (
          <div className="empty big">{t('pickCustomer')}</div>
        )}
      </main>
    </div>
  )
}
