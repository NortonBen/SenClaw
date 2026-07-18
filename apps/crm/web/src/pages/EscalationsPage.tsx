import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Button, Card, Space } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { api, fmtDateTime, type Escalation } from '../api'
import { tk, type T } from '../i18n'
import { subscribeEvents } from '../events'
import { PageShell } from '../components/PageShell'
import { Avatar } from '../components/Avatar'
import { Chip } from '../components/chips'

export function EscalationsPage({ t, onPickCustomer }: { t: T; onPickCustomer: (id: number) => void }) {
  const [rows, setRows] = useState<Escalation[]>([])
  const [loading, setLoading] = useState(false)
  const [busyId, setBusyId] = useState<number | null>(null)
  const { message } = AntApp.useApp()

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      setRows(await api.listEscalations({ status: 'open', limit: 200 }))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
    return subscribeEvents(refresh)
  }, [refresh])

  async function resolve(id: number) {
    setBusyId(id)
    try {
      await api.resolveEscalation(id, { by: 'operator' })
      await refresh()
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <PageShell
      title={t('navEscalations')}
      subtitle={`${rows.length} ${t('openEscalations').toLowerCase()}`}
      actions={
        <Button icon={<ReloadOutlined />} loading={loading} onClick={refresh}>
          {t('refresh')}
        </Button>
      }
    >
      {rows.length === 0 && <div className="empty big">{t('noEscalations')}</div>}
      <div className="review-grid">
        {rows.map((e) => (
          <Card
            key={e.id}
            className="review-card"
            title={
              <Space>
                <Avatar name={e.customer_name || `#${e.customer_id}`} size={28} />
                <button className="linklike" onClick={() => onPickCustomer(e.customer_id)}>
                  {e.customer_name || `#${e.customer_id}`}
                </button>
              </Space>
            }
            extra={<Chip color="#ff3b30">⚠ {tk(t, 'risk', e.reason)}</Chip>}
          >
            {e.context && (
              <div className="esc-block">
                <div className="muted small">{t('context')}</div>
                <div className="notes">{e.context}</div>
              </div>
            )}
            {e.draft && (
              <div className="esc-block">
                <div className="muted small">{t('draftLabel')}</div>
                <div className="notes">{e.draft}</div>
              </div>
            )}
            <div className="review-foot">
              <span className="muted small">{fmtDateTime(e.created_at)}</span>
              <Button
                size="small"
                type="primary"
                loading={busyId === e.id}
                onClick={() => resolve(e.id)}
              >
                {busyId === e.id ? t('resolving') : t('resolve')}
              </Button>
            </div>
          </Card>
        ))}
      </div>
    </PageShell>
  )
}
