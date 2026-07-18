import { useEffect, useState } from 'react'
import { Row, Col, Card, Select, Switch, Space, Descriptions, Typography, Tag } from 'antd'
import { api } from '../api'
import type { Stats } from '../api'
import type { Lang, T } from '../i18n'

export default function SettingsPage({ t, lang, setLang }: { t: T; lang: Lang; setLang: (l: Lang) => void }) {
  const [stats, setStats] = useState<Stats>({})
  const [features, setFeatures] = useState<Record<string, boolean>>({})
  const [llm, setLlm] = useState<{ available: boolean; config?: Record<string, unknown> }>({ available: false })
  const [crmEnabled, setCrmEnabled] = useState(true)
  const [crmBase, setCrmBase] = useState('')

  useEffect(() => {
    api.stats().then(setStats).catch(() => {})
    api.getSettings().then((s) => { setFeatures(s.features); setCrmEnabled(s.crmEnabled); setCrmBase(s.crmBase) }).catch(() => {})
    api.llmInfo().then(setLlm).catch(() => {})
  }, [])

  const toggle = async (k: string, v: boolean) => {
    const next = { ...features, [k]: v }
    setFeatures(next)
    await api.updateSettings({ features: next })
  }
  const changeLang = async (l: Lang) => { setLang(l); await api.updateSettings({ language: l }) }

  return (
    <Row gutter={16}>
      <Col span={12}>
        <Card title={t('tabSettings')} size="small">
          <Space direction="vertical" style={{ width: '100%' }}>
            <div>
              <Typography.Text type="secondary">{t('language')}</Typography.Text>
              <br />
              <Select value={lang} style={{ width: 200 }} onChange={(v) => changeLang(v as Lang)}
                options={[{ label: 'Tiếng Việt', value: 'vi' }, { label: 'English', value: 'en' }]} />
            </div>
            <Typography.Text type="secondary">{t('features')}</Typography.Text>
            {['knowledge', 'wiki', 'tools'].map((k) => (
              <Space key={k}><Switch checked={features[k] ?? true} onChange={(v) => toggle(k, v)} /> {k}</Space>
            ))}
            <Typography.Text type="secondary" style={{ marginTop: 8 }}>{t('crm')}</Typography.Text>
            <Space>
              <Switch checked={crmEnabled} onChange={(v) => { setCrmEnabled(v); api.updateSettings({ crmEnabled: v }) }} /> {t('crmEnabled')}
            </Space>
            {/* Auto-discovered from SenClaw's installed Space Apps — not entered by hand. */}
            <Space size={6}>
              {crmBase ? <Tag color="green">{t('crmAuto')}</Tag> : <Tag color="orange">{t('crmNotFound')}</Tag>}
              <Typography.Text type="secondary" code>{crmBase || '—'}</Typography.Text>
            </Space>
          </Space>
        </Card>
      </Col>
      <Col span={12}>
        <Card title={t('stats')} size="small">
          <Descriptions column={1} size="small">
            {Object.entries(stats).map(([k, v]) => (
              <Descriptions.Item key={k} label={k}>{String(v)}</Descriptions.Item>
            ))}
          </Descriptions>
          <Typography.Paragraph type="secondary" style={{ marginTop: 12 }}>
            LLM: {llm.available ? <Typography.Text code>{JSON.stringify(llm.config)}</Typography.Text> : 'daemon offline'}
          </Typography.Paragraph>
        </Card>
      </Col>
    </Row>
  )
}
