import { useState } from 'react'
import { Alert, Card, Collapse, Empty, List, Space, Tag, Typography } from 'antd'
import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  ExperimentOutlined,
  FileTextOutlined,
  SafetyCertificateOutlined,
} from '@ant-design/icons'
import type { ResearchOutcome } from './api'
import Claims from './Claims'
import EvidenceModal from './EvidenceModal'
import Md from './Md'

const { Text } = Typography

function depthLabel(d: string): string {
  return d === 'deep' ? 'chuyên sâu' : d === 'quick' ? 'nhanh' : 'tiêu chuẩn'
}

/** The checkpoint verdict, shown ABOVE the report. A report that failed the
 *  review must never be read before the reason it failed. */
function Checkpoint({ out }: { out: ResearchOutcome }) {
  const status = out.status ?? 'ok'
  const r = out.review

  if (status === 'insufficient') {
    return (
      <Alert
        type="error"
        showIcon
        icon={<SafetyCertificateOutlined />}
        style={{ marginBottom: 12 }}
        message="Kiểm định: không đủ dữ liệu đúng chủ đề — chưa trả lời được câu hỏi"
        description={
          <Text style={{ fontSize: 13 }}>
            Không tư liệu nào thu được nói về chủ đề bạn hỏi, nên hệ thống KHÔNG tổng hợp báo cáo
            (tránh trả về một báo cáo đúng định dạng nhưng lạc đề). Xem “Tư liệu đã loại” và “Nguồn
            không trả kết quả” bên dưới.
          </Text>
        }
      />
    )
  }

  if (status === 'off_topic') {
    return (
      <Alert
        type="error"
        showIcon
        icon={<SafetyCertificateOutlined />}
        style={{ marginBottom: 12 }}
        message={`Kiểm định: báo cáo KHÔNG trả lời được câu hỏi${
          r?.used_llm ? ` (${r.score}/100)` : ''
        }`}
        description={
          <Space direction="vertical" size={4} style={{ width: '100%' }}>
            {r?.issues.map((i, k) => (
              <Text key={k} style={{ fontSize: 13 }}>
                · {i}
              </Text>
            ))}
            {!!r?.missing.length && (
              <Text type="secondary" style={{ fontSize: 12.5 }}>
                Nên tìm thêm: {r.missing.join(' · ')}
              </Text>
            )}
          </Space>
        }
      />
    )
  }

  if (!r?.used_llm) return null
  // On topic but thin is its own state: a green tick over a 10/100 report would
  // be the same lie in a friendlier colour.
  const thin = r.score < 50
  return (
    <Alert
      type={thin ? 'warning' : 'success'}
      showIcon
      icon={<SafetyCertificateOutlined />}
      style={{ marginBottom: 12 }}
      message={
        thin
          ? `Đã kiểm định — đúng chủ đề nhưng chưa đủ dữ liệu để trả lời trọn vẹn (${r.score}/100)`
          : `Đã kiểm định trước khi trả kết quả — báo cáo trả lời đúng câu hỏi (${r.score}/100)`
      }
      description={
        r.issues.length || r.missing.length ? (
          <Space direction="vertical" size={2} style={{ width: '100%' }}>
            {r.issues.map((i, k) => (
              <Text key={k} type="secondary" style={{ fontSize: 12.5 }}>
                · {i}
              </Text>
            ))}
            {!!r.missing.length && (
              <Text type="secondary" style={{ fontSize: 12.5 }}>
                Nên tìm thêm: {r.missing.join(' · ')}
              </Text>
            )}
          </Space>
        ) : undefined
      }
    />
  )
}

export default function Report({ out }: { out: ResearchOutcome }) {
  const failed = out.sources.filter((s) => s.status !== 'ok')
  const offTopic = out.off_topic ?? []
  const [cite, setCite] = useState<number | null>(null)

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card
        size="small"
        title={
          <Space>
            <FileTextOutlined />
            <span>{out.report_llm ? 'Báo cáo tổng hợp' : 'Báo cáo tự dựng'}</span>
          </Space>
        }
        extra={
          <Space size={4} wrap>
            <Tag icon={<ExperimentOutlined />} color="purple">
              {depthLabel(out.depth)}
            </Tag>
            <Tag>{out.rounds} vòng</Tag>
            <Tag icon={<ClockCircleOutlined />}>{(out.ms / 1000).toFixed(1)}s</Tag>
          </Space>
        }
      >
        <Space size={[4, 4]} wrap style={{ marginBottom: 12 }}>
          <Tag color="blue">{out.claims.length} khẳng định</Tag>
          <Tag color="cyan">{out.evidence.length} bằng chứng</Tag>
          {offTopic.length > 0 && <Tag color="default">{offTopic.length} tư liệu đã loại</Tag>}
          <Tag>{out.sub_queries.length} truy vấn con</Tag>
          {out.saved?.map((s, i) => (
            <Tag key={i} icon={<CheckCircleOutlined />} color="success">
              đã lưu {s.target}
              {s.version ? ` v${s.version}` : ''}
            </Tag>
          ))}
          {out.run_id && <Tag>{out.run_id}</Tag>}
        </Space>

        {out.sub_queries.length > 1 && (
          <div style={{ marginBottom: 12 }}>
            <Text type="secondary" style={{ fontSize: 12.5, marginRight: 6 }}>
              Truy vấn con:
            </Text>
            <Space size={[4, 4]} wrap>
              {out.sub_queries.map((q, i) => (
                <Tag key={i} bordered={false} color={i === 0 ? 'default' : 'geekblue'}>
                  {q}
                </Tag>
              ))}
            </Space>
          </div>
        )}

        <Checkpoint out={out} />

        {out.warnings.length > 0 && (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 12 }}
            message="Ghi chú về độ đầy đủ"
            description={out.warnings.map((w, i) => (
              <div key={i}>{w}</div>
            ))}
          />
        )}

        <Md onCite={setCite}>{out.report_markdown}</Md>
      </Card>

      {out.claims.length > 0 && (
        <Card size="small" title="Khẳng định đã kiểm chứng">
          <Claims
            claims={out.claims}
            contradictions={out.contradictions}
            evidence={out.evidence}
            note={out.confidence_note}
            onCite={setCite}
          />
        </Card>
      )}

      {offTopic.length > 0 && (
        <Collapse
          size="small"
          items={[
            {
              key: 'off',
              label: `Tư liệu đã loại vì không đúng chủ đề (${offTopic.length})`,
              children: (
                <List
                  size="small"
                  dataSource={offTopic}
                  rowKey={(e) => e.id}
                  renderItem={(e) => (
                    <List.Item style={{ paddingInline: 0, display: 'block' }}>
                      {e.url ? (
                        <a href={e.url} target="_blank" rel="noreferrer">
                          {e.title || e.url}
                        </a>
                      ) : (
                        <Text>{e.title || '(không có tiêu đề)'}</Text>
                      )}
                      <div>
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          {e.domain ?? e.hits[0]?.source_id ?? '—'}
                        </Text>
                      </div>
                    </List.Item>
                  )}
                />
              ),
            },
          ]}
        />
      )}

      {failed.length > 0 && (
        <Card size="small" title="Nguồn không trả kết quả">
          <Space direction="vertical" size={6} style={{ width: '100%' }}>
            {failed.map((s, i) => (
              <div key={`${s.source_id}-${i}`}>
                <Space>
                  <Text strong>{s.source_id}</Text>
                  <Tag color={s.status === 'timeout' ? 'warning' : 'error'}>{s.status}</Tag>
                  <Text type="secondary" style={{ fontSize: 12.5 }}>
                    {s.error ?? '—'}
                  </Text>
                </Space>
              </div>
            ))}
          </Space>
        </Card>
      )}

      {out.evidence.length === 0 && out.claims.length === 0 && offTopic.length === 0 && (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={
            <Text type="secondary">
              Không thu được bằng chứng nào. Xem “Nguồn không trả kết quả” trước khi kết luận là
              không có thông tin.
            </Text>
          }
        />
      )}

      <EvidenceModal
        evidence={out.evidence}
        index={cite}
        onClose={() => setCite(null)}
        onNavigate={setCite}
      />
    </Space>
  )
}
