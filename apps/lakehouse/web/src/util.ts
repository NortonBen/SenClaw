import { ApiError } from './api'

export function errMsg(e: unknown): string {
  if (e instanceof ApiError) return e.message
  if (e instanceof Error) return e.message
  return String(e)
}

// true nếu lỗi là "endpoint chưa khả dụng" (404 hoặc SPA-fallback).
export function isUnavailable(e: unknown): boolean {
  return e instanceof ApiError && e.status === 404
}

export function fmtBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0 B'
  const u = ['B', 'KB', 'MB', 'GB', 'TB']
  let i = 0
  let v = n
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${u[i]}`
}

export function fmtNum(n: number): string {
  return Number(n ?? 0).toLocaleString('vi-VN')
}

export function fmtTime(s: string | null | undefined): string {
  if (!s) return '—'
  const d = new Date(s)
  if (Number.isNaN(d.getTime())) return s
  return d.toLocaleString('vi-VN')
}

// Đọc <input type=file> → base64 (không có prefix data-url).
export function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => {
      const res = r.result as string
      const comma = res.indexOf('base64,')
      resolve(comma >= 0 ? res.slice(comma + 'base64,'.length) : res)
    }
    r.onerror = () => reject(r.error ?? new Error('đọc file thất bại'))
    r.readAsDataURL(file)
  })
}

// Màu Tag theo status run/step.
export function statusColor(s: string): string {
  switch (s) {
    case 'success':
      return 'green'
    case 'running':
      return 'processing'
    case 'queued':
      return 'gold'
    case 'failed':
    case 'error':
      return 'red'
    case 'cancelled':
      return 'default'
    default:
      return 'blue'
  }
}

export const isActiveStatus = (s: string) => s === 'queued' || s === 'running'
export const isTerminalStatus = (s: string) =>
  s === 'success' || s === 'failed' || s === 'error' || s === 'cancelled'
