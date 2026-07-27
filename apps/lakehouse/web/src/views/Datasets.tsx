import { useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  App,
  Button,
  Descriptions,
  Drawer,
  Empty,
  Popconfirm,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import { DeleteOutlined, ReloadOutlined, UploadOutlined } from '@ant-design/icons'
import {
  deleteDataset,
  getDataset,
  importFile,
  listDatasets,
  previewDataset,
} from '../api'
import { DataTable } from '../components/DataTable'
import { ResultGrid } from '../components/ResultGrid'
import { errMsg, fileToBase64, fmtBytes, fmtNum, fmtTime } from '../util'
import type { Dataset } from '../types'

export function Datasets() {
  const { message } = App.useApp()
  const qc = useQueryClient()
  const [selected, setSelected] = useState<Dataset | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const datasets = useQuery({
    queryKey: ['datasets'],
    queryFn: () => listDatasets(undefined, 500, 0),
  })

  const importMut = useMutation({
    mutationFn: async (files: FileList) => {
      const results: string[] = []
      for (const f of Array.from(files)) {
        const contentBase64 = await fileToBase64(f)
        try {
          await importFile({ filename: f.name, contentBase64 })
          results.push(`✓ ${f.name}`)
          message.success(`Đã import ${f.name}`)
        } catch (e) {
          message.error(`${f.name}: ${errMsg(e)}`)
        }
      }
      return results
    },
    onSettled: () => qc.invalidateQueries({ queryKey: ['datasets'] }),
  })

  function pick() {
    inputRef.current?.click()
  }

  return (
    <div>
      <Space style={{ marginBottom: 12 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          Datasets
        </Typography.Title>
        <Button
          icon={<UploadOutlined />}
          type="primary"
          loading={importMut.isPending}
          onClick={pick}
        >
          Import
        </Button>
        <Button
          icon={<ReloadOutlined />}
          onClick={() => datasets.refetch()}
          loading={datasets.isFetching}
        >
          Làm mới
        </Button>
        <input
          ref={inputRef}
          type="file"
          multiple
          hidden
          onChange={(e) => {
            if (e.target.files?.length) importMut.mutate(e.target.files)
            e.target.value = ''
          }}
        />
      </Space>

      <DataTable<Dataset>
        rowKey="id"
        loading={datasets.isLoading}
        dataSource={datasets.data?.datasets ?? []}
        onRow={(r) => ({ onClick: () => setSelected(r), style: { cursor: 'pointer' } })}
        columns={[
          { title: 'Namespace', dataIndex: 'namespace', sorter: (a, b) => a.namespace.localeCompare(b.namespace) },
          { title: 'Tên', dataIndex: 'name', sorter: (a, b) => a.name.localeCompare(b.name) },
          {
            title: 'Số dòng',
            dataIndex: 'row_count',
            align: 'right',
            sorter: (a, b) => a.row_count - b.row_count,
            render: (v: number) => fmtNum(v),
          },
          {
            title: 'Dung lượng',
            dataIndex: 'byte_size',
            align: 'right',
            sorter: (a, b) => a.byte_size - b.byte_size,
            render: (v: number) => fmtBytes(v),
          },
          {
            title: 'Owner flow',
            dataIndex: 'owner_flow_id',
            render: (v: string | null) => (v ? <Tag color="blue">{v}</Tag> : '—'),
          },
          { title: 'Cập nhật', dataIndex: 'updated_at', render: (v: string) => fmtTime(v) },
        ]}
      />

      <DatasetDrawer
        ds={selected}
        onClose={() => setSelected(null)}
        onDeleted={() => {
          setSelected(null)
          qc.invalidateQueries({ queryKey: ['datasets'] })
        }}
      />
    </div>
  )
}

function DatasetDrawer({
  ds,
  onClose,
  onDeleted,
}: {
  ds: Dataset | null
  onClose: () => void
  onDeleted: () => void
}) {
  const { message } = App.useApp()
  const open = !!ds
  const detail = useQuery({
    queryKey: ['dataset', ds?.namespace, ds?.name],
    queryFn: () => getDataset(ds!.namespace, ds!.name),
    enabled: open,
  })
  const preview = useQuery({
    queryKey: ['preview', ds?.namespace, ds?.name],
    queryFn: () => previewDataset(ds!.namespace, ds!.name, 100),
    enabled: open,
  })

  const del = useMutation({
    mutationFn: () => deleteDataset(ds!.namespace, ds!.name),
    onSuccess: () => {
      message.success('Đã xoá dataset')
      onDeleted()
    },
    onError: (e) => message.error(errMsg(e)),
  })

  const schemaFields = extractFields(detail.data?.schema)

  return (
    <Drawer
      title={ds ? `${ds.namespace}.${ds.name}` : ''}
      width={720}
      open={open}
      onClose={onClose}
      extra={
        <Popconfirm
          title="Xoá dataset này?"
          description="Không thể hoàn tác. Flow chủ đang chạy sẽ chặn (409)."
          okButtonProps={{ danger: true, loading: del.isPending }}
          onConfirm={() => del.mutate()}
        >
          <Button danger icon={<DeleteOutlined />}>
            Xoá
          </Button>
        </Popconfirm>
      }
    >
      {ds && (
        <Descriptions size="small" column={2} bordered style={{ marginBottom: 16 }}>
          <Descriptions.Item label="Định dạng">{ds.format}</Descriptions.Item>
          <Descriptions.Item label="Layer">{ds.layer ?? '—'}</Descriptions.Item>
          <Descriptions.Item label="Số dòng">{fmtNum(ds.row_count)}</Descriptions.Item>
          <Descriptions.Item label="Dung lượng">{fmtBytes(ds.byte_size)}</Descriptions.Item>
          <Descriptions.Item label="File active">
            {detail.data?.files.active ?? '—'}
          </Descriptions.Item>
          <Descriptions.Item label="Partition">
            {ds.partition_cols ?? '—'}
          </Descriptions.Item>
        </Descriptions>
      )}

      <Tabs
        items={[
          {
            key: 'schema',
            label: `Schema (${schemaFields.length})`,
            children: schemaFields.length ? (
              <Table
                size="small"
                rowKey="name"
                pagination={false}
                dataSource={schemaFields}
                columns={[
                  { title: 'Cột', dataIndex: 'name' },
                  { title: 'Kiểu', dataIndex: 'type' },
                  {
                    title: 'Nullable',
                    dataIndex: 'nullable',
                    render: (v: boolean) => (v ? 'có' : 'không'),
                  },
                ]}
              />
            ) : (
              <Empty description="Chưa có schema" />
            ),
          },
          {
            key: 'preview',
            label: 'Xem trước',
            children: preview.isLoading ? (
              <Empty description="Đang tải…" />
            ) : (
              <ResultGrid
                columns={preview.data?.columns ?? []}
                rows={preview.data?.rows ?? []}
              />
            ),
          },
          {
            key: 'versions',
            label: `Phiên bản schema (${detail.data?.schema_versions.length ?? 0})`,
            children: (
              <Table
                size="small"
                rowKey="version"
                pagination={false}
                dataSource={detail.data?.schema_versions ?? []}
                locale={{ emptyText: 'Chưa có lịch sử' }}
                columns={[
                  { title: 'Version', dataIndex: 'version' },
                  { title: 'Thay đổi', dataIndex: 'change', render: (v: string | null) => v ?? '—' },
                  { title: 'Thời điểm', dataIndex: 'created_at', render: (v: string) => fmtTime(v) },
                ]}
              />
            ),
          },
        ]}
      />
    </Drawer>
  )
}

interface FieldRow {
  name: string
  type: string
  nullable: boolean
}

// Arrow-schema JSON có nhiều biến thể; đọc best-effort mảng `fields`.
function extractFields(schema: unknown): FieldRow[] {
  if (!schema || typeof schema !== 'object') return []
  const fields = (schema as { fields?: unknown }).fields
  if (!Array.isArray(fields)) return []
  return fields.map((f) => {
    const o = f as Record<string, unknown>
    const t = o.data_type ?? o.type ?? o.dataType
    return {
      name: String(o.name ?? '?'),
      type: typeof t === 'object' ? JSON.stringify(t) : String(t ?? '?'),
      nullable: Boolean(o.nullable),
    }
  })
}
