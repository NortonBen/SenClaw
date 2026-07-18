import { fmtDate, type Task } from '../api'
import type { T } from '../i18n'

export function TaskRow({
  t: task,
  tr,
  onToggle,
  onDelete,
}: {
  t: Task
  tr: T
  onToggle: () => void
  onDelete: () => void
}) {
  const overdue = !task.done && task.due_at !== null && task.due_at < Date.now() / 1000
  return (
    <div className={'task-row' + (task.done ? ' done' : '') + (overdue ? ' overdue' : '')}>
      <input type="checkbox" checked={task.done} onChange={onToggle} />
      <div className="task-body">
        <div className="task-title">{task.title}</div>
        <div className="task-sub">
          {task.due_at ? '📅 ' + fmtDate(task.due_at) : tr('noDue')}
          {task.customer_name && ` · ${task.customer_name}`}
          {overdue && <span className="warn"> · {tr('overdue')}</span>}
        </div>
      </div>
      <button className="tl-del" onClick={onDelete} title={tr('del')}>
        ×
      </button>
    </div>
  )
}
