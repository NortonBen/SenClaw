import React, { useEffect, useRef, useState } from 'react';
import { Alert, Button, Spin, Typography, theme } from 'antd';
import { AppstoreOutlined, ReloadOutlined, LoadingOutlined } from '@ant-design/icons';
import { useAppContext } from '../../contexts/AppContext';

const { Text } = Typography;

export interface SpaceAppRuntime {
  id: string;
  name: string;
  description?: string;
  icon?: string;
  integration: { type: 'iframe' | 'esm'; url: string };
  enabled: boolean;
}

interface Props {
  app: SpaceAppRuntime;
}

/// Forward the outer page's query string (e.g. a chat deep link
/// /space/app/drawio?d=3) into the app iframe so apps can open a specific
/// resource. Apps ignore params they don't know.
function withOuterQuery(base: string): string {
  const outer = window.location.search.replace(/^\?/, '');
  if (!outer) return base;
  return `${base}${base.includes('?') ? '&' : '?'}${outer}`;
}

type GatePhase = 'checking' | 'starting' | 'ready' | 'failed';

export function SpaceAppFrame({ app }: Props) {
  const { token } = theme.useToken();
  const { isDarkMode } = useAppContext();
  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const [loaded, setLoaded] = useState(false);

  // Don't mount the iframe until the app is answering. A server Space App is
  // its own process on its own port; pointed at a stopped one, the iframe is a
  // blank white rectangle with no error in it — and "stopped" is the resting
  // state for a `session` app, not a fault.
  const [phase, setPhase] = useState<GatePhase>('checking');
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);

  const open = React.useCallback(async () => {
    setPhase('checking');
    setError('');
    setLoaded(false);
    const id = encodeURIComponent(app.id);
    try {
      const probe = await fetch(`/api/space/apps/${id}/ready`);
      if (probe.ok && (await probe.json())?.ready === true) {
        setPhase('ready');
        return;
      }
    } catch {
      /* a failed probe is not an answer — try to start it */
    }
    setPhase('starting');
    try {
      const res = await fetch(`/api/space/apps/${id}/start`, { method: 'POST' });
      const text = await res.text();
      if (!res.ok) throw new Error(text || `start failed (HTTP ${res.status})`);
      setPhase('ready');
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setPhase('failed');
    }
  }, [app.id]);

  useEffect(() => { void open(); }, [open, attempt]);

  const themeMode = isDarkMode ? 'dark' : 'light';

  const sendInit = React.useCallback(() => {
    const mode = isDarkMode ? 'dark' : 'light';
    const env = {
      appId: app.id,
      apiBase: '/api/space/apps',
      coreBase: '/api',
      staticBase: `/api/space/apps/${encodeURIComponent(app.id)}/static`,
      bridgeEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/bridge`,
      configEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/config`,
      sqliteEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/sqlite/query`,
      mcpRegisterEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/mcp/register`,
      theme: mode,
    };
    iframeRef.current?.contentWindow?.postMessage({
      type: 'senclaw:init',
      appId: app.id,
      env,
      theme: mode,
      capabilities: ['llm.request', 'mcp.call', 'space.rest'],
    }, '*');
  }, [app.id, isDarkMode]);

  // Push theme changes to the embedded app so it can follow senclaw's dark/light mode.
  useEffect(() => {
    iframeRef.current?.contentWindow?.postMessage(
      { type: 'senclaw:theme', theme: themeMode },
      '*',
    );
  }, [themeMode]);

  useEffect(() => {
    const handleMessage = async (event: MessageEvent) => {
      const data = event.data;
      if (data?.type === 'senclaw:ready') {
        sendInit();
        return;
      }
      if (!data || data.type !== 'senclaw:request' || !data.action) return;
      try {
        const res = await fetch(`/api/space/apps/${encodeURIComponent(app.id)}/bridge`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ action: data.action, payload: data.payload }),
        });
        const payload = await res.json();
        iframeRef.current?.contentWindow?.postMessage({
          type: 'senclaw:response',
          requestId: data.requestId,
          ok: res.ok,
          payload,
        }, '*');
      } catch (err) {
        iframeRef.current?.contentWindow?.postMessage({
          type: 'senclaw:response',
          requestId: data.requestId,
          ok: false,
          error: err instanceof Error ? err.message : String(err),
        }, '*');
      }
    };
    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [app.id, sendInit]);

  if (app.integration.type !== 'iframe') {
    return (
      <div className="h-full p-4">
        <Alert
          type="warning"
          showIcon
          message="Unsupported app integration"
          description="Only iframe Space Apps are supported in the current runtime."
        />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <AppstoreOutlined />
        <Text strong className="flex-1">{app.name}</Text>
        <Button size="small" icon={<ReloadOutlined />} onClick={() => setAttempt(a => a + 1)}>
          Reload
        </Button>
      </div>
      <div className="relative flex-1">
        {phase === 'failed' ? (
          <div className="absolute inset-0 overflow-auto p-6">
            <Alert
              type="error"
              showIcon
              message={`${app.name} did not start`}
              description={
                <>
                  {/* The daemon appends the tail of the app's own log, which is
                      nearly always the real answer — missing binary, port in
                      use, a stack trace on boot. */}
                  <pre style={{
                    whiteSpace: 'pre-wrap', margin: '8px 0 12px', maxHeight: 280,
                    overflow: 'auto', fontSize: 11.5, opacity: 0.85,
                  }}>{error}</pre>
                  <Button size="small" icon={<ReloadOutlined />} onClick={() => setAttempt(a => a + 1)}>
                    Try again
                  </Button>
                </>
              }
            />
          </div>
        ) : (phase !== 'ready' || !loaded) && (
          <div
            className="absolute inset-0 flex flex-col items-center justify-center gap-3 z-10"
            style={{ background: token.colorBgContainer }}
          >
            {/* The app's own icon inside the spinner, so the wait reads as
                "this app is opening" rather than a generic loading state. */}
            <Spin
              size="large"
              indicator={<LoadingOutlined style={{ fontSize: 56 }} spin />}
              style={{ position: 'relative' }}
            />
            <div style={{ marginTop: -46, fontSize: 22, lineHeight: 1, pointerEvents: 'none' }}>
              {app.icon || <AppstoreOutlined style={{ color: token.colorPrimary }} />}
            </div>
            <Text strong style={{ marginTop: 22 }}>
              {phase === 'starting' ? `Đang khởi động ${app.name}…` : `Đang tải ${app.name}…`}
            </Text>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {phase === 'ready'
                ? 'Đang mở giao diện.'
                : 'Chờ app trả lời health check.'}
            </Text>
          </div>
        )}
        {phase === 'ready' && (
          <iframe
            key={attempt}
            ref={iframeRef}
            title={app.name}
            src={withOuterQuery(app.integration.url === '/' ? `/api/space/apps/${app.id}/proxy/` : app.integration.url)}
            onLoad={() => { setLoaded(true); sendInit(); }}
            sandbox="allow-forms allow-modals allow-popups allow-same-origin allow-scripts"
            style={{
              width: '100%',
              height: '100%',
              border: 0,
              background: token.colorBgContainer,
              visibility: loaded ? 'visible' : 'hidden',
            }}
          />
        )}
      </div>
    </div>
  );
}
