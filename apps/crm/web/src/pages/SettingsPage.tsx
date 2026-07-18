import { useEffect, useState } from 'react'
import { App as AntApp, Button, Divider, Input, InputNumber, Segmented, Space, Switch } from 'antd'
import { DownloadOutlined } from '@ant-design/icons'
import { api, fmtDateTime, type CrmSettings } from '../api'
import { fmt, type Lang, type T } from '../i18n'
import { PageShell } from '../components/PageShell'
import { Field } from '../components/Field'
import type { View } from '../components/Sidebar'

export type UiSettings = {
  splitRight: View | null
  syncSpaceCalendar: boolean
  lastSyncedAt: number | null
}
export const UI_SETTINGS_DEFAULT: UiSettings = {
  splitRight: null,
  syncSpaceCalendar: true,
  lastSyncedAt: null,
}

export function SettingsPage({
  t,
  lang,
  setLang,
  settings,
  setSettings,
}: {
  t: T
  lang: Lang
  setLang: (l: Lang) => void
  settings: UiSettings
  setSettings: (u: (s: UiSettings) => UiSettings) => void
}) {
  const [syncing, setSyncing] = useState(false)
  const [reindexing, setReindexing] = useState(false)
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null)
  const [guard, setGuard] = useState<CrmSettings>({})
  const [savingGuard, setSavingGuard] = useState(false)
  const { message } = AntApp.useApp()

  useEffect(() => {
    api.getSettings().then(setGuard).catch(() => {})
  }, [])

  async function saveGuard(patch: CrmSettings) {
    setSavingGuard(true)
    try {
      setGuard(await api.updateSettings(patch))
      message.success(t('saved'))
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setSavingGuard(false)
    }
  }

  /// The language lives server-side under the `language` settings key, so it
  /// follows the operator across browsers rather than sticking to one profile.
  async function changeLang(l: Lang) {
    setLang(l)
    try {
      await api.updateSettings({ language: l })
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    }
  }

  async function syncNow() {
    setSyncing(true)
    setMsg(null)
    try {
      const r = await fetch('/api/sync/calendar', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ space_calendar: settings.syncSpaceCalendar }),
      }).then((x) => x.json())
      setSettings((s) => ({ ...s, lastSyncedAt: Math.floor(Date.now() / 1000) }))
      setMsg({
        ok: true,
        text:
          fmt(t('syncDone'), { a: r.pushed_tasks ?? 0, b: r.pushed_birthdays ?? 0 }) + ` ${r.note ?? ''}`,
      })
    } catch (e) {
      setMsg({ ok: false, text: t('syncFailed') + String(e) })
    } finally {
      setSyncing(false)
    }
  }

  async function reindex() {
    setReindexing(true)
    try {
      await fetch('/api/reindex', { method: 'POST' })
      setMsg({ ok: true, text: t('rebuildDone') })
    } finally {
      setReindexing(false)
    }
  }

  return (
    <PageShell title={t('settingsTitle')}>
      <div className="card">
        <div className="section-title">🎨 {t('appearance')}</div>
        <div className="settings-row">
          <div>
            <div>{t('language')}</div>
            <div className="muted small">vi / en</div>
          </div>
          <Segmented
            value={lang}
            onChange={(v) => changeLang(v as Lang)}
            options={[
              { label: '🇻🇳 Tiếng Việt', value: 'vi' },
              { label: '🇬🇧 English', value: 'en' },
            ]}
          />
        </div>
        <Divider style={{ margin: '12px 0' }} />
        <div className="settings-row">
          <div>
            <div>{t('splitLabel')}</div>
            <div className="muted small">{t('splitHint')}</div>
          </div>
          <Switch
            checked={settings.splitRight !== null}
            onChange={(v) => setSettings((s) => ({ ...s, splitRight: v ? 'tasks' : null }))}
          />
        </div>
      </div>

      <div className="card">
        <div className="section-title">📅 {t('calendarSync')}</div>
        <div className="muted small" style={{ marginBottom: 10 }}>
          {t('calendarSyncHint')}
        </div>
        <div className="settings-row">
          <span>📅 {t('calendarSyncToggle')}</span>
          <Switch
            checked={settings.syncSpaceCalendar}
            onChange={(v) => setSettings((s) => ({ ...s, syncSpaceCalendar: v }))}
          />
        </div>
        <Space style={{ marginTop: 10 }}>
          <Button type="primary" loading={syncing} onClick={syncNow} disabled={!settings.syncSpaceCalendar}>
            🔄 {t('syncNow')}
          </Button>
          <span className="muted small">
            {settings.lastSyncedAt ? `${t('lastSynced')}: ${fmtDateTime(settings.lastSyncedAt)}` : t('neverSynced')}
          </span>
        </Space>
      </div>

      <div className="card">
        <div className="section-title">🛡 {t('guardrails')}</div>
        <div className="edit-grid">
          <Field label={t('brandVoice')} full>
            <Input.TextArea
              rows={2}
              value={guard.brand_voice ?? ''}
              onChange={(e) => setGuard((g) => ({ ...g, brand_voice: e.target.value }))}
            />
          </Field>
          <Field label={t('riskyKeywords')} full>
            <Input
              value={guard.risky_keywords ?? ''}
              onChange={(e) => setGuard((g) => ({ ...g, risky_keywords: e.target.value }))}
            />
          </Field>
          <Field label={t('complaintKeywords')} full>
            <Input
              value={guard.complaint_keywords ?? ''}
              onChange={(e) => setGuard((g) => ({ ...g, complaint_keywords: e.target.value }))}
            />
          </Field>
          <Field label={t('maxPerDay')}>
            <InputNumber
              min={1}
              style={{ width: '100%' }}
              value={Number(guard.max_messages_per_customer_24h ?? 5)}
              onChange={(v) =>
                setGuard((g) => ({ ...g, max_messages_per_customer_24h: String(v ?? 5) }))
              }
            />
          </Field>
          <Field label={t('autoWelcome')}>
            <Switch
              checked={guard.auto_welcome === '1'}
              onChange={(v) => setGuard((g) => ({ ...g, auto_welcome: v ? '1' : '0' }))}
            />
          </Field>
        </div>
        <div className="formactions">
          <Button
            type="primary"
            loading={savingGuard}
            onClick={() =>
              saveGuard({
                brand_voice: guard.brand_voice,
                risky_keywords: guard.risky_keywords,
                complaint_keywords: guard.complaint_keywords,
                max_messages_per_customer_24h: guard.max_messages_per_customer_24h,
                auto_welcome: guard.auto_welcome,
              })
            }
          >
            {t('save')}
          </Button>
        </div>
      </div>

      <div className="card">
        <div className="section-title">🗄 {t('dataSection')}</div>
        <Space wrap>
          <Button loading={reindexing} onClick={reindex}>
            🔧 {t('rebuildIndex')}
          </Button>
          <Button href="/api/export.csv" download icon={<DownloadOutlined />}>
            {t('exportAll')}
          </Button>
        </Space>
      </div>

      {msg && <div className={msg.ok ? 'ai-out' : 'err inline'}>{msg.text}</div>}
    </PageShell>
  )
}
