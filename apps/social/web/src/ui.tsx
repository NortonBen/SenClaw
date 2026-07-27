import { Tag } from 'antd'
import type { Capability } from './api'

const STATUS_COLOR: Record<string, string> = {
  ok: 'green',
  sent: 'green',
  reserved: 'green',
  online: 'green',
  official: 'blue',
  pending: 'gold',
  unsupported: 'orange',
  error: 'red',
  blocked: 'red',
  offline: 'red',
  rejected: 'red',
}

export const StatusTag = ({ value }: { value: string }) => (
  <Tag color={STATUS_COLOR[value]}>{value}</Tag>
)

const CAP_COLOR: Record<Capability, string> = {
  official: 'green',
  replay: 'purple',
  'page-sign': 'orange',
  dom: 'gold',
  none: 'red',
}

export const CapTag = ({ value }: { value: Capability }) => (
  <Tag color={CAP_COLOR[value]}>{value}</Tag>
)
