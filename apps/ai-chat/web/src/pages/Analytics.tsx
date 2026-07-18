import { useCallback, useEffect, useState } from 'react'
import type { ReactNode } from 'react'
import { Row, Col, Card, Statistic, Button, Empty, Space, Avatar } from 'antd'
import {
  ReloadOutlined, WarningOutlined, ExclamationCircleOutlined, InboxOutlined,
  CustomerServiceOutlined, ThunderboltOutlined, ImportOutlined, ExportOutlined,
} from '@ant-design/icons'
import {
  PieChart, Pie, Cell, BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, Legend,
} from 'recharts'
import { api } from '../api'
import type { Analytics } from '../api'
import type { T } from '../i18n'

const COLORS = ['#1890ff', '#13c2c2', '#faad14', '#f5222d', '#722ed1', '#52c41a', '#eb2f96']

function toData(rec: Record<string, number> | undefined) {
  return Object.entries(rec || {}).map(([name, value]) => ({ name, value }))
}

function StatCard({ icon, color, title, value }: { icon: ReactNode; color: string; title: string; value: number | string }) {
  return (
    <Card size="small" styles={{ body: { display: 'flex', alignItems: 'center', gap: 14 } }}>
      <Avatar shape="square" size={44} style={{ background: `${color}1f`, color, fontSize: 20 }} icon={icon} />
      <Statistic title={title} value={value} valueStyle={{ color, fontSize: 22, fontWeight: 600 }} />
    </Card>
  )
}

export default function AnalyticsPage({ t }: { t: T }) {
  const [a, setA] = useState<Analytics | null>(null)
  const load = useCallback(() => { api.analytics().then(setA).catch(() => setA(null)) }, [])
  useEffect(load, [load])

  if (!a) return <Empty description={t('overview')} />

  const statusData = toData(a.issues.byStatus)
  const priorityData = toData(a.issues.byPriority)
  const categoryData = toData(a.issues.byCategory)
  const channelData = toData(a.sessions.byChannel)

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Space>
        <Button icon={<ReloadOutlined />} onClick={load}>{t('refresh')}</Button>
      </Space>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}><StatCard icon={<WarningOutlined />} color="#1890ff" title={t('tabIssues')} value={a.issues.total} /></Col>
        <Col xs={12} md={6}><StatCard icon={<ExclamationCircleOutlined />} color="#faad14" title={t('openIssues')} value={a.issues.open} /></Col>
        <Col xs={12} md={6}><StatCard icon={<InboxOutlined />} color="#13c2c2" title={t('tabInbox')} value={a.sessions.total} /></Col>
        <Col xs={12} md={6}><StatCard icon={<CustomerServiceOutlined />} color="#722ed1" title={t('withOperator')} value={a.sessions.openHandoffs} /></Col>
      </Row>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={8}><StatCard icon={<ThunderboltOutlined />} color="#52c41a" title="LLM calls" value={a.llmCalls} /></Col>
        <Col xs={12} md={8}><StatCard icon={<ImportOutlined />} color="#1890ff" title="Tokens in" value={a.tokensIn} /></Col>
        <Col xs={12} md={8}><StatCard icon={<ExportOutlined />} color="#eb2f96" title="Tokens out" value={a.tokensOut} /></Col>
      </Row>
      <Row gutter={16}>
        <Col span={12}>
          <Card title={t('byStatus')} size="small">
            <ResponsiveContainer width="100%" height={240}>
              <PieChart>
                <Pie data={statusData} dataKey="value" nameKey="name" outerRadius={90} label>
                  {statusData.map((_, i) => <Cell key={i} fill={COLORS[i % COLORS.length]} />)}
                </Pie>
                <Tooltip /><Legend />
              </PieChart>
            </ResponsiveContainer>
          </Card>
        </Col>
        <Col span={12}>
          <Card title={t('byChannel')} size="small">
            <ResponsiveContainer width="100%" height={240}>
              <PieChart>
                <Pie data={channelData} dataKey="value" nameKey="name" outerRadius={90} label>
                  {channelData.map((_, i) => <Cell key={i} fill={COLORS[i % COLORS.length]} />)}
                </Pie>
                <Tooltip /><Legend />
              </PieChart>
            </ResponsiveContainer>
          </Card>
        </Col>
      </Row>
      <Row gutter={16}>
        <Col span={12}>
          <Card title={t('byPriority')} size="small">
            <ResponsiveContainer width="100%" height={240}>
              <BarChart data={priorityData}>
                <XAxis dataKey="name" /><YAxis allowDecimals={false} /><Tooltip />
                <Bar dataKey="value" fill="#4f8cff" />
              </BarChart>
            </ResponsiveContainer>
          </Card>
        </Col>
        <Col span={12}>
          <Card title={t('byCategory')} size="small">
            <ResponsiveContainer width="100%" height={240}>
              <BarChart data={categoryData}>
                <XAxis dataKey="name" /><YAxis allowDecimals={false} /><Tooltip />
                <Bar dataKey="value" fill="#2dd4bf" />
              </BarChart>
            </ResponsiveContainer>
          </Card>
        </Col>
      </Row>
    </Space>
  )
}
