// Client-side mirror of `Registry::inputs/outputs` (src/engine/registry.rs).
//
// One function feeds both the canvas handles and the connection validator, so a
// handle can never exist that the backend would reject — and vice versa.

import type { JsonObject, PortSpec, RuleSpec } from './types'

export const PORT_ERROR = 'error'
export const PORT_IN = 'in'
export const PORT_OUT = 'out'
const PORT_DEFAULT = 'default'

/** Rules whose ports grow from their config. */
const DYNAMIC_OUTPUTS = new Set(['switch'])
const DYNAMIC_INPUTS = new Set(['join', 'merge'])

export interface NodePorts {
  inputs: PortSpec[]
  outputs: PortSpec[]
}

function text(v: unknown): string {
  if (v === null || v === undefined) return ''
  if (typeof v === 'string') return v
  return JSON.stringify(v)
}

/**
 * Port ids end up as edge keys and DOM handle ids. Same rule as
 * `switch_rule::sanitize`: keep letters, digits, `_` and `-`.
 */
export function sanitizePort(value: unknown, index: number): string {
  const cleaned = text(value)
    .trim()
    .replace(/[^\p{L}\p{N}_-]/gu, '_')
    .replace(/^_+|_+$/g, '')
  return cleaned === '' ? `case${index + 1}` : cleaned
}

function switchOutputs(config: JsonObject): PortSpec[] {
  const raw = Array.isArray(config.cases) ? (config.cases as unknown[]) : []
  const out: PortSpec[] = raw.map((c, i) => {
    const row = (c && typeof c === 'object' ? c : {}) as JsonObject
    const value = row.value
    const explicit = typeof row.port === 'string' ? row.port.trim() : ''
    const id = explicit !== '' ? explicit : sanitizePort(value, i)
    const label = text(value).trim() || id
    return {
      id,
      label,
      color: '#52c41a',
      arity: 'one',
      description: `Khớp giá trị \`${text(value)}\``,
    }
  })
  out.push({
    id: PORT_DEFAULT,
    label: 'default',
    color: '#faad14',
    arity: 'one',
    description: 'Không case nào khớp.',
  })
  return out
}

/**
 * `join` / `merge` grow one input per name in `config.inputs`. The port id is the
 * name **verbatim** — the exact string `join_rule::input_names` uses on the
 * backend, with no sanitizing on either side. That is the whole point: the id the
 * canvas draws, the id Rust declares, and the id stored on the edge are one and
 * the same string. `validate` (both here and in Rust) rejects blank / whitespace
 * / duplicate names so a bad name is caught instead of silently mismatching.
 */
function dynamicInputs(config: JsonObject): PortSpec[] {
  const raw = config.inputs
  if (!Array.isArray(raw)) return []
  return raw.map((item, i) => {
    const id = typeof item === 'string' ? item.trim() : text(item)
    return {
      id,
      label: id || `#${i + 1}`,
      color: '#8c8c8c',
      arity: 'many',
    }
  })
}

/** Keeps `error` visually last without changing the port set. */
function errorLast(ports: PortSpec[]): PortSpec[] {
  const rest = ports.filter((p) => p.id !== PORT_ERROR)
  const err = ports.filter((p) => p.id === PORT_ERROR)
  return [...rest, ...err]
}

/**
 * The ports a node actually has, given its rule spec and its current config.
 * Declared ports come first; dynamic outputs are appended (skipping ids that
 * already exist), dynamic inputs *replace* the declared ones — exactly what
 * `Registry::inputs` does.
 */
export function portsOf(spec: RuleSpec | undefined, config: JsonObject): NodePorts {
  if (!spec) return { inputs: [], outputs: [] }

  let inputs = [...spec.inputs]
  if (DYNAMIC_INPUTS.has(spec.id)) {
    const dyn = dynamicInputs(config)
    if (dyn.length > 0) inputs = dyn
  }

  const outputs = [...spec.outputs]
  if (DYNAMIC_OUTPUTS.has(spec.id)) {
    for (const p of switchOutputs(config)) {
      if (!outputs.some((x) => x.id === p.id)) outputs.push(p)
    }
  }
  // Every node has `error` whether or not it declares one.
  if (!outputs.some((p) => p.id === PORT_ERROR)) {
    outputs.push({ id: PORT_ERROR, label: 'error', color: '#f5222d', arity: 'many' })
  }

  return { inputs, outputs: errorLast(outputs) }
}

export const portColor = (p: PortSpec | undefined, fallback = '#8c8c8c') => p?.color || fallback
