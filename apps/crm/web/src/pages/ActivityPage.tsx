import { useEffect, useState } from 'react'
import { Button, Card, Timeline } from 'antd'
import { api, fmtDateTime, type ActivityItem } from '../api'
import { KIND_ICONS } from '../constants'
import { tk, type T } from '../i18n'
import { PageShell } from '../components/PageShell'

export function ActivityPage({ t, onPickCustomer }: { t: T; onPickCustomer: (id: number) => void }) {
  const [items, setItems] = useState<ActivityItem[]>([])

  useEffect(() => {
    api.activity(200).then(setItems)
  }, [])

  return (
    <PageShell title={t('navActivity')} subtitle={`${items.length} ${t('kpiTotal')}`}>
      <Card>
        {items.length === 0 ? (
          <div className="empty">{t('noInteractions')}</div>
        ) : (
          <Timeline
            items={items.map((i) => ({
              dot: <span style={{ fontSize: 16 }}>{KIND_ICONS[i.kind] ?? '•'}</span>,
              children: (
                <div>
                  <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
                    <div>
                      <Button
                        type="link"
                        size="small"
                        style={{ padding: 0 }}
                        onClick={() => onPickCustomer(i.customer_id)}
                      >
                        {i.customer_name}
                      </Button>
                      {' — '}
                      <span style={{ fontWeight: 500 }}>{i.summary}</span>
                      <span className="muted small"> · {tk(t, 'kind', i.kind)}</span>
                    </div>
                    <span style={{ color: 'var(--muted)', fontSize: 12, whiteSpace: 'nowrap' }}>
                      {fmtDateTime(i.occurred_at)}
                    </span>
                  </div>
                  {i.details && (
                    <div style={{ color: 'var(--muted)', whiteSpace: 'pre-wrap', fontSize: 13, marginTop: 3 }}>
                      {i.details}
                    </div>
                  )}
                </div>
              ),
            }))}
          />
        )}
      </Card>
    </PageShell>
  )
}
