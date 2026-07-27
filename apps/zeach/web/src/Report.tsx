import { Alert, Card, Empty, Space, Tag, Typography } from 'antd'
import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  ExperimentOutlined,
  FileTextOutlined,
} from '@ant-design/icons'
import type { ResearchOutcome } from './api'
import Claims from './Claims'
import Md from './Md'

const { Text } = Typography

function depthLabel(d: string): string {
  return d === 'deep' ? 'chuyên sâu' : d === 'quick' ? 'nhanh' : 'tiêu chuẩn'
}

export default function Report({ out }: { out: ResearchOutcome }) {
  const failed = out.sources.filter((s) => s.status !== 'ok')

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

        <Md>{out.report_markdown}</Md>
      </Card>

      {out.claims.length > 0 && (
        <Card size="small" title="Khẳng định đã kiểm chứng">
          <Claims
            claims={out.claims}
            contradictions={out.contradictions}
            evidence={out.evidence}
            note={out.confidence_note}
          />
        </Card>
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

      {out.evidence.length === 0 && out.claims.length === 0 && (
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
    </Space>
  )
}
