import { useCallback, useEffect, useState } from 'react';
import { theme, Typography, Button, Tooltip } from 'antd';
import { EyeOutlined, StopOutlined, LoadingOutlined } from '@ant-design/icons';

const { Text } = Typography;

/** One in-flight watch, as `/api/watches` reports it. */
interface Watch {
  id: string;
  label: string | null;
  tool: string | null;
  checks: number;
  maxChecks: number;
  givesUpAt: string | null;
  lastError: string | null;
  intervalSecs: number;
  nextCheck: string | null;
}

/** Poll cadence. A watch checks at most once a minute, so this is plenty. */
const REFRESH_MS = 15_000;

function shortTime(iso: string | null): string {
  if (!iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ''
    : d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

/**
 * What this chat is waiting on, and the button that stops it.
 *
 * A watch is armed by the agent mid-turn and then runs silently — that silence
 * is correct (each check costs no tokens) but it left the user unable to tell
 * "waiting" from "forgotten", with no way to cancel. This is the visible half:
 * it says what is being watched, how many checks it has made against its
 * ceiling, and when it gives up.
 *
 * Polled rather than pushed: the state changes at most once a minute, and the
 * chat's WebSocket carries no watch events — adding some would mean pushing to
 * every client whether or not a chat is open.
 */
export function WatchStrip({ jid }: { jid: string }) {
  const { token } = theme.useToken();
  const [watches, setWatches] = useState<Watch[]>([]);
  const [stopping, setStopping] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await fetch(`/api/watches?chatJid=${encodeURIComponent(jid)}`);
      if (!r.ok) return;
      const body = await r.json();
      setWatches(Array.isArray(body?.watches) ? body.watches : []);
    } catch {
      // A failed poll is not worth surfacing: the next one is 15s away, and an
      // error banner here would sit above the composer over a transient blip.
    }
  }, [jid]);

  useEffect(() => {
    setWatches([]); // switching chats must not show the previous one's watches
    void load();
    const t = setInterval(() => void load(), REFRESH_MS);
    return () => clearInterval(t);
  }, [load]);

  const stop = async (id: string) => {
    setStopping(id);
    try {
      await fetch(`/api/watches/${encodeURIComponent(id)}/stop`, { method: 'POST' });
      // Drop it immediately rather than waiting for the next poll — the user
      // just clicked Stop and needs to see that it took.
      setWatches(w => w.filter(x => x.id !== id));
    } finally {
      setStopping(null);
    }
  };

  if (watches.length === 0) return null;

  return (
    <div style={{ padding: '0 24px 8px' }}>
      {watches.map(w => (
        <div
          key={w.id}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 8,
            padding: '6px 10px',
            marginTop: 6,
            borderRadius: 8,
            border: `1px solid ${token.colorBorderSecondary}`,
            background: token.colorFillQuaternary,
          }}
        >
          <EyeOutlined style={{ color: token.colorPrimary, fontSize: 13 }} />
          <Text style={{ fontSize: 12, flex: 1, minWidth: 0 }} ellipsis>
            Đang theo dõi <strong>{w.label || w.tool || 'tác vụ nền'}</strong>
            <Text type="secondary" style={{ fontSize: 11, marginLeft: 6 }}>
              · đã kiểm tra {w.checks}/{w.maxChecks}
              {w.intervalSecs > 0 && ` · mỗi ${w.intervalSecs}s`}
              {w.givesUpAt && ` · dừng lúc ${shortTime(w.givesUpAt)}`}
            </Text>
          </Text>
          {w.lastError && (
            <Tooltip title={w.lastError}>
              <Text type="warning" style={{ fontSize: 11 }}>
                lỗi tạm thời
              </Text>
            </Tooltip>
          )}
          <Button
            size="small"
            type="text"
            danger
            icon={stopping === w.id ? <LoadingOutlined /> : <StopOutlined />}
            disabled={stopping === w.id}
            onClick={() => void stop(w.id)}
            style={{ fontSize: 11 }}
          >
            Dừng
          </Button>
        </div>
      ))}
    </div>
  );
}
