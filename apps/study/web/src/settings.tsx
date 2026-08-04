import { useEffect, useState } from 'react'
import { App as AntApp, Button, Card, Form, Input, InputNumber, List, Select, Space, Tag, Typography } from 'antd'
import { get, patch } from './api'

interface Settings {
  tz: string
  slotHm: string
  studySlots: string
  searchMcp: string
  voice?: string | null
  speed: string
}

interface SourceInfo {
  key: string
  server: string
  tool: string
  description: string
  score: number
}

export default function SettingsView() {
  const { message } = AntApp.useApp()
  const [form] = Form.useForm()
  const [sources, setSources] = useState<{ setting: string; available: SourceInfo[]; selected: string[] } | null>(null)

  useEffect(() => {
    get<Settings>('/settings')
      .then((s) => form.setFieldsValue({ ...s, speed: Number(s.speed) }))
      .catch(() => {})
    get<typeof sources>('/sources')
      .then(setSources)
      .catch(() => {})
  }, [form])

  return (
    <Space direction="vertical" style={{ width: '100%', maxWidth: 760 }} size="middle">
      <Card title="Cài đặt học">
        <Form
          form={form}
          layout="vertical"
          onFinish={async (v) => {
            try {
              await patch('/settings', {
                tz: v.tz,
                slot_hm: v.slotHm,
                search_mcp: v.searchMcp,
                voice: v.voice ?? '',
                speed: v.speed,
              })
              message.success('Đã lưu')
            } catch (e: any) {
              message.error(String(e.message ?? e))
            }
          }}
        >
          <Form.Item name="tz" label="Múi giờ" tooltip="Quyết định 'hôm nay' là ngày nào và giờ ôn được ghim vào đâu.">
            <Input placeholder="Asia/Ho_Chi_Minh" />
          </Form.Item>
          <Form.Item name="slotHm" label="Giờ học mặc định">
            <Input placeholder="20:00" />
          </Form.Item>
          <Form.Item
            name="searchMcp"
            label="Nguồn tra cứu ngoài"
            tooltip="'auto' = tự chọn 2 nguồn điểm cao nhất đang chạy. Hoặc nhập danh sách server.tool ngăn cách bởi dấu phẩy."
          >
            <Select
              mode="tags"
              tokenSeparators={[',']}
              placeholder="auto"
              options={(sources?.available ?? []).map((s) => ({ value: s.key, label: `${s.key} (${s.score})` }))}
              onChange={(v: string[]) => form.setFieldValue('searchMcp', v.join(','))}
            />
          </Form.Item>
          <Form.Item name="voice" label="Giọng đọc (TTS)">
            <Input placeholder="để trống = dùng giọng đã chọn trong Cài đặt SenClaw" />
          </Form.Item>
          <Form.Item name="speed" label="Tốc độ đọc">
            <InputNumber min={0.5} max={2} step={0.1} />
          </Form.Item>
          <Button type="primary" htmlType="submit">Lưu</Button>
        </Form>
      </Card>

      <Card title="Nguồn MCP phát hiện được">
        <Typography.Paragraph type="secondary">
          Study không gắn cứng địa chỉ nguồn nào — nó hỏi daemon xem MCP nào đang chạy và chấm điểm
          các công cụ tra cứu. Công cụ có tác dụng phụ (tạo/sửa/xoá/gửi) bị loại khỏi danh sách.
        </Typography.Paragraph>
        <List
          dataSource={sources?.available ?? []}
          locale={{ emptyText: 'Chưa có MCP tra cứu nào đang chạy' }}
          renderItem={(s) => (
            <List.Item>
              <List.Item.Meta
                title={
                  <Space wrap>
                    <b>{s.key}</b>
                    <Tag>điểm {s.score}</Tag>
                    {sources?.selected.includes(s.key) && <Tag color="green">đang dùng</Tag>}
                  </Space>
                }
                description={s.description}
              />
            </List.Item>
          )}
        />
      </Card>
    </Space>
  )
}
