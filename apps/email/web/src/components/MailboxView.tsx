import { useCallback, useEffect, useMemo, useState } from 'react';
import { App, Button, Typography, theme } from 'antd';
import { MailOutlined, SettingOutlined } from '@ant-design/icons';
import { api, type Account, type Email, type EmailDetail, type Folder, type FolderCounts } from '../api';
import { isUnread, replySubject } from '../lib/mail';
import { Sidebar, type View } from './Sidebar';
import { MailList } from './MailList';
import { MailView } from './MailView';
import { AccountsView } from './AccountsView';
import { ComposeModal } from './ComposeModal';

const { Paragraph, Title } = Typography;

const EMPTY_COUNTS: FolderCounts = { inbox: 0, unread: 0, sent: 0 };

/** Which cache folder backs each sidebar view. */
const FOLDER_FOR: Record<Exclude<View, 'accounts'>, Folder> = {
  inbox: 'INBOX',
  unread: 'INBOX',
  sent: 'Sent',
};

const TITLE_FOR: Record<Exclude<View, 'accounts'>, string> = {
  inbox: 'Hộp thư đến',
  unread: 'Chưa đọc',
  sent: 'Đã gửi',
};

const EMPTY_FOR: Record<Exclude<View, 'accounts'>, string> = {
  inbox: 'Hộp thư trống',
  unread: 'Không có thư chưa đọc',
  sent: 'Chưa gửi thư nào',
};

export function MailboxView() {
  const { token } = theme.useToken();
  const { message } = App.useApp();

  const [view, setView] = useState<View>('inbox');
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [accountsLoading, setAccountsLoading] = useState(true);
  const [accountId, setAccountId] = useState<string | undefined>();
  const [counts, setCounts] = useState<FolderCounts>(EMPTY_COUNTS);

  const [emails, setEmails] = useState<Email[]>([]);
  const [listLoading, setListLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [searchResults, setSearchResults] = useState<Email[] | null>(null);

  const [selected, setSelected] = useState<EmailDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const [syncing, setSyncing] = useState(false);
  const [composeOpen, setComposeOpen] = useState(false);
  const [replyTo, setReplyTo] = useState<{ to: string; subject: string } | undefined>();

  const folder: Folder = view === 'accounts' ? 'INBOX' : FOLDER_FOR[view];

  const loadAccounts = useCallback(async () => {
    setAccountsLoading(true);
    try {
      const data = await api.listAccounts();
      const list = Array.isArray(data) ? data : [];
      setAccounts(list);
      setAccountId(prev => prev ?? list[0]?.id);
    } catch {
      setAccounts([]);
    } finally {
      setAccountsLoading(false);
    }
  }, []);

  const loadCounts = useCallback(async (id?: string) => {
    try {
      setCounts(await api.folders(id));
    } catch {
      setCounts(EMPTY_COUNTS);
    }
  }, []);

  const loadEmails = useCallback(async (id: string | undefined, f: Folder) => {
    setListLoading(true);
    try {
      const data = await api.inbox(id, f);
      setEmails(Array.isArray(data) ? data : []);
    } catch {
      setEmails([]);
    } finally {
      setListLoading(false);
    }
  }, []);

  useEffect(() => { loadAccounts(); }, [loadAccounts]);

  // Reload the list whenever the account or folder changes. Searching is a
  // separate overlay, so clear it here to avoid showing stale hits — and drop
  // the open message, which belongs to the folder we just left.
  useEffect(() => {
    if (view === 'accounts') return;
    setSearchResults(null);
    setQuery('');
    setSelected(null);
    loadEmails(accountId, folder);
    loadCounts(accountId);
  }, [accountId, folder, view, loadEmails, loadCounts]);

  const handleSearch = async (q: string) => {
    if (!q.trim()) { setSearchResults(null); return; }
    setListLoading(true);
    try {
      setSearchResults(await api.search(q, accountId));
    } catch (e) {
      message.error(e instanceof Error ? e.message : 'Tìm kiếm thất bại');
    } finally {
      setListLoading(false);
    }
  };

  const handleSelect = async (email: Email) => {
    setDetailLoading(true);
    try {
      const detail = await api.read(email.id);
      setSelected(detail);

      // Opening a message marks it read; reflect that locally right away rather
      // than refetching the whole list for one flag.
      if (isUnread(email.flags)) {
        try {
          const res = await api.markRead(email.id, true);
          const patch = (e: Email) => (e.id === email.id ? { ...e, flags: res.flags } : e);
          setEmails(prev => prev.map(patch));
          setSearchResults(prev => (prev ? prev.map(patch) : prev));
          setCounts(prev => ({ ...prev, unread: Math.max(0, prev.unread - 1) }));
        } catch { /* a failed flag write is not worth interrupting the read */ }
      }
    } catch (e) {
      message.error(e instanceof Error ? e.message : 'Không đọc được thư');
    } finally {
      setDetailLoading(false);
    }
  };

  const handleSync = async () => {
    setSyncing(true);
    try {
      const res = await api.sync(accountId);
      message.success(`Đã đồng bộ ${res.synced} thư`);
      await Promise.all([loadEmails(accountId, folder), loadCounts(accountId)]);
    } catch (e) {
      message.error(e instanceof Error ? e.message : 'Đồng bộ thất bại');
    } finally {
      setSyncing(false);
    }
  };

  const handleReply = () => {
    if (!selected) return;
    setReplyTo({ to: selected.from ?? '', subject: replySubject(selected.subject) });
    setComposeOpen(true);
  };

  const handleSend = async (to: string, subject: string, body: string) => {
    await api.send(to, subject, body, accountId);
    await Promise.all([loadEmails(accountId, folder), loadCounts(accountId)]);
  };

  // The Unread view is the inbox list minus anything already seen.
  const listed = useMemo(() => {
    const base = searchResults ?? emails;
    return view === 'unread' ? base.filter(e => isUnread(e.flags)) : base;
  }, [searchResults, emails, view]);

  const sidebar = (
    <Sidebar
      view={view}
      onViewChange={setView}
      accounts={accounts}
      accountsLoading={accountsLoading}
      selectedAccountId={accountId}
      onAccountChange={setAccountId}
      counts={counts}
      onCompose={() => { setReplyTo(undefined); setComposeOpen(true); }}
    />
  );

  const noAccounts = !accountsLoading && accounts.length === 0;

  return (
    <div style={{ display: 'flex', height: '100vh', minHeight: 0, background: token.colorBgContainer }}>
      {sidebar}

      {view === 'accounts' ? (
        <div style={{ flex: 1, minWidth: 0, overflowY: 'auto' }}>
          <AccountsView onChanged={() => { loadAccounts(); loadCounts(accountId); }} />
        </div>
      ) : noAccounts ? (
        <Onboarding onConfigure={() => setView('accounts')} />
      ) : (
        <>
          <MailList
            title={searchResults ? 'Kết quả tìm kiếm' : TITLE_FOR[view]}
            emails={listed}
            loading={listLoading}
            selectedId={selected?.id}
            onSelect={handleSelect}
            query={query}
            onQueryChange={q => { setQuery(q); if (!q) setSearchResults(null); }}
            onSearch={handleSearch}
            onRefresh={() => { loadEmails(accountId, folder); loadCounts(accountId); }}
            onSync={handleSync}
            syncing={syncing}
            showRecipient={view === 'sent'}
            emptyText={searchResults ? 'Không tìm thấy thư nào' : EMPTY_FOR[view]}
            canSync={view !== 'sent'}
          />
          <MailView email={selected} loading={detailLoading} onReply={handleReply} />
        </>
      )}

      <ComposeModal
        open={composeOpen}
        onClose={() => { setComposeOpen(false); setReplyTo(undefined); }}
        onSend={handleSend}
        initialTo={replyTo?.to}
        initialSubject={replyTo?.subject}
      />
    </div>
  );
}

function Onboarding({ onConfigure }: { onConfigure: () => void }) {
  const { token } = theme.useToken();
  return (
    <div
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 16,
        padding: 24,
        textAlign: 'center',
      }}
    >
      <div
        style={{
          width: 76, height: 76, borderRadius: 22,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          background: token.colorPrimaryBg, color: token.colorPrimary,
        }}
      >
        <MailOutlined style={{ fontSize: 36 }} />
      </div>
      <div>
        <Title level={4} style={{ marginBottom: 6 }}>Chào mừng đến với Email</Title>
        <Paragraph type="secondary" style={{ maxWidth: 440, margin: 0 }}>
          Thêm tài khoản IMAP/SMTP để đọc, tìm kiếm và soạn thư ngay trong SenClaw.
        </Paragraph>
      </div>
      <Button type="primary" size="large" icon={<SettingOutlined />} onClick={onConfigure}>
        Thêm tài khoản
      </Button>
    </div>
  );
}
