import { useEffect, useMemo, useState } from 'react';
import {
  Card, Button, Tag, Empty, Input, Typography, theme, Tooltip, Popconfirm,
  Alert,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, AppstoreOutlined, LinkOutlined,
  InfoCircleOutlined, SearchOutlined,
} from '@ant-design/icons';
import { SpaceAppDetailModal } from './SpaceAppDetailModal';
import { AppInstallDialog, type InstallSource } from './AppInstallDialog';
import { appMatches, normalizeApp, type SpaceApp, type SpaceAppRow } from './spaceApp';

const { Paragraph } = Typography;

interface Props {
  groupFolder: string;
  onAppsChanged?: () => void;
  onOpenApp?: (appId: string) => void;
}

export function AppsGallery({ groupFolder, onAppsChanged, onOpenApp }: Props) {
  const { token } = theme.useToken();
  const [apps, setApps] = useState<SpaceApp[]>([]);
  const [query, setQuery] = useState('');
  const [showInstall, setShowInstall] = useState(false);
  const [detailApp, setDetailApp] = useState<SpaceApp | null>(null);

  useEffect(() => {
    fetch('/api/space/apps')
      .then(r => r.ok ? r.json() : [])
      .then((rows: SpaceAppRow[]) => {
        // `integration.launcher: false` keeps an app out of the launcher
        // without pretending it has no UI. The engine apps (mlx-lm, candle)
        // use it: their screen is a settings panel, reached from Settings →
        // Models, and a tile that opened a model-management page from the app
        // grid would be a second, competing entry point to the same thing.
        setApps(rows.map(normalizeApp).filter(a => a.integration?.launcher !== false));
      })
      .catch(() => {});
  }, []);

  const handleRemove = (id: string) => {
    setApps(prev => prev.filter(a => a.id !== id));
    fetch(`/api/space/apps/${id}`, { method: 'DELETE' })
      .then(() => onAppsChanged?.())
      .catch(() => {});
  };

  /**
   * A newly installed app joins the grid immediately, and a store/ZIP install
   * opens it — an app installed by picking it is one the user wants to see. A
   * manifest-URL registration does not: that flow is how a developer points the
   * daemon at an app still being built, whose server may not be up yet.
   */
  const handleInstalled = (app: SpaceApp, from: InstallSource) => {
    if (app.integration?.launcher !== false) {
      setApps(prev => [...prev.filter(a => a.id !== app.id), app]);
    }
    onAppsChanged?.();
    if (from !== 'url') onOpenApp?.(app.id);
  };

  const visible = useMemo(() => apps.filter(a => appMatches(a, query)), [apps, query]);

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <span className="font-semibold text-sm" style={{ color: token.colorText }}>
          Micro-Frontend Apps
        </span>
        <Input
          allowClear
          size="small"
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder="Tìm app…"
          prefix={<SearchOutlined style={{ color: token.colorTextQuaternary }} />}
          style={{ maxWidth: 260 }}
          className="flex-1"
        />
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          onClick={() => setShowInstall(true)}
        >
          Cài app mới
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        <Alert
          type="info"
          showIcon
          className="mb-4"
          title="Space Apps"
          description="Kết nối dịch vụ năng suất như Google Workspace hoặc nhúng micro-frontend app qua manifest."
        />

        {apps.length === 0 && (
          <Empty
            image={<AppstoreOutlined style={{ fontSize: 48, color: token.colorTextQuaternary }} />}
            description={
              <span>
                Chưa có app nào.{' '}
                <Button type="link" size="small" onClick={() => setShowInstall(true)}>
                  Cài app đầu tiên
                </Button>
              </span>
            }
            className="py-8"
          />
        )}

        {apps.length > 0 && visible.length === 0 && (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            className="py-8"
            description={`Không có app nào khớp “${query}”`}
          />
        )}

        <div className="grid grid-cols-2 gap-3">
          {visible.map(app => (
            <Card
              key={app.id}
              size="small"
              hoverable
              extra={
                <Popconfirm
                  title="Gỡ app này?"
                  onConfirm={() => handleRemove(app.id)}
                  okText="Gỡ"
                  cancelText="Hủy"
                >
                  <Button type="text" size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              }
              title={
                <div className="flex items-center gap-2">
                  <span>{app.icon ?? '🔌'}</span>
                  <span>{app.name}</span>
                </div>
              }
            >
              <Paragraph type="secondary" className="text-xs mb-2" ellipsis={{ rows: 2 }}>
                {app.description ?? '—'}
              </Paragraph>
              <div className="flex items-center justify-between">
                <Tag color={app.integration.type === 'iframe' ? 'blue' : 'purple'}>
                  {app.integration.type}
                </Tag>
                <div className="flex items-center">
                  <Button
                    type="link"
                    size="small"
                    icon={<InfoCircleOutlined />}
                    onClick={() => setDetailApp(app)}
                  >
                    Chi tiết
                  </Button>
                  <Tooltip title={app.integration.url}>
                    <Button
                      type="link"
                      size="small"
                      icon={<LinkOutlined />}
                      onClick={() => onOpenApp?.(app.id)}
                    >
                      Mở
                    </Button>
                  </Tooltip>
                </div>
              </div>
            </Card>
          ))}
        </div>
      </div>

      <AppInstallDialog
        open={showInstall}
        onClose={() => setShowInstall(false)}
        onInstalled={handleInstalled}
      />

      <SpaceAppDetailModal
        app={detailApp}
        open={!!detailApp}
        onClose={() => setDetailApp(null)}
      />
    </div>
  );
}
