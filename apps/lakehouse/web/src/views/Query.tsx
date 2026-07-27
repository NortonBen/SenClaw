import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  App,
  Button,
  Card,
  Col,
  Collapse,
  Input,
  Modal,
  Row,
  Space,
  Tag,
  Typography,
} from 'antd'
import { CaretRightOutlined, InfoCircleOutlined } from '@ant-design/icons'
import { explainQuery, getDataset, listDatasets, runQuery } from '../api'
import { ResultGrid } from '../components/ResultGrid'
import { errMsg, fmtNum } from '../util'
import type { Dataset, Page } from '../types'

const PAGE = 100

export function Query() {
  const { message } = App.useApp()
  const [sql, setSql] = useState('SELECT 1')
  const [offset, setOffset] = useState(0)
  const [page, setPage] = useState<Page | null>(null)
  const [running, setRunning] = useState(false)
  const [plan, setPlan] = useState<string | null>(null)

  const datasets = useQuery({
    queryKey: ['datasets'],
    queryFn: () => listDatasets(undefined, 500, 0),
  })

  async function run(off: number) {
    setRunning(true)
    try {
      const p = await runQuery(sql, PAGE, off)
      setPage(p)
      setOffset(off)
    } catch (e) {
      message.error(errMsg(e))
    } finally {
      setRunning(false)
    }
  }

  async function explain() {
    try {
      const r = await explainQuery(sql)
      setPlan(typeof r.plan === 'string' ? r.plan : JSON.stringify(r.plan, null, 2))
    } catch (e) {
      message.error(errMsg(e))
    }
  }

  function insert(text: string) {
    setSql((s) => `${s} ${text}`.trim())
  }

  return (
    <div>
      <Typography.Title level={4}>Truy vấn SQL</Typography.Title>
      <Row gutter={16}>
        <Col xs={24} md={7} lg={6}>
          <Card size="small" title="Datasets" loading={datasets.isLoading} styles={{ body: { maxHeight: 520, overflow: 'auto' } }}>
            <DatasetColumns datasets={datasets.data?.datasets ?? []} onInsert={insert} />
          </Card>
        </Col>
        <Col xs={24} md={17} lg={18}>
          <Input.TextArea
            value={sql}
            onChange={(e) => setSql(e.target.value)}
            rows={6}
            style={{ fontFamily: 'monospace' }}
            placeholder='SELECT * FROM "raw"."orders" LIMIT 100'
          />
          <Space style={{ marginTop: 12 }}>
            <Button
              type="primary"
              icon={<CaretRightOutlined />}
              loading={running}
              onClick={() => run(0)}
            >
              Chạy
            </Button>
            <Button icon={<InfoCircleOutlined />} onClick={explain}>
              Explain
            </Button>
            {page && (
              <Typography.Text type="secondary">
                {page.returned} dòng
                {page.total_estimate != null && ` · ~${fmtNum(page.total_estimate)} ước tính`}
              </Typography.Text>
            )}
          </Space>

          {page && (
            <div style={{ marginTop: 16 }}>
              <ResultGrid columns={page.columns} rows={page.rows} pageSize={PAGE} />
              <Space style={{ marginTop: 12 }}>
                <Button disabled={offset === 0 || running} onClick={() => run(Math.max(0, offset - PAGE))}>
                  ← Trang trước
                </Button>
                <Typography.Text type="secondary">offset {offset}</Typography.Text>
                <Button disabled={!page.has_more || running} onClick={() => run(offset + PAGE)}>
                  Trang sau →
                </Button>
              </Space>
            </div>
          )}
        </Col>
      </Row>

      <Modal
        open={!!plan}
        title="Query plan"
        footer={null}
        width={720}
        onCancel={() => setPlan(null)}
      >
        <pre style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{plan}</pre>
      </Modal>
    </div>
  )
}

function DatasetColumns({
  datasets,
  onInsert,
}: {
  datasets: Dataset[]
  onInsert: (text: string) => void
}) {
  if (!datasets.length) return <Typography.Text type="secondary">Chưa có dataset</Typography.Text>
  return (
    <Collapse
      size="small"
      accordion
      items={datasets.map((d) => ({
        key: String(d.id),
        label: (
          <a
            onClick={(e) => {
              e.stopPropagation()
              onInsert(`"${d.namespace}"."${d.name}"`)
            }}
          >
            {d.namespace}.{d.name}
          </a>
        ),
        children: <ColumnList ns={d.namespace} name={d.name} onInsert={onInsert} />,
      }))}
    />
  )
}

function ColumnList({
  ns,
  name,
  onInsert,
}: {
  ns: string
  name: string
  onInsert: (text: string) => void
}) {
  const detail = useQuery({
    queryKey: ['dataset', ns, name],
    queryFn: () => getDataset(ns, name),
  })
  const fields = extractFieldNames(detail.data?.schema)
  if (detail.isLoading) return <Typography.Text type="secondary">Đang tải…</Typography.Text>
  if (!fields.length) return <Typography.Text type="secondary">Không rõ cột</Typography.Text>
  return (
    <Space size={[4, 4]} wrap>
      {fields.map((c) => (
        <Tag key={c} style={{ cursor: 'pointer' }} onClick={() => onInsert(`"${c}"`)}>
          {c}
        </Tag>
      ))}
    </Space>
  )
}

function extractFieldNames(schema: unknown): string[] {
  if (!schema || typeof schema !== 'object') return []
  const fields = (schema as { fields?: unknown }).fields
  if (!Array.isArray(fields)) return []
  return fields.map((f) => String((f as { name?: unknown }).name ?? '?'))
}
