import { useState } from 'react'
import {
  Alert,
  App as AntApp,
  Badge,
  Button,
  Card,
  Collapse,
  Divider,
  Empty,
  Input,
  List,
  Segmented,
  Space,
  Spin,
  Switch,
  Tag,
  theme,
  Tooltip,
  Typography,
} from 'antd'
import {
  ExperimentOutlined,
  QuestionCircleOutlined,
  SearchOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import {
  api,
  type Depth,
  type Evidence,
  type ResearchOutcome,
  type SearchOutcome,
  type SourceInfo,
} from './api'
import Claims from './Claims'
import Report from './Report'
import { healthStatus, kindColor } from './theme'

const { Text } = Typography

type Mode = 'research' | 'search' | 'ask'

function healthReason(s: SourceInfo): string | null {
  return 'reason' in s.health ? s.health.reason : null
}

function Provenance({ e }: { e: Evidence }) {
  return (
    <Space size={[4, 4]} wrap style={{ marginTop: 4 }}>
      {e.domain && (
        <Tag bordered={false} color="default">
          {e.domain}
        </Tag>
      )}
      {e.hits.map((h) => (
        <Tag key={h.source_id} bordered={false} color={kindColor(h.kind)}>
          {h.source_id} · #{h.rank + 1}
        </Tag>
      ))}
      {e.independent_kinds > 1 && (
        <Tag color="success" bordered={false}>
          {e.independent_kinds} loại nguồn độc lập
        </Tag>
      )}
      {e.full_text && <Tag bordered={false}>đã tải toàn văn</Tag>}
      <Text type="secondary" style={{ fontSize: 11.5 }}>
        rrf {e.fused_score.toFixed(4)}
      </Text>
    </Space>
  )
}

export default function SearchPage({
  sources,
  selected,
  onToggle,
  onSourcesChanged,
}: {
  sources: SourceInfo[]
  selected: Set<string>
  onToggle: (id: string) => void
  onSourcesChanged: () => void
}) {
  const { token } = theme.useToken()
  const { message } = AntApp.useApp()

  const [query, setQuery] = useState('')
  const [mode, setMode] = useState<Mode>('research')
  const [rDepth, setRDepth] = useState<Depth>('standard')
  const [fullText, setFullText] = useState(false)
  const [saveWiki, setSaveWiki] = useState(false)
  const [saveKnowledge, setSaveKnowledge] = useState(false)
  const [busy, setBusy] = useState(false)
  const [out, setOut] = useState<SearchOutcome | null>(null)
  const [report, setReport] = useState<ResearchOutcome | null>(null)

  async function run(nextMode: Mode) {
    if (!query.trim() || busy) return
    setBusy(true)
    setMode(nextMode)
    try {
      const picked = selected.size ? [...selected] : undefined
      if (nextMode === 'research') {
        setOut(null)
        setReport(
          await api.research({
            query: query.trim(),
            depth: rDepth,
            sources: picked,
            save_wiki: saveWiki,
            save_knowledge: saveKnowledge,
          }),
        )
      } else {
        setReport(null)
        const body = { query: query.trim(), sources: picked, depth: fullText ? 2 : 1 }
        setOut(nextMode === 'ask' ? await api.ask(body) : await api.search(body))
      }
      onSourcesChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  const runLabel =
    mode === 'research' ? 'Nghiên cứu' : mode === 'ask' ? 'Hỏi & kiểm chứng' : 'Tìm'
  const failed = out?.sources.filter((s) => s.status !== 'ok') ?? []
  const okSources = out?.sources.filter((s) => s.status === 'ok') ?? []

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Segmented<Mode>
          value={mode}
          onChange={setMode}
          block
          options={[
            { label: 'Nghiên cứu', value: 'research', icon: <ExperimentOutlined /> },
            { label: 'Tìm nhanh', value: 'search', icon: <ThunderboltOutlined /> },
            { label: 'Hỏi & kiểm chứng', value: 'ask', icon: <QuestionCircleOutlined /> },
          ]}
        />

        <Space.Compact style={{ width: '100%', marginTop: 12 }}>
          <Input
            size="large"
            allowClear
            prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
            placeholder={mode === 'research' ? 'Bạn muốn nghiên cứu điều gì?' : 'Bạn muốn tìm gì?'}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onPressEnter={() => run(mode)}
          />
          <Button
            size="large"
            type="primary"
            loading={busy}
            disabled={!query.trim()}
            onClick={() => run(mode)}
          >
            {runLabel}
          </Button>
        </Space.Compact>

        {mode === 'research' ? (
          <Space wrap style={{ marginTop: 12 }} align="center">
            <Segmented<Depth>
              value={rDepth}
              onChange={setRDepth}
              options={[
                { label: 'Nhanh', value: 'quick' },
                { label: 'Tiêu chuẩn', value: 'standard' },
                { label: 'Chuyên sâu', value: 'deep' },
              ]}
            />
            <Tooltip title="Lưu báo cáo vào knowledge graph để tái dùng">
              <Space size={4}>
                <Switch size="small" checked={saveKnowledge} onChange={setSaveKnowledge} />
                <Text type="secondary" style={{ fontSize: 12.5 }}>
                  lưu knowledge
                </Text>
              </Space>
            </Tooltip>
            <Tooltip title="Ghi báo cáo vào wiki">
              <Space size={4}>
                <Switch size="small" checked={saveWiki} onChange={setSaveWiki} />
                <Text type="secondary" style={{ fontSize: 12.5 }}>
                  lưu wiki
                </Text>
              </Space>
            </Tooltip>
          </Space>
        ) : (
          <Space style={{ marginTop: 12 }} size={4}>
            <Switch size="small" checked={fullText} onChange={setFullText} />
            <Text type="secondary" style={{ fontSize: 12.5 }}>
              tải toàn văn trang web đầu bảng
            </Text>
          </Space>
        )}

        <Divider style={{ margin: '12px 0' }} />

        <Space size={[6, 6]} wrap>
          {sources.map((s) => (
            <Tooltip key={s.id} title={healthReason(s) ?? 'sẵn sàng'}>
              <Tag.CheckableTag checked={selected.has(s.id)} onChange={() => onToggle(s.id)}>
                <Badge status={healthStatus(s.health.state)} /> {s.label}
              </Tag.CheckableTag>
            </Tooltip>
          ))}
        </Space>
      </Card>

      {busy && (
        <div style={{ textAlign: 'center', padding: 48 }}>
          <Spin
            tip={
              mode === 'research'
                ? 'Đang gom nguồn, kiểm chứng chéo và tổng hợp báo cáo…'
                : 'Đang tìm…'
            }
          >
            <div style={{ height: 60 }} />
          </Spin>
        </div>
      )}

      {!busy && report && <Report out={report} />}

      {!busy && out && (
        <Space direction="vertical" size={16} style={{ width: '100%' }}>
          <Space size={[4, 4]} wrap>
            <Tag color="cyan">{out.evidence.length} bằng chứng</Tag>
            <Tag>gom từ {out.total_before_dedupe} kết quả thô</Tag>
            {out.deepened > 0 && <Tag>{out.deepened} trang tải toàn văn</Tag>}
            <Tag>{out.ms} ms</Tag>
            {out.run_id && <Tag>{out.run_id}</Tag>}
          </Space>

          {out.claims_error && <Alert type="error" showIcon message={out.claims_error} />}

          {out.claims && out.claims.length > 0 && (
            <Card size="small" title="Khẳng định đã kiểm chứng">
              <Claims
                claims={out.claims}
                contradictions={out.contradictions ?? []}
                evidence={out.evidence}
                note={out.confidence_note}
              />
            </Card>
          )}

          {out.claims_note && (
            <Text type="secondary" style={{ fontSize: 12.5 }}>
              {out.claims_note}
            </Text>
          )}

          {out.unknown_sources.length > 0 && (
            <Alert
              type="error"
              showIcon
              message={`Không có nguồn: ${out.unknown_sources.join(', ')}`}
            />
          )}

          {(failed.length > 0 || okSources.length > 0) && (
            <Collapse
              size="small"
              items={[
                ...(failed.length
                  ? [
                      {
                        key: 'failed',
                        label: `Nguồn không trả kết quả (${failed.length})`,
                        children: (
                          <Space direction="vertical" size={6} style={{ width: '100%' }}>
                            {failed.map((s, i) => (
                              <Space key={`${s.source_id}-${i}`}>
                                <Text strong>{s.source_id}</Text>
                                <Tag color={s.status === 'timeout' ? 'warning' : 'error'}>
                                  {s.status}
                                </Tag>
                                <Text type="secondary" style={{ fontSize: 12.5 }}>
                                  {s.error ?? '—'}
                                </Text>
                              </Space>
                            ))}
                          </Space>
                        ),
                      },
                    ]
                  : []),
                {
                  key: 'ok',
                  label: `Nguồn đã chạy (${okSources.length})`,
                  children: (
                    <Space direction="vertical" size={6} style={{ width: '100%' }}>
                      {okSources.map((s, i) => (
                        <Space key={`${s.source_id}-${i}`}>
                          <Text strong>{s.source_id}</Text>
                          <Tag color="success">{s.item_count} kết quả</Tag>
                          <Text type="secondary" style={{ fontSize: 12.5 }}>
                            {s.ms} ms
                            {s.dropped_count > 0 && ` · bỏ bớt ${s.dropped_count}`}
                          </Text>
                        </Space>
                      ))}
                    </Space>
                  ),
                },
              ]}
            />
          )}

          <List
            itemLayout="vertical"
            dataSource={out.evidence}
            rowKey={(e) => e.id}
            locale={{
              emptyText: (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description="Không có bằng chứng. Xem “Nguồn không trả kết quả” trước khi kết luận."
                />
              ),
            }}
            renderItem={(e) => (
              <List.Item style={{ paddingInline: 0 }}>
                {e.url ? (
                  <a href={e.url} target="_blank" rel="noreferrer" style={{ fontWeight: 600 }}>
                    {e.title || e.url}
                  </a>
                ) : (
                  <Text strong>{e.title || '(không có tiêu đề)'}</Text>
                )}
                <div style={{ color: token.colorTextSecondary, margin: '4px 0', fontSize: 13.5 }}>
                  {e.snippet}
                </div>
                <Provenance e={e} />
              </List.Item>
            )}
          />
        </Space>
      )}

      {!busy && !report && !out && (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={
            <Text type="secondary">
              Nhập câu hỏi rồi bấm <b>Nghiên cứu</b> để nhận báo cáo tổng hợp có trích dẫn — hoặc{' '}
              <b>Tìm nhanh</b> để tra cứu liên nguồn.
            </Text>
          }
        />
      )}
    </>
  )
}
