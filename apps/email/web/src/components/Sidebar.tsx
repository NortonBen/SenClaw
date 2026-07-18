import { Badge, Button, Select, Tooltip, Typography, theme } from 'antd';
import {
  EditOutlined, InboxOutlined, MailOutlined, SendOutlined,
  SettingOutlined, StarOutlined,
} from '@ant-design/icons';
import type { Account, FolderCounts } from '../api';

const { Text } = Typography;

/** Which list the middle pane is showing. */
export type View = 'inbox' | 'unread' | 'sent' | 'accounts';

interface Props {
  view: View;
  onViewChange: (v: View) => void;
  accounts: Account[];
  accountsLoading: boolean;
  selectedAccountId?: string;
  onAccountChange: (id: string) => void;
  counts: FolderCounts;
  onCompose: () => void;
}

const NAV: { key: View; label: string; icon: React.ReactNode; count: keyof FolderCounts }[] = [
  { key: 'inbox', label: 'Hộp thư đến', icon: <InboxOutlined />, count: 'inbox' },
  { key: 'unread', label: 'Chưa đọc', icon: <StarOutlined />, count: 'unread' },
  { key: 'sent', label: 'Đã gửi', icon: <SendOutlined />, count: 'sent' },
];

export function Sidebar({
  view, onViewChange, accounts, accountsLoading,
  selectedAccountId, onAccountChange, counts, onCompose,
}: Props) {
  const { token } = theme.useToken();

  return (
    <div
      style={{
        width: 236,
        flexShrink: 0,
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        padding: 12,
        background: token.colorBgLayout,
        borderRight: `1px solid ${token.colorBorderSecondary}`,
      }}
    >
      {/* Brand */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 9, padding: '6px 8px 12px' }}>
        <MailOutlined style={{ fontSize: 18, color: token.colorPrimary }} />
        <Text strong style={{ fontSize: 15 }}>Email</Text>
      </div>

      <Button
        type="primary"
        size="large"
        icon={<EditOutlined />}
        onClick={onCompose}
        disabled={accounts.length === 0}
        style={{ marginBottom: 12, height: 42, fontWeight: 600 }}
        block
      >
        Soạn thư
      </Button>

      {/* Account switcher — hidden until there's a choice to make. */}
      {accounts.length > 0 && (
        <div style={{ marginBottom: 8 }}>
          <Select
            value={selectedAccountId}
            loading={accountsLoading}
            onChange={onAccountChange}
            style={{ width: '100%' }}
            options={accounts.map(a => ({
              value: a.id,
              label: a.label || a.email,
            }))}
            optionRender={o => {
              const acct = accounts.find(a => a.id === o.value);
              return (
                <div style={{ lineHeight: 1.3, padding: '2px 0' }}>
                  <div style={{ fontWeight: 500 }}>{acct?.label || acct?.email}</div>
                  {acct?.label && (
                    <div style={{ fontSize: 11, color: token.colorTextTertiary }}>{acct.email}</div>
                  )}
                </div>
              );
            }}
          />
        </div>
      )}

      {/* Folders */}
      <nav style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
        {NAV.map(item => {
          const active = view === item.key;
          const n = counts[item.count];
          return (
            <button
              key={item.key}
              onClick={() => onViewChange(item.key)}
              className="email-nav-item"
              data-active={active || undefined}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 10,
                width: '100%',
                padding: '8px 10px',
                border: 'none',
                borderRadius: token.borderRadius,
                cursor: 'pointer',
                textAlign: 'left',
                fontSize: 13.5,
                fontWeight: active ? 600 : 450,
                background: active ? token.colorPrimaryBg : 'transparent',
                color: active ? token.colorPrimary : token.colorText,
                transition: 'background 0.15s',
              }}
            >
              <span style={{ fontSize: 15, display: 'flex' }}>{item.icon}</span>
              <span style={{ flex: 1 }}>{item.label}</span>
              {n > 0 && (
                <Badge
                  count={n}
                  overflowCount={999}
                  style={{
                    background: item.key === 'unread' ? token.colorPrimary : token.colorFillSecondary,
                    color: item.key === 'unread' ? '#fff' : token.colorTextSecondary,
                    fontSize: 11,
                    fontWeight: 600,
                    boxShadow: 'none',
                  }}
                />
              )}
            </button>
          );
        })}
      </nav>

      <div style={{ flex: 1 }} />

      <Tooltip title="Quản lý tài khoản IMAP/SMTP" placement="right">
        <button
          onClick={() => onViewChange('accounts')}
          className="email-nav-item"
          data-active={view === 'accounts' || undefined}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            width: '100%',
            padding: '8px 10px',
            border: 'none',
            borderRadius: token.borderRadius,
            cursor: 'pointer',
            textAlign: 'left',
            fontSize: 13.5,
            fontWeight: view === 'accounts' ? 600 : 450,
            background: view === 'accounts' ? token.colorPrimaryBg : 'transparent',
            color: view === 'accounts' ? token.colorPrimary : token.colorTextSecondary,
          }}
        >
          <SettingOutlined style={{ fontSize: 15 }} />
          <span style={{ flex: 1 }}>Tài khoản</span>
        </button>
      </Tooltip>
    </div>
  );
}
