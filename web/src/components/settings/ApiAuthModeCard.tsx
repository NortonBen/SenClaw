import React, { useCallback, useEffect, useState } from 'react';
import { Alert, Card, Segmented, Space, Spin, Tag, Typography, message, theme } from 'antd';
import { LockOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

type Mode = 'auto' | 'always' | 'off';

interface AuthModeState {
  mode: Mode;
  /** Where the value in force came from. */
  source: 'ui' | 'env' | 'default';
  /** What the daemon falls back to if the UI choice is cleared. */
  envMode: Mode;
  envSet: boolean;
  defaultMode: Mode;
  /** Whether a token is being demanded right now. */
  required: boolean;
  /** True when the daemon binds a host that only resolves to its own machine. */
  bindIsLoopback: boolean;
  /** Path of the token file — never the token itself. */
  tokenPath: string | null;
  canOverride: boolean;
}

const MODES: { value: Mode; label: string; blurb: string }[] = [
  {
    value: 'auto',
    label: 'Automatic',
    blurb:
      'Ask for the token only when SenClaw is bound beyond this machine, and only from devices that are not this machine. Right for a laptop or a desktop install.',
  },
  {
    value: 'always',
    label: 'Always require',
    blurb:
      'Every request needs the token, including ones that appear to come from this machine. Required in the cloud: a reverse proxy that terminates HTTPS on the same host makes every visitor look local, so nothing else can tell them apart.',
  },
  {
    value: 'off',
    label: 'Never',
    blurb:
      'No token, ever. Only safe when something in front of SenClaw already authenticates every request, or nothing but this machine can reach the port.',
  },
];

/**
 * The daemon's own access gate — separate from the per-app token switch in
 * Space Apps, which isolates apps from each other rather than guarding the
 * daemon's API.
 *
 * `Always` is not a paranoid variant of `Automatic`: it is the only correct
 * setting behind a same-host reverse proxy, which is how essentially every
 * cloud deployment terminates TLS. Automatic there would exempt the entire
 * Internet.
 */
export const ApiAuthModeCard: React.FC = () => {
  const { token } = theme.useToken();
  const [state, setState] = useState<AuthModeState | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/auth/mode');
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
      const r = await fetch('/api/auth/mode', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ mode }),
      });
      if (!r.ok) throw new Error((await r.text()) || `HTTP ${r.status}`);
      const next: AuthModeState = await r.json();
      setState(next);
      if (mode === 'always') {
        // This browser's very next /api call is now gated. It already holds
        // the token if it logged in; if it did not, the gate takes over and
        // asks — which is why the token's location is worth repeating here.
        message.warning(
          `Every request now needs the token${
            next.tokenPath ? ` — it is in ${next.tokenPath}` : ''
          }.`,
          8
        );
      } else {
        message.success(`Access token: ${mode}. In force now — no restart needed.`);
      }
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
        <Spin size="small" /> <Text type="secondary">Reading the access setting…</Text>
      </Card>
    );
  }

  const active = MODES.find((m) => m.value === state.mode);

  return (
    <Card
      size="small"
      style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }}
      title={
        <Space>
          <LockOutlined style={{ color: token.colorPrimary }} />
          <Text strong>Access token</Text>
          {state.source === 'ui' && <Tag color="blue">set here</Tag>}
          {state.source === 'env' && <Tag>SENCLAW_AUTH_MODE</Tag>}
          {state.source === 'default' && <Tag>default</Tag>}
          {state.required ? <Tag color="green">in force</Tag> : <Tag>not asked for</Tag>}
        </Space>
      }
    >
      <Paragraph type="secondary" style={{ marginBottom: 12 }}>
        Who has to prove they are allowed in before SenClaw answers. The token lives on the machine
        running SenClaw
        {state.tokenPath ? (
          <>
            , at <Text code>{state.tokenPath}</Text>
          </>
        ) : null}
        .
      </Paragraph>

      <Segmented
        value={state.mode}
        disabled={saving || !state.canOverride}
        onChange={(v) => void choose(v as Mode)}
        options={MODES.map((m) => ({ label: m.label, value: m.value }))}
        style={{ marginBottom: 12 }}
      />

      {active && (
        <Paragraph style={{ marginBottom: 12 }}>
          <Text>{active.blurb}</Text>
        </Paragraph>
      )}

      {state.mode === 'auto' && !state.bindIsLoopback && (
        <Alert
          type="info"
          showIcon
          message="Devices on your network need the token"
          description="SenClaw is reachable beyond this machine, so anything that is not this machine is being asked for the token. This machine is not."
        />
      )}

      {state.mode === 'auto' && (
        <Alert
          type="warning"
          showIcon
          style={{ marginTop: 12 }}
          message="Running behind a reverse proxy?"
          description="If nginx, Caddy or a load balancer terminates HTTPS on this same machine, every visitor arrives looking like this machine and Automatic lets them all in without a token. Choose Always require for that setup."
        />
      )}

      {state.mode === 'off' && (
        <Alert
          type="error"
          showIcon
          message="No token is being asked for"
          description={
            state.bindIsLoopback
              ? 'Only this machine can reach SenClaw right now, so nothing is exposed — but a change of bind host would not re-enable the gate.'
              : 'SenClaw is reachable beyond this machine and is answering everyone. Turn this back on unless something in front of it already authenticates every request.'
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
              This overrides <Text code>SENCLAW_AUTH_MODE={state.envMode}</Text> from the
              daemon&rsquo;s environment.
            </>
          }
        />
      )}

      {!state.canOverride && (
        <Alert
          type="warning"
          showIcon
          style={{ marginTop: 12 }}
          message="This daemon cannot store a choice — set SENCLAW_AUTH_MODE in its environment instead."
        />
      )}
    </Card>
  );
};

export default ApiAuthModeCard;
