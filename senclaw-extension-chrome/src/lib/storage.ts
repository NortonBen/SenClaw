// chrome.storage wrapper for persisting settings.

const KEYS = {
  WS_HOST: 'ws_host',
  WS_PORT: 'ws_port',
  LAST_TAB_ID: 'last_tab_id',
  CRAWL_JOBS: 'crawl_jobs',
} as const;

export const DEFAULT_WS_HOST = '127.0.0.1';
export const DEFAULT_WS_PORT = 18789;

export async function getWsHost(): Promise<string> {
  const result = await chrome.storage.local.get(KEYS.WS_HOST);
  const host = result[KEYS.WS_HOST];
  return typeof host === 'string' && host.length > 0 ? host : DEFAULT_WS_HOST;
}

export async function setWsHost(host: string): Promise<void> {
  await chrome.storage.local.set({ [KEYS.WS_HOST]: host });
}

export async function getWsPort(): Promise<number> {
  const result = await chrome.storage.local.get(KEYS.WS_PORT);
  const port = Number(result[KEYS.WS_PORT]);
  return Number.isFinite(port) && port > 0 ? port : DEFAULT_WS_PORT;
}

export async function setWsPort(port: number): Promise<void> {
  await chrome.storage.local.set({ [KEYS.WS_PORT]: port });
}

export async function getLastTabId(): Promise<string | null> {
  const result = await chrome.storage.local.get(KEYS.LAST_TAB_ID);
  return result[KEYS.LAST_TAB_ID] ?? null;
}

export async function setLastTabId(tabId: string): Promise<void> {
  await chrome.storage.local.set({ [KEYS.LAST_TAB_ID]: tabId });
}
