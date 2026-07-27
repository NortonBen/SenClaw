// Kiểu dữ liệu 1:1 với backend REST (src/api.rs, src/db.rs). Không đoán field.

export interface StatusResp {
  ok: boolean
  version: string
  datasets: number
  total_rows: number
  total_bytes: number
  runs_active: number
  runs_24h: number
  next?: string
}

export interface Dataset {
  id: number
  namespace: string
  name: string
  format: string
  layer: string | null
  partition_cols: string | null
  owner_flow_id: string | null
  current_schema_version: number | null
  row_count: number
  byte_size: number
  created_at: string
  updated_at: string
}

export interface DatasetListResp {
  total: number
  datasets: Dataset[]
  next?: string
}

export interface SchemaVersion {
  dataset_id: number
  version: number
  arrow_schema: string
  change: string | null
  created_at: string
}

// `schema` là arrow-schema JSON đã parse (object có `fields`, hoặc null).
export interface DatasetDetail {
  dataset: Dataset
  schema: unknown
  schema_versions: SchemaVersion[]
  files: { active: number; bytes: number }
  owner_flow_id: string | null
  next?: string
}

export interface Page {
  columns: string[]
  rows: unknown[][]
  returned: number
  has_more: boolean
  total_estimate?: number | null
  next?: string
}

export interface PreviewResp extends Page {
  namespace: string
  dataset: string
}

export interface ConnectionView {
  id: string
  kind: string
  dsn: string
  created_at: string
  last_ok_at: string | null
}

export interface ConnectionListResp {
  total: number
  connections: ConnectionView[]
  next?: string
}

export interface ColumnInfo {
  name: string
  data_type: string
  nullable: boolean
}

export interface TableInfo {
  schema: string | null
  name: string
  columns: ColumnInfo[]
}

export interface IntrospectResp {
  connection_id: string
  total: number
  tables: TableInfo[]
  next?: string
}

export interface FlowView {
  id: string
  name: string | null
  def: unknown
  def_version: number
  enabled: boolean
  schedule: string | null
  last_scheduled_at: string | null
  created_at: string
  updated_at: string
  dag: string[] | null
}

export interface FlowListResp {
  total: number
  flows: FlowView[]
  next?: string
}

export interface FlowGetResp {
  flow: FlowView
  next?: string
}

export interface FlowImpact {
  steps_reset: string[]
  steps_kept: string[]
  datasets_orphaned: string[]
}

export interface Run {
  id: string
  flow_id: string
  trigger: string
  status: string
  started_at: string | null
  ended_at: string | null
  error: string | null
  updated_at: string
}

export interface RunListResp {
  total: number
  runs: Run[]
  next?: string
}

export interface StepRun {
  run_id: string
  step_id: string
  status: string
  rows_read: number
  rows_written: number
  started_at: string | null
  ended_at: string | null
  error: string | null
}

export interface RunGetResp {
  run: Run
  steps: StepRun[]
  next?: string
}

export interface RunLogLine {
  seq: number
  ts: string
  level: string
  step_id: string | null
  message: string
}

export interface RunLogsResp {
  run_id: string
  returned: number
  logs: RunLogLine[]
  next?: string
}

export type Settings = Record<string, string>

export interface FieldError {
  // step_id rỗng = lỗi cấp flow (khớp src/flow.rs FieldError).
  step_id: string
  field: string
  message: string
}
