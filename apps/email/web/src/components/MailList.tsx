import { Avatar, Button, Empty, Input, Spin, Tooltip, Typography, theme } from 'antd';
import { CloudSyncOutlined, ReloadOutlined } from '@ant-design/icons';
import type { Email } from '../api';
import { avatarColor, displayName, formatListDate, isUnread } from '../lib/mail';

const { Text } = Typography;

interface Props {
  title: string;
  emails: Email[];
  loading: boolean;
  selectedId?: string;
  onSelect: (email: Email) => void;
  query: string;
  onQueryChange: (q: string) => void;
  onSearch: (q: string) => void;
  onRefresh: () => void;
  onSync: () => void;
  syncing: boolean;
  /** Show the recipient instead of the sender (Sent folder). */
  showRecipient?: boolean;
  emptyText: string;
  /** Sync pulls from the server's INBOX, so hide it where it can't help. */
  canSync?: boolean;
}

export function MailList({
  title, emails, loading, selectedId, onSelect,
  query, onQueryChange, onSearch, onRefresh, onSync, syncing,
  showRecipient, emptyText, canSync = true,
}: Props) {
  const { token } = theme.useToken();

  return (
    <div
      style={{
        width: 380,
        flexShrink: 0,
        display: 'flex',
        flexDirection: 'column',
        minHeight: 0,
        borderRight: `1px solid ${token.colorBorderSecondary}`,
        background: token.colorBgContainer,
      }}
    >
      {/* Search + actions */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          padding: '10px 12px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        <Input.Search
          placeholder="Tìm thư…"
          allowClear
          value={query}
          onChange={e => onQueryChange(e.target.value)}
          onSearch={onSearch}
          style={{ flex: 1 }}
        />
        {canSync && (
          <Tooltip title="Tải thư mới từ máy chủ">
            <Button icon={<CloudSyncOutlined />} loading={syncing} onClick={onSync} />
          </Tooltip>
        )}
        <Tooltip title="Làm mới danh sách">
          <Button icon={<ReloadOutlined />} loading={loading} onClick={onRefresh} />
        </Tooltip>
      </div>

      {/* Column heading */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '10px 16px 6px',
        }}
      >
        <Text
          strong
          style={{
            fontSize: 11,
            letterSpacing: 0.6,
            textTransform: 'uppercase',
            color: token.colorTextTertiary,
          }}
        >
          {title}
        </Text>
        <Text style={{ fontSize: 11, color: token.colorTextQuaternary }}>
          {emails.length > 0 && `${emails.length} thư`}
        </Text>
      </div>

      {/* Rows */}
      <div style={{ flex: 1, minHeight: 0, overflowY: 'auto', padding: '0 8px 12px' }}>
        {loading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 32 }}>
            <Spin />
          </div>
        )}

        {!loading && emails.length === 0 && (
          <div
            style={{
              display: 'flex', flexDirection: 'column', alignItems: 'center',
              justifyContent: 'center', gap: 12, paddingTop: 48,
            }}
          >
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={<Text type="secondary" style={{ fontSize: 13 }}>{emptyText}</Text>}
            />
            {canSync && (
              <Button size="small" icon={<CloudSyncOutlined />} loading={syncing} onClick={onSync}>
                Đồng bộ ngay
              </Button>
            )}
          </div>
        )}

        {!loading && emails.map(email => {
          // Sent mail is identified by who it went to; received mail by who sent it.
          const unread = isUnread(email.flags);
          const active = selectedId === email.id;
          const label = showRecipient ? displayName(email.to) : displayName(email.from);

          return (
            <div
              key={email.id}
              role="button"
              tabIndex={0}
              aria-pressed={active}
              aria-label={`${label}: ${email.subject || '(Không có tiêu đề)'}`}
              onClick={() => onSelect(email)}
              onKeyDown={e => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  onSelect(email);
                }
              }}
              className="email-row"
              data-active={active || undefined}
              style={{
                display: 'flex',
                gap: 11,
                padding: '10px 10px',
                marginBottom: 2,
                borderRadius: token.borderRadius,
                cursor: 'pointer',
                background: active ? token.colorPrimaryBg : 'transparent',
                boxShadow: active ? `inset 0 0 0 1px ${token.colorPrimaryBorder}` : 'none',
              }}
            >
              {/* Unread rail */}
              <span
                style={{
                  width: 3,
                  borderRadius: 3,
                  flexShrink: 0,
                  background: unread ? token.colorPrimary : 'transparent',
                }}
              />
              <Avatar
                size={34}
                style={{
                  backgroundColor: avatarColor(label),
                  flexShrink: 0,
                  fontSize: 14,
                  fontWeight: 600,
                }}
              >
                {label.charAt(0).toUpperCase()}
              </Avatar>

              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
                  <Text
                    ellipsis
                    style={{
                      flex: 1,
                      fontSize: 13,
                      fontWeight: unread ? 700 : 500,
                      color: active ? token.colorPrimary : token.colorText,
                    }}
                  >
                    {label}
                  </Text>
                  <Text
                    style={{
                      flexShrink: 0,
                      fontSize: 11,
                      fontVariantNumeric: 'tabular-nums',
                      color: unread ? token.colorPrimary : token.colorTextQuaternary,
                      fontWeight: unread ? 600 : 400,
                    }}
                  >
                    {formatListDate(email.date)}
                  </Text>
                </div>

                <Text
                  ellipsis
                  style={{
                    display: 'block',
                    fontSize: 12.5,
                    marginTop: 2,
                    fontWeight: unread ? 600 : 400,
                    color: unread ? token.colorText : token.colorTextSecondary,
                  }}
                >
                  {email.subject || '(Không có tiêu đề)'}
                </Text>

                {email.snippet && (
                  <Text
                    ellipsis
                    style={{
                      display: 'block',
                      fontSize: 11.5,
                      marginTop: 2,
                      color: token.colorTextQuaternary,
                    }}
                  >
                    {email.snippet}
                  </Text>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
