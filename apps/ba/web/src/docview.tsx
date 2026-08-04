/** Drawer xem/sửa một tài liệu: render markdown+mermaid hoặc iframe cho
 * wireframe/prototype HTML; đổi lifecycle status; sửa tay; lịch sử version. */
import { useEffect, useState } from 'react'
import {
  App,
  Button,
  Drawer,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Input,
} from 'antd'
import { DeleteOutlined, ReloadOutlined } from '@ant-design/icons'
import MarkdownView from './md'
import { del, fmtTime, get, patch, STATUS_COLOR, STATUS_LABEL, type Doc } from './api'

export default function DocViewer({
  docId,
  onClose,
  onChanged,
  onRegenerate,
}: {
  docId: number | null
  onClose: () => void
  onChanged: () => void
  onRegenerate?: (doc: Doc) => void
}) {
  const { message } = App.useApp()
  const [doc, setDoc] = useState<Doc | null>(null)
  const [draft, setDraft] = useState('')
  const [versions, setVersions] = useState<any[]>([])
  const [verView, setVerView] = useState<{ version: number; content: string } | null>(null)
  const [saving, setSaving] = useState(false)

  const load = async () => {
    if (docId == null) return
    try {
      const r = await get(`/docs/${docId}`)
      setDoc(r.document)
      setDraft(r.document.content ?? '')
      const v = await get(`/docs/${docId}/versions`)
      setVersions(v.versions ?? [])
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }
  useEffect(() => {
    setDoc(null)
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [docId])

  const save = async () => {
    if (!doc) return
    setSaving(true)
    try {
      await patch(`/docs/${doc.id}`, { content: draft })
      message.success('Đã lưu (version mới)')
      await load()
      onChanged()
    } catch (e: any) {
      message.error(String(e.message ?? e))
    } finally {
      setSaving(false)
    }
  }

  const setStatus = async (status: string) => {
    if (!doc) return
    try {
      await patch(`/docs/${doc.id}`, { status })
      message.success(`→ ${STATUS_LABEL[status] ?? status}`)
      await load()
      onChanged()
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  const remove = async () => {
    if (!doc) return
    try {
      await del(`/docs/${doc.id}`)
      message.success('Đã xoá tài liệu')
      onClose()
      onChanged()
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  return (
    <Drawer
      open={docId != null}
      onClose={onClose}
      width="min(1100px, 96vw)"
      title={
        doc ? (
          <Space wrap>
            <span>{doc.title}</span>
            <Tag>v{doc.version}</Tag>
            <Tag color={doc.source === 'ai' ? 'geekblue' : 'default'}>{doc.source}</Tag>
            {doc.doc_type === 'reverse_doc' && <Tag color="orange">tái lập — soi cột Tin cậy</Tag>}
          </Space>
        ) : (
          '…'
        )
      }
      extra={
        doc && (
          <Space>
            <Select
              size="small"
              value={doc.status}
              style={{ width: 130 }}
              onChange={setStatus}
              options={Object.keys(STATUS_LABEL).map((s) => ({
                value: s,
                label: <Tag color={STATUS_COLOR[s]}>{STATUS_LABEL[s]}</Tag>,
              }))}
            />
            {onRegenerate && (
              <Button size="small" icon={<ReloadOutlined />} onClick={() => onRegenerate(doc)}>
                Sinh lại
              </Button>
            )}
            <Popconfirm title="Xoá tài liệu này (mất cả version cũ)?" onConfirm={remove}>
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        )
      }
    >
      {doc && (
        <Tabs
          items={[
            {
              key: 'view',
              label: 'Xem',
              children:
                doc.format === 'html' ? (
                  <iframe
                    className="doc-iframe"
                    sandbox="allow-scripts"
                    srcDoc={doc.content ?? ''}
                    title={doc.title}
                  />
                ) : (
                  <MarkdownView md={doc.content ?? ''} />
                ),
            },
            {
              key: 'edit',
              label: 'Sửa',
              children: (
                <div>
                  <Input.TextArea
                    rows={24}
                    value={draft}
                    onChange={(e) => setDraft(e.target.value)}
                    style={{ fontFamily: 'ui-monospace, Menlo, monospace', fontSize: 12.5 }}
                  />
                  <Button
                    type="primary"
                    onClick={save}
                    loading={saving}
                    disabled={draft === doc.content}
                    style={{ marginTop: 10 }}
                  >
                    Lưu (tạo version mới)
                  </Button>
                </div>
              ),
            },
            {
              key: 'versions',
              label: `Version (${versions.length})`,
              children: (
                <Table
                  size="small"
                  rowKey="version"
                  pagination={false}
                  dataSource={versions}
                  columns={[
                    { title: 'Version', dataIndex: 'version', width: 90 },
                    { title: 'Ghi chú', dataIndex: 'note' },
                    {
                      title: 'Lúc',
                      dataIndex: 'created_at',
                      width: 150,
                      render: (v: number) => fmtTime(v),
                    },
                    {
                      title: '',
                      width: 90,
                      render: (_: any, r: any) => (
                        <Button
                          size="small"
                          onClick={async () => {
                            const c = await get(`/docs/${doc.id}/versions/${r.version}`)
                            setVerView({ version: r.version, content: c.content })
                          }}
                        >
                          Xem
                        </Button>
                      ),
                    },
                  ]}
                />
              ),
            },
          ]}
        />
      )}
      <Modal
        open={verView != null}
        onCancel={() => setVerView(null)}
        footer={null}
        width={900}
        title={`Version ${verView?.version}`}
      >
        {verView &&
          (doc?.format === 'html' ? (
            <iframe className="doc-iframe" sandbox="allow-scripts" srcDoc={verView.content} title="v" />
          ) : (
            <MarkdownView md={verView.content} />
          ))}
      </Modal>
    </Drawer>
  )
}
