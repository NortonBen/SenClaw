import { useEffect, useMemo, useState } from 'react';
import { Alert, Avatar, Button, Spin, Tag, Tooltip, Typography, theme } from 'antd';
import { EditOutlined, InboxOutlined, PictureOutlined } from '@ant-design/icons';
import type { EmailDetail } from '../api';
import {
  avatarColor, displayName, emailAddress, escapeHtml,
  formatFullDate, sanitizeEmailHtml, textToHtml,
} from '../lib/mail';

const { Text, Title } = Typography;

interface Props {
  email: EmailDetail | null;
  loading: boolean;
  onReply: () => void;
}

export function MailView({ email, loading, onReply }: Props) {
  const { token } = theme.useToken();
  const [showImages, setShowImages] = useState(false);

  // Every message starts with images withheld, so switching mail never carries
  // a previous "show images" decision onto a new sender.
  useEffect(() => { setShowImages(false); }, [email?.id]);

  if (loading) {
    return (
      <Pane>
        <div style={{ display: 'flex', height: '100%', alignItems: 'center', justifyContent: 'center' }}>
          <Spin />
        </div>
      </Pane>
    );
  }

  if (!email) {
    return (
      <Pane>
        <div
          style={{
            display: 'flex', flexDirection: 'column', height: '100%',
            alignItems: 'center', justifyContent: 'center', gap: 12,
          }}
        >
          <InboxOutlined style={{ fontSize: 52, color: token.colorTextQuaternary }} />
          <Text type="secondary">Chọn một thư để đọc</Text>
        </div>
      </Pane>
    );
  }

  const name = displayName(email.from);
  const addr = emailAddress(email.from);

  return (
    <Pane>
      <div style={{ display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 }}>
        {/* Header */}
        <div
          style={{
            padding: '24px 32px 20px',
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            flexShrink: 0,
          }}
        >
          <Title level={4} style={{ margin: '0 0 16px', lineHeight: 1.35 }}>
            {email.subject || '(Không có tiêu đề)'}
          </Title>

          <div style={{ display: 'flex', alignItems: 'flex-start', gap: 12 }}>
            <Avatar
              size={40}
              style={{ backgroundColor: avatarColor(name), flexShrink: 0, fontWeight: 600 }}
            >
              {name.charAt(0).toUpperCase()}
            </Avatar>

            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
                <Text strong style={{ fontSize: 14 }}>{name}</Text>
                {addr && (
                  <Text style={{ fontSize: 12, color: token.colorTextTertiary }}>&lt;{addr}&gt;</Text>
                )}
                {email.folder === 'Sent' && <Tag color="blue">Đã gửi</Tag>}
              </div>
              <Text style={{ display: 'block', fontSize: 12, color: token.colorTextTertiary, marginTop: 2 }}>
                Đến: {email.to || '—'}
              </Text>
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 8, flexShrink: 0 }}>
              <Text style={{ fontSize: 12, color: token.colorTextTertiary }}>
                {formatFullDate(email.date)}
              </Text>
              <Tooltip title="Trả lời người gửi">
                <Button size="small" type="primary" ghost icon={<EditOutlined />} onClick={onReply}>
                  Trả lời
                </Button>
              </Tooltip>
            </div>
          </div>
        </div>

        <MailBody
          email={email}
          showImages={showImages}
          onShowImages={() => setShowImages(true)}
        />
      </div>
    </Pane>
  );
}

function Pane({ children }: { children: React.ReactNode }) {
  const { token } = theme.useToken();
  return (
    <div style={{ flex: 1, minWidth: 0, minHeight: 0, background: token.colorBgContainer }}>
      {children}
    </div>
  );
}

/**
 * Renders the message body inside a `sandbox=""` iframe: sender HTML gets no
 * scripts, no forms, and no access to this origin, so a hostile mail cannot
 * touch the app. The frame is sized to its content because a sandboxed document
 * cannot report its own height back to us.
 */
function MailBody({
  email, showImages, onShowImages,
}: {
  email: EmailDetail;
  showImages: boolean;
  onShowImages: () => void;
}) {
  const { token } = theme.useToken();

  const { html, blockedImages } = useMemo(() => {
    if (email.body_html?.trim()) {
      const res = sanitizeEmailHtml(email.body_html, showImages);
      return { html: res.html, blockedImages: res.blockedImages };
    }
    const text = email.body_text?.trim();
    if (!text) {
      return { html: '<p class="empty">(Thư không có nội dung)</p>', blockedImages: false };
    }
    return { html: `<pre class="plain">${textToHtml(text)}</pre>`, blockedImages: false };
  }, [email.body_html, email.body_text, showImages]);

  // Theme the sandboxed document to match the app; sender styles still win
  // inside their own markup, which is what a mail client should do.
  const doc = useMemo(() => `<!doctype html>
<html><head><meta charset="utf-8">
<style>
  :root { color-scheme: ${isDarkColor(token.colorBgContainer) ? 'dark' : 'light'}; }
  html, body { margin: 0; padding: 0; }
  body {
    font-family: ${escapeHtml(token.fontFamily)};
    font-size: 14px;
    line-height: 1.7;
    color: ${token.colorText};
    background: ${token.colorBgContainer};
    padding: 24px 32px 32px;
    word-break: break-word;
    overflow-wrap: anywhere;
  }
  a { color: ${token.colorPrimary}; }
  img { max-width: 100%; height: auto; }
  table { max-width: 100%; }
  pre.plain {
    margin: 0;
    font-family: inherit;
    font-size: 14px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .empty { color: ${token.colorTextQuaternary}; font-style: italic; }
  blockquote {
    margin: 12px 0; padding: 4px 0 4px 14px;
    border-left: 3px solid ${token.colorBorder};
    color: ${token.colorTextSecondary};
  }
  img[data-src] {
    display: inline-block; min-width: 18px; min-height: 18px;
    background: ${token.colorFillSecondary};
    border: 1px dashed ${token.colorBorder};
    border-radius: 4px;
  }
</style></head>
<body>${html}</body></html>`, [html, token]);

  return (
    <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column' }}>
      {blockedImages && !showImages && (
        <Alert
          type="warning"
          showIcon
          icon={<PictureOutlined />}
          style={{ margin: '16px 32px 0', borderRadius: token.borderRadius, flexShrink: 0 }}
          message="Ảnh từ xa đã bị chặn để người gửi không biết bạn đã mở thư."
          action={
            <Button size="small" type="text" onClick={onShowImages}>
              Hiển thị ảnh
            </Button>
          }
        />
      )}
      <iframe
        title={email.subject || 'Nội dung thư'}
        // No allow-scripts and no allow-same-origin: sender HTML stays inert and
        // cannot reach this origin. allow-popups (plus -to-escape-sandbox) is the
        // narrow exception that lets a link open in a real new tab; without it
        // every link in every email would silently do nothing.
        //
        // Withholding allow-same-origin also means we cannot read the frame's
        // scrollHeight to size it to its content, so the frame fills the pane
        // and its own document scrolls instead.
        sandbox="allow-popups allow-popups-to-escape-sandbox"
        srcDoc={doc}
        style={{
          display: 'block',
          flex: 1,
          width: '100%',
          minHeight: 0,
          border: 'none',
          background: token.colorBgContainer,
        }}
      />
    </div>
  );
}

/** Perceived-luminance check, so the frame's color-scheme follows any theme. */
function isDarkColor(hex: string): boolean {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return false;
  const n = parseInt(m[1], 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255 < 0.5;
}
