import { useCallback, useEffect, useState } from 'react'
import { App as AntApp, Button, Card, Input, Space } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import { api, fmtDateTime, type Review } from '../api'
import { tk, type T } from '../i18n'
import { subscribeEvents } from '../events'
import { PageShell } from '../components/PageShell'
import { Avatar } from '../components/Avatar'
import { Chip } from '../components/chips'
import { inboxChannelMeta } from '../constants'

/// Drafts the guardrail parked instead of sending. Each card is editable —
/// approving with an edit sends the edited text, not the original.
export function ReviewsPage({ t, onPickCustomer }: { t: T; onPickCustomer: (id: number) => void }) {
  const [reviews, setReviews] = useState<Review[]>([])
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      setReviews(await api.listReviews({ status: 'pending', limit: 200 }))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    refresh()
    return subscribeEvents(refresh)
  }, [refresh])

  return (
    <PageShell
      title={t('navReviews')}
      subtitle={`${reviews.length} ${t('pendingReviews').toLowerCase()}`}
      actions={
        <Button icon={<ReloadOutlined />} loading={loading} onClick={refresh}>
          {t('refresh')}
        </Button>
      }
    >
      {reviews.length === 0 && <div className="empty big">{t('noReviews')}</div>}
      <div className="review-grid">
        {reviews.map((r) => (
          <ReviewCard key={r.id} r={r} t={t} onDone={refresh} onPickCustomer={onPickCustomer} />
        ))}
      </div>
    </PageShell>
  )
}

function ReviewCard({
  r,
  t,
  onDone,
  onPickCustomer,
}: {
  r: Review
  t: T
  onDone: () => Promise<void>
  onPickCustomer: (id: number) => void
}) {
  const [draft, setDraft] = useState(r.edited || r.draft)
  const [busy, setBusy] = useState<'ok' | 'no' | null>(null)
  const { message } = AntApp.useApp()
  const meta = inboxChannelMeta(r.channel)

  async function approve() {
    setBusy('ok')
    try {
      // Only send `edited` when it actually differs — an untouched draft should
      // go out as the original, not as a same-text "edit".
      const edited = draft.trim() !== r.draft.trim() ? draft.trim() : undefined
      await api.approveReview(r.id, { edited, by: 'operator' })
      message.success(t('approveSend'))
      await onDone()
    } catch (e) {
      message.error(String(e instanceof Error ? e.message : e))
    } finally {
      setBusy(null)
    }
  }

  async function reject() {
    setBusy('no')
    try {
      await api.rejectReview(r.id, { by: 'operator' })
      await onDone()
    } finally {
      setBusy(null)
    }
  }

  return (
    <Card
      className="review-card"
      title={
        <Space>
          <Avatar name={r.customer_name || `#${r.customer_id}`} size={28} />
          <button className="linklike" onClick={() => onPickCustomer(r.customer_id)}>
            {r.customer_name || `#${r.customer_id}`}
          </button>
        </Space>
      }
      extra={
        <Space size={4}>
          <Chip color={meta.color}>
            {meta.icon} {r.channel}
          </Chip>
          <Chip color="#ff9500">⚠ {tk(t, 'risk', r.risk_reason)}</Chip>
        </Space>
      }
    >
      {r.subject && (
        <div style={{ marginBottom: 6 }}>
          <b>{r.subject}</b>
        </div>
      )}
      <Input.TextArea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        autoSize={{ minRows: 3, maxRows: 12 }}
      />
      <div className="review-foot">
        <span className="muted small">{fmtDateTime(r.created_at)}</span>
        <Space>
          <Button size="small" danger loading={busy === 'no'} onClick={reject}>
            {t('reject')}
          </Button>
          <Button
            size="small"
            type="primary"
            loading={busy === 'ok'}
            disabled={!draft.trim()}
            onClick={approve}
          >
            {busy === 'ok' ? t('approving') : t('approveSend')}
          </Button>
        </Space>
      </div>
    </Card>
  )
}
