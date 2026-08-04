import { useCallback, useEffect, useState } from 'react'
import { Button, Card, Empty, Flex, List, message, Progress, Select, Space, Spin, Tag, Typography } from 'antd'
import { RobotOutlined } from '@ant-design/icons'
import { api, fmtTime, type Trend } from './api'
import { JobRunningCard } from './jobs'
import { Md } from './md'

const { Text, Link } = Typography

export default function TrendsTab() {
  const [hours, setHours] = useState(48)
  const [trends, setTrends] = useState<Trend[] | null>(null)
  const [articleCount, setArticleCount] = useState(0)
  const [ai, setAi] = useState<{ text: string; model: string; truncated: boolean } | null>(null)
  const [aiBusy, setAiBusy] = useState(false)
  const [aiSec, setAiSec] = useState(0)

  const load = useCallback(() => {
    setTrends(null)
    api.trends(hours).then((r) => {
      setTrends(r.trends)
      setArticleCount(r.article_count)
    })
  }, [hours])

  useEffect(() => {
    load()
  }, [load])

  const analyze = async () => {
    setAiBusy(true)
    setAiSec(0)
    const started = Date.now()
    const ticker = window.setInterval(() => setAiSec(Math.round((Date.now() - started) / 1000)), 1000)
    try {
      const r = await api.analyzeTrends(hours)
      if (r.error) message.error(String(r.error))
      else setAi({ text: r.analysis, model: r.model, truncated: !!r.truncated })
    } finally {
      window.clearInterval(ticker)
      setAiBusy(false)
    }
  }

  if (!trends) return <Spin style={{ marginTop: 40 }} />
  const maxScore = Math.max(1, ...trends.map((t) => t.score))

  return (
    <Space direction="vertical" size={14} style={{ width: '100%' }}>
      <Flex gap={8} align="center" wrap>
        <Select
          value={hours}
          onChange={setHours}
          style={{ width: 180 }}
          options={[
            { value: 12, label: '12 giờ so với 12 giờ trước' },
            { value: 24, label: '24 giờ so với 24 giờ trước' },
            { value: 48, label: '48 giờ so với 48 giờ trước' },
            { value: 96, label: '4 ngày so với 4 ngày trước' },
          ]}
        />
        <Text type="secondary">{articleCount} bài trong cửa sổ hiện tại</Text>
        <Button type="primary" icon={<RobotOutlined />} loading={aiBusy} onClick={analyze} disabled={trends.length === 0}>
          AI nhận định xu hướng
        </Button>
      </Flex>

      {aiBusy ? (
        <JobRunningCard label={`Đang phân tích xu hướng ${hours}h`} elapsed={aiSec} />
      ) : (
        ai && (
          <Card size="small" title={`Nhận định AI${ai.model ? ` · ${ai.model}` : ''}`}>
            <Md text={ai.text} truncated={ai.truncated} />
          </Card>
        )
      )}

      {trends.length === 0 ? (
        <Empty description="Chưa phát hiện cụm từ nào tăng nhiệt — cần thêm bài (bấm Thu thập ngay, đợi vài chu kỳ quét)" />
      ) : (
        <List
          grid={{ gutter: 12, column: 2 }}
          dataSource={trends}
          renderItem={(t) => (
            <List.Item>
              <Card size="small">
                <Space direction="vertical" size={6} style={{ width: '100%' }}>
                  <Flex justify="space-between" align="center">
                    <Text strong style={{ fontSize: 15 }}>{t.phrase}</Text>
                    <Space size={4}>
                      <Tag color="geekblue">{t.count} bài</Tag>
                      <Text type="secondary" style={{ fontSize: 12 }}>kỳ trước: {t.prev_count}</Text>
                    </Space>
                  </Flex>
                  <Progress
                    percent={Math.round((t.score / maxScore) * 100)}
                    showInfo={false}
                    strokeColor="#0ea5e9"
                    size="small"
                  />
                  {t.samples.map((s) => (
                    <div key={s.id}>
                      <Link href={s.url} target="_blank" style={{ fontSize: 13 }} ellipsis>
                        {s.title}
                      </Link>
                      <Text type="secondary" style={{ fontSize: 12 }}> — {s.source} · {fmtTime(s.published_at)}</Text>
                    </div>
                  ))}
                </Space>
              </Card>
            </List.Item>
          )}
        />
      )}
    </Space>
  )
}
