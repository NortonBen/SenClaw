import { useEffect, useState } from 'react'
import { Alert, Card, Col, Row, Statistic, Table, Typography } from 'antd'
import { ago, getActions, getPosts, getSessions, type ActionRow, type PostRow, type Status } from '../api'
import { StatusTag } from '../ui'

export default function Dashboard({ status }: { status: Status | null }) {
  const [actions, setActions] = useState<ActionRow[]>([])
  const [posts, setPosts] = useState<PostRow[]>([])
  const [sessions, setSessions] = useState(0)

  useEffect(() => {
    const load = async () => {
      const [a, p, s] = await Promise.all([getActions(), getPosts(), getSessions()])
      setActions(a?.actions ?? [])
      setPosts(p?.posts ?? [])
      setSessions(s?.sessions?.length ?? 0)
    }
    load()
    const t = setInterval(load, 6000)
    return () => clearInterval(t)
  }, [])

  const kpis: { title: string; value: number }[] = [
    { title: 'Tài khoản', value: status?.accounts ?? 0 },
    { title: 'Nháp chờ duyệt', value: status?.drafts_pending ?? 0 },
    { title: 'Lượt đăng', value: status?.posts_logged ?? 0 },
    { title: 'Hành động API', value: status?.actions_logged ?? 0 },
    { title: 'Phiên đang mở', value: status?.extension_hosts_ready?.length ?? 0 },
    { title: 'Sự kiện phiên', value: sessions },
  ]

  return (
    <>
      <Row gutter={[12, 12]}>
        {kpis.map((k) => (
          <Col key={k.title} xs={12} sm={8} md={4}>
            <Card size="small">
              <Statistic title={k.title} value={k.value} />
            </Card>
          </Col>
        ))}
      </Row>

      {status && !status.extension_connected && (
        <Alert
          style={{ marginTop: 14 }}
          type="warning"
          showIcon
          message="Extension chưa kết nối"
          description={
            <>
              Tìm kiếm / duyệt / nhắn tin cần extension. Cài thư mục <code>apps/social/extension</code> vào
              Chrome (Load unpacked) rồi đăng nhập nền tảng.
            </>
          }
        />
      )}

      <Row gutter={[14, 14]} style={{ marginTop: 14 }}>
        <Col xs={24} lg={12}>
          <Card size="small" title="Hành động API gần đây">
            <Table<ActionRow>
              size="small"
              rowKey={(r) => r.created_at + r.action}
              dataSource={actions.slice(0, 6)}
              pagination={false}
              locale={{ emptyText: 'Chưa có hành động nào.' }}
              columns={[
                { title: 'Nền tảng', dataIndex: 'platform' },
                { title: 'Hành động', dataIndex: 'action' },
                { title: 'Kết quả', dataIndex: 'status', render: (v: string) => <StatusTag value={v} /> },
                {
                  title: 'Lúc',
                  dataIndex: 'created_at',
                  render: (v: string) => <span className="mono">{ago(v)}</span>,
                },
              ]}
            />
          </Card>
        </Col>
        <Col xs={24} lg={12}>
          <Card size="small" title="Lượt đăng gần đây">
            <Table<PostRow>
              size="small"
              rowKey={(r) => r.created_at + r.platform}
              dataSource={posts.slice(0, 6)}
              pagination={false}
              locale={{ emptyText: 'Chưa có lượt đăng nào.' }}
              columns={[
                { title: 'Nền tảng', dataIndex: 'platform' },
                { title: 'Kết quả', dataIndex: 'status', render: (v: string) => <StatusTag value={v} /> },
                {
                  title: 'Chi tiết',
                  dataIndex: 'detail',
                  render: (v: string) => (
                    <Typography.Text type="secondary" className="clamp2" style={{ fontSize: 12 }}>
                      {v}
                    </Typography.Text>
                  ),
                },
                {
                  title: 'Lúc',
                  dataIndex: 'created_at',
                  render: (v: string) => <span className="mono">{ago(v)}</span>,
                },
              ]}
            />
          </Card>
        </Col>
      </Row>
    </>
  )
}
