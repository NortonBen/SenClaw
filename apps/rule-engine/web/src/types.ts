// Wire shapes mirrored from the Rust backend.
// Sources: src/model.rs, src/engine/spec.rs, src/engine/types.rs,
// src/engine/graph.rs, src/engine/services.rs.

export type Json = unknown
export type JsonObject = Record<string, unknown>

// ------------------------------------------------------------------ registry

export type PortArity = 'one' | 'many'

export interface PortSpec {
  id: string
  label: string
  color?: string
  arity: PortArity
  description?: string
}

export type RuleCategory = 'source' | 'transform' | 'logic' | 'filter' | 'sink' | 'ai'

/** JSON Schema subset the config forms understand. */
export interface JsonSchema {
  type?: string
  title?: string
  description?: string
  placeholder?: string
  default?: Json
  enum?: Json[]
  /** Non-standard hint: textarea | password | select | keyvalue | table | json */
  ui?: string
  required?: string[]
  properties?: Record<string, JsonSchema>
  items?: JsonSchema
  minimum?: number
  maximum?: number
}

export interface RuleSpec {
  id: string
  name: string
  description: string
  category: RuleCategory
  icon: string
  color: string
  inputs: PortSpec[]
  outputs: PortSpec[]
  config_schema: JsonSchema
  doc: string
  /** Injected by GET /api/registry. */
  isSource: boolean
}

// -------------------------------------------------------------------- chains

export type ChainStatus = 'ACTIVE' | 'INACTIVE' | 'ERROR'

export interface Chain {
  id: number
  name: string
  description: string
  status: ChainStatus
  debug: boolean
  version: number
  created_at: string
  updated_at: string
  /** Only present on list / get responses. */
  deployed?: boolean
}

export type JoinPolicy = 'any' | 'all' | 'merge'

export interface NodeOpts {
  join: JoinPolicy
  corrKey: string | null
  joinTimeoutMs: number | null
  concurrency: number
  retries: number
  retryBackoffMs: number
}

export const DEFAULT_OPTS: NodeOpts = {
  join: 'any',
  corrKey: null,
  joinTimeoutMs: null,
  concurrency: 1,
  retries: 0,
  retryBackoffMs: 500,
}

export interface ChainNode {
  id: string
  chain_id?: number
  rule: string
  name: string
  config: JsonObject
  opts: NodeOpts
  x: number
  y: number
  debug: boolean
}

export interface PortRef {
  node: string
  port: string
}

export interface ChainEdge {
  id: string
  from: PortRef
  to: PortRef
}

export type IssueLevel = 'error' | 'warning'

export interface Issue {
  level: IssueLevel
  node?: string
  edge?: string
  message: string
}

// ---------------------------------------------------------------- runs / logs

export interface RunRow {
  id: number
  chain_id: number
  status: string
  trigger_node: string
  started_at: number
  ended_at: number | null
  hops: number
  error: string | null
}

export interface HopRow {
  id: number
  run_id: number
  chain_id: number
  seq: number
  node: string
  rule: string
  in_port: string
  out_port: string
  kind: string
  data: string
  error: string
  ts: number
  dur_ms: number
}

export interface LogRow {
  id: number
  chain_id: number
  run_id: number | null
  level: string
  node: string | null
  message: string
  ts: number
}

/** Normalised hop used by the trace table (SSE and REST feed the same shape). */
export interface TraceHop {
  key: string
  runId: number
  seq: number
  node: string
  rule: string
  inPort: string
  outPort: string
  kind: string
  error: string
  durMs: number
  ts: number
  data: string
}

// -------------------------------------------------------------------- events

export interface EvRunStart {
  type: 'runStart'
  runId: number
  chainId: number
  node: string
}

export interface EvHop {
  type: 'hop'
  runId: number
  chainId: number
  seq: number
  node: string
  rule: string
  inPort: string
  outPort: string
  kind: string
  data: Json
  error: string | null
  durMs: number
}

export interface EvRunEnd {
  type: 'runEnd'
  runId: number
  chainId: number
  status: string
  hops: number
  error: string | null
}

export interface EvLog {
  type: 'log'
  chainId: number
  runId: number | null
  level: string
  node: string | null
  message: string
  ts: number
}

export interface EvChainStatus {
  type: 'chainStatus'
  chainId: number
  status: string
  error?: string | null
}

export type EngineEvent = EvRunStart | EvHop | EvRunEnd | EvLog | EvChainStatus

// -------------------------------------------------------------------- status

export interface EngineStatus {
  ok: boolean
  chains: number
  active: number
  deployed: number
  runningRuns: number
  nodeTypes: number
}
