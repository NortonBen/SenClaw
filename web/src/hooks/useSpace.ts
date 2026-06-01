import { useState, useCallback, useEffect } from 'react';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface SpaceNote {
  id: string;
  title: string;
  body: string;
  tags: string[];
  folder_id: string | null;
  pinned: boolean;
  created_at: number;
  updated_at: number;
}

export interface SpaceEvent {
  id: string;
  title: string;
  description: string | null;
  start_at: number;
  end_at: number;
  all_day: boolean;
  location: string | null;
  color: string | null;
  reminder_min: number | null;
  renotify_min: number | null;
  /** 'upcoming' | 'ongoing' | 'done' | 'cancelled' */
  status: string;
  source: string;
}

export interface SpaceEmail {
  id: string;
  account_id: string;
  subject: string | null;
  from: string | null;
  date: number | null;
  flags: string;
}

export interface SpaceEmailDetail extends SpaceEmail {
  to: string | null;
  body_text: string | null;
}

export interface SpaceEmailAccount {
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

export interface SpaceEmailAccountCreate {
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

export interface SpaceSchedule {
  id: string;
  prompt: string;
  schedule_type: string;
  schedule_value: string;
  status: string;
  next_run: string | null;
  last_run: string | null;
  created_at: string;
}

export interface TodaySummary {
  date: string;
  events: SpaceEvent[];
  recent_notes: SpaceNote[];
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

export interface UseSpaceHook {
  // Notes
  notes: SpaceNote[];
  notesLoading: boolean;
  loadNotes: (tag?: string) => Promise<void>;
  createNote: (title: string, body: string, tags?: string[]) => Promise<SpaceNote | null>;
  updateNote: (id: string, patch: Partial<Pick<SpaceNote, 'title' | 'body' | 'tags'>>) => Promise<void>;
  deleteNote: (id: string) => Promise<void>;
  searchNotes: (q: string) => Promise<SpaceNote[]>;

  // Calendar
  events: SpaceEvent[];
  eventsLoading: boolean;
  loadEvents: (from: number, to: number) => Promise<void>;
  createEvent: (payload: Omit<SpaceEvent, 'id' | 'source'>) => Promise<SpaceEvent | null>;
  updateEvent: (id: string, patch: Partial<Omit<SpaceEvent, 'id' | 'source'>>) => Promise<void>;
  deleteEvent: (id: string) => Promise<void>;
  todaySummary: TodaySummary | null;
  loadTodaySummary: () => Promise<void>;

  // Email
  emails: SpaceEmail[];
  emailsLoading: boolean;
  emailAccounts: SpaceEmailAccount[];
  emailAccountsLoading: boolean;
  loadEmails: (accountId?: string) => Promise<void>;
  readEmail: (id: string) => Promise<SpaceEmailDetail | null>;
  searchEmails: (q: string) => Promise<SpaceEmail[]>;
  loadEmailAccounts: () => Promise<void>;
  createEmailAccount: (payload: SpaceEmailAccountCreate) => Promise<SpaceEmailAccount | null>;
  deleteEmailAccount: (id: string) => Promise<void>;

  // Schedules
  schedules: SpaceSchedule[];
  schedulesLoading: boolean;
  loadSchedules: (groupFolder: string) => Promise<void>;
  createSchedule: (prompt: string, cron: string, groupFolder: string, chatJid: string) => Promise<void>;
  cancelSchedule: (id: string, groupFolder: string) => Promise<void>;
}

async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, { headers: { 'Content-Type': 'application/json' }, ...opts });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
  return res.json() as Promise<T>;
}

export function useSpace(): UseSpaceHook {
  const [notes, setNotes] = useState<SpaceNote[]>([]);
  const [notesLoading, setNotesLoading] = useState(false);
  const [events, setEvents] = useState<SpaceEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [emails, setEmails] = useState<SpaceEmail[]>([]);
  const [emailsLoading, setEmailsLoading] = useState(false);
  const [emailAccounts, setEmailAccounts] = useState<SpaceEmailAccount[]>([]);
  const [emailAccountsLoading, setEmailAccountsLoading] = useState(false);
  const [schedules, setSchedules] = useState<SpaceSchedule[]>([]);
  const [schedulesLoading, setSchedulesLoading] = useState(false);
  const [todaySummary, setTodaySummary] = useState<TodaySummary | null>(null);

  // ── Notes ──────────────────────────────────────────────────────────────────

  const loadNotes = useCallback(async (tag?: string) => {
    setNotesLoading(true);
    try {
      const qs = tag ? `?tag=${encodeURIComponent(tag)}` : '';
      const data = await apiFetch<SpaceNote[]>(`/api/space/notes${qs}`);
      setNotes(Array.isArray(data) ? data : []);
    } catch {
      setNotes([]);
    } finally {
      setNotesLoading(false);
    }
  }, []);

  const createNote = useCallback(async (title: string, body: string, tags?: string[]) => {
    try {
      const data = await apiFetch<SpaceNote>('/api/space/notes', {
        method: 'POST',
        body: JSON.stringify({ title, body, tags: tags ?? [] }),
      });
      setNotes(prev => [data, ...prev]);
      return data;
    } catch {
      return null;
    }
  }, []);

  const updateNote = useCallback(async (id: string, patch: Partial<Pick<SpaceNote, 'title' | 'body' | 'tags'>>) => {
    try {
      await apiFetch(`/api/space/notes/${id}`, { method: 'PUT', body: JSON.stringify(patch) });
      setNotes(prev => prev.map(n => n.id === id ? { ...n, ...patch } : n));
    } catch {}
  }, []);

  const deleteNote = useCallback(async (id: string) => {
    try {
      await apiFetch(`/api/space/notes/${id}`, { method: 'DELETE' });
      setNotes(prev => prev.filter(n => n.id !== id));
    } catch {}
  }, []);

  const searchNotes = useCallback(async (q: string): Promise<SpaceNote[]> => {
    try {
      return await apiFetch<SpaceNote[]>(`/api/space/notes/search?q=${encodeURIComponent(q)}`);
    } catch {
      return [];
    }
  }, []);

  // ── Calendar ───────────────────────────────────────────────────────────────

  const loadEvents = useCallback(async (from: number, to: number) => {
    setEventsLoading(true);
    try {
      const data = await apiFetch<SpaceEvent[]>(`/api/space/calendar/events?from=${from}&to=${to}`);
      setEvents(Array.isArray(data) ? data : []);
    } catch {
      setEvents([]);
    } finally {
      setEventsLoading(false);
    }
  }, []);

  const loadTodaySummary = useCallback(async () => {
    try {
      const data = await apiFetch<TodaySummary>('/api/space/calendar/today');
      setTodaySummary(data);
    } catch {}
  }, []);

  const createEvent = useCallback(async (payload: Omit<SpaceEvent, 'id' | 'source'>) => {
    try {
      const data = await apiFetch<SpaceEvent>('/api/space/calendar/events', {
        method: 'POST',
        body: JSON.stringify(payload),
      });
      setEvents(prev => [...prev, data].sort((a, b) => a.start_at - b.start_at));
      return data;
    } catch {
      return null;
    }
  }, []);

  const updateEvent = useCallback(async (id: string, patch: Partial<Omit<SpaceEvent, 'id' | 'source'>>) => {
    try {
      await apiFetch(`/api/space/calendar/events/${id}`, { method: 'PATCH', body: JSON.stringify(patch) });
      setEvents(prev => prev.map(e => e.id === id ? { ...e, ...patch } : e));
    } catch {}
  }, []);

  const deleteEvent = useCallback(async (id: string) => {
    try {
      await apiFetch(`/api/space/calendar/events/${id}`, { method: 'DELETE' });
      setEvents(prev => prev.filter(e => e.id !== id));
    } catch {}
  }, []);

  // ── Email ──────────────────────────────────────────────────────────────────

  const loadEmails = useCallback(async (accountId?: string) => {
    setEmailsLoading(true);
    try {
      const qs = accountId ? `?account_id=${encodeURIComponent(accountId)}` : '';
      const data = await apiFetch<SpaceEmail[]>(`/api/space/email/inbox${qs}`);
      setEmails(Array.isArray(data) ? data : []);
    } catch {
      setEmails([]);
    } finally {
      setEmailsLoading(false);
    }
  }, []);

  const readEmail = useCallback(async (id: string): Promise<SpaceEmailDetail | null> => {
    try {
      return await apiFetch<SpaceEmailDetail>(`/api/space/email/messages/${id}`);
    } catch {
      return null;
    }
  }, []);

  const searchEmails = useCallback(async (q: string): Promise<SpaceEmail[]> => {
    try {
      return await apiFetch<SpaceEmail[]>(`/api/space/email/search?q=${encodeURIComponent(q)}`);
    } catch {
      return [];
    }
  }, []);

  const loadEmailAccounts = useCallback(async () => {
    setEmailAccountsLoading(true);
    try {
      const data = await apiFetch<SpaceEmailAccount[]>('/api/space/email/accounts');
      setEmailAccounts(Array.isArray(data) ? data : []);
    } catch {
      setEmailAccounts([]);
    } finally {
      setEmailAccountsLoading(false);
    }
  }, []);

  const createEmailAccount = useCallback(async (payload: SpaceEmailAccountCreate): Promise<SpaceEmailAccount | null> => {
    try {
      const data = await apiFetch<SpaceEmailAccount>('/api/space/email/accounts', {
        method: 'POST',
        body: JSON.stringify(payload),
      });
      await loadEmailAccounts();
      return data;
    } catch {
      return null;
    }
  }, [loadEmailAccounts]);

  const deleteEmailAccount = useCallback(async (id: string) => {
    try {
      await apiFetch(`/api/space/email/accounts/${encodeURIComponent(id)}`, { method: 'DELETE' });
      setEmailAccounts(prev => prev.filter(a => a.id !== id));
      setEmails(prev => prev.filter(e => e.account_id !== id));
    } catch {}
  }, []);

  // ── Schedules ──────────────────────────────────────────────────────────────

  const loadSchedules = useCallback(async (groupFolder: string) => {
    setSchedulesLoading(true);
    try {
      const data = await apiFetch<SpaceSchedule[]>(`/api/space/schedules?group=${encodeURIComponent(groupFolder)}`);
      setSchedules(Array.isArray(data) ? data : []);
    } catch {
      setSchedules([]);
    } finally {
      setSchedulesLoading(false);
    }
  }, []);

  const createSchedule = useCallback(async (prompt: string, cron: string, groupFolder: string, chatJid: string) => {
    try {
      const data = await apiFetch<SpaceSchedule>('/api/space/schedules', {
        method: 'POST',
        body: JSON.stringify({ prompt, cron, group_folder: groupFolder, chat_jid: chatJid }),
      });
      setSchedules(prev => [...prev, data]);
    } catch {}
  }, []);

  const cancelSchedule = useCallback(async (id: string, groupFolder: string) => {
    try {
      await apiFetch(`/api/space/schedules/${id}`, {
        method: 'DELETE',
        body: JSON.stringify({ group_folder: groupFolder }),
      });
      setSchedules(prev => prev.filter(s => s.id !== id));
    } catch {}
  }, []);

  // Load today summary on mount
  useEffect(() => {
    loadTodaySummary();
    loadEmailAccounts();
  }, [loadTodaySummary, loadEmailAccounts]);

  return {
    notes, notesLoading, loadNotes, createNote, updateNote, deleteNote, searchNotes,
    events, eventsLoading, loadEvents, createEvent, updateEvent, deleteEvent, todaySummary, loadTodaySummary,
    emails, emailsLoading, emailAccounts, emailAccountsLoading,
    loadEmails, readEmail, searchEmails, loadEmailAccounts, createEmailAccount, deleteEmailAccount,
    schedules, schedulesLoading, loadSchedules, createSchedule, cancelSchedule,
  };
}
