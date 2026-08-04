/** Change Request: tạo CR → AI phân tích tác động → bảng impact → apply từng
 * tài liệu (draft-first). */
import { useCallback, useEffect, useState } from 'react'
import {
  App,
  Button,
  Card,
  Drawer,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Tag,
  Typography,
} from 'antd'
import { PlusOutlined, ThunderboltOutlined } from '@ant-design/icons'
import MarkdownView from './md'
import { get, post, waitJob, fmtTime } from './api'

const SEV_COLOR: Record<string, string> = { low: 'default', medium: 'orange', high: 'red' }
const CR_COLOR: Record<string, string> = {
  open: 'gold',
  analyzed: 'blue',
  applied: 'green',
  closed: 'default',
}

export default function CrPanel({
  projectId,
  features,
  onDocChanged,
}: {
  projectId: number
  features: any[]
  onDocChanged: () => void
}) {
  const { message } = App.useApp()
  const [crs, setCrs] = useState<any[]>([])
  const [createOpen, setCreateOpen] = useState(false)
  const [title, setTitle] = useState('')
  const [desc, setDesc] = useState('')
  const [sev, setSev] = useState('medium')
  const [featKey, setFeatKey] = useState<string>('')
  const [creating, setCreating] = useState(false)
  const [detail, setDetail] = useState<any>(null)
  const [applying, setApplying] = useState<number | null>(null)

  const load = useCallback(async () => {
    try {
      const r = await get(`/projects/${projectId}/crs`)
      setCrs(r.crs ?? [])
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }, [projectId, message])

  useEffect(() => {
    load()
  }, [load])

  const openDetail = async (id: number) => {
    const r = await get(`/crs/${id}`)
    setDetail(r.cr)
  }

  const create = async () => {
    setCreating(true)
    try {
      const r = await post(`/projects/${projectId}/crs`, {
        title,
        description: desc,
        severity: sev,
        feature: featKey,
      })
      const result = await waitJob(r.job_id)
      message.success(`Đã tạo ${result.cr.code} — ${result.cr.impacts.length} tài liệu bị ảnh hưởng`)
      setCreateOpen(false)
      setTitle('')
      setDesc('')
      load()
      setDetail(result.cr)
    } catch (e: any) {
      message.error(String(e.message ?? e), 7)
    } finally {
      setCreating(false)
    }
  }

  const apply = async (impactId: number) => {
    if (!detail) return
    setApplying(impactId)
    try {
      const r = await post(`/crs/${detail.id}/apply`, { impact_id: impactId })
      const result = await waitJob(r.job_id)
      message.success('Đã cập nhật tài liệu theo CR (version mới, trạng thái draft)')
      setDetail(result.cr)
      load()
      onDocChanged()
    } catch (e: any) {
      message.error(String(e.message ?? e), 7)
    } finally {
      setApplying(null)
    }
  }

  const skip = async (impactId: number) => {
    if (!detail) return
    try {
      const r = await post(`/crs/${detail.id}/update`, { skip_impact: impactId })
      setDetail(r.cr)
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  const close = async () => {
    if (!detail) return
    try {
      const r = await post(`/crs/${detail.id}/update`, { close: true })
      setDetail(r.cr)
      load()
      message.success('Đã đóng CR')
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  return (
    <Card
      size="small"
      title="Change Request — một thay đổi, cập nhật đồng bộ tài liệu"
      extra={
        <Button size="small" type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
          Mở CR
        </Button>
      }
    >
      <Table
        size="small"
        rowKey="id"
        pagination={false}
        dataSource={crs}
        onRow={(r: any) => ({ onClick: () => openDetail(r.id), style: { cursor: 'pointer' } })}
        columns={[
          { title: 'Mã', dataIndex: 'code', width: 160, render: (v: string) => <code>{v}</code> },
          { title: 'Tiêu đề', dataIndex: 'title' },
          {
            title: 'Mức',
            dataIndex: 'severity',
            width: 90,
            render: (v: string) => <Tag color={SEV_COLOR[v]}>{v}</Tag>,
          },
          {
            title: 'Trạng thái',
            dataIndex: 'status',
            width: 110,
            render: (v: string) => <Tag color={CR_COLOR[v]}>{v}</Tag>,
          },
          {
            title: 'Impact',
            width: 110,
            render: (_: any, r: any) => (
              <span>
                {r.impacts_pending > 0 ? <Tag color="orange">{r.impacts_pending} treo</Tag> : null}
                {r.impacts}
              </span>
            ),
          },
          { title: 'Lúc', dataIndex: 'created_at', width: 150, render: (v: number) => fmtTime(v) },
        ]}
      />

      <Modal
        open={createOpen}
        onCancel={creating ? undefined : () => setCreateOpen(false)}
        footer={null}
        title="Mở Change Request"
        width={640}
      >
        <Space direction="vertical" style={{ width: '100%' }}>
          <Input placeholder="Tiêu đề thay đổi" value={title} onChange={(e) => setTitle(e.target.value)} />
          <Input.TextArea
            rows={5}
            placeholder="Thay đổi là gì, vì sao? Càng cụ thể AI phân tích tác động càng trúng…"
            value={desc}
            onChange={(e) => setDesc(e.target.value)}
          />
          <Space>
            <Select
              value={sev}
              onChange={setSev}
              style={{ width: 130 }}
              options={['low', 'medium', 'high'].map((s) => ({ value: s, label: s }))}
            />
            <Select
              value={featKey}
              onChange={setFeatKey}
              style={{ width: 260 }}
              options={[
                { value: '', label: 'Toàn dự án' },
                ...features.map((f: any) => ({ value: String(f.id), label: `Tính năng: ${f.name}` })),
              ]}
            />
            <Button type="primary" loading={creating} onClick={create} icon={<ThunderboltOutlined />}>
              Tạo + phân tích tác động
            </Button>
          </Space>
          {creating && (
            <Typography.Text type="secondary">AI đang đọc tài liệu và phân tích tác động…</Typography.Text>
          )}
        </Space>
      </Modal>

      <Drawer
        open={detail != null}
        onClose={() => setDetail(null)}
        width="min(900px, 95vw)"
        title={
          detail && (
            <Space>
              <code>{detail.code}</code>
              <span>{detail.title}</span>
              <Tag color={CR_COLOR[detail.status]}>{detail.status}</Tag>
            </Space>
          )
        }
        extra={detail && detail.status !== 'closed' && <Button size="small" onClick={close}>Đóng CR</Button>}
      >
        {detail && (
          <>
            <Typography.Paragraph type="secondary">{detail.description}</Typography.Paragraph>
            <Card size="small" title="Phân tích tác động" style={{ marginBottom: 12 }}>
              <MarkdownView md={detail.analysis || '_chưa có phân tích_'} />
            </Card>
            <Table
              size="small"
              rowKey="id"
              pagination={false}
              dataSource={detail.impacts ?? []}
              columns={[
                { title: 'Tài liệu', dataIndex: 'doc_title' },
                { title: 'Sửa gì', dataIndex: 'summary' },
                {
                  title: 'Trạng thái',
                  dataIndex: 'status',
                  width: 100,
                  render: (v: string) => (
                    <Tag color={v === 'applied' ? 'green' : v === 'skipped' ? 'default' : 'orange'}>{v}</Tag>
                  ),
                },
                {
                  title: '',
                  width: 170,
                  render: (_: any, r: any) =>
                    r.status === 'pending' && (
                      <Space size={4}>
                        <Button size="small" type="primary" loading={applying === r.id} onClick={() => apply(r.id)}>
                          Áp dụng
                        </Button>
                        <Button size="small" onClick={() => skip(r.id)}>
                          Bỏ qua
                        </Button>
                      </Space>
                    ),
                },
              ]}
            />
          </>
        )}
      </Drawer>
    </Card>
  )
}
