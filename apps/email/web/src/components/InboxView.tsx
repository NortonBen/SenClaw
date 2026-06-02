import { useEffect, useState } from 'react';
import {
  Input, Button, Empty, Spin, Typography, theme,
  Tooltip, Select, App, Avatar, Tag,
} from 'antd';
import {
  ReloadOutlined, EditOutlined, MailOutlined, CloudSyncOutlined,
  InboxOutlined, SettingOutlined, UserOutlined,
} from '@ant-design/icons';
import { api, type Account, type Email, type EmailDetail } from '../api';
import { ComposeModal } from './ComposeModal';

const { Text, Title, Paragraph } = Typography;
const { Search } = Input;

interface Props {
  /** Switch the host App to the Accounts tab. */
  onConfigure?: () => void;
}

function formatDate(ms: number | null) {
  if (!ms) return '';
  const d = new Date(ms);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString('vi', { hour: '2-digit', minute: '2-digit' });
  }
  if (d.getFullYear() === now.getFullYear()) {
    return d.toLocaleDateString('vi', { day: '2-digit', month: 'short' });
  }
  return d.toLocaleDateString('vi', { day: '2-digit', month: '2-digit', year: '2-digit' });
}

function isUnread(flags: string) {
  try {
    const arr: string[] = JSON.parse(flags);
    return !arr.includes('\\Seen');
  } catch {
    return false;
  }
}

/** "Name <a@b.com>" → "Name"; bare address → local part. */
function displayName(from: string | null): string {
  if (!from) return '(Không rõ)';
  const m = from.match(/^\s*"?([^"<]+?)"?\s*<.+>/);
  if (m) return m[1].trim();
  const addr = from.replace(/[<>]/g, '').trim();
  return addr.split('@')[0] || addr;
}

function avatarColor(seed: string): string {
  const palette = ['#2563eb', '#0891b2', '#7c3aed', '#db2777', '#ea580c', '#16a34a', '#ca8a04'];
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0;
  return palette[h % palette.length];
}

export function InboxView({ onConfigure }: Props) {
  const { token } = theme.useToken();
  const { message } = App.useApp();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [accountsLoading, setAccountsLoading] = useState(false);
  const [selectedAccountId, setSelectedAccountId] = useState<string | undefined>();
  const [emails, setEmails] = useState<Email[]>([]);
  const [emailsLoading, setEmailsLoading] = useState(false);
  const [displayList, setDisplayList] = useState<Email[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [searching, setSearching] = useState(false);
  const [selected, setSelected] = useState<EmailDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [showCompose, setShowCompose] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [replyTo, setReplyTo] = useState<{ to: string; subject: string } | undefined>();
  const [hoveredId, setHoveredId] = useState<string | null>(null);

  const loadAccounts = async () => {
    setAccountsLoading(true);
    try {
      const data = await api.listAccounts();
      setAccounts(Array.isArray(data) ? data : []);
    } catch {
      setAccounts([]);
    } finally {
      setAccountsLoading(false);
    }
  };

  const loadEmails = async (accountId?: string) => {
    setEmailsLoading(true);
    try {
      const data = await api.inbox(accountId);
      setEmails(Array.isArray(data) ? data : []);
    } catch {
      setEmails([]);
    } finally {
      setEmailsLoading(false);
    }
  };

  useEffect(() => { loadAccounts(); loadEmails(); }, []);
  useEffect(() => { setDisplayList(emails); }, [emails]);
  useEffect(() => {
    if (accounts.length > 0 && !selectedAccountId) setSelectedAccountId(accounts[0].id);
  }, [accounts, selectedAccountId]);
  useEffect(() => { if (selectedAccountId) loadEmails(selectedAccountId); }, [selectedAccountId]);

  const handleSearch = async (q: string) => {
    if (!q.trim()) { setDisplayList(emails); return; }
    setSearching(true);
    try {
      const res = await api.search(q, selectedAccountId);
      setDisplayList(res);
    } finally {
      setSearching(false);
    }
  };

  const handleSelect = async (email: Email) => {
    setLoadingDetail(true);
    try {
      setSelected(await api.read(email.id));
    } finally {
      setLoadingDetail(false);
    }
  };

  const handleSync = async () => {
    setSyncing(true);
    try {
      const res = await api.sync(selectedAccountId);
      message.success(`Đồng bộ ${res.synced} email`);
      await loadEmails(selectedAccountId);
    } catch (e) {
      message.error(e instanceof Error ? e.message : 'Đồng bộ thất bại');
    } finally {
      setSyncing(false);
    }
  };

  const handleReply = () => {
    if (!selected) return;
    setReplyTo({
      to: selected.from ?? '',
      subject: selected.subject?.startsWith('Re:') ? selected.subject : `Re: ${selected.subject ?? ''}`,
    });
    setShowCompose(true);
  };

  const handleSend = async (to: string, subject: string, body: string) => {
    await api.send(to, subject, body, selectedAccountId);
    await loadEmails(selectedAccountId);
  };

  const noAccounts = !accountsLoading && accounts.length === 0;
  const unreadCount = displayList.filter(e => isUnread(e.flags)).length;

  // ── Onboarding: no account configured ──────────────────────────────────────
  if (noAccounts) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 px-6 text-center">
        <div
          style={{
            width: 72, height: 72, borderRadius: 20,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            background: token.colorPrimaryBg, color: token.colorPrimary,
          }}
        >
          <MailOutlined style={{ fontSize: 34 }} />
        </div>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>Chào mừng đến với Email</Title>
          <Paragraph type="secondary" style={{ maxWidth: 420, margin: 0 }}>
            Thêm tài khoản IMAP/SMTP để xem hộp thư, tìm kiếm và soạn email ngay trong SenClaw.
          </Paragraph>
        </div>
        <Button type="primary" size="large" icon={<SettingOutlined />} onClick={onConfigure}>
          Thêm tài khoản
        </Button>
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div
        className="flex items-center gap-3 px-6 py-4 flex-shrink-0"
        style={{ borderBottom: `1px solid ${token.colorBorderSecondary}` }}
      >
        <Search
          placeholder="Tìm email..."
          allowClear
          value={searchQuery}
          onChange={e => { setSearchQuery(e.target.value); if (!e.target.value) setDisplayList(emails); }}
          onSearch={handleSearch}
          style={{ flex: 1, maxWidth: 480 }}
        />
        <div className="flex-1" />
        <Select
          placeholder="Tài khoản"
          value={selectedAccountId}
          loading={accountsLoading}
          style={{ width: 200 }}
          options={accounts.map(a => ({ value: a.id, label: a.label || a.email }))}
          onChange={setSelectedAccountId}
          disabled={accounts.length === 0}
        />
        <Tooltip title="Đồng bộ IMAP">
          <Button icon={<CloudSyncOutlined />} loading={syncing} onClick={handleSync} />
        </Tooltip>
        <Tooltip title="Làm mới">
          <Button icon={<ReloadOutlined />} loading={emailsLoading} onClick={() => loadEmails(selectedAccountId)} />
        </Tooltip>
        <Button type="primary" icon={<EditOutlined />} onClick={() => { setReplyTo(undefined); setShowCompose(true); }}>
          Soạn
        </Button>
      </div>

      {/* Split: list + detail */}
      <div className="flex flex-1 min-h-0">
        {/* Email list */}
        <div
          className="flex flex-col overflow-y-auto"
          style={{ width: 348, borderRight: `1px solid ${token.colorBorderSecondary}` }}
        >
          <div className="flex items-center justify-between px-5 pt-4 pb-2 flex-shrink-0">
            <Text strong style={{ fontSize: 12, letterSpacing: 0.4, textTransform: 'uppercase', color: token.colorTextSecondary }}>
              Hộp thư đến
            </Text>
            {unreadCount > 0 && <Tag color="blue" style={{ marginInlineEnd: 0 }}>{unreadCount} chưa đọc</Tag>}
          </div>

          {(emailsLoading || searching) && <div className="flex justify-center py-8"><Spin /></div>}

          {!emailsLoading && !searching && displayList.length === 0 && (
            <div className="flex flex-col items-center justify-center flex-1 gap-3 px-4">
              <Empty
                image={Empty.PRESENTED_IMAGE_SIMPLE}
                description={<Text type="secondary">Hộp thư trống</Text>}
              />
              <Button size="small" icon={<CloudSyncOutlined />} loading={syncing} onClick={handleSync}>
                Đồng bộ ngay
              </Button>
            </div>
          )}

          <div style={{ padding: '2px 12px 14px' }}>
            {displayList.map(email => {
              const unread = isUnread(email.flags);
              const active = selected?.id === email.id;
              const hovered = hoveredId === email.id;
              const name = displayName(email.from);
              return (
                <div
                  key={email.id}
                  onClick={() => handleSelect(email)}
                  onMouseEnter={() => setHoveredId(email.id)}
                  onMouseLeave={() => setHoveredId(null)}
                  className="flex gap-3.5 cursor-pointer"
                  style={{
                    padding: '13px 14px',
                    marginBottom: 6,
                    borderRadius: token.borderRadiusLG,
                    background: active ? token.colorPrimaryBg : hovered ? token.colorFillTertiary : 'transparent',
                    boxShadow: active ? `inset 0 0 0 1px ${token.colorPrimaryBorder}` : 'none',
                    transition: 'background 0.15s',
                  }}
                >
                  <Avatar
                    size={38}
                    style={{ backgroundColor: avatarColor(name), flexShrink: 0, fontSize: 16 }}
                  >
                    {name.charAt(0).toUpperCase() || <UserOutlined />}
                  </Avatar>
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center justify-between gap-2">
                      <Text strong={unread} ellipsis style={{ fontSize: 13.5, color: active ? token.colorPrimary : token.colorText }}>
                        {name}
                      </Text>
                      <Text type="secondary" style={{ fontSize: 11, flexShrink: 0 }}>{formatDate(email.date)}</Text>
                    </div>
                    <Text
                      ellipsis
                      style={{ display: 'block', fontSize: 12.5, marginTop: 3, color: unread ? token.colorText : token.colorTextSecondary, fontWeight: unread ? 600 : 400 }}
                    >
                      {email.subject ?? '(Không có tiêu đề)'}
                    </Text>
                  </div>
                  {unread && (
                    <span style={{ width: 8, height: 8, borderRadius: 8, background: token.colorPrimary, flexShrink: 0, marginTop: 7 }} />
                  )}
                </div>
              );
            })}
          </div>
        </div>

        {/* Email detail */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {loadingDetail && <div className="flex justify-center items-center h-full"><Spin /></div>}
          {!loadingDetail && !selected && (
            <div className="flex flex-col items-center justify-center h-full gap-3">
              <InboxOutlined style={{ fontSize: 56, color: token.colorTextQuaternary }} />
              <Text type="secondary">Chọn một email để đọc</Text>
            </div>
          )}
          {!loadingDetail && selected && (
            <div className="flex flex-col h-full overflow-auto">
              <div className="pt-8 pb-6" style={{ paddingInline: 36, borderBottom: `1px solid ${token.colorBorderSecondary}` }}>
                <Title level={4} style={{ marginBottom: 18 }}>{selected.subject ?? '(Không có tiêu đề)'}</Title>
                <div className="flex items-start gap-3">
                  <Avatar size={40} style={{ backgroundColor: avatarColor(displayName(selected.from)), flexShrink: 0 }}>
                    {displayName(selected.from).charAt(0).toUpperCase() || <UserOutlined />}
                  </Avatar>
                  <div className="flex-1 min-w-0">
                    <Text strong style={{ display: 'block' }}>{displayName(selected.from)}</Text>
                    <Text type="secondary" style={{ fontSize: 12 }}>{selected.from}</Text>
                    <div>
                      <Text type="secondary" style={{ fontSize: 12 }}>Đến: {selected.to}</Text>
                    </div>
                  </div>
                  <div className="flex flex-col items-end gap-2">
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {selected.date ? new Date(selected.date).toLocaleString('vi') : ''}
                    </Text>
                    <Button size="small" icon={<EditOutlined />} onClick={handleReply}>Trả lời</Button>
                  </div>
                </div>
              </div>
              <Paragraph
                className="whitespace-pre-wrap"
                style={{ color: token.colorText, fontSize: 14, lineHeight: 1.8, padding: '32px 36px', flex: 1, margin: 0 }}
              >
                {selected.body_text ?? '(Không có nội dung text)'}
              </Paragraph>
            </div>
          )}
        </div>
      </div>

      <ComposeModal
        open={showCompose}
        onClose={() => { setShowCompose(false); setReplyTo(undefined); }}
        onSend={handleSend}
        initialTo={replyTo?.to}
        initialSubject={replyTo?.subject}
      />
    </div>
  );
}
