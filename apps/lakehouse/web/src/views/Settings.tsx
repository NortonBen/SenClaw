import { useEffect } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { App, Button, Card, Form, InputNumber, Typography } from 'antd'
import { getSettings, putSettings } from '../api'
import { errMsg } from '../util'
import type { Settings as SettingsMap } from '../types'

// Các key + nhãn tiếng Việt (allowlist khớp db::validate_setting).
const FIELDS: { key: string; label: string; help: string }[] = [
  { key: 'max_concurrent', label: 'Số run đồng thời tối đa', help: 'Giới hạn worker chạy song song' },
  { key: 'memory_limit_mb', label: 'Giới hạn bộ nhớ (MB)', help: 'Ngân sách RAM cho engine truy vấn' },
  { key: 'query_max_seconds', label: 'Timeout truy vấn (giây)', help: 'Truy vấn quá lâu bị huỷ' },
  { key: 'gc_grace_seconds', label: 'GC grace (giây)', help: 'Chờ trước khi dọn file tombstone' },
  { key: 'log_retention_days', label: 'Giữ log (ngày)', help: 'Số ngày lưu log run' },
  { key: 'import_base64_max_mb', label: 'Trần import base64 (MB)', help: 'Kích thước tối đa mỗi file import' },
]

export function Settings() {
  const { message } = App.useApp()
  const qc = useQueryClient()
  const [form] = Form.useForm<Record<string, number>>()

  const settings = useQuery({ queryKey: ['settings'], queryFn: getSettings })

  useEffect(() => {
    if (settings.data) {
      const init: Record<string, number> = {}
      for (const f of FIELDS) {
        const v = settings.data[f.key]
        if (v != null && v !== '') init[f.key] = Number(v)
      }
      form.setFieldsValue(init)
    }
  }, [settings.data, form])

  const save = useMutation({
    mutationFn: (v: Record<string, number>) => {
      const patch: SettingsMap = {}
      for (const [k, val] of Object.entries(v)) {
        if (val != null) patch[k] = String(val)
      }
      return putSettings(patch)
    },
    onSuccess: () => {
      message.success('Đã lưu cài đặt')
      qc.invalidateQueries({ queryKey: ['settings'] })
    },
    onError: (e) => message.error(errMsg(e)),
  })

  return (
    <div>
      <Typography.Title level={4}>Cài đặt</Typography.Title>
      <Card size="small" style={{ maxWidth: 560 }} loading={settings.isLoading}>
        <Form form={form} layout="vertical" onFinish={(v) => save.mutate(v)}>
          {FIELDS.map((f) => (
            <Form.Item key={f.key} name={f.key} label={f.label} help={f.help}>
              <InputNumber min={0} style={{ width: '100%' }} />
            </Form.Item>
          ))}
          <Button type="primary" htmlType="submit" loading={save.isPending}>
            Lưu
          </Button>
        </Form>
      </Card>
    </div>
  )
}
