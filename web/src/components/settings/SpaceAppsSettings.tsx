import React, { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Empty,
  Input,
  message,
  Modal,
  Popconfirm,
  Space,
  Tag,
  Typography,
  theme,
} from 'antd';
import {
  AppstoreOutlined, CloudDownloadOutlined, DeleteOutlined, InfoCircleOutlined, SettingOutlined,
  PlayCircleOutlined, PlusOutlined, PoweroffOutlined, SafetyCertificateOutlined,
  SearchOutlined, SyncOutlined,
} from '@ant-design/icons';
import { SpaceAppDetailModal, type DetailApp } from '../space/SpaceAppDetailModal';
import { AppInstallDialog } from '../space/AppInstallDialog';
import { appMatches } from '../space/spaceApp';
import SpaceAppSandboxModal from './SpaceAppSandboxModal';
import AppTokenModeCard from './AppTokenModeCard';

const { Title, Text, Paragraph } = Typography;

interface SpaceAppRow {
  id: string;
  manifest: any;
  enabled: boolean;
  installed_at: number;
}

interface UpdateStatus {
  id: string;
  slug: string;
  installed?: string | null;
  latest?: string | null;
  hasUpdate: boolean;
  yanked?: boolean;
  deprecated?: string | null;
  error?: string | null;
}

export const SpaceAppsSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [apps, setApps] = useState<SpaceAppRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState('');
  const [showInstall, setShowInstall] = useState(false);
  const [detailApp, setDetailApp] = useState<DetailApp | null>(null);
  // App whose backend-settings page is open in the iframe modal — the way an
  // app hidden from the launcher (`integration.launcher: false`) exposes its
  // management UI. The iframe goes through the daemon's app proxy, which also
  // starts a stopped session app on the first request.
  const [settingsApp, setSettingsApp] = useState<{ id: string; name: string } | null>(null);
  const [updates, setUpdates] = useState<Record<string, UpdateStatus>>({});
  const [checking, setChecking] = useState(false);
  const [updatingId, setUpdatingId] = useState<string | null>(null);
  const [sandboxApp, setSandboxApp] = useState<{ id: string; name: string } | null>(null);
  const [lifecycleBusy, setLifecycleBusy] = useState<string | null>(null);

  // Stop / start an app's server process by hand. Stopping a session app just
  // does early what the idle timer would do; stopping a background one is an
  // override the supervisor honours until it is started again.
  const setRunning = async (id: string, run: boolean) => {
    setLifecycleBusy(id);
    try {
      const r = await fetch(`/api/space/apps/${encodeURIComponent(id)}/${run ? 'start' : 'stop'}`, {
        method: 'POST',
      });
      const body = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(body?.error ?? `HTTP ${r.status}`);
      message.success(body?.note ?? (run ? 'Started' : 'Stopped'));
    } catch (e: any) {
      message.error(e?.message ?? 'Failed');
    } finally {
      setLifecycleBusy(null);
    }
  };

  const loadApps = async () => {
    setLoading(true);
    try {
      const data = await fetch('/api/space/apps').then(r => r.ok ? r.json() : []);
      setApps(Array.isArray(data) ? data : []);
    } catch {
      setApps([]);
    } finally {
      setLoading(false);
    }
  };

  // Ask the hub which installed apps have a newer version. Non-fatal: a hub
  // that is unreachable just leaves the badges absent.
  const checkUpdates = async (announce = false) => {
    setChecking(true);
    try {
      const data: UpdateStatus[] = await fetch('/api/space/apps/updates')
        .then(r => (r.ok ? r.json() : []));
      const map: Record<string, UpdateStatus> = {};
      for (const u of Array.isArray(data) ? data : []) map[u.id] = u;
      setUpdates(map);
      if (announce) {
        const n = Object.values(map).filter(u => u.hasUpdate).length;
        message[n > 0 ? 'info' : 'success'](
          n > 0 ? `${n} app có bản mới` : 'Mọi app đã ở phiên bản mới nhất',
        );
      }
    } catch {
      /* leave badges absent */
    } finally {
      setChecking(false);
    }
  };

  const updateApp = async (id: string) => {
    setUpdatingId(id);
    try {
      const res = await fetch(`/api/space/apps/${encodeURIComponent(id)}/update`, { method: 'POST' });
      const body = await res.json().catch(() => ({}));
      if (!res.ok) throw new Error(body?.error || 'Update failed');
      if (body?.updated) message.success(`${id} → ${body.latest}`);
      else message.info(`${id} đã ở bản mới nhất`);
      await loadApps();
      await checkUpdates();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Update failed');
    } finally {
      setUpdatingId(null);
    }
  };

  useEffect(() => {
    loadApps().then(() => checkUpdates());
  }, []);

  const uninstall = async (id: string) => {
    try {
      const res = await fetch(`/api/space/apps/${encodeURIComponent(id)}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      message.success('Space App uninstalled');
      setApps(prev => prev.filter(app => app.id !== id));
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Uninstall failed');
    }
  };

  // Search filters what is rendered, never what is loaded — an app hidden by a
  // query is still installed, still running, and still counted by the updates
  // check.
  const visible = apps.filter(app =>
    appMatches(
      { id: app.id, name: app.manifest?.name, description: app.manifest?.description },
      query,
    ),
  );

  return (
    <div style={{ padding: '24px', maxWidth: 980, margin: '0 auto', width: '100%' }}>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Space Apps</Title>
          <Text type="secondary">Install, register, and remove embedded Space Apps.</Text>
        </div>
        <Space>
          <Input
            allowClear
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Tìm app…"
            prefix={<SearchOutlined />}
            style={{ width: 220 }}
          />
          <Button
            icon={<SyncOutlined spin={checking} />}
            loading={checking}
            onClick={() => checkUpdates(true)}
          >
            Check updates
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowInstall(true)}>
            Cài app mới
          </Button>
        </Space>
      </Space>

      <AppTokenModeCard />

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Space App package contract"
        description="A ZIP app must contain senclaw-manifest.json or senclaw-app.json at the archive root. Static Next.js exports are served from /api/space/apps/:id/static/index.html and appear as child items under Apps in the Space sidebar."
      />

      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        {visible.map(app => {
          const manifest = app.manifest ?? {};
          const integration = manifest.integration ?? {};
          const upd = updates[app.id];
          const detail: DetailApp = {
            id: app.id,
            name: manifest.name ?? app.id,
            description: manifest.description,
            icon: manifest.icon,
            integration: {
              type: integration.type ?? 'iframe',
              url: integration.url ?? 'no url',
            },
            manifest,
          };
          return (
            <Card
              key={app.id}
              size="small"
              style={{ borderColor: token.colorBorderSecondary }}
              title={
                <Space>
                  <AppstoreOutlined />
                  <span>{manifest.name ?? app.id}</span>
                  <Tag>{app.id}</Tag>
                  {manifest.runtime?.kind === 'server' && (
                    <Tag color={manifest.runtime?.mode === 'background' ? 'volcano' : 'default'}>
                      {manifest.runtime?.mode === 'background' ? 'always on' : 'on demand'}
                    </Tag>
                  )}
                  {manifest.install?.type === 'zip' && <Tag color="green">ZIP</Tag>}
                  {upd?.hasUpdate && (
                    <Tag color="orange">
                      {(upd.installed ?? '?')} → {upd.latest}
                    </Tag>
                  )}
                </Space>
              }
              extra={
                <Space>
                  {upd?.hasUpdate && (
                    <Button
                      type="primary"
                      size="small"
                      icon={<CloudDownloadOutlined />}
                      loading={updatingId === app.id}
                      onClick={() => updateApp(app.id)}
                    >
                      Update
                    </Button>
                  )}
                  <Popconfirm
                    title="Uninstall this Space App?"
                    description="Local files installed from ZIP will be removed."
                    okText="Uninstall"
                    okButtonProps={{ danger: true }}
                    onConfirm={() => uninstall(app.id)}
                  >
                    <Button danger type="text" icon={<DeleteOutlined />} />
                  </Popconfirm>
                </Space>
              }
            >
              <Paragraph type="secondary" style={{ marginBottom: 8 }}>
                {manifest.description ?? 'No description'}
              </Paragraph>
              <Space wrap>
                <Tag color={integration.type === 'iframe' ? 'blue' : 'purple'}>
                  {integration.type ?? 'iframe'}
                </Tag>
                <Tag>{integration.url ?? 'no url'}</Tag>
                {manifest.bridge?.postMessage && <Tag color="cyan">SenClaw bridge</Tag>}
                {integration.settings === true && (
                  <Button
                    size="small"
                    type="primary"
                    ghost
                    icon={<SettingOutlined />}
                    onClick={() => setSettingsApp({ id: app.id, name: detail.name })}
                  >
                    Model & cài đặt
                  </Button>
                )}
                <Button
                  size="small"
                  icon={<InfoCircleOutlined />}
                  onClick={() => setDetailApp(detail)}
                >
                  Details & logs
                </Button>
                <Button
                  size="small"
                  icon={<SafetyCertificateOutlined />}
                  onClick={() => setSandboxApp({ id: app.id, name: detail.name })}
                >
                  Sandbox
                </Button>
                {manifest.runtime?.kind === 'server' && (
                  <>
                    <Button
                      size="small"
                      icon={<PlayCircleOutlined />}
                      loading={lifecycleBusy === app.id}
                      onClick={() => setRunning(app.id, true)}
                    >
                      Start
                    </Button>
                    <Button
                      size="small"
                      icon={<PoweroffOutlined />}
                      loading={lifecycleBusy === app.id}
                      onClick={() => setRunning(app.id, false)}
                    >
                      Stop
                    </Button>
                  </>
                )}
              </Space>
            </Card>
          );
        })}
        {!loading && apps.length === 0 && (
          <Card style={{ borderColor: token.colorBorderSecondary }}>
            <Text type="secondary">No Space Apps installed.</Text>
          </Card>
        )}
        {!loading && apps.length > 0 && visible.length === 0 && (
          <Card style={{ borderColor: token.colorBorderSecondary }}>
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={`Không có app nào khớp “${query}”`}
            />
          </Card>
        )}
      </Space>

      <SpaceAppDetailModal
        app={detailApp}
        open={!!detailApp}
        onClose={() => setDetailApp(null)}
      />

            <Modal
        title={settingsApp ? `${settingsApp.name} — Model & cài đặt` : ''}
        open={!!settingsApp}
        onCancel={() => setSettingsApp(null)}
        footer={null}
        width="min(960px, 94vw)"
        destroyOnClose
        styles={{ body: { padding: 0, height: '72vh' } }}
      >
        {settingsApp && (
          <iframe
            title={settingsApp.name}
            src={`/api/space/apps/${encodeURIComponent(settingsApp.id)}/proxy/`}
            style={{ width: '100%', height: '100%', border: 'none', display: 'block' }}
            sandbox="allow-scripts allow-same-origin allow-forms"
          />
        )}
      </Modal>
      <SpaceAppSandboxModal
        appId={sandboxApp?.id ?? null}
        appName={sandboxApp?.name}
        open={!!sandboxApp}
        onClose={() => setSandboxApp(null)}
      />

      <AppInstallDialog
        open={showInstall}
        onClose={() => setShowInstall(false)}
        onInstalled={() => {
          void loadApps().then(() => checkUpdates());
        }}
      />
    </div>
  );
};
