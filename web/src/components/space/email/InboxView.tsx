import React, { useEffect, useState } from 'react';
import {
  Input, Button, Tag, Empty, Spin, Typography, theme, Drawer,
  Tooltip, Divider, Alert,
} from 'antd';
import {
  SearchOutlined, ReloadOutlined, EditOutlined, MailOutlined,
  StarOutlined, PaperClipOutlined,
} from '@ant-design/icons';
import type { SpaceEmail, SpaceEmailDetail, UseSpaceHook } from '../../../hooks/useSpace';
import { ComposeModal } from './ComposeModal';

const { Text, Title, Paragraph } = Typography;
const { Search } = Input;

interface Props {
  hook: UseSpaceHook;
}

function formatDate(ms: number | null) {
  if (!ms) return '';
  const d = new Date(ms);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString('vi', { hour: '2-digit', minute: '2-digit' });
  }
  return d.toLocaleDateString('vi', { day: '2-digit', month: '2-digit' });
}

function isUnread(flags: string) {
  try {
    const arr: string[] = JSON.parse(flags);
    return !arr.includes('\\Seen');
  } catch {
    return false;
  }
}

export function InboxView({ hook }: Props) {
  const { token } = theme.useToken();
  const [searchQuery, setSearchQuery] = useState('');
  const [displayList, setDisplayList] = useState<SpaceEmail[]>([]);
  const [searching, setSearching] = useState(false);
  const [selected, setSelected] = useState<SpaceEmailDetail | null>(null);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [showCompose, setShowCompose] = useState(false);
  const [replyTo, setReplyTo] = useState<{ to: string; subject: string } | undefined>();

  useEffect(() => {
    hook.loadEmails();
  }, []);

  useEffect(() => {
    setDisplayList(hook.emails ?? []);
  }, [hook.emails]);

  const handleSearch = async (q: string) => {
    if (!q.trim()) { setDisplayList(hook.emails); return; }
    setSearching(true);
    const res = await hook.searchEmails(q);
    setDisplayList(res);
    setSearching(false);
  };

  const handleSelect = async (email: SpaceEmail) => {
    setLoadingDetail(true);
    const detail = await hook.readEmail(email.id);
    setSelected(detail);
    setLoadingDetail(false);
  };

  const handleReply = () => {
    if (!selected) return;
    setReplyTo({
      to: selected.from ?? '',
      subject: selected.subject?.startsWith('Re:')
        ? selected.subject
        : `Re: ${selected.subject ?? ''}`,
    });
    setShowCompose(true);
  };

  const handleSend = async (to: string, subject: string, body: string) => {
    await fetch('/api/space/email/send', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ to, subject, body }),
    });
  };

  const noAccounts = !hook.emailsLoading && (hook.emails ?? []).length === 0;

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Search
          placeholder="Tìm email..."
          allowClear
          size="small"
          value={searchQuery}
          onChange={e => { setSearchQuery(e.target.value); if (!e.target.value) setDisplayList(hook.emails); }}
          onSearch={handleSearch}
          className="flex-1"
        />
        <Tooltip title="Làm mới">
          <Button
            size="small"
            icon={<ReloadOutlined />}
            loading={hook.emailsLoading}
            onClick={() => hook.loadEmails()}
          />
        </Tooltip>
        <Button
          type="primary"
          size="small"
          icon={<EditOutlined />}
          onClick={() => { setReplyTo(undefined); setShowCompose(true); }}
        >
          Soạn
        </Button>
      </div>

      {/* No account banner */}
      {noAccounts && (
        <Alert
          type="info"
          className="m-4"
          title="Chưa cấu hình tài khoản email"
          description="Vào Settings → Space → Email để thêm tài khoản IMAP/SMTP."
          showIcon
        />
      )}

      {/* Split: list + detail */}
      <div className="flex flex-1 min-h-0">
        {/* Email list */}
        <div
          className="flex flex-col border-r overflow-y-auto"
          style={{ width: 320, borderColor: token.colorBorderSecondary }}
        >
          {(hook.emailsLoading || searching) && (
            <div className="flex justify-center py-6"><Spin /></div>
          )}
          {!hook.emailsLoading && !searching && displayList.length === 0 && !noAccounts && (
            <Empty description="Hộp thư trống" className="py-8" />
          )}
          {displayList.map(email => {
            const unread = isUnread(email.flags);
            const active = selected?.id === email.id;
            return (
              <div
                key={email.id}
                onClick={() => handleSelect(email)}
                className="px-3 py-2.5 border-b cursor-pointer"
                style={{
                  borderColor: token.colorBorderSecondary,
                  background: active ? token.colorPrimaryBg : unread ? token.colorFillSecondary : 'transparent',
                }}
              >
                <div className="flex items-center justify-between gap-1">
                  <Text
                    strong={unread}
                    ellipsis
                    className="text-sm flex-1"
                    style={{ color: active ? token.colorPrimary : token.colorText }}
                  >
                    {email.from ?? '(Không rõ)'}
                  </Text>
                  <Text type="secondary" style={{ fontSize: 10, flexShrink: 0 }}>
                    {formatDate(email.date)}
                  </Text>
                </div>
                <Text
                  ellipsis
                  className="text-xs block mt-0.5"
                  style={{ color: unread ? token.colorText : token.colorTextSecondary, fontWeight: unread ? 600 : 400 }}
                >
                  {email.subject ?? '(Không có tiêu đề)'}
                </Text>
              </div>
            );
          })}
        </div>

        {/* Email detail */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {loadingDetail && (
            <div className="flex justify-center items-center h-full"><Spin /></div>
          )}
          {!loadingDetail && !selected && (
            <div className="flex flex-col items-center justify-center h-full gap-2">
              <MailOutlined style={{ fontSize: 48, color: token.colorTextQuaternary }} />
              <Text type="secondary">Chọn email để đọc</Text>
            </div>
          )}
          {!loadingDetail && selected && (
            <div className="flex flex-col h-full overflow-auto px-6 py-4">
              <Title level={5} className="mb-1">{selected.subject ?? '(Không có tiêu đề)'}</Title>
              <div className="flex flex-wrap gap-x-4 gap-y-1 mb-3">
                <Text type="secondary" className="text-xs">
                  <span className="font-medium">Từ:</span> {selected.from}
                </Text>
                <Text type="secondary" className="text-xs">
                  <span className="font-medium">Đến:</span> {selected.to}
                </Text>
                <Text type="secondary" className="text-xs">
                  {selected.date ? new Date(selected.date).toLocaleString('vi') : ''}
                </Text>
              </div>

              <div className="flex gap-2 mb-4">
                <Button size="small" icon={<EditOutlined />} onClick={handleReply}>
                  Trả lời
                </Button>
              </div>

              <Divider className="my-2" />

              <Paragraph
                className="text-sm whitespace-pre-wrap flex-1"
                style={{ color: token.colorText }}
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
