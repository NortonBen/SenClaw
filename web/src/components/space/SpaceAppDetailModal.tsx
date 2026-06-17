import React, { useEffect, useState } from 'react';
import {
  Modal, Descriptions, Tag, Collapse, Empty, Spin, Alert, Typography, theme,
  Button, Space, message,
} from 'antd';
import {
  ApiOutlined, ToolOutlined, ReloadOutlined, ClearOutlined, FileTextOutlined, PoweroffOutlined, RobotOutlined,
} from '@ant-design/icons';

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

interface LogsResponse {
  appId: string;
  path: string;
  exists: boolean;
  size: number;
  maxBytes: number;
  content: string;
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
  const [logs, setLogs] = useState<LogsResponse | null>(null);
  const [logsLoading, setLogsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [restarting, setRestarting] = useState(false);

  const restartApp = async () => {
    if (!app) return;
    setRestarting(true);
    try {
      const res = await fetch(`/api/space/apps/${encodeURIComponent(app.id)}/restart`, {
        method: 'POST',
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      message.success('Đã gửi lệnh restart app server');
      // Wait a moment and reload MCP status
      setTimeout(() => {
        loadMcpStatus();
        loadLogs();
      }, 1500);
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Lỗi restart app');
    } finally {
      setRestarting(false);
    }
  };

  const loadMcpStatus = () => {
    if (!app) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
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
    return () => { cancelled = true; };
  };


  const loadLogs = async (targetApp = app) => {
    if (!targetApp) return;
    setLogsLoading(true);
    try {
      const res = await fetch(
        `/api/space/apps/${encodeURIComponent(targetApp.id)}/logs?max_bytes=131072`
      );
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      setLogs(await res.json());
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Không tải được log');
    } finally {
      setLogsLoading(false);
    }
  };

  const clearLogs = async () => {
    if (!app) return;
    setLogsLoading(true);
    try {
      const res = await fetch(`/api/space/apps/${encodeURIComponent(app.id)}/logs`, {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      await loadLogs(app);
      message.success('Đã xóa log');
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Không xóa được log');
      setLogsLoading(false);
    }
  };

  useEffect(() => {
    if (!open || !app) return;
    setMcp(null);
    setLogs(null);
    const cancelMcp = loadMcpStatus();
    loadLogs(app);
    return () => {
      cancelMcp?.();
    };
  }, [open, app]);

  const declared = mcp?.declared ?? null;
  const server = mcp?.server ?? null;
  const tools = server?.tools ?? [];

  return (
    <Modal
      title={
        <div className="flex items-center gap-2 flex-1 w-full justify-between pr-8">
          <div className="flex items-center gap-2">
            <span>{app?.icon ?? '🔌'}</span>
            <span>{app?.name ?? 'App'}</span>
            {app && <Tag>{app.id}</Tag>}
          </div>
          <Button 
            size="small" 
            type="primary" 
            icon={<PoweroffOutlined />} 
            onClick={restartApp}
            loading={restarting}
            disabled={!app?.manifest?.runtime}
          >
            Re-load App
          </Button>
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
                <Tag color="cyan">SenClaw bridge</Tag>
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

          <Collapse
            defaultActiveKey={['mcp', 'logs', 'skills']}
            items={[
              ...(app.manifest?.skills && Array.isArray(app.manifest.skills) && app.manifest.skills.length > 0 ? [{
                key: 'skills',
                label: (
                  <div className="flex items-center gap-2">
                    <RobotOutlined style={{ color: token.colorPrimary }} />
                    <span className="font-semibold" style={{ color: token.colorText }}>
                      Agent Skills
                    </span>
                    <Tag color="purple">{app.manifest.skills.length} skills</Tag>
                  </div>
                ),
                children: (
                  <div className="flex flex-col gap-2">
                    {app.manifest.skills.map((skill: any, idx: number) => (
                      <div key={idx} className="p-2 rounded border" style={{ borderColor: token.colorBorderSecondary, background: token.colorFillQuaternary }}>
                        <div className="font-medium">{skill.name || 'Unnamed Skill'}</div>
                        <div className="text-xs text-gray-500 mt-1">{skill.description || 'No description provided'}</div>
                      </div>
                    ))}
                  </div>
                )
              }] : []),
              {
                key: 'mcp',
                label: (
                  <div className="flex items-center gap-2">
                    <ApiOutlined style={{ color: token.colorPrimary }} />
                    <span className="font-semibold" style={{ color: token.colorText }}>
                      MCP Server
                    </span>
                    {server?.status && (
                      <Tag color={STATUS_COLOR[server.status] ?? 'default'}>{server.status}</Tag>
                    )}
                    {declared?.autoRegister && <Tag color="geekblue">auto-register</Tag>}
                  </div>
                ),
                children: (
                  <>
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
                              {declared.launch.port
                                ? <Tag className="ml-2">:{declared.launch.port}</Tag>
                                : null}
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
                                <div className="flex items-center gap-2 min-w-0 w-full">
                                  <Text
                                    code
                                    className="text-xs flex-shrink-0"
                                    style={{
                                      maxWidth: 180,
                                      overflow: 'hidden',
                                      textOverflow: 'ellipsis',
                                      whiteSpace: 'nowrap',
                                    }}
                                    title={t.name}
                                  >
                                    {t.name}
                                  </Text>
                                  <Text
                                    type="secondary"
                                    className="text-xs min-w-0 flex-1"
                                    style={{
                                      overflow: 'hidden',
                                      textOverflow: 'ellipsis',
                                      whiteSpace: 'nowrap',
                                    }}
                                    title={(t.description ?? '').split('\n')[0]}
                                  >
                                    {(t.description ?? '').split('\n')[0]}
                                  </Text>
                                </div>
                              ),
                              children: (
                                <div className="min-w-0">
                                  {t.description && (
                                    <Paragraph
                                      type="secondary"
                                      className="text-xs whitespace-pre-wrap"
                                      style={{ wordBreak: 'break-word' }}
                                    >
                                      {t.description}
                                    </Paragraph>
                                  )}
                                  {t.inputSchema && (
                                    <pre
                                      className="text-xs p-2 rounded overflow-auto"
                                      style={{
                                        background: token.colorFillTertiary,
                                        maxHeight: 220,
                                        whiteSpace: 'pre-wrap',
                                        wordBreak: 'break-word',
                                      }}
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
                ),
              },
              {
                key: 'logs',
                label: (
                  <div className="flex items-center gap-2">
                    <FileTextOutlined style={{ color: token.colorPrimary }} />
                    <span className="font-semibold" style={{ color: token.colorText }}>
                      Runtime logs
                    </span>
                    {logs?.size ? <Tag>{Math.round(logs.size / 1024)} KB</Tag> : <Tag>empty</Tag>}
                  </div>
                ),
                children: (
                  <div>
                    <Space className="mb-2" wrap>
                      <Button
                        size="small"
                        icon={<ReloadOutlined />}
                        onClick={() => loadLogs()}
                        loading={logsLoading}
                      >
                        Refresh
                      </Button>
                      <Button
                        size="small"
                        icon={<ClearOutlined />}
                        onClick={clearLogs}
                        disabled={logsLoading}
                      >
                        Clear
                      </Button>
                      {logs?.path && (
                        <Text
                          code
                          className="text-xs"
                          style={{
                            maxWidth: '100%',
                            whiteSpace: 'normal',
                            wordBreak: 'break-all',
                          }}
                        >
                          {logs.path}
                        </Text>
                      )}
                    </Space>
                    {logsLoading && !logs ? (
                      <div className="py-6 flex justify-center">
                        <Spin />
                      </div>
                    ) : (
                      <pre
                        className="text-xs p-3 rounded overflow-auto whitespace-pre-wrap"
                        style={{
                          background: token.colorFillTertiary,
                          border: `1px solid ${token.colorBorderSecondary}`,
                          maxHeight: 320,
                          minHeight: 140,
                        }}
                      >
                        {logs?.content || 'Chưa có log runtime. Log sẽ xuất hiện sau khi app server chạy.'}
                      </pre>
                    )}
                  </div>
                ),
              },
            ]}
          />
        </>
      )}
    </Modal>
  );
}
