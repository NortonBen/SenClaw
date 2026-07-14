import { StrictMode, useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import type { Agent, Task } from './types'
import './styles.css'

function Widget() {
  const [agents, setAgents] = useState<Agent[]>([])
  const [task, setTask] = useState<Task | null>(null)

  useEffect(() => {
    const load = async () => {
      try {
        const a = await fetch('api/agents').then((r) => r.json())
        const t = await fetch('api/tasks?limit=1').then((r) => r.json())
        setAgents(a.agents ?? [])
        setTask(t.tasks?.[0] ?? null)
      } catch {
        /* daemon may proxy before the app is up */
      }
    }
    load()
    const iv = setInterval(load, 5000)
    return () => clearInterval(iv)
  }, [])

  const working = agents.filter((a) => a.status === 'working')
  return (
    <div style={{ padding: 10, fontSize: 12 }}>
      <div style={{ fontWeight: 700, letterSpacing: 1, marginBottom: 6 }}>
        🏢 AI OFFICE — {agents.length} agent trực ca
      </div>
      {task ? (
        <div>
          <div style={{ color: 'var(--faint)' }}>Nhiệm vụ gần nhất ({task.mode.toUpperCase()}):</div>
          <div>{task.title}</div>
          <div style={{ marginTop: 4 }}>
            Trạng thái: <b>{task.status}</b>
            {working.length > 0 && <> · đang làm: {working.map((w) => w.name).join(', ')}</>}
          </div>
        </div>
      ) : (
        <div style={{ color: 'var(--faint)' }}>Chưa có nhiệm vụ — giao việc đầu tiên cho phòng!</div>
      )}
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Widget />
  </StrictMode>,
)
