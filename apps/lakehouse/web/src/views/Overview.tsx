import { useQuery } from '@tanstack/react-query'
import { Card, Col, List, Row, Statistic, Tag, Typography } from 'antd'
import {
  DatabaseOutlined,
  HddOutlined,
  SyncOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import { getStatus, listFlows, listRuns } from '../api'
import { DataTable } from '../components/DataTable'
import { fmtBytes, fmtNum, fmtTime, statusColor } from '../util'
import type { FlowView, Run } from '../types'

export function Overview({ onOpenFlows }: { onOpenFlows: () => void }) {
  const status = useQuery({ queryKey: ['status'], queryFn: getStatus, refetchInterval: 30000 })
  const runs = useQuery({
    queryKey: ['runs', 'recent'],
    queryFn: () => listRuns({ limit: 8 }),
    refetchInterval: 30000,
  })
  const flows = useQuery({ queryKey: ['flows'], queryFn: listFlows })

  const s = status.data

  return (
    <div>
      <Typography.Title level={4}>Tổng quan</Typography.Title>
      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Datasets"
              value={s?.datasets ?? 0}
              prefix={<DatabaseOutlined />}
              loading={status.isLoading}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Dung lượng"
              value={fmtBytes(s?.total_bytes ?? 0)}
              prefix={<HddOutlined />}
              loading={status.isLoading}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Runs 24h"
              value={s?.runs_24h ?? 0}
              prefix={<SyncOutlined />}
              loading={status.isLoading}
            />
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card>
            <Statistic
              title="Runs đang chạy"
              value={s?.runs_active ?? 0}
              prefix={<ThunderboltOutlined />}
              valueStyle={{ color: (s?.runs_active ?? 0) > 0 ? '#1677ff' : undefined }}
              loading={status.isLoading}
            />
          </Card>
        </Col>
      </Row>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col xs={24} lg={14}>
          <Card title="Runs gần nhất" size="small">
            <DataTable<Run>
              rowKey="id"
              loading={runs.isLoading}
              dataSource={runs.data?.runs ?? []}
              pagination={false}
              columns={[
                {
                  title: 'Run',
                  dataIndex: 'id',
                  render: (v: string) => <code>{v.slice(0, 8)}</code>,
                },
                { title: 'Flow', dataIndex: 'flow_id', ellipsis: true },
                {
                  title: 'Trạng thái',
                  dataIndex: 'status',
                  render: (v: string) => <Tag color={statusColor(v)}>{v}</Tag>,
                },
                { title: 'Trigger', dataIndex: 'trigger' },
                {
                  title: 'Cập nhật',
                  dataIndex: 'updated_at',
                  render: (v: string) => fmtTime(v),
                },
              ]}
            />
          </Card>
        </Col>
        <Col xs={24} lg={10}>
          <Card
            title="Flows"
            size="small"
            extra={
              <a onClick={onOpenFlows}>Quản lý</a>
            }
          >
            <List<FlowView>
              loading={flows.isLoading}
              dataSource={flows.data?.flows ?? []}
              locale={{ emptyText: 'Chưa có flow nào' }}
              renderItem={(f) => (
                <List.Item>
                  <List.Item.Meta
                    title={<span>{f.name || f.id}</span>}
                    description={`${f.dag?.length ?? 0} bước · v${f.def_version}`}
                  />
                  <Tag color={f.enabled ? 'green' : 'default'}>
                    {f.enabled ? 'Bật' : 'Tắt'}
                  </Tag>
                </List.Item>
              )}
            />
          </Card>
        </Col>
      </Row>
      {s?.version && (
        <Typography.Text type="secondary" style={{ display: 'block', marginTop: 16 }}>
          Lakehouse v{s.version} · {fmtNum(s.total_rows)} dòng tổng
        </Typography.Text>
      )}
    </div>
  )
}
