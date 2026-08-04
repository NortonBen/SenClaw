// api.ts — the single data layer for the UI. Every operation runs in the Rust
// backend (`/api/*`), the same engine the MCP tools use, so a person and an
// agent always see identical results.

export type ApiResult = Record<string, unknown> & {
  ok: boolean;
  error?: string;
};

async function request(path: string, init?: RequestInit): Promise<ApiResult> {
  try {
    const res = await fetch(`/api/${path}`, init);
    if (!res.ok) {
      return { ok: false, error: `HTTP ${res.status} ${res.statusText}` };
    }
    return (await res.json()) as ApiResult;
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

function post(path: string, body: unknown): Promise<ApiResult> {
  return request(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

// ── types ──────────────────────────────────────────────────────────────────

export type Settings = {
  clientId: string;
  clientSecret: string; // "" or "***"
  days: number;
  services: string[];
  connected: boolean;
  hasRefreshToken: boolean;
  tokenExpiresAt: number;
};

export type EmailMeta = {
  id: string;
  threadId?: string;
  subject?: string;
  from?: string;
  date?: string;
  snippet?: string;
};

export type EmailFull = EmailMeta & {
  to?: string;
  bodyText?: string;
  bodyHtml?: string;
  attachments?: { filename: string; mimeType: string; size?: number }[];
};

export type CalEvent = {
  id: string;
  summary?: string;
  description?: string;
  location?: string;
  start?: string;
  end?: string;
  htmlLink?: string;
};

export type DriveFile = {
  id: string;
  name: string;
  mimeType?: string;
  modifiedTime?: string;
  size?: string;
  webViewLink?: string;
};

export type SyncRun = {
  id: number;
  service: string;
  status: string;
  detail: string;
  created_at: number;
};

// ── endpoints ──────────────────────────────────────────────────────────────

export const getStatus = () => request("status");
export const getSettings = () => request("settings");
export const saveSettings = (s: {
  clientId?: string;
  clientSecret?: string;
  days?: number;
  services?: string[];
}) => post("settings", s);

export const getAuthUrl = () => request("auth/url");
/** Kick off OAuth: backend asks the daemon to open the consent URL in the
 * HOST system browser (works regardless of webview bridge/popup policy). */
export const openAuthInBrowser = () => post("auth/open", {});
/** Open any external URL via daemon → system browser (last-resort path). */
export const openUrlViaDaemon = (url: string) => post("open-url", { url });
export const connectWithToken = (accessToken: string, refreshToken?: string) =>
  post("auth/token", { accessToken, refreshToken: refreshToken ?? "" });
export const disconnect = () => post("auth/disconnect", {});

export const listEmails = (max = 10, q = "") =>
  request(`gmail/messages?max=${max}${q ? `&q=${encodeURIComponent(q)}` : ""}`);
export const readEmail = (id: string) =>
  request(`gmail/messages/${encodeURIComponent(id)}`);
export const sendEmail = (to: string, subject: string, body: string) =>
  post("gmail/send", { to, subject, body });

export const listEvents = (max = 10, days = 0) =>
  request(`calendar/events?max=${max}${days ? `&days=${days}` : ""}`);
export const createEvent = (e: {
  summary: string;
  description?: string;
  startTime: string;
  endTime: string;
}) => post("calendar/events", e);

export const listFiles = (max = 10, q = "") =>
  request(`drive/files?max=${max}${q ? `&q=${encodeURIComponent(q)}` : ""}`);
export const uploadFile = (name: string, mimeType: string, textContent: string) =>
  post("drive/upload", { name, mimeType, textContent });

export const runSync = (services?: string[], days?: number) =>
  post("sync", { services, days });
export const getRuns = () => request("runs");
