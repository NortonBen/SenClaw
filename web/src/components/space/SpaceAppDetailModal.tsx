import React, { useEffect, useState } from 'react';
import { Modal, Descriptions, Tag, Collapse, Empty, Spin, Alert, Typography, theme } from 'antd';
import { ApiOutlined, ToolOutlined } from '@ant-design/icons';

const { Paragraph, Text } = Typography;

export interface DetailApp {
  id: string;
  name: string;
  description?: string;
  icon?: string;
  integration: { type: 'iframe' | 'esm'; url: string };
  manifest?: any;
}

interface McpToolDef {
  name: string;
  description?: string | null;
  inputSchema?: Record<string, unknown> | null;
}

interface DeclaredMcp {
  name?: string;
  transport?: string;
  url?: string;
  description?: string;
  autoRegister?: boolean;
  launch?: { command?: string; args?: string[]; port?: number; healthPath?: string };
}

interface ServerInfo {
  name?: string;
  transport?: string;
  status?: 'connected' | 'connecting' | 'disconnected' | 'error';
  enabled?: boolean;
  error?: string | null;
  tools?: McpToolDef[] | null;
}

interface McpResponse {
  appId: string;
  declared: DeclaredMcp | null;
  server: ServerInfo | null;
}

const STATUS_COLOR: Record<string, string> = {
  connected: 'green',
  connecting: 'gold',
  disconnected: 'default',
  error: 'red',
};

interface Props {
  app: DetailApp | null;
  open: boolean;
  onClose: () => void;
}

export function SpaceAppDetailModal({ app, open, onClose }: Props) {
  const { token } = theme.useToken();
  const [loading, setLoading] = useState(false);
  const [mcp, setMcp] = useState<McpResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open || !app) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    setMcp(null);
    fetch(`/api/space/apps/${encodeURIComponent(app.id)}/mcp`)
      .then(r => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then((data: McpResponse) => {
        if (!cancelled) setMcp(data);
      })
      .catch(err => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, app]);

  const declared = mcp?.declared ?? null;
  const server = mcp?.server ?? null;
  const tools = server?.tools ?? [];

  return (
    <Modal
      title={
        <div className="flex items-center gap-2">
          <span>{app?.icon ?? '🔌'}</span>
          <span>{app?.name ?? 'App'}</span>
          {app && <Tag>{app.id}</Tag>}
        </div>
      }
      open={open}
      onCancel={onClose}
      footer={null}
      width={720}
    >
      {app && (
        <>
          <Paragraph type="secondary">{app.description ?? '—'}</Paragraph>

          <Descriptions size="small" column={1} bordered className="mb-4">
            <Descriptions.Item label="Integration">
              <Tag color={app.integration.type === 'iframe' ? 'blue' : 'purple'}>
                {app.integration.type}
              </Tag>
              <Text code className="text-xs">{app.integration.url}</Text>
            </Descriptions.Item>
            {app.manifest?.bridge?.postMessage && (
              <Descriptions.Item label="Bridge">
                <Tag color="cyan">SemaClaw bridge</Tag>
                {(app.manifest.bridge.capabilities ?? []).map((c: string) => (
                  <Tag key={c}>{c}</Tag>
                ))}
              </Descriptions.Item>
            )}
            {app.manifest?.runtime?.kind && (
              <Descriptions.Item label="Runtime">
                <Tag>{app.manifest.runtime.kind}</Tag>
              </Descriptions.Item>
            )}
          </Descriptions>

          {/* MCP section */}
          <div className="flex items-center gap-2 mb-2">
            <ApiOutlined style={{ color: token.colorPrimary }} />
            <span className="font-semibold" style={{ color: token.colorText }}>
              MCP Server
            </span>
            {server?.status && (
              <Tag color={STATUS_COLOR[server.status] ?? 'default'}>{server.status}</Tag>
            )}
            {declared?.autoRegister && <Tag color="geekblue">auto-register</Tag>}
          </div>

          {loading && (
            <div className="py-6 flex justify-center">
              <Spin />
            </div>
          )}

          {error && !loading && (
            <Alert type="error" showIcon message={`Không tải được thông tin MCP: ${error}`} />
          )}

          {!loading && !error && !declared && (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="App này không khai báo MCP server."
            />
          )}

          {!loading && !error && declared && (
            <>
              <Descriptions size="small" column={1} bordered className="mb-3">
                <Descriptions.Item label="Tên">{declared.name ?? '—'}</Descriptions.Item>
                <Descriptions.Item label="Transport">
                  <Tag>{declared.transport ?? '—'}</Tag>
                </Descriptions.Item>
                {declared.url && (
                  <Descriptions.Item label="URL">
                    <Text code className="text-xs">{declared.url}</Text>
                  </Descriptions.Item>
                )}
                {declared.launch?.command && (
                  <Descriptions.Item label="Launch">
                    <Text code className="text-xs">
                      {declared.launch.command} {(declared.launch.args ?? []).join(' ')}
                    </Text>
                    {declared.launch.port ? <Tag className="ml-2">:{declared.launch.port}</Tag> : null}
                  </Descriptions.Item>
                )}
              </Descriptions>

              {server?.error && (
                <Alert
                  type="warning"
                  showIcon
                  className="mb-3"
                  message={`Server: ${server.error}`}
                />
              )}

              <div className="flex items-center gap-2 mb-2">
                <ToolOutlined style={{ color: token.colorTextSecondary }} />
                <span className="text-sm font-medium" style={{ color: token.colorText }}>
                  Tools hỗ trợ ({tools.length})
                </span>
              </div>

              {tools.length === 0 ? (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description={
                    server?.status === 'connected'
                      ? 'Server không expose tool nào.'
                      : 'Chưa kết nối — connect server để thấy danh sách tool.'
                  }
                />
              ) : (
                <Collapse
                  size="small"
                  items={tools.map(t => ({
                    key: t.name,
                    label: (
                      <div className="flex items-center gap-2">
                        <Text code>{t.name}</Text>
                        <Text type="secondary" className="text-xs truncate">
                          {(t.description ?? '').split('\n')[0]}
                        </Text>
                      </div>
                    ),
                    children: (
                      <div>
                        {t.description && (
                          <Paragraph type="secondary" className="text-xs whitespace-pre-wrap">
                            {t.description}
                          </Paragraph>
                        )}
                        {t.inputSchema && (
                          <pre
                            className="text-xs p-2 rounded overflow-auto"
                            style={{ background: token.colorFillTertiary, maxHeight: 220 }}
                          >
                            {JSON.stringify(t.inputSchema, null, 2)}
                          </pre>
                        )}
                      </div>
                    ),
                  }))}
                />
              )}
            </>
          )}
        </>
      )}
    </Modal>
  );
}
