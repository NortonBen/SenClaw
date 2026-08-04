import { useCallback, useEffect, useState } from 'react'
import { api } from './api'
import type { Project } from './api'
import {
  SourcesPanel,
  TboxPanel,
  MappingPanel,
  ExplorerPanel,
  CompetencyPanel,
  ValidationPanel,
  GovernancePanel,
  setPanelNotify,
} from './panels'
import { StudioPanel } from './studio'
import { AssistSidebar } from './assist'
import { ModelPicker } from './ModelPicker'
import { LogicPanel } from './logic'

type Toast = { msg: string; err?: boolean } | null

const TABS = [
  { key: 'studio', step: 0, label: 'Studio', icon: '✨' },
  { key: 'sources', step: 1, label: 'Sources', icon: '📥' },
  { key: 'tbox', step: 2, label: 'Ontology', icon: '🕸️' },
  { key: 'mapping', step: 3, label: 'Mapping', icon: '🔀' },
  { key: 'explore', step: 4, label: 'Explore', icon: '🔎' },
  { key: 'competency', step: 5, label: 'Competency', icon: '✅' },
  { key: 'validate', step: 6, label: 'Validate', icon: '🛡️' },
  { key: 'governance', step: 7, label: 'Reason & Provenance', icon: '🧬' },
  { key: 'logic', step: 8, label: 'Logic', icon: '⚙️' },
] as const

type TabKey = (typeof TABS)[number]['key']

export default function App() {
  const [projects, setProjects] = useState<Project[]>([])
  const [pid, setPid] = useState<number | null>(null)
  const [tab, setTab] = useState<TabKey>('studio')
  const [toast, setToast] = useState<Toast>(null)
  const [assistOpen, setAssistOpen] = useState(false)
  const [selection, setSelection] = useState<string | undefined>()
  const [theme, setTheme] = useState<'light' | 'dark'>(
    () => (document.documentElement.getAttribute('data-theme') as 'light' | 'dark') || 'light',
  )

  const notify = useCallback((msg: string, err = false) => {
    setToast({ msg, err })
    setTimeout(() => setToast(null), err ? 5000 : 2600)
  }, [])

  useEffect(() => {
    setPanelNotify(notify)
  }, [notify])

  const refresh = useCallback(async () => {
    try {
      const ps = await api.listProjects()
      setProjects(ps)
      setPid((cur) => (cur == null ? ps[0]?.id ?? null : cur))
    } catch (e) {
      notify((e as Error).message, true)
    }
  }, [notify])

  useEffect(() => {
    void refresh()
  }, [refresh])

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
  }, [theme])

  const current = projects.find((p) => p.id === pid) || null

  async function createProject() {
    const name = prompt('New ontology project name (e.g. "Supply Chain")')?.trim()
    if (!name) return
    try {
      const r = await api.createProject(name, '')
      await refresh()
      setPid(r.id)
      setTab('sources')
      notify('Project created — base ' + r.baseIri)
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  async function deleteProject(id: number) {
    if (!confirm('Delete this project and all its triples?')) return
    try {
      await api.deleteProject(id)
      if (pid === id) setPid(null)
      await refresh()
    } catch (e) {
      notify((e as Error).message, true)
    }
  }

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="brand">
          <div className="logo">◎</div>
          <div>
            <h1>SenClaw Ontology</h1>
            <small>raw data → knowledge graph</small>
          </div>
        </div>
        <div className="proj-list">
          <div className="section">Projects</div>
          {projects.length === 0 && <div className="proj-item m">No projects yet</div>}
          {projects.map((p) => (
            <div key={p.id} className={'proj-item' + (p.id === pid ? ' active' : '')} onClick={() => setPid(p.id)}>
              <span className="n">{p.name}</span>
              <span className="m">
                {p.tripleCount.toLocaleString()} triples ·{' '}
                <span className="mono" title={p.baseIri}>{p.baseIri.replace(/^https?:\/\//, '')}</span>
              </span>
            </div>
          ))}
        </div>
        <div className="foot" style={{ flexDirection: 'column', alignItems: 'stretch', gap: 10 }}>
          <ModelPicker notify={notify} />
          <div className="row" style={{ gap: 8 }}>
            <button className="primary" style={{ flex: 1 }} onClick={createProject}>＋ New project</button>
            <button className="ghost" title="Toggle theme" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>
              {theme === 'dark' ? '☀' : '☾'}
            </button>
          </div>
        </div>
      </aside>

      <main className="main">
        {current ? (
          <>
            <nav className="tabs">
              {TABS.map((t) => (
                <div key={t.key} className={'tab' + (tab === t.key ? ' active' : '')} onClick={() => setTab(t.key)}>
                  {t.step > 0 && <span className="step">{t.step}</span>}
                  <span>{t.icon} {t.label}</span>
                </div>
              ))}
              <div className="spacer" />
              <a className="tab" href={api.exportUrl(current.id)} target="_blank" rel="noreferrer" title="Download TriG">⬇ Export</a>
              <div
                className={'tab' + (assistOpen ? ' active' : '')}
                title="AIP Assist — context-aware help about this project's metadata"
                onClick={() => setAssistOpen((o) => !o)}
              >
                ◆ Assist
              </div>
            </nav>
            <div className="content" key={current.id + tab}>
              {tab === 'studio' && (
                <StudioPanel pid={current.id} notify={notify} onChanged={refresh} onGoTo={setTab} />
              )}
              {tab === 'sources' && (
                <SourcesPanel pid={current.id} notify={notify} onChanged={refresh} onSelection={setSelection} />
              )}
              {tab === 'tbox' && <TboxPanel pid={current.id} notify={notify} onChanged={refresh} />}
              {tab === 'mapping' && <MappingPanel pid={current.id} notify={notify} onChanged={refresh} />}
              {tab === 'explore' && <ExplorerPanel pid={current.id} notify={notify} onChanged={refresh} />}
              {tab === 'competency' && <CompetencyPanel pid={current.id} notify={notify} onChanged={refresh} />}
              {tab === 'validate' && <ValidationPanel pid={current.id} notify={notify} onChanged={refresh} />}
              {tab === 'governance' && (
                <GovernancePanel pid={current.id} notify={notify} onChanged={refresh} onDeleteProject={() => deleteProject(current.id)} />
              )}
              {tab === 'logic' && <LogicPanel pid={current.id} notify={notify} onChanged={refresh} />}
            </div>
          </>
        ) : (
          <div className="empty" style={{ margin: 'auto' }}>
            <div className="big">◎</div>
            <h2>Drop your data in, ask it questions</h2>
            <p style={{ maxWidth: 460 }}>
              Any file — spreadsheet, JSON, XML, PDF, Word, prose — is profiled, modelled into an ontology, lifted into
              RDF and validated for you. Then just ask, in plain language. Every step stays open for review, and every
              triple keeps its provenance.
            </p>
            <button className="primary" onClick={createProject}>＋ Create your first project</button>
          </div>
        )}
      </main>

      {current && (
        <AssistSidebar
          pid={current.id}
          tab={tab}
          source={selection}
          open={assistOpen}
          onClose={() => setAssistOpen(false)}
          onGoTo={(t) => setTab(t as TabKey)}
        />
      )}

      {toast && <div className={'toast' + (toast.err ? ' err' : '')}>{toast.msg}</div>}
    </div>
  )
}
