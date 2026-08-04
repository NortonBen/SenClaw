// Which SenClaw LLM this app runs on.
//
// Picking here sets a per-app profile — it never changes the daemon's active
// model, which the agent and every other Space App share. "" = follow the
// active model. Mirrors the moltbook / rewrite-story settings pattern so a
// model is chosen the same way across every Space App.

import { useEffect, useState } from 'react'
import { api } from './api'

type Model = { id: string; label?: string; modelName?: string; provider?: string; baseURL?: string; adapt?: string }

// The Space-App bridge reaches a model over HTTP, so it can only serve configs
// with a real endpoint. `local-mlx` models run IN-PROCESS in the daemon (empty
// baseURL) and the bridge's chat_completion cannot route to them yet — picking
// one would make every AI action fail. Mark them unavailable rather than offer a
// footgun. (Fixing this needs a core change to the bridge; tracked separately.)
function servable(m: Model): boolean {
  if (m.provider === 'local-mlx' || m.adapt === 'local-mlx') return false
  return !!(m.baseURL && m.baseURL.trim())
}

export function ModelPicker({ notify }: { notify: (m: string, err?: boolean) => void }) {
  const [models, setModels] = useState<Model[]>([])
  const [activeId, setActiveId] = useState<string | undefined>()
  const [profile, setProfile] = useState('')
  const [ready, setReady] = useState(false)

  useEffect(() => {
    let alive = true
    Promise.all([api.models(), api.getSettings()])
      .then(([m, s]) => {
        if (!alive) return
        setModels(m.configs ?? [])
        setActiveId(m.activeId)
        setProfile(s.llmProfile)
        setReady(true)
      })
      .catch(() => {
        // The daemon may not be reachable (e.g. running the app standalone).
        // A missing picker is fine — the app still follows the active model.
        if (alive) setReady(true)
      })
    return () => {
      alive = false
    }
  }, [])

  async function pick(value: string) {
    setProfile(value)
    try {
      await api.setLlmProfile(value)
      const chosen = models.find((m) => m.id === value)
      notify(value ? `AI model: ${chosen?.label ?? chosen?.modelName ?? value}` : 'AI model: follow SenClaw active')
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  // No configured models to choose from → nothing useful to show.
  if (!ready || models.length === 0) return null

  const name = (m: Model) => m.label ?? m.modelName ?? m.id

  // The stored profile may be an id OR a label (the daemon accepts both, and
  // ONTOLOGY_LLM_PROFILE is usually a label). The <select> options are keyed by
  // id, so resolve the stored value to its id or the dropdown shows the wrong
  // entry. Unmatched → "" (follow active).
  const selectedId =
    models.find((m) => m.id === profile)?.id ?? models.find((m) => name(m) === profile)?.id ?? ''

  return (
    <label className="model-picker" title="Which SenClaw LLM this app uses. Does not change the daemon's active model.">
      <span>◆ AI model</span>
      <select value={selectedId} onChange={(e) => pick(e.target.value)}>
        <option value="">
          SenClaw active{activeId ? ` (${name(models.find((m) => m.id === activeId) ?? { id: activeId })})` : ''}
        </option>
        {models.map((m) => {
          const ok = servable(m)
          return (
            <option key={m.id} value={m.id} disabled={!ok}>
              {name(m)}
              {m.provider ? ` · ${m.provider}` : ''}
              {ok ? '' : ' — not available via app bridge'}
            </option>
          )
        })}
      </select>
    </label>
  )
}
