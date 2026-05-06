import { useState, useCallback } from 'react';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface CodeSession {
  id: string;
  name: string;
  workspace: string;
  language: string | null;
  status: 'active' | 'archived';
  git_enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface CodeChatGroup {
  id: string;
  project_id: string;
  name: string;
  created_at: number;
  updated_at: number;
}

export interface CodeChatMessage {
  id: string;
  role: 'user' | 'agent';
  content: string;
  status: 'queued' | 'processing' | 'done' | 'failed';
  queue_position?: number | null;
  dag_plan?: string | null;
  created_at: number;
  processed_at?: number | null;
}

export interface FileNode {
  name: string;
  path: string;
  type: 'file' | 'dir';
  children?: FileNode[];
}

export interface GitCommit {
  hash: string;
  message: string;
  date: string;
}

export interface CodeChatResponse {
  ok: boolean;
  reply: string;
  parsed?: {
    command?: string | null;
    refs?: string[];
    skills?: string[];
    plain_text?: string;
    normalized_prompt?: string;
  };
  resolved_refs?: string[];
  dag_plan?: string;
  messages?: CodeChatMessage[];
  queued_preview?: CodeChatMessage[];
}

export interface StopChatTaskResponse {
  ok: boolean;
  action: 'stopped' | 'removed' | 'noop';
  target_id?: string | null;
}

export interface CreateSessionParams {
  name: string;
  workspace: string;
  language?: string;
  init_git?: boolean;
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

export function useCode() {
  const [sessions, setSessions] = useState<CodeSession[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSessions = useCallback(async (status = 'active') => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(`/api/code/sessions?status=${status}`);
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      setSessions(data.sessions ?? []);
    } catch (e: any) {
      setError(e.message);
    } finally {
      setLoading(false);
    }
  }, []);

  const createSession = useCallback(async (params: CreateSessionParams): Promise<CodeSession | null> => {
    setError(null);
    try {
      const res = await fetch('/api/code/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(params),
      });
      if (!res.ok) throw new Error(await res.text());
      const session: CodeSession = await res.json();
      setSessions(prev => [session, ...prev]);
      return session;
    } catch (e: any) {
      setError(e.message);
      return null;
    }
  }, []);

  const getSession = useCallback(async (id: string): Promise<CodeSession | null> => {
    try {
      const res = await fetch(`/api/code/sessions/${id}`);
      if (!res.ok) return null;
      return res.json();
    } catch {
      return null;
    }
  }, []);

  const archiveSession = useCallback(async (id: string): Promise<boolean> => {
    setError(null);
    try {
      const res = await fetch(`/api/code/sessions/${id}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      setSessions(prev => prev.filter(s => s.id !== id));
      return true;
    } catch (e: any) {
      setError(e.message);
      return false;
    }
  }, []);

  const getFiles = useCallback(async (id: string): Promise<{ workspace: string; tree: FileNode[] } | null> => {
    try {
      const res = await fetch(`/api/code/sessions/${id}/files`);
      if (!res.ok) return null;
      return res.json();
    } catch {
      return null;
    }
  }, []);

  const getGitLog = useCallback(async (id: string): Promise<GitCommit[]> => {
    try {
      const res = await fetch(`/api/code/sessions/${id}/git-log`);
      if (!res.ok) return [];
      const data = await res.json();
      return data.log ?? [];
    } catch {
      return [];
    }
  }, []);

  const getFileContent = useCallback(async (id: string, path: string): Promise<string | null> => {
    try {
      const res = await fetch(`/api/code/sessions/${id}/file-content?path=${encodeURIComponent(path)}`);
      if (!res.ok) return null;
      const data = await res.json();
      return typeof data.content === 'string' ? data.content : null;
    } catch {
      return null;
    }
  }, []);

  const rollback = useCallback(async (id: string, steps = 1): Promise<boolean> => {
    setError(null);
    try {
      const res = await fetch(`/api/code/sessions/${id}/rollback`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ steps }),
      });
      if (!res.ok) throw new Error(await res.text());
      return true;
    } catch (e: any) {
      setError(e.message);
      return false;
    }
  }, []);

  const sendChat = useCallback(async (id: string, groupId: string, prompt: string): Promise<CodeChatResponse | null> => {
    setError(null);
    try {
      const res = await fetch(`/api/code/sessions/${id}/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ prompt, group_id: groupId }),
      });
      if (!res.ok) throw new Error(await res.text());
      return res.json();
    } catch (e: any) {
      setError(e.message);
      return null;
    }
  }, []);

  const listChatGroups = useCallback(async (projectId: string): Promise<CodeChatGroup[]> => {
    try {
      const res = await fetch(`/api/code/projects/${projectId}/groups`);
      if (!res.ok) return [];
      const data = await res.json();
      return data.groups ?? [];
    } catch {
      return [];
    }
  }, []);

  const createChatGroup = useCallback(async (projectId: string, name: string): Promise<CodeChatGroup | null> => {
    setError(null);
    try {
      const res = await fetch(`/api/code/projects/${projectId}/groups`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      });
      if (!res.ok) throw new Error(await res.text());
      return res.json();
    } catch (e: any) {
      setError(e.message);
      return null;
    }
  }, []);

  const listGroupMessages = useCallback(async (groupId: string): Promise<CodeChatMessage[]> => {
    try {
      const res = await fetch(`/api/code/groups/${groupId}/messages`);
      if (!res.ok) return [];
      const data = await res.json();
      return data.messages ?? [];
    } catch {
      return [];
    }
  }, []);

  const stopCurrentChatTask = useCallback(async (groupId: string): Promise<StopChatTaskResponse | null> => {
    setError(null);
    try {
      const res = await fetch(`/api/code/groups/${groupId}/stop-current`, { method: 'POST' });
      if (!res.ok) throw new Error(await res.text());
      return res.json();
    } catch (e: any) {
      setError(e.message);
      return null;
    }
  }, []);

  return {
    sessions,
    loading,
    error,
    loadSessions,
    createSession,
    getSession,
    archiveSession,
    getFiles,
    getFileContent,
    getGitLog,
    rollback,
    sendChat,
    listChatGroups,
    createChatGroup,
    listGroupMessages,
    stopCurrentChatTask,
  };
}
