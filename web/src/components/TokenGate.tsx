import { FormEvent, ReactNode, useEffect, useState } from 'react';
import { fetchAuthStatus, login, UNAUTHORIZED_EVENT } from '../lib/auth';

type GateState = 'checking' | 'open' | 'locked';

/**
 * Blocks the whole app behind a token prompt when the daemon requires API
 * auth (non-loopback bind) and this browser is not yet authorized. Local
 * setups (loopback bind, or loopback-exempt peers) pass straight through.
 *
 * Mounted around <App/> — App opens the WebSocket and fires /api fetches
 * immediately on mount, so it must not render until auth is settled.
 */
export function TokenGate({ children }: { children: ReactNode }) {
  const [state, setState] = useState<GateState>('checking');
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const status = await fetchAuthStatus();
      if (cancelled) return;
      // Unreachable daemon → let App render its own "connecting" states
      // rather than trapping the user on a token prompt that can't succeed.
      if (!status || !status.authRequired || status.authorized) setState('open');
      else setState('locked');
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const onUnauthorized = () => setState('locked');
    window.addEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, onUnauthorized);
  }, []);

  if (state === 'open') return <>{children}</>;
  if (state === 'checking') return null;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const value = token.trim();
    if (!value || busy) return;
    setBusy(true);
    setError('');
    const ok = await login(value);
    setBusy(false);
    if (ok) {
      // Full reload instead of just unmount-gate: sockets/fetches that
      // already failed inside a half-mounted App would otherwise stay dead.
      window.location.reload();
    } else {
      setError('Invalid token. Check ~/.senclaw/api_token on the daemon machine.');
    }
  };

  return (
    <div className="fixed inset-0 flex items-center justify-center bg-neutral-950 text-neutral-100">
      <form
        onSubmit={submit}
        className="w-full max-w-sm mx-4 rounded-xl border border-neutral-800 bg-neutral-900 p-6 shadow-2xl"
      >
        <div className="text-lg font-semibold mb-1">SenClaw</div>
        <div className="text-sm text-neutral-400 mb-4">
          This daemon is exposed beyond localhost and requires an access
          token. Find it in <code className="text-neutral-300">~/.senclaw/api_token</code> on
          the machine running SenClaw.
        </div>
        <input
          type="password"
          autoFocus
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="Access token"
          className="w-full rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm outline-none focus:border-neutral-500"
        />
        {error && <div className="mt-2 text-sm text-red-400">{error}</div>}
        <button
          type="submit"
          disabled={busy || !token.trim()}
          className="mt-4 w-full rounded-md bg-blue-600 px-3 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-50"
        >
          {busy ? 'Verifying…' : 'Unlock'}
        </button>
      </form>
    </div>
  );
}
