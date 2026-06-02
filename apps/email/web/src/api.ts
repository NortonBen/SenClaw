// Typed fetch client for the Email App's own backend (/api/*).

export interface Email {
  id: string;
  account_id: string;
  subject: string | null;
  from: string | null;
  date: number | null;
  flags: string;
}

export interface EmailDetail extends Email {
  to: string | null;
  body_text: string | null;
  body_html: string | null;
}

export interface Account {
  id: string;
  label: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  use_tls: boolean;
  created_at: number;
}

export interface AccountCreate {
  label: string;
  email: string;
  imap_host: string;
  imap_port: number;
  smtp_host: string;
  smtp_port: number;
  username: string;
  password: string;
  use_tls: boolean;
}

async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  });
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    let msg = text;
    try {
      const j = JSON.parse(text);
      msg = j.error ?? text;
    } catch { /* keep raw text */ }
    throw new Error(msg || `${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

export const api = {
  listAccounts: () => apiFetch<Account[]>('/api/accounts'),
  createAccount: (payload: AccountCreate) =>
    apiFetch<Account>('/api/accounts', { method: 'POST', body: JSON.stringify(payload) }),
  deleteAccount: (id: string) =>
    apiFetch<{ success: boolean }>(`/api/accounts/${encodeURIComponent(id)}`, { method: 'DELETE' }),

  inbox: (accountId?: string, limit = 50) => {
    const qs = new URLSearchParams();
    if (accountId) qs.set('account_id', accountId);
    qs.set('limit', String(limit));
    return apiFetch<Email[]>(`/api/inbox?${qs.toString()}`);
  },
  read: (id: string) => apiFetch<EmailDetail>(`/api/messages/${encodeURIComponent(id)}`),
  search: (q: string, accountId?: string) => {
    const qs = new URLSearchParams({ q });
    if (accountId) qs.set('account_id', accountId);
    return apiFetch<Email[]>(`/api/search?${qs.toString()}`);
  },
  send: (to: string, subject: string, body: string, accountId?: string) =>
    apiFetch<{ success: boolean; message_id: string }>('/api/send', {
      method: 'POST',
      body: JSON.stringify({ to, subject, body, account_id: accountId }),
    }),
  draft: (prompt: string) =>
    apiFetch<{ subject: string; body: string }>('/api/draft', {
      method: 'POST',
      body: JSON.stringify({ prompt }),
    }),
  sync: (accountId?: string, limit = 30) =>
    apiFetch<{ success: boolean; synced: number }>('/api/sync', {
      method: 'POST',
      body: JSON.stringify({ account_id: accountId, limit }),
    }),
};
