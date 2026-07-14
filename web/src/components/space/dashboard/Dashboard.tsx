import React, { useState, useEffect, useCallback } from 'react';
import { Typography, Button, Empty, theme, Modal, Card, Badge } from 'antd';
import { PlusOutlined, AppstoreOutlined, DeleteOutlined, SettingOutlined } from '@ant-design/icons';

const { Text } = Typography;

export interface WidgetDef {
  id: string;
  name: string;
  description?: string;
  size: 'small' | 'medium' | 'large';
  refreshMs?: number;
  entryUrl: string;
  render: 'client' | 'server';
}

export interface AppWithWidgets {
  appId: string;
  appName: string;
  appIcon?: string;
  baseUrl: string;
  widgets: WidgetDef[];
}

interface PlacedWidget {
  instanceId: string;
  appId: string;
  widgetId: string;
  order: number;
}

const SIZE_MAP: Record<string, { col: number; minH: number }> = {
  small: { col: 1, minH: 180 },
  medium: { col: 2, minH: 180 },
  large: { col: 2, minH: 340 },
};

const STORAGE_KEY = 'senclaw:dashboard:widgets';

function loadPlaced(): PlacedWidget[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch { return []; }
}

function savePlaced(items: PlacedWidget[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
}

interface WidgetFrameProps {
  widget: WidgetDef;
  baseUrl: string;
  appIcon?: string;
  appName: string;
  onRemove: () => void;
  isDarkMode: boolean;
}

function WidgetFrame({ widget, baseUrl, appIcon, appName, onRemove, isDarkMode }: WidgetFrameProps) {
  const { token } = theme.useToken();
  const size = SIZE_MAP[widget.size] ?? SIZE_MAP.small;
  const iframeSrc = `${baseUrl.replace(/\/$/, '')}${widget.entryUrl}`;
  const [hovered, setHovered] = useState(false);

  return (
    <div
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        gridColumn: `span ${size.col}`,
        minHeight: size.minH,
        borderRadius: 16,
        overflow: 'hidden',
        background: token.colorBgContainer,
        border: `1px solid ${token.colorBorderSecondary}`,
        boxShadow: '0 1px 4px rgba(0,0,0,0.06)',
        position: 'relative',
        transition: 'box-shadow 0.2s',
      }}
    >
      {hovered && (
        <div style={{
          position: 'absolute', top: 6, right: 6, zIndex: 10,
          display: 'flex', gap: 4,
        }}>
          <Button
            type="text" size="small" danger icon={<DeleteOutlined />}
            onClick={onRemove}
            style={{ background: 'rgba(0,0,0,0.5)', color: '#fff', borderRadius: 8 }}
          />
        </div>
      )}
      <div style={{
        position: 'absolute', bottom: 0, left: 0, right: 0,
        padding: '6px 12px',
        background: 'linear-gradient(transparent, rgba(0,0,0,0.4))',
        display: 'flex', alignItems: 'center', gap: 6,
        zIndex: 5, pointerEvents: 'none',
      }}>
        <span style={{ fontSize: 14 }}>{appIcon ?? '▣'}</span>
        <Text style={{ fontSize: 11, color: '#fff', opacity: 0.9 }}>{widget.name}</Text>
      </div>
      <iframe
        title={widget.name}
        src={`${iframeSrc}${iframeSrc.includes('?') ? '&' : '?'}theme=${isDarkMode ? 'dark' : 'light'}`}
        style={{
          width: '100%', height: '100%', border: 0,
          minHeight: size.minH,
          background: 'transparent',
        }}
        sandbox="allow-scripts allow-same-origin"
      />
    </div>
  );
}

interface DashboardProps {
  apps: AppWithWidgets[];
  isDarkMode: boolean;
}

export function Dashboard({ apps, isDarkMode }: DashboardProps) {
  const { token } = theme.useToken();
  const [placed, setPlaced] = useState<PlacedWidget[]>(() => loadPlaced());
  const [showPicker, setShowPicker] = useState(false);

  useEffect(() => { savePlaced(placed); }, [placed]);

  const addWidget = useCallback((appId: string, widgetId: string) => {
    const instanceId = `${appId}:${widgetId}:${Date.now()}`;
    setPlaced(prev => [...prev, { instanceId, appId, widgetId, order: prev.length }]);
    setShowPicker(false);
  }, []);

  const removeWidget = useCallback((instanceId: string) => {
    setPlaced(prev => prev.filter(w => w.instanceId !== instanceId));
  }, []);

  const resolveWidget = (p: PlacedWidget) => {
    const app = apps.find(a => a.appId === p.appId);
    if (!app) return null;
    const widget = app.widgets.find(w => w.id === p.widgetId);
    if (!widget) return null;
    return { app, widget };
  };

  const availableWidgets = apps.flatMap(app =>
    app.widgets.map(w => ({ app, widget: w }))
  );

  return (
    <div className="h-full flex flex-col">
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <AppstoreOutlined />
        <Text strong className="flex-1" style={{ fontSize: 14 }}>Dashboard</Text>
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          onClick={() => setShowPicker(true)}
          disabled={availableWidgets.length === 0}
        >
          Thêm widget
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto p-4">
        {placed.length === 0 ? (
          <Empty
            image={<SettingOutlined style={{ fontSize: 48, color: token.colorTextQuaternary }} />}
            description={
              <div>
                <div style={{ marginBottom: 8 }}>Chưa có widget nào trên dashboard</div>
                {availableWidgets.length > 0 ? (
                  <Button type="link" size="small" onClick={() => setShowPicker(true)}>
                    Thêm widget đầu tiên
                  </Button>
                ) : (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    Cài đặt app có hỗ trợ widget để bắt đầu
                  </Text>
                )}
              </div>
            }
            className="py-12"
          />
        ) : (
          <div style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(2, 1fr)',
            gap: 16,
            maxWidth: 800,
            margin: '0 auto',
          }}>
            {placed.map(p => {
              const resolved = resolveWidget(p);
              if (!resolved) return null;
              return (
                <WidgetFrame
                  key={p.instanceId}
                  widget={resolved.widget}
                  baseUrl={resolved.app.baseUrl}
                  appIcon={resolved.app.appIcon}
                  appName={resolved.app.appName}
                  isDarkMode={isDarkMode}
                  onRemove={() => removeWidget(p.instanceId)}
                />
              );
            })}
          </div>
        )}
      </div>

      <Modal
        title="Thêm Widget"
        open={showPicker}
        onCancel={() => setShowPicker(false)}
        footer={null}
        width={480}
      >
        {availableWidgets.length === 0 ? (
          <Empty description="Không có widget nào khả dụng" />
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
            {apps.filter(a => a.widgets.length > 0).map(app => (
              <div key={app.appId}>
                <Text strong style={{ fontSize: 13 }}>
                  <span style={{ marginRight: 6 }}>{app.appIcon ?? '▣'}</span>
                  {app.appName}
                </Text>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, marginTop: 8, marginBottom: 12 }}>
                  {app.widgets.map(w => (
                    <Card
                      key={w.id}
                      size="small"
                      hoverable
                      onClick={() => addWidget(app.appId, w.id)}
                      style={{ cursor: 'pointer' }}
                    >
                      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                        <Badge
                          count={SIZE_MAP[w.size]?.col === 2 ? 'W' : 'S'}
                          style={{
                            background: w.size === 'small' ? token.colorFillSecondary : token.colorPrimaryBg,
                            color: token.colorTextSecondary, fontSize: 10,
                          }}
                        />
                        <div>
                          <div style={{ fontWeight: 500, fontSize: 13 }}>{w.name}</div>
                          {w.description && (
                            <div style={{ fontSize: 11, color: token.colorTextSecondary }}>{w.description}</div>
                          )}
                        </div>
                      </div>
                    </Card>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </Modal>
    </div>
  );
}
