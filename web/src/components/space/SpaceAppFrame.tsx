import React, { useEffect, useRef, useState } from 'react';
import { Alert, Button, Spin, Typography, theme } from 'antd';
import { AppstoreOutlined, ReloadOutlined, LoadingOutlined } from '@ant-design/icons';

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

export function SpaceAppFrame({ app }: Props) {
  const { token } = theme.useToken();
  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const [loaded, setLoaded] = useState(false);

  const sendInit = React.useCallback(() => {
    const env = {
      appId: app.id,
      apiBase: '/api/space/apps',
      coreBase: '/api',
      staticBase: `/api/space/apps/${encodeURIComponent(app.id)}/static`,
      bridgeEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/bridge`,
      configEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/config`,
      sqliteEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/sqlite/query`,
      mcpRegisterEndpoint: `/api/space/apps/${encodeURIComponent(app.id)}/mcp/register`,
    };
    iframeRef.current?.contentWindow?.postMessage({
      type: 'senclaw:init',
      appId: app.id,
      env,
      capabilities: ['llm.request', 'mcp.call', 'space.rest'],
    }, '*');
  }, [app.id]);

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
        <Button size="small" icon={<ReloadOutlined />} onClick={() => {
          if (iframeRef.current) {
            setLoaded(false);
            iframeRef.current.src = app.integration.url;
          }
        }}>
          Reload
        </Button>
      </div>
      <div className="relative flex-1">
        {!loaded && (
          <div
            className="absolute inset-0 flex flex-col items-center justify-center gap-3 z-10"
            style={{ background: token.colorBgContainer }}
          >
            <Spin indicator={<LoadingOutlined style={{ fontSize: 32 }} spin />} />
            <Text type="secondary">Đang tải {app.name}…</Text>
          </div>
        )}
        <iframe
          ref={iframeRef}
          title={app.name}
          src={app.integration.url}
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
      </div>
    </div>
  );
}
