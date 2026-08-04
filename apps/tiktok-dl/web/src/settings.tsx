// Tab Cài đặt: thư mục lưu, chất lượng mặc định, mẫu tên file, số tải đồng
// thời… lưu vào SQLite phía backend (dùng chung cho cả MCP tools). Kèm nhật
// ký hoạt động gần đây. Riêng giao diện sáng/tối nằm ở góc phải header (lưu
// theo trình duyệt).

import { useEffect, useState } from 'react'
import {
  Button,
  Card,
  Flex,
  Form,
  Input,
  InputNumber,
  List,
  message,
  Select,
  Switch,
  Tag,
  Typography,
} from 'antd'
import { FolderOpenOutlined, SaveOutlined } from '@ant-design/icons'
import { api } from './api'

const { Text } = Typography

interface FormShape {
  download_dir: string
  default_quality: string
  filename_template: string
  max_concurrent: number
  photo_audio: boolean
  save_meta_json: boolean
  profile_max: number
}

export default function SettingsTab() {
  const [form] = Form.useForm<FormShape>()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [activity, setActivity] = useState<{ kind: string; message: string; ref_id: string; at: string }[]>([])

  const load = () => {
    setLoading(true)
    api
      .settings()
      .then((r) => {
        const s = r.settings
        form.setFieldsValue({
          download_dir: s.download_dir,
          default_quality: s.default_quality,
          filename_template: s.filename_template,
          max_concurrent: Number(s.max_concurrent) || 2,
          photo_audio: s.photo_audio === '1',
          save_meta_json: s.save_meta_json === '1',
          profile_max: Number(s.profile_max) || 30,
        })
      })
      .finally(() => setLoading(false))
    api.activity().then((r) => setActivity(r.activity ?? [])).catch(() => {})
  }

  useEffect(load, []) // eslint-disable-line react-hooks/exhaustive-deps

  const save = async (v: FormShape) => {
    setSaving(true)
    try {
      const r = await api.setSettings({
        download_dir: v.download_dir,
        default_quality: v.default_quality,
        filename_template: v.filename_template,
        max_concurrent: v.max_concurrent,
        photo_audio: v.photo_audio,
        save_meta_json: v.save_meta_json,
        profile_max: v.profile_max,
      })
      if (r.error) message.error(String(r.error))
      else message.success('Đã lưu cài đặt')
    } finally {
      setSaving(false)
    }
  }

  return (
    <Flex gap={16} wrap align="flex-start">
      <Card title="Cài đặt tải" style={{ flex: '1 1 460px' }} loading={loading}>
        <Form form={form} layout="vertical" onFinish={save} requiredMark={false}>
          <Form.Item
            name="download_dir"
            label="Thư mục lưu file"
            rules={[{ required: true, message: 'Không được để trống' }]}
            extra="Đường dẫn trên máy đang chạy SenClaw. Thư mục sẽ tự được tạo."
          >
            <Input
              addonAfter={
                <Button
                  type="text"
                  size="small"
                  icon={<FolderOpenOutlined />}
                  onClick={() => api.openDir().then((r) => r?.error && message.error(String(r.error)))}
                  title="Mở thư mục trong Finder"
                />
              }
            />
          </Form.Item>
          <Flex gap={12} wrap>
            <Form.Item name="default_quality" label="Chất lượng mặc định" style={{ flex: 1, minWidth: 180 }}>
              <Select
                options={[
                  { value: 'nowm', label: 'Không logo' },
                  { value: 'hd', label: 'HD (không logo)' },
                  { value: 'wm', label: 'Bản gốc có logo' },
                  { value: 'audio', label: 'Chỉ nhạc MP3' },
                ]}
              />
            </Form.Item>
            <Form.Item name="max_concurrent" label="Tải đồng thời" style={{ width: 130 }} extra="1–4 job">
              <InputNumber min={1} max={4} style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="profile_max" label="Trần video / profile" style={{ width: 150 }} extra="khi tải cả trang">
              <InputNumber min={1} max={200} style={{ width: '100%' }} />
            </Form.Item>
          </Flex>
          <Form.Item
            name="filename_template"
            label="Mẫu tên file"
            extra={
              <span>
                Ghép từ: <Tag>{'{author}'}</Tag>
                <Tag>{'{id}'}</Tag>
                <Tag>{'{title}'}</Tag>
                <Tag>{'{date}'}</Tag>
                <Tag>{'{quality}'}</Tag> — ví dụ <code>{'{date}_{author}_{title}'}</code>
              </span>
            }
          >
            <Input />
          </Form.Item>
          <Flex gap={24} wrap>
            <Form.Item name="photo_audio" label="Post ảnh: tải kèm nhạc nền" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="save_meta_json" label="Ghi metadata .json cạnh file" valuePropName="checked">
              <Switch />
            </Form.Item>
          </Flex>
          <Button type="primary" htmlType="submit" icon={<SaveOutlined />} loading={saving}>
            Lưu cài đặt
          </Button>
        </Form>
      </Card>

      <Card title="Hoạt động gần đây" style={{ flex: '1 1 320px' }} size="small">
        <List
          size="small"
          dataSource={activity}
          locale={{ emptyText: 'Chưa có hoạt động' }}
          renderItem={(a) => (
            <List.Item>
              <Flex vertical style={{ width: '100%' }}>
                <Flex justify="space-between" gap={8}>
                  <Text style={{ fontSize: 13 }}>
                    {a.message}
                    {a.ref_id && <Text type="secondary"> #{a.ref_id}</Text>}
                  </Text>
                  <Tag style={{ margin: 0 }}>{a.kind}</Tag>
                </Flex>
                <Text type="secondary" style={{ fontSize: 11 }}>
                  {a.at.replace('T', ' ').slice(0, 16)}
                </Text>
              </Flex>
            </List.Item>
          )}
          style={{ maxHeight: 480, overflow: 'auto' }}
        />
      </Card>
    </Flex>
  )
}
