import React, { useCallback, useEffect, useState } from 'react';
import { Alert, Card, Segmented, Space, Spin, Tag, Typography, message, theme } from 'antd';
import { SafetyCertificateOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

type Mode = 'off' | 'warn' | 'strict';

interface TokenModeState {
  mode: Mode;
  /** Where the value in force came from. */
  source: 'ui' | 'env' | 'default';
  /** What the daemon falls back to if the UI choice is cleared. */
  envMode: Mode;
  envSet: boolean;
  defaultMode: Mode;
  apiVersion: number;
}

/** What each mode does to a request that carries no app access token. */
const MODES: { value: Mode; label: string; blurb: string }[] = [
  {
    value: 'strict',
    label: 'Require',
    blurb:
      'A call that does not prove which app it is gets refused. This is what keeps one app out of another app’s settings, database and AI bridge.',
  },
  {
    value: 'warn',
    label: 'Warn only',
    blurb:
      'Everything is served, but each app that calls without a token is logged once. Use this to find out what would break before requiring it.',
  },
  {
    value: 'off',
    label: 'Off',
    blurb:
      'Served as it was before per-app tokens existed. Any local process that knows an app’s id — which is public — can read that app’s data.',
  },
];

/**
 * The fleet-wide app-isolation switch.
 *
 * Deliberately not a plain on/off toggle: the middle setting is the one that
 * makes this safe to turn on, because it names the apps that would break
 * without breaking them. Jumping straight from Off to Require on a fleet of
 * installed apps is how an operator ends up reverting the whole feature.
 */
export const AppTokenModeCard: React.FC = () => {
  const { token } = theme.useToken();
  const [state, setState] = useState<TokenModeState | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/space/app-token-mode');
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      setState(await r.json());
      setError(null);
    } catch (e: any) {
      setError(String(e?.message ?? e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const choose = async (mode: Mode) => {
    setSaving(true);
    try {
      const r = await fetch('/api/space/app-token-mode', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ mode }),
      });
      if (!r.ok) throw new Error((await r.text()) || `HTTP ${r.status}`);
      setState(await r.json());
      // No restart: the daemon reads this per request, so the next call an app
      // makes is already judged by the new setting.
      message.success(`App access token: ${mode}. In force now — no restart needed.`);
    } catch (e: any) {
      message.error(`Could not change it: ${e?.message ?? e}`);
      void load();
    } finally {
      setSaving(false);
    }
  };

  if (error) {
    return <Alert type="error" showIcon message={`Could not read the setting: ${error}`} />;
  }
  if (!state) {
    return (
      <Card size="small" style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }}>
        <Spin size="small" /> <Text type="secondary">Reading the app isolation setting…</Text>
      </Card>
    );
  }

  const active = MODES.find(m => m.value === state.mode);

  return (
    <Card
      size="small"
      style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }}
      title={
        <Space>
          <SafetyCertificateOutlined style={{ color: token.colorPrimary }} />
          <Text strong>App access token</Text>
          {state.source === 'ui' && <Tag color="blue">set here</Tag>}
          {state.source === 'env' && <Tag>SENCLAW_APP_TOKEN_MODE</Tag>}
          {state.source === 'default' && <Tag>default</Tag>}
        </Space>
      }
    >
      <Paragraph type="secondary" style={{ marginBottom: 12 }}>
        Every installed app gets its own secret, and the daemon treats it as the app&rsquo;s name.
        This decides what happens to a call that arrives <em>without</em> one. A token that is
        present is always checked and always scoped &mdash; one app can never act on another&rsquo;s
        id, whatever this is set to.
      </Paragraph>

      <Segmented
        value={state.mode}
        disabled={saving}
        onChange={v => void choose(v as Mode)}
        options={MODES.map(m => ({ label: m.label, value: m.value }))}
        style={{ marginBottom: 12 }}
      />

      {active && (
        <Paragraph style={{ marginBottom: 12 }}>
          <Text>{active.blurb}</Text>
        </Paragraph>
      )}

      {state.mode === 'off' && (
        <Alert
          type="warning"
          showIcon
          message="App isolation is off"
          description="Any process on this machine can read any app's settings and query its database by naming the app's id. Turn this back to Warn or Require unless an app genuinely cannot send the token."
        />
      )}

      {state.mode === 'strict' && (
        <Alert
          type="info"
          showIcon
          message="What can still break"
          description={
            <>
              Apps built on a SenClaw SDK send the token on their own, and the daemon stamps it on
              everything it proxies. An app that reaches the daemon with its own HTTP client will
              get <Text code>401</Text> until it sends <Text code>SENCLAW_TOKEN_ACCESS_APP</Text>.
              Switch to <Text strong>Warn only</Text> for a while to see which ones those are.
            </>
          }
        />
      )}

      {state.source === 'ui' && state.envSet && (
        <Alert
          type="info"
          showIcon
          style={{ marginTop: 12 }}
          message={
            <>
              This overrides <Text code>SENCLAW_APP_TOKEN_MODE={state.envMode}</Text> from the
              daemon&rsquo;s environment.
            </>
          }
        />
      )}
    </Card>
  );
};

export default AppTokenModeCard;
