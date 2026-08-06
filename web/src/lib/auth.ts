// API access-token support for the daemon connection.
//
// The daemon only enforces a token when it is bound beyond loopback
// (SENCLAW_UI_BIND_HOST=0.0.0.0); local setups never see any of this.
// Strategy: a successful POST /api/auth/login sets an HttpOnly session
// cookie (which also covers WS upgrades and Space-App proxy iframes), and as
// belt-and-braces every /api fetch carries the token in an X-SenClaw-Token
// header via the installAuthFetch() patch below — one patch instead of
// touching the ~180 scattered fetch('/api/…') call sites.

const STORAGE_KEY = 'senclaw:apiToken';

/** Fired when a gated /api request answers 401 — the TokenGate re-locks. */
export const UNAUTHORIZED_EVENT = 'senclaw:unauthorized';

export function getApiToken(): string {
  try {
    return localStorage.getItem(STORAGE_KEY) ?? '';
  } catch {
    return '';
  }
}

export function setApiToken(token: string): void {
  try {
    if (token) localStorage.setItem(STORAGE_KEY, token);
    else localStorage.removeItem(STORAGE_KEY);
  } catch {
    // Private-mode storage failures: the session cookie still carries auth.
  }
}

function isDaemonApiUrl(input: RequestInfo | URL): URL | null {
  try {
    const raw = input instanceof Request ? input.url : String(input);
    const url = new URL(raw, window.location.href);
    if (url.origin !== window.location.origin) return null;
    if (!url.pathname.startsWith('/api/') && url.pathname !== '/api') return null;
    return url;
  } catch {
    return null;
  }
}

let installed = false;

/**
 * Patch window.fetch: attach the stored token to same-origin /api requests
 * and broadcast UNAUTHORIZED_EVENT on a 401 so the gate can re-appear.
 * Requests to other origins (hubs, providers) are passed through untouched.
 */
export function installAuthFetch(): void {
  if (installed) return;
  installed = true;
  const original = window.fetch.bind(window);
  window.fetch = async (input: RequestInfo | URL, init?: RequestInit) => {
    const apiUrl = isDaemonApiUrl(input);
    if (!apiUrl) return original(input, init);

    const token = getApiToken();
    let nextInit = init;
    if (token) {
      // init.headers (when given) fully replaces Request headers per the
      // fetch spec, so merge from whichever source is in effect.
      const headers = new Headers(
        init?.headers ?? (input instanceof Request ? input.headers : undefined)
      );
      if (!headers.has('x-senclaw-token') && !headers.has('authorization')) {
        headers.set('x-senclaw-token', token);
      }
      nextInit = { ...(init ?? {}), headers };
    }
    const res = await original(input, nextInit);
    if (res.status === 401 && !apiUrl.pathname.startsWith('/api/auth/')) {
      window.dispatchEvent(new CustomEvent(UNAUTHORIZED_EVENT));
    }
    return res;
  };
}

export interface AuthStatus {
  authRequired: boolean;
  authorized: boolean;
}

/** Probe the daemon's auth posture. Null when the daemon is unreachable. */
export async function fetchAuthStatus(): Promise<AuthStatus | null> {
  try {
    const res = await fetch('/api/auth/status');
    if (!res.ok) return null;
    const body = (await res.json()) as Partial<AuthStatus>;
    return {
      authRequired: !!body.authRequired,
      authorized: !!body.authorized,
    };
  } catch {
    return null;
  }
}

/** Verify a token and mint the session cookie. Returns true on success. */
export async function login(token: string): Promise<boolean> {
  try {
    const res = await fetch('/api/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ token }),
    });
    if (!res.ok) return false;
    setApiToken(token);
    return true;
  } catch {
    return false;
  }
}
