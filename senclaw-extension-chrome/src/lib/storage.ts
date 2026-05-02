// chrome.storage wrapper for persisting settings.

const KEYS = {
  WS_PORT: 'ws_port',
  LAST_TAB_ID: 'last_tab_id',
  CRAWL_JOBS: 'crawl_jobs',
} as const;

export async function getWsPort(): Promise<number> {
  const result = await chrome.storage.local.get(KEYS.WS_PORT);
  return result[KEYS.WS_PORT] ?? 18789;
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
