// Config form generated from a rule's `config_schema` (JSON Schema subset).
// Adding a rule to the backend must never require touching this file.

import { useEffect, useMemo, useState } from 'react'
import {
  Button,
  Form,
  Input,
  InputNumber,
  Select,
  Space,
  Switch,
  Table,
  Typography,
  theme,
} from 'antd'
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons'
import type { JsonObject, JsonSchema } from '../types'

const { TextArea, Password } = Input

function asObject(v: unknown): JsonObject {
  return v && typeof v === 'object' && !Array.isArray(v) ? (v as JsonObject) : {}
}

function isEmpty(v: unknown): boolean {
  if (v === undefined || v === null) return true
  if (typeof v === 'string') return v.trim() === ''
  if (Array.isArray(v)) return v.length === 0
  return false
}

// ------------------------------------------------------------- key/value

interface KvRow {
  k: string
  v: string
}

function KeyValueField({
  value,
  onChange,
}: {
  value: unknown
  onChange: (next: JsonObject) => void
}) {
  const [rows, setRows] = useState<KvRow[]>(() =>
    Object.entries(asObject(value)).map(([k, v]) => ({
      k,
      v: typeof v === 'string' ? v : JSON.stringify(v),
    })),
  )

  const push = (next: KvRow[]) => {
    setRows(next)
    const out: JsonObject = {}
    for (const r of next) if (r.k.trim() !== '') out[r.k.trim()] = r.v
    onChange(out)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      {rows.map((r, i) => (
        <Space.Compact key={i} style={{ width: '100%' }}>
          <Input
            style={{ width: '38%' }}
            placeholder="khoá"
            value={r.k}
            onChange={(e) => push(rows.map((x, j) => (j === i ? { ...x, k: e.target.value } : x)))}
          />
          <Input
            placeholder="giá trị"
            value={r.v}
            onChange={(e) => push(rows.map((x, j) => (j === i ? { ...x, v: e.target.value } : x)))}
          />
          <Button
            icon={<DeleteOutlined />}
            danger
            onClick={() => push(rows.filter((_, j) => j !== i))}
          />
        </Space.Compact>
      ))}
      <Button
        size="small"
        icon={<PlusOutlined />}
        onClick={() => push([...rows, { k: '', v: '' }])}
        style={{ alignSelf: 'flex-start' }}
      >
        Thêm dòng
      </Button>
    </div>
  )
}

// ----------------------------------------------------------------- table

function cellEditor(
  colSchema: JsonSchema,
  val: unknown,
  set: (v: unknown) => void,
): React.ReactNode {
  if (Array.isArray(colSchema.enum)) {
    return (
      <Select
        size="small"
        style={{ width: '100%' }}
        value={val === undefined || val === null ? undefined : String(val)}
        options={colSchema.enum.map((o) => ({ value: String(o), label: String(o) }))}
        onChange={set}
      />
    )
  }
  if (colSchema.type === 'boolean') {
    return <Switch size="small" checked={Boolean(val)} onChange={set} />
  }
  if (colSchema.type === 'number' || colSchema.type === 'integer') {
    return (
      <InputNumber
        size="small"
        style={{ width: '100%' }}
        value={typeof val === 'number' ? val : undefined}
        onChange={(v) => set(v)}
      />
    )
  }
  return (
    <Input
      size="small"
      placeholder={colSchema.placeholder}
      value={typeof val === 'string' ? val : val === undefined || val === null ? '' : String(val)}
      onChange={(e) => set(e.target.value)}
    />
  )
}

function TableField({
  schema,
  value,
  onChange,
}: {
  schema: JsonSchema
  value: unknown
  onChange: (next: unknown[]) => void
}) {
  const props = schema.items?.properties ?? {}
  const keys = Object.keys(props)
  const rows = Array.isArray(value) ? (value as JsonObject[]) : []

  const setCell = (rowIndex: number, key: string, v: unknown) =>
    onChange(rows.map((r, i) => (i === rowIndex ? { ...asObject(r), [key]: v } : r)))

  // Rows carry no id of their own, so wrap them with their index for `rowKey`.
  interface Wrapped {
    __i: number
    row: JsonObject
  }
  const data: Wrapped[] = rows.map((row, __i) => ({ __i, row: asObject(row) }))

  const columns = [
    ...keys.map((k) => ({
      title: props[k].title ?? k,
      key: k,
      render: (_: unknown, rec: Wrapped) =>
        cellEditor(props[k], rec.row[k], (v) => setCell(rec.__i, k, v)),
    })),
    {
      title: '',
      key: '__act',
      width: 40,
      render: (_: unknown, rec: Wrapped) => (
        <Button
          size="small"
          danger
          type="text"
          icon={<DeleteOutlined />}
          onClick={() => onChange(rows.filter((_, i) => i !== rec.__i))}
        />
      ),
    },
  ]

  return (
    <div>
      <Table<Wrapped>
        size="small"
        pagination={false}
        rowKey="__i"
        dataSource={data}
        columns={columns}
        locale={{ emptyText: 'Chưa có dòng nào' }}
      />
      <Button
        size="small"
        icon={<PlusOutlined />}
        style={{ marginTop: 8 }}
        onClick={() => {
          const blank: JsonObject = {}
          for (const k of keys) if (props[k].default !== undefined) blank[k] = props[k].default
          onChange([...rows, blank])
        }}
      >
        Thêm dòng
      </Button>
    </div>
  )
}

// ------------------------------------------------------------ free JSON

function JsonField({
  value,
  onChange,
  placeholder,
}: {
  value: unknown
  onChange: (next: unknown) => void
  placeholder?: string
}) {
  const initial = useMemo(
    () => (value === undefined || value === null ? '' : JSON.stringify(value, null, 2)),
    // Seed once; typing must not be clobbered by our own updates.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  )
  const [text, setText] = useState(initial)
  const [err, setErr] = useState('')

  useEffect(() => {
    if (text.trim() === '') {
      setErr('')
      return
    }
    try {
      JSON.parse(text)
      setErr('')
    } catch (e) {
      setErr(e instanceof Error ? e.message : 'JSON không hợp lệ')
    }
  }, [text])

  return (
    <div>
      <TextArea
        className="mono"
        rows={5}
        value={text}
        placeholder={placeholder ?? '{ }'}
        status={err ? 'error' : undefined}
        onChange={(e) => {
          const t = e.target.value
          setText(t)
          if (t.trim() === '') {
            onChange(undefined)
            return
          }
          try {
            onChange(JSON.parse(t))
          } catch {
            /* keep the last valid value until the text parses again */
          }
        }}
      />
      {err && (
        <Typography.Text type="danger" style={{ fontSize: 12 }}>
          JSON không hợp lệ: {err}
        </Typography.Text>
      )}
    </div>
  )
}

// -------------------------------------------------------------- one field

function Field({
  name,
  schema,
  value,
  required,
  onChange,
}: {
  name: string
  schema: JsonSchema
  value: unknown
  required: boolean
  onChange: (v: unknown) => void
}) {
  const label = schema.title ?? name
  const missing = required && isEmpty(value)

  let control: React.ReactNode
  const ui = schema.ui ?? ''

  if (ui === 'keyvalue') {
    control = <KeyValueField value={value} onChange={onChange} />
  } else if (ui === 'table' || (schema.type === 'array' && schema.items?.type === 'object')) {
    control = <TableField schema={schema} value={value} onChange={onChange} />
  } else if (schema.type === 'array') {
    control = (
      <Select
        mode="tags"
        style={{ width: '100%' }}
        placeholder={schema.placeholder ?? 'Gõ rồi Enter để thêm'}
        value={Array.isArray(value) ? (value as string[]).map(String) : []}
        onChange={(v) => onChange(v)}
        options={(schema.items?.enum ?? []).map((o) => ({ value: String(o), label: String(o) }))}
      />
    )
  } else if (schema.type === 'object' || ui === 'json') {
    control = <JsonField value={value} onChange={onChange} placeholder={schema.placeholder} />
  } else if (schema.type === 'boolean') {
    control = <Switch checked={Boolean(value)} onChange={onChange} />
  } else if (schema.type === 'number' || schema.type === 'integer') {
    control = (
      <InputNumber
        style={{ width: '100%' }}
        min={schema.minimum}
        max={schema.maximum}
        placeholder={schema.placeholder}
        value={typeof value === 'number' ? value : undefined}
        onChange={(v) => onChange(v ?? undefined)}
      />
    )
  } else if (Array.isArray(schema.enum)) {
    control = (
      <Select
        style={{ width: '100%' }}
        allowClear
        placeholder={schema.placeholder ?? 'Chọn…'}
        value={value === undefined || value === null || value === '' ? undefined : String(value)}
        options={schema.enum.map((o) => ({ value: String(o), label: String(o) }))}
        onChange={(v) => onChange(v)}
      />
    )
  } else if (ui === 'password') {
    control = (
      <Password
        placeholder={schema.placeholder}
        value={typeof value === 'string' ? value : ''}
        onChange={(e) => onChange(e.target.value)}
      />
    )
  } else if (ui === 'textarea') {
    control = (
      <TextArea
        className="mono"
        rows={4}
        placeholder={schema.placeholder}
        value={typeof value === 'string' ? value : ''}
        onChange={(e) => onChange(e.target.value)}
      />
    )
  } else {
    control = (
      <Input
        placeholder={schema.placeholder}
        value={typeof value === 'string' ? value : value === undefined || value === null ? '' : String(value)}
        onChange={(e) => onChange(e.target.value)}
      />
    )
  }

  return (
    <Form.Item
      label={label}
      required={required}
      extra={schema.description}
      validateStatus={missing ? 'error' : undefined}
      help={missing ? 'Bắt buộc nhập.' : undefined}
      style={{ marginBottom: 14 }}
    >
      {control}
    </Form.Item>
  )
}

// ------------------------------------------------------------------ form

export default function SchemaForm({
  schema,
  value,
  onChange,
}: {
  schema: JsonSchema | undefined
  value: JsonObject
  onChange: (next: JsonObject) => void
}) {
  const { token } = theme.useToken()
  const props = schema?.properties ?? {}
  const keys = Object.keys(props)
  const required = new Set(schema?.required ?? [])

  if (keys.length === 0) {
    return (
      <Typography.Text type="secondary" style={{ fontSize: 13 }}>
        Node này không có tham số cấu hình.
      </Typography.Text>
    )
  }

  const set = (k: string, v: unknown) => {
    const next = { ...value }
    if (v === undefined || v === '') delete next[k]
    else next[k] = v
    onChange(next)
  }

  return (
    <Form layout="vertical" size="middle" style={{ color: token.colorText }}>
      {keys.map((k) => (
        <Field
          key={k}
          name={k}
          schema={props[k]}
          required={required.has(k)}
          value={value[k] ?? props[k].default}
          onChange={(v) => set(k, v)}
        />
      ))}
    </Form>
  )
}
