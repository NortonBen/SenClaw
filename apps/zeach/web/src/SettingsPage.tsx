import { Space, Typography } from 'antd'
import type { SourceInfo } from './api'
import Sources from './Sources'
import Corpus from './Corpus'

const { Title, Paragraph } = Typography

export default function SettingsPage({
  sources,
  onChanged,
}: {
  sources: SourceInfo[]
  onChanged: () => void
}) {
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <div>
        <Title level={4} style={{ marginBottom: 4 }}>
          Cài đặt
        </Title>
        <Paragraph type="secondary" style={{ marginBottom: 0 }}>
          Bật/tắt và cân trọng số các nguồn, đăng ký thêm nguồn từ bất kỳ MCP nào, và quản lý kho tài
          liệu riêng của Zeach.
        </Paragraph>
      </div>
      <Sources sources={sources} onChanged={onChanged} />
      <Corpus onChanged={onChanged} />
    </Space>
  )
}
