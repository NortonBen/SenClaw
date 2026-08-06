import React, { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  message,
  Popconfirm,
  Space,
  Tag,
  Typography,
  Upload,
  theme,
} from 'antd';
import {
  AppstoreOutlined, CloudDownloadOutlined, DeleteOutlined, InfoCircleOutlined,
  LinkOutlined, SafetyCertificateOutlined, SyncOutlined, UploadOutlined,
} from '@ant-design/icons';
import { SpaceAppDetailModal, type DetailApp } from '../space/SpaceAppDetailModal';
import ScanReportDialog, { readScanError, type ScanReport } from '../security/ScanReportDialog';
import SpaceAppSandboxModal from './SpaceAppSandboxModal';

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
  const [installing, setInstalling] = useState(false);
  const [scanState, setScanState] = useState<{
    report: ScanReport;
    file: File;
    blocked: boolean;
  } | null>(null);
  const [registering, setRegistering] = useState(false);
  const [detailApp, setDetailApp] = useState<DetailApp | null>(null);
  const [updates, setUpdates] = useState<Record<string, UpdateStatus>>({});
  const [checking, setChecking] = useState(false);
  const [updatingId, setUpdatingId] = useState<string | null>(null);
  const [sandboxApp, setSandboxApp] = useState<{ id: string; name: string } | null>(null);
  const [form] = Form.useForm();

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

  const installZip = async (file: File, force = false) => {
    setInstalling(true);
    try {
      const formData = new FormData();
      formData.append('file', file);
      if (force) formData.append('force', 'true');
      const res = await fetch('/api/space/apps/install-zip', { method: 'POST', body: formData });
      if (!res.ok) {
        const { blocked, error, scan } = await readScanError(res);
        if (blocked && scan) {
          setScanState({ report: scan, file, blocked: true });
          return;
        }
        throw new Error(error);
      }
      const row = (await res.json()) as { scan?: ScanReport };
      if (row?.scan?.findings?.length) {
        setScanState({ report: row.scan, file, blocked: false });
      } else {
        message.success('Space App installed');
      }
      await loadApps();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Install failed');
    } finally {
      setInstalling(false);
    }
  };

  const registerManifest = async () => {
    const values = await form.validateFields();
    setRegistering(true);
    try {
      const res = await fetch('/api/space/apps/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ manifest_url: values.manifest_url }),
      });
      if (!res.ok) throw new Error(await res.text());
      message.success('Space App registered');
      form.resetFields();
      await loadApps();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Register failed');
    } finally {
      setRegistering(false);
    }
  };

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

  return (
    <div style={{ padding: '24px', maxWidth: 980, margin: '0 auto', width: '100%' }}>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Space Apps</Title>
          <Text type="secondary">Install, register, and remove embedded Space Apps.</Text>
        </div>
        <Space>
          <Button
            icon={<SyncOutlined spin={checking} />}
            loading={checking}
            onClick={() => checkUpdates(true)}
          >
            Check updates
          </Button>
          <Upload
            accept=".zip"
            showUploadList={false}
            beforeUpload={file => {
              installZip(file);
              return false;
            }}
          >
            <Button type="primary" icon={<UploadOutlined />} loading={installing}>
              Install ZIP
            </Button>
          </Upload>
        </Space>
      </Space>

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Space App package contract"
        description="A ZIP app must contain senclaw-manifest.json or senclaw-app.json at the archive root. Static Next.js exports are served from /api/space/apps/:id/static/index.html and appear as child items under Apps in the Space sidebar."
      />

      <Card size="small" style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }}>
        <Form form={form} layout="inline" style={{ gap: 8 }}>
          <Form.Item
            name="manifest_url"
            rules={[{ required: true, type: 'url', message: 'Enter a manifest URL' }]}
            style={{ flex: 1, marginBottom: 0 }}
          >
            <Input prefix={<LinkOutlined />} placeholder="https://app.example.com/senclaw-manifest.json" />
          </Form.Item>
          <Button onClick={registerManifest} loading={registering}>
            Register URL
          </Button>
        </Form>
      </Card>

      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        {apps.map(app => {
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
              </Space>
            </Card>
          );
        })}
        {!loading && apps.length === 0 && (
          <Card style={{ borderColor: token.colorBorderSecondary }}>
            <Text type="secondary">No Space Apps installed.</Text>
          </Card>
        )}
      </Space>

      <SpaceAppDetailModal
        app={detailApp}
        open={!!detailApp}
        onClose={() => setDetailApp(null)}
      />

      <SpaceAppSandboxModal
        appId={sandboxApp?.id ?? null}
        appName={sandboxApp?.name}
        open={!!sandboxApp}
        onClose={() => setSandboxApp(null)}
      />

      <ScanReportDialog
        open={!!scanState}
        report={scanState?.report}
        blocked={!!scanState?.blocked}
        busy={installing}
        onCancel={() => setScanState(null)}
        onForceInstall={() => {
          const f = scanState?.file;
          setScanState(null);
          if (f) void installZip(f, true);
        }}
      />
    </div>
  );
};
