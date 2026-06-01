export type SenclawSpaceEnv = {
  appId: string;
  apiBase: string;
  coreBase?: string;
  staticBase?: string;
  bridgeEndpoint?: string;
  configEndpoint?: string;
  sqliteEndpoint?: string;
  mcpRegisterEndpoint?: string;
};

export type SqliteQueryResult<T = Record<string, unknown>> = {
  rows?: T[];
  rowsAffected?: number;
  lastInsertRowId?: number;
};

export type McpRegistration = {
  name?: string;
  transport: 'stdio' | 'sse' | 'http';
  description?: string;
  url?: string;
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  headers?: Record<string, string>;
  use_tools?: string[];
  enabled?: boolean;
};

type InitMessage = {
  type: 'senclaw:init';
  appId: string;
  env?: Partial<SenclawSpaceEnv>;
};

function isBrowser() {
  return typeof window !== 'undefined';
}

function getWindowEnv(): Partial<SenclawSpaceEnv> {
  if (!isBrowser()) return {};
  return (window as unknown as { __SENCLAW_SPACE_ENV__?: Partial<SenclawSpaceEnv> }).__SENCLAW_SPACE_ENV__ ?? {};
}

function fromProcessEnv(): Partial<SenclawSpaceEnv> {
  if (typeof process === 'undefined') return {};
  return {
    appId: process.env.SENCLAW_SPACE_APP_ID,
    apiBase: process.env.SENCLAW_SPACE_API_BASE,
    coreBase: process.env.SENCLAW_SPACE_CORE_BASE,
  };
}

function joinUrl(base: string, path: string) {
  return `${base.replace(/\/$/, '')}/${path.replace(/^\//, '')}`;
}

function appIdFromLocation(): string | null {
  if (!isBrowser()) return null;
  const match = window.location.pathname.match(/\/api\/space\/apps\/([^/]+)\/static(?:\/|$)/);
  return match ? decodeURIComponent(match[1]) : null;
}

async function parseResponse<T>(response: Response): Promise<T> {
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    const message = typeof payload === 'object' && payload && 'error' in payload
      ? String((payload as { error: unknown }).error)
      : text || response.statusText;
    throw new Error(message);
  }
  return payload as T;
}

export class SenclawSpace {
  env: SenclawSpaceEnv;

  constructor(env: Partial<SenclawSpaceEnv> = {}) {
    const merged = {
      ...fromProcessEnv(),
      ...getWindowEnv(),
      ...env,
    };
    const appId = merged.appId;
    if (!appId) {
      throw new Error('SenclawSpace requires appId. Wait for init() in browser or pass env explicitly.');
    }
    const apiBase = merged.apiBase ?? '/api/space/apps';
    this.env = {
      appId,
      apiBase,
      coreBase: merged.coreBase ?? '/api',
      staticBase: merged.staticBase ?? joinUrl(apiBase, `${appId}/static`),
      bridgeEndpoint: merged.bridgeEndpoint ?? joinUrl(apiBase, `${appId}/bridge`),
      configEndpoint: merged.configEndpoint ?? joinUrl(apiBase, `${appId}/config`),
      sqliteEndpoint: merged.sqliteEndpoint ?? joinUrl(apiBase, `${appId}/sqlite/query`),
      mcpRegisterEndpoint: merged.mcpRegisterEndpoint ?? joinUrl(apiBase, `${appId}/mcp/register`),
    };
  }

  /**
   * Construct a client for a standalone Node process (e.g. a bundled MCP server)
   * that must reach the daemon over an absolute URL rather than relative paths.
   */
  static forDaemon(appId: string, baseUrl = 'http://127.0.0.1:18788'): SenclawSpace {
    const base = baseUrl.replace(/\/$/, '');
    return new SenclawSpace({
      appId,
      apiBase: `${base}/api/space/apps`,
      coreBase: `${base}/api`,
    });
  }

  static async init(timeoutMs = 1500): Promise<SenclawSpace> {
    if (!isBrowser()) return new SenclawSpace();
    const existing = getWindowEnv();
    if (existing.appId) return new SenclawSpace(existing);

    const message = await new Promise<InitMessage | null>(resolve => {
      const timer = window.setTimeout(() => {
        window.removeEventListener('message', onMessage);
        resolve(null);
      }, timeoutMs);
      const onMessage = (event: MessageEvent) => {
        if (event.data?.type !== 'senclaw:init') return;
        window.clearTimeout(timer);
        window.removeEventListener('message', onMessage);
        resolve(event.data as InitMessage);
      };
      window.addEventListener('message', onMessage);
      window.parent?.postMessage({ type: 'senclaw:ready' }, '*');
    });

    if (!message) {
      const appId = appIdFromLocation();
      if (!appId) {
        throw new Error('Timed out waiting for senclaw:init.');
      }
      const fallback = await parseResponse<SenclawSpaceEnv>(
        await fetch(`/api/space/apps/${encodeURIComponent(appId)}/env`)
      );
      (window as unknown as { __SENCLAW_SPACE_ENV__?: Partial<SenclawSpaceEnv> }).__SENCLAW_SPACE_ENV__ = fallback;
      return new SenclawSpace(fallback);
    }
    const env = { appId: message.appId, ...message.env };
    (window as unknown as { __SENCLAW_SPACE_ENV__?: Partial<SenclawSpaceEnv> }).__SENCLAW_SPACE_ENV__ = env;
    return new SenclawSpace(env);
  }

  async getConfig<T = unknown>(key: string): Promise<T | null> {
    const response = await fetch(`${this.env.configEndpoint}/${encodeURIComponent(key)}`);
    if (response.status === 404) return null;
    const payload = await parseResponse<{ value: T }>(response);
    return payload.value;
  }

  async setConfig<T = unknown>(key: string, value: T): Promise<T> {
    const response = await fetch(`${this.env.configEndpoint}/${encodeURIComponent(key)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    });
    const payload = await parseResponse<{ value: T }>(response);
    return payload.value;
  }

  async deleteConfig(key: string): Promise<void> {
    await parseResponse(await fetch(`${this.env.configEndpoint}/${encodeURIComponent(key)}`, { method: 'DELETE' }));
  }

  async listConfig(): Promise<Array<{ key: string; value: unknown; updated_at: number }>> {
    const payload = await parseResponse<{ items: Array<{ key: string; value: unknown; updated_at: number }> }>(
      await fetch(this.env.configEndpoint ?? '')
    );
    return payload.items;
  }

  async sqlite<T = Record<string, unknown>>(sql: string, params: unknown[] = []): Promise<SqliteQueryResult<T>> {
    return parseResponse<SqliteQueryResult<T>>(await fetch(this.env.sqliteEndpoint ?? '', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sql, params }),
    }));
  }

  async registerMcp(registration: McpRegistration): Promise<unknown> {
    return parseResponse(await fetch(this.env.mcpRegisterEndpoint ?? '', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(registration),
    }));
  }

  async core<T = unknown>(path: string, init?: RequestInit): Promise<T> {
    return parseResponse<T>(await fetch(joinUrl(this.env.coreBase ?? '/api', path), init));
  }
}
