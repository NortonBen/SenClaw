import { useCallback, useEffect, useState } from 'react'
import { Button, InputNumber, Select } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import { api, fmtDate, formatMoney, type Deal, type Organization, type Task } from '../../api'
import { STAGE_ORDER } from '../../constants'
import { tk, type T } from '../../i18n'
import { Field } from '../Field'
import { DealServicesEditor } from '../DealServices'
import { TaskRow } from '../TaskRow'
import { dealStageOptions } from '../chips'

// ---------- deals under a customer ----------

export function DealsSection({ customerId, t }: { customerId: number; t: T }) {
  const [deals, setDeals] = useState<Deal[]>([])
  const [showNew, setShowNew] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    setDeals(await api.customerDeals(customerId))
  }, [customerId])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function quickStage(d: Deal, stage: string) {
    // Quick inline stage change — auto-logged, no user note.
    await api.updateDeal(d.id, { stage })
    await refresh()
  }
  async function del(id: number) {
    if (!confirm(t('deleteDeal'))) return
    await api.deleteDeal(id)
    await refresh()
  }

  return (
    <div className="card">
      <div className="section-title">
        📊 {t('deals')} ({deals.length})
        <Button size="small" type="text" className="right" icon={<PlusOutlined />} onClick={() => setShowNew(true)}>
          {t('addDeal')}
        </Button>
      </div>
      {deals.length === 0 && !showNew && <div className="empty small">{t('noDeals')}</div>}
      {deals.map((d) => {
        const isEditing = editingId === d.id
        return (
          <div key={d.id}>
            <div className="deal-row">
              <div className="deal-row-main">
                <div className="deal-title">{d.title}</div>
                <div className="deal-sub">
                  {formatMoney(d.amount, d.currency)} · {d.probability}%
                  {d.expected_close_at ? ' · ' + fmtDate(d.expected_close_at) : ''}
                </div>
              </div>
              <Select
                size="small"
                value={d.stage}
                onChange={(v) => quickStage(d, v)}
                style={{ minWidth: 130 }}
                options={dealStageOptions(t)}
              />
              <Button size="small" type="text" onClick={() => setEditingId(isEditing ? null : d.id)}>
                {isEditing ? '×' : '✎'}
              </Button>
              <button className="tl-del" onClick={() => del(d.id)} title={t('del')}>
                ×
              </button>
            </div>
            {isEditing && (
              <EditDealForm
                deal={d}
                t={t}
                onClose={() => setEditingId(null)}
                onSaved={async () => {
                  setEditingId(null)
                  await refresh()
                }}
                onLinesChanged={refresh}
              />
            )}
          </div>
        )
      })}
      {showNew && (
        <NewDealForm
          customerId={customerId}
          t={t}
          onClose={() => setShowNew(false)}
          onCreated={async () => {
            setShowNew(false)
            await refresh()
          }}
        />
      )}
    </div>
  )
}

export function EditDealForm({
  deal,
  t,
  onClose,
  onSaved,
  onLinesChanged,
}: {
  deal: Deal
  t: T
  onClose: () => void
  onSaved: () => Promise<void>
  onLinesChanged?: () => Promise<void>
}) {
  const [title, setTitle] = useState(deal.title)
  const [amount, setAmount] = useState(deal.amount)
  const [currency, setCurrency] = useState(deal.currency)
  const [stage, setStage] = useState(deal.stage)
  const [probability, setProbability] = useState(deal.probability)
  const [close, setClose] = useState(
    deal.expected_close_at ? new Date(deal.expected_close_at * 1000).toISOString().slice(0, 10) : '',
  )
  const [notes, setNotes] = useState(deal.notes)
  const [orgId, setOrgId] = useState<number>(deal.organization_id)
  const [pStart, setPStart] = useState(toDateInput(deal.period_start))
  const [pEnd, setPEnd] = useState(toDateInput(deal.period_end))
  const [orgs, setOrgs] = useState<Organization[]>([])
  const [changeNote, setChangeNote] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.listOrganizations({ limit: 500 }).then(setOrgs).catch(() => setOrgs([]))
  }, [])

  async function save() {
    if (!title.trim()) return
    setBusy(true)
    try {
      const expected_close_at = close ? Math.floor(new Date(close).getTime() / 1000) : null
      await api.updateDeal(deal.id, {
        title: title.trim(),
        amount,
        currency,
        stage,
        probability,
        expected_close_at,
        notes,
        organization_id: orgId,
        period_start: fromDateInput(pStart),
        period_end: fromDateInput(pEnd),
        change_note: changeNote.trim() || undefined,
      } as Partial<Deal> & { change_note?: string })
      await onSaved()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="edit-inline">
      <div className="edit-inline-title">{t('editDeal')}</div>
      <div className="edit-grid">
        <Field label={t('dealTitle')}>
          <input className="plain-input" value={title} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        <Field label={t('stage')}>
          <Select value={stage} onChange={setStage} options={dealStageOptions(t)} style={{ width: '100%' }} />
        </Field>
        <Field label={t('amount')}>
          <InputNumber
            value={amount}
            onChange={(v) => setAmount(Number(v) || 0)}
            style={{ width: '100%' }}
          />
        </Field>
        <Field label={t('currency')}>
          <input
            className="plain-input"
            value={currency}
            onChange={(e) => setCurrency(e.target.value.toUpperCase().slice(0, 4))}
          />
        </Field>
        <Field label={t('probability')}>
          <InputNumber
            min={0}
            max={100}
            value={probability}
            onChange={(v) => setProbability(Math.max(0, Math.min(100, Number(v) || 0)))}
            style={{ width: '100%' }}
          />
        </Field>
        <Field label={t('expectedClose')}>
          <input
            className="plain-input"
            type="date"
            value={close}
            onChange={(e) => setClose(e.target.value)}
          />
        </Field>
        <Field label={t('organizations')}>
          {/* 0 is the server's "unlinked" sentinel, so clearing maps to 0. */}
          <Select
            allowClear
            showSearch
            value={orgId || undefined}
            onChange={(v) => setOrgId(v ?? 0)}
            style={{ width: '100%' }}
            placeholder={t('orgPickerPh')}
            optionFilterProp="label"
            options={orgs.map((o) => ({ value: o.id, label: o.name }))}
          />
        </Field>
        <Field label={t('projectPeriod')}>
          <div className="row-inline">
            <input
              className="plain-input"
              type="date"
              value={pStart}
              onChange={(e) => setPStart(e.target.value)}
            />
            <input
              className="plain-input"
              type="date"
              value={pEnd}
              onChange={(e) => setPEnd(e.target.value)}
            />
          </div>
        </Field>
        <Field label={t('dealNotes')} full>
          <textarea rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </Field>
        <Field label={t('changeNote')} full>
          <input
            className="plain-input"
            value={changeNote}
            onChange={(e) => setChangeNote(e.target.value)}
            placeholder={t('dealChangeNotePh')}
          />
        </Field>
      </div>
      <DealServicesEditor
        dealId={deal.id}
        currency={currency}
        t={t}
        onChanged={() => onLinesChanged?.()}
      />
      <div className="formactions">
        <Button onClick={onClose}>{t('cancel')}</Button>
        <Button type="primary" loading={busy} disabled={!title.trim()} onClick={save}>
          {busy ? t('saving') : t('saveWithLog')}
        </Button>
      </div>
    </div>
  )
}

function NewDealForm({
  customerId,
  t,
  onClose,
  onCreated,
}: {
  customerId: number
  t: T
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [title, setTitle] = useState('')
  const [amount, setAmount] = useState(0)
  const [currency, setCurrency] = useState('VND')
  const [stage, setStage] = useState(STAGE_ORDER[0]!)
  const [close, setClose] = useState('')
  const [busy, setBusy] = useState(false)

  async function save() {
    if (!title.trim()) return
    setBusy(true)
    try {
      const expected_close_at = close ? Math.floor(new Date(close).getTime() / 1000) : undefined
      await api.createDeal(customerId, {
        title: title.trim(),
        amount,
        currency,
        stage,
        expected_close_at,
      } as Partial<Deal>)
      await onCreated()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="new-deal">
      <input
        className="plain-input"
        placeholder={t('dealTitlePh')}
        value={title}
        onChange={(e) => setTitle(e.target.value)}
      />
      <div className="row-inline">
        <InputNumber
          placeholder={t('amount')}
          value={amount || null}
          onChange={(v) => setAmount(Number(v) || 0)}
          style={{ flex: 1 }}
        />
        <input
          className="plain-input"
          placeholder="VND"
          value={currency}
          onChange={(e) => setCurrency(e.target.value.toUpperCase().slice(0, 4))}
          style={{ width: 70 }}
        />
        <Select value={stage} onChange={setStage} options={dealStageOptions(t)} style={{ minWidth: 130 }} />
        <input className="plain-input" type="date" value={close} onChange={(e) => setClose(e.target.value)} />
      </div>
      <div className="formactions">
        <Button onClick={onClose}>{t('cancel')}</Button>
        <Button type="primary" loading={busy} disabled={!title.trim()} onClick={save}>
          {busy ? t('creating') : t('addDeal')}
        </Button>
      </div>
    </div>
  )
}

// ---------- tasks under a customer ----------

export function TasksSection({ customerId, t }: { customerId: number; t: T }) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [title, setTitle] = useState('')
  const [due, setDue] = useState('')

  const refresh = useCallback(async () => {
    setTasks(await api.customerTasks(customerId))
  }, [customerId])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function add() {
    if (!title.trim()) return
    const due_at = due ? Math.floor(new Date(due).getTime() / 1000) : undefined
    await api.createTask({ customer_id: customerId, title: title.trim(), due_at })
    setTitle('')
    setDue('')
    await refresh()
  }
  async function toggle(task: Task) {
    await api.toggleTask(task.id, !task.done)
    await refresh()
  }
  async function del(id: number) {
    await api.deleteTask(id)
    await refresh()
  }

  return (
    <div className="card">
      <div className="section-title">
        ✅ {t('todo')} ({tasks.filter((x) => !x.done).length} {t('open')})
      </div>
      <div className="new-task">
        <input
          className="plain-input"
          placeholder={t('taskPh')}
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <input className="plain-input" type="date" value={due} onChange={(e) => setDue(e.target.value)} />
        <Button type="primary" disabled={!title.trim()} onClick={add}>
          {t('add')}
        </Button>
      </div>
      <div className="tasklist">
        {tasks.length === 0 && <div className="empty small">{t('noTasksSmall')}</div>}
        {tasks.map((task) => (
          <TaskRow key={task.id} t={task} tr={t} onToggle={() => toggle(task)} onDelete={() => del(task.id)} />
        ))}
      </div>
    </div>
  )
}

/// Deal stage label helper reused by the board.
export function dealStageLabel(t: T, stage: string) {
  return tk(t, 'dealStage', stage)
}

/// epoch seconds → `<input type="date">` value ('' when unset).
function toDateInput(secs: number | null | undefined): string {
  return secs ? new Date(secs * 1000).toISOString().slice(0, 10) : ''
}

/// `<input type="date">` value → epoch seconds. `null` clears the field —
/// DealPatch models these as `Option<Option<i64>>`, so an explicit null is the
/// clear and an absent key is "leave alone".
function fromDateInput(v: string): number | null {
  return v ? Math.floor(new Date(v).getTime() / 1000) : null
}
