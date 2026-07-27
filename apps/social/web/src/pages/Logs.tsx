import { useCallback, useEffect, useState } from 'react'
import { Card, Checkbox, Input, Space, Table, Tabs, Typography } from 'antd'
import {
  ago,
  getActions,
  getPosts,
  getSessions,
  type ActionRow,
  type PostRow,
  type SessionRow,
  type Status,
} from '../api'
import { StatusTag } from '../ui'
import RemotePanel from './RemotePanel'

type Tab = 'actions' | 'posts' | 'sessions'

export default function Logs({ status }: { status: Status | null }) {
  const [tab, setTab] = useState<Tab>('actions')
  const [q, setQ] = useState('')
  const [errorsOnly, setErrorsOnly] = useState(false)
  const [actions, setActions] = useState<ActionRow[]>([])
  const [posts, setPosts] = useState<PostRow[]>([])
  const [sessions, setSessions] = useState<SessionRow[]>([])

  const load = useCallback(async () => {
    const [a, p, s] = await Promise.all([getActions(), getPosts(), getSessions()])
    setActions(a?.actions ?? [])
    setPosts(p?.posts ?? [])
    setSessions(s?.sessions ?? [])
  }, [])

  useEffect(() => {
    load()
    const t = setInterval(load, 6000)
    return () => clearInterval(t)
  }, [load])

  const hit = (...f: (string | undefined)[]) =>
    !q || f.some((x) => (x ?? '').toLowerCase().includes(q.toLowerCase()))

  const time = (v: string) => <span className="mono">{ago(v)}</span>
  const st = (v: string) => <StatusTag value={v} />

  const isErr = (s: string) => s === 'error' || s === 'blocked' || s === 'unsupported'
  const aRows = actions
    .filter((x) => hit(x.platform, x.action, x.status, x.detail))
    .filter((x) => !errorsOnly || isErr(x.status))
  const pRows = posts.filter((x) => hit(x.platform, x.status, x.detail, x.ref_id))
  const sRows = sessions.filter((x) => hit(x.platform, x.event))

  const shown = tab === 'actions' ? aRows.length : tab === 'posts' ? pRows.length : sRows.length

  return (
    <>
      <RemotePanel status={status} actions={actions} />
      <Card size="small">
        <Space style={{ width: '100%', justifyContent: 'space-between' }} align="start" wrap>
        <Tabs
          activeKey={tab}
          onChange={(k) => setTab(k as Tab)}
          items={[
            { key: 'actions', label: `Audit hành động API (${actions.length})` },
            { key: 'posts', label: `Lượt đăng (${posts.length})` },
            { key: 'sessions', label: `Phiên đăng nhập (${sessions.length})` },
          ]}
        />
        <Space>
          {tab === 'actions' && (
            <Checkbox checked={errorsOnly} onChange={(e) => setErrorsOnly(e.target.checked)}>
              Chỉ lỗi
            </Checkbox>
          )}
          <Input.Search
            allowClear
            placeholder="lọc…"
            style={{ width: 220 }}
            value={q}
            onChange={(e) => setQ(e.target.value)}
          />
        </Space>
      </Space>

      {tab === 'actions' && (
        <Table<ActionRow>
          size="small"
          rowKey={(r) => r.created_at + r.action + r.platform}
          dataSource={aRows}
          rowClassName={(r) => (isErr(r.status) ? 'row-error' : '')}
          pagination={{ pageSize: 25, hideOnSinglePage: true }}
          locale={{ emptyText: 'Chưa có hành động API nào được ghi.' }}
          columns={[
            { title: 'Nền tảng', dataIndex: 'platform', width: 110 },
            { title: 'Hành động', dataIndex: 'action', width: 110 },
            { title: 'Kết quả', dataIndex: 'status', width: 120, render: st },
            { title: 'Chi tiết', dataIndex: 'detail' },
            { title: 'Lúc', dataIndex: 'created_at', width: 90, render: time },
          ]}
        />
      )}
      {tab === 'posts' && (
        <Table<PostRow>
          size="small"
          rowKey={(r) => r.created_at + r.platform}
          dataSource={pRows}
          pagination={{ pageSize: 25, hideOnSinglePage: true }}
          locale={{ emptyText: 'Chưa có lượt đăng nào.' }}
          columns={[
            { title: 'Nền tảng', dataIndex: 'platform', width: 110 },
            { title: 'Loại', dataIndex: 'kind', width: 80 },
            { title: 'Kết quả', dataIndex: 'status', width: 100, render: st },
            {
              title: 'Ref',
              dataIndex: 'ref_id',
              width: 110,
              render: (v: string) => <span className="mono">{v || '—'}</span>,
            },
            { title: 'Chi tiết', dataIndex: 'detail' },
            { title: 'Lúc', dataIndex: 'created_at', width: 90, render: time },
          ]}
        />
      )}
      {tab === 'sessions' && (
        <Table<SessionRow>
          size="small"
          rowKey={(r) => r.created_at + r.platform + r.event}
          dataSource={sRows}
          pagination={{ pageSize: 25, hideOnSinglePage: true }}
          locale={{ emptyText: 'Chưa ghi nhận phiên đăng nhập nào (extension báo online/offline).' }}
          columns={[
            { title: 'Nền tảng', dataIndex: 'platform', width: 140 },
            { title: 'Sự kiện', dataIndex: 'event', width: 120, render: st },
            { title: 'Lúc', dataIndex: 'created_at', render: time },
          ]}
        />
      )}

        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          Hiển thị {shown} dòng{q ? ` (lọc "${q}")` : ''}.
        </Typography.Text>
      </Card>
    </>
  )
}
