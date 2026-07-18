import { useCallback, useEffect, useState } from 'react'
import { Button, Modal, Select, Switch } from 'antd'
import { PlusOutlined } from '@ant-design/icons'
import { api, fmtDate, type Customer, type Task, type Upcoming } from '../api'
import type { T } from '../i18n'
import { PageShell } from '../components/PageShell'
import { TaskRow } from '../components/TaskRow'
import { Field } from '../components/Field'

export function TasksPage({ t, customers }: { t: T; customers: Customer[] }) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [upcoming, setUpcoming] = useState<Upcoming | null>(null)
  const [openOnly, setOpenOnly] = useState(true)
  const [showNew, setShowNew] = useState(false)

  const refresh = useCallback(async () => {
    const [a, b] = await Promise.all([
      api.listTasks({ open_only: openOnly, limit: 300 }),
      api.upcoming(30),
    ])
    setTasks(a)
    setUpcoming(b)
  }, [openOnly])

  useEffect(() => {
    refresh()
  }, [refresh])

  return (
    <PageShell
      title={t('navTasks')}
      subtitle={`${tasks.length} ${t('tasks').toLowerCase()}`}
      filters={
        <span className="muted small">
          <Switch size="small" checked={openOnly} onChange={setOpenOnly} /> {t('openOnly')}
        </span>
      }
      actions={
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowNew(true)}>
          {t('addTask')}
        </Button>
      }
    >
      <div className="tasksview">
        <div className="tasksview-main">
          <div className="tasklist card">
            {tasks.length === 0 && <div className="empty">{t('noTasks')}</div>}
            {tasks.map((task) => (
              <TaskRow
                key={task.id}
                t={task}
                tr={t}
                onToggle={async () => {
                  await api.toggleTask(task.id, !task.done)
                  await refresh()
                }}
                onDelete={async () => {
                  await api.deleteTask(task.id)
                  await refresh()
                }}
              />
            ))}
          </div>
        </div>
        <aside className="upcoming card">
          <div className="section-title">🎂 {t('upcoming30')}</div>
          {upcoming && upcoming.birthdays.length === 0 && upcoming.tasks.length === 0 && (
            <div className="empty small">{t('noEvents')}</div>
          )}
          {upcoming?.birthdays.map((b) => (
            <div key={b.customer_id} className="upcoming-row">
              <span className="upcoming-icon">🎂</span>
              <div>
                <b>{b.customer_name}</b>
                <div className="task-sub">{fmtDate(b.next_at)}</div>
              </div>
            </div>
          ))}
          {upcoming?.tasks.map((task) => (
            <div key={task.id} className="upcoming-row">
              <span className="upcoming-icon">📌</span>
              <div>
                <b>{task.title}</b>
                <div className="task-sub">
                  {fmtDate(task.due_at)} {task.customer_name && `· ${task.customer_name}`}
                </div>
              </div>
            </div>
          ))}
        </aside>
      </div>

      {showNew && (
        <NewTaskModal
          t={t}
          customers={customers}
          onClose={() => setShowNew(false)}
          onCreated={async () => {
            setShowNew(false)
            await refresh()
          }}
        />
      )}
    </PageShell>
  )
}

function NewTaskModal({
  t,
  customers,
  onClose,
  onCreated,
}: {
  t: T
  customers: Customer[]
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [title, setTitle] = useState('')
  const [customerId, setCustomerId] = useState<number | undefined>()
  const [due, setDue] = useState('')
  const [details, setDetails] = useState('')
  const [busy, setBusy] = useState(false)

  async function save() {
    if (!title.trim()) return
    setBusy(true)
    try {
      await api.createTask({
        title: title.trim(),
        details: details.trim() || undefined,
        due_at: due ? Math.floor(new Date(due).getTime() / 1000) : undefined,
        customer_id: customerId,
      })
      await onCreated()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      open
      onCancel={onClose}
      title={t('addTask')}
      width={560}
      footer={[
        <Button key="c" onClick={onClose}>
          {t('cancel')}
        </Button>,
        <Button key="s" type="primary" loading={busy} disabled={!title.trim()} onClick={save}>
          {t('addTask')}
        </Button>,
      ]}
    >
      <div className="edit-grid">
        <Field label={t('taskTitle')} full>
          <input
            className="plain-input"
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder={t('taskTitlePh')}
          />
        </Field>
        <Field label={t('customerOptional')}>
          <Select
            allowClear
            showSearch
            value={customerId}
            onChange={setCustomerId}
            style={{ width: '100%' }}
            placeholder={t('noCustomerLink')}
            optionFilterProp="label"
            options={customers.map((c) => ({
              value: c.id,
              label: c.company ? `${c.name} · ${c.company}` : c.name,
            }))}
          />
        </Field>
        <Field label={t('due')}>
          <input className="plain-input" type="date" value={due} onChange={(e) => setDue(e.target.value)} />
        </Field>
        <Field label={t('details')} full>
          <textarea rows={3} value={details} onChange={(e) => setDetails(e.target.value)} />
        </Field>
      </div>
    </Modal>
  )
}
