import { Button, Modal, Space, Tag, Typography, theme } from 'antd'
import { ExportOutlined, LeftOutlined, RightOutlined } from '@ant-design/icons'
import type { Evidence } from './api'
import { kindColor } from './theme'

const { Text, Paragraph } = Typography

function when(ms?: number): string | null {
  if (!ms) return null
  const d = new Date(ms)
  return Number.isNaN(d.getTime()) ? null : d.toLocaleString('vi-VN')
}

/**
 * The dẫn chứng dialog: what a citation `[n]` actually rests on.
 *
 * A citation is only checkable if the reader can reach the underlying text
 * without leaving the report — a tooltip showing a chunk id proves nothing, and
 * an `<a>` that navigates away loses the report. So `[n]` opens this instead:
 * the retrieved text itself, its provenance, and only then a link out.
 *
 * `index` is the 1-based citation number, i.e. the position in the SAME evidence
 * array the report numbered — the invariant `synthesize::number_evidence` keeps.
 */
export default function EvidenceModal({
  evidence,
  index,
  onClose,
  onNavigate,
}: {
  evidence: Evidence[]
  /** 1-based citation number, or null when closed. */
  index: number | null
  onClose: () => void
  onNavigate: (next: number) => void
}) {
  const { token } = theme.useToken()
  const e = index != null ? evidence[index - 1] : undefined

  return (
    <Modal
      open={index != null}
      onCancel={onClose}
      width={760}
      title={
        e ? (
          <Space size={8} wrap>
            <Tag color="purple" style={{ marginInlineEnd: 0 }}>
              [{index}]
            </Tag>
            <span>Dẫn chứng dữ liệu</span>
          </Space>
        ) : (
          'Dẫn chứng dữ liệu'
        )
      }
      footer={
        <Space style={{ width: '100%', justifyContent: 'space-between' }}>
          <Space size={4}>
            <Button
              size="small"
              icon={<LeftOutlined />}
              disabled={!index || index <= 1}
              onClick={() => index && onNavigate(index - 1)}
            >
              Trước
            </Button>
            <Button
              size="small"
              icon={<RightOutlined />}
              disabled={!index || index >= evidence.length}
              onClick={() => index && onNavigate(index + 1)}
            >
              Sau
            </Button>
            <Text type="secondary" style={{ fontSize: 12.5 }}>
              {index} / {evidence.length}
            </Text>
          </Space>
          <Space size={4}>
            {e?.url && (
              <Button
                size="small"
                type="primary"
                icon={<ExportOutlined />}
                href={e.url}
                target="_blank"
                rel="noreferrer"
              >
                Mở nguồn
              </Button>
            )}
            <Button size="small" onClick={onClose}>
              Đóng
            </Button>
          </Space>
        </Space>
      }
    >
      {!e ? (
        <Text type="secondary">
          Trích dẫn [{index}] không có trong danh sách bằng chứng của lần chạy này.
        </Text>
      ) : (
        <Space direction="vertical" size={10} style={{ width: '100%' }}>
          <div style={{ fontSize: 15.5, fontWeight: 600 }}>
            {e.title?.trim() || '(không có tiêu đề)'}
          </div>

          <Space size={[4, 4]} wrap>
            {e.domain && <Tag bordered={false}>{e.domain}</Tag>}
            {e.hits.map((h) => (
              <Tag key={`${h.source_id}-${h.rank}`} bordered={false} color={kindColor(h.kind)}>
                {h.source_id} · #{h.rank + 1}
              </Tag>
            ))}
            {e.independent_kinds > 1 && (
              <Tag color="success" bordered={false}>
                {e.independent_kinds} loại nguồn độc lập
              </Tag>
            )}
            {e.full_text && <Tag bordered={false}>đã tải toàn văn</Tag>}
          </Space>

          <Text type="secondary" style={{ fontSize: 12 }}>
            {when(e.published_at) && <>đăng {when(e.published_at)} · </>}
            lấy về {when(e.retrieved_at) ?? '—'} · rrf {e.fused_score?.toFixed(4) ?? '—'}
          </Text>

          {e.url && (
            <Paragraph copyable={{ text: e.url }} style={{ marginBottom: 0 }}>
              <a href={e.url} target="_blank" rel="noreferrer" style={{ fontSize: 12.5 }}>
                {e.url}
              </a>
            </Paragraph>
          )}

          <div>
            <Text type="secondary" style={{ fontSize: 12.5 }}>
              Đoạn trích đã dùng để rút khẳng định
            </Text>
            <div
              style={{
                marginTop: 4,
                padding: 12,
                borderRadius: token.borderRadius,
                background: token.colorFillQuaternary,
                border: `1px solid ${token.colorBorderSecondary}`,
                fontSize: 13.5,
                whiteSpace: 'pre-wrap',
              }}
            >
              {e.snippet?.trim() || '(nguồn không trả về đoạn trích)'}
            </div>
          </div>

          {e.full_text && (
            <div>
              <Text type="secondary" style={{ fontSize: 12.5 }}>
                Toàn văn đã tải về ({e.full_text.length.toLocaleString('vi-VN')} ký tự)
              </Text>
              <div
                style={{
                  marginTop: 4,
                  padding: 12,
                  borderRadius: token.borderRadius,
                  border: `1px solid ${token.colorBorderSecondary}`,
                  maxHeight: '38vh',
                  overflow: 'auto',
                  fontSize: 13,
                  whiteSpace: 'pre-wrap',
                }}
              >
                {e.full_text}
              </div>
            </div>
          )}
        </Space>
      )}
    </Modal>
  )
}
