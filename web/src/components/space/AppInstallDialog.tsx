/**
 * "Install a new app" dialog for the Apps screen — the three ways a Space App
 * can arrive, in one place:
 *
 *   • Store — the hub catalog. Entries with `kind: "app"` install by slug
 *     through `POST /api/marketplace/hub/install`, which resolves the version,
 *     downloads the artifact and verifies the SHA-512 the hub published before
 *     handing the bytes to the installer. That digest check is why the store
 *     tab does not simply link to a download URL.
 *   • ZIP — a bundle on disk, `POST /api/space/apps/install-zip`.
 *   • Manifest URL — an app that serves its own `senclaw-manifest.json`,
 *     `POST /api/space/apps/register`.
 *
 * All three run the pre-install security scan and answer with the same
 * `{blocked, error, scan}` body, so one `ScanReportDialog` covers them: a
 * refused install (422) opens it with the override, a successful install that
 * still produced findings opens it read-only.
 */
import { useCallback, useEffect, useState } from 'react';
import {
  Alert, Button, Empty, Form, Input, List, Modal, Spin, Tabs, Tag, Tooltip,
  Typography, Upload, message, theme,
} from 'antd';
import {
  CloudDownloadOutlined, InboxOutlined, LinkOutlined, ReloadOutlined,
  SearchOutlined, ShopOutlined,
} from '@ant-design/icons';
import ScanReportDialog, { readScanError, type ScanReport } from '../security/ScanReportDialog';
import { appMatches, normalizeApp, type SpaceApp } from './spaceApp';

const { Paragraph, Text } = Typography;

/** Which tab an install came from — the caller opens the app for store/zip only. */
export type InstallSource = 'store' | 'zip' | 'url';

/** One hub-catalog entry, narrowed to the fields an app row needs. */
interface StoreApp {
  name: string;
  description: string;
  version?: string;
  author?: string;
  slug: string;
  downloads?: number;
  installed: boolean;
  installedVersion?: string;
  updateAvailable?: boolean;
  sourceName?: string;
}

interface Props {
  open: boolean;
  onClose: () => void;
  onInstalled: (app: SpaceApp, from: InstallSource) => void;
}

export function AppInstallDialog({ open, onClose, onInstalled }: Props) {
  const { token } = theme.useToken();
  const [tab, setTab] = useState<InstallSource>('store');

  // Store catalog
  const [catalog, setCatalog] = useState<StoreApp[]>([]);
  const [loadingCatalog, setLoadingCatalog] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [storeQuery, setStoreQuery] = useState('');
  const [busySlug, setBusySlug] = useState<string | null>(null);

  // ZIP + manifest URL
  const [installingZip, setInstallingZip] = useState(false);
  const [registering, setRegistering] = useState(false);
  const [form] = Form.useForm();

  // Pending install whose scan report the user is reading. `retry` re-runs
  // exactly the install that produced it, with force — so this dialog needs no
  // memory of which tab the install came from.
  const [scanState, setScanState] = useState<{
    report: ScanReport;
    target: string;
    blocked: boolean;
    retry: () => void;
  } | null>(null);

  const loadCatalog = useCallback(async () => {
    setLoadingCatalog(true);
    setCatalogError(null);
    try {
      const res = await fetch('/api/marketplace/sources');
      if (!res.ok) throw new Error(await res.text());
      const data = await res.json();
      const sources: Array<{ id: string; name: string; enabled: boolean }> =
        (data.sources ?? []).filter((s: any) => s.enabled !== false);

      const lists = await Promise.all(
        sources.map(async src => {
          try {
            const r = await fetch(`/api/marketplace/sources/${encodeURIComponent(src.id)}`);
            if (!r.ok) return [] as StoreApp[];
            const d = await r.json();
            // Only registry entries install as apps: a `marketplace.json` entry
            // has no slug and installs by git clone, which is another endpoint
            // and not a Space App at all.
            return (d.plugins ?? [])
              .filter((p: any) => p.kind === 'app' && p.slug)
              .map((p: any): StoreApp => ({
                name: p.name,
                description: p.description ?? '',
                version: p.version,
                author: p.author,
                slug: p.slug,
                downloads: p.downloads,
                installed: !!p.installed,
                installedVersion: p.installedVersion,
                updateAvailable: !!p.updateAvailable,
                sourceName: src.name,
              }));
          } catch {
            // One unreachable source must not empty the whole store.
            return [] as StoreApp[];
          }
        }),
      );

      const bySlug = new Map<string, StoreApp>();
      for (const app of lists.flat()) if (!bySlug.has(app.slug)) bySlug.set(app.slug, app);
      setCatalog([...bySlug.values()].sort((a, b) => a.name.localeCompare(b.name)));
    } catch (err) {
      setCatalogError(err instanceof Error ? err.message : String(err));
      setCatalog([]);
    } finally {
      setLoadingCatalog(false);
    }
  }, []);

  useEffect(() => {
    if (open && tab === 'store' && !catalog.length && !loadingCatalog && !catalogError) {
      void loadCatalog();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, tab]);

  /** Hand a freshly installed row to the caller and close. */
  const finish = (row: { id: string; manifest: any; enabled: boolean }, from: InstallSource) => {
    onInstalled(normalizeApp(row), from);
    onClose();
  };

  const installFromStore = async (app: StoreApp, force = false) => {
    setBusySlug(app.slug);
    try {
      const res = await fetch('/api/marketplace/hub/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ slug: app.slug, force }),
      });
      if (!res.ok) {
        const { blocked, error, scan } = await readScanError(res);
        if (blocked && scan) {
          setScanState({
            report: scan,
            target: app.name,
            blocked: true,
            retry: () => void installFromStore(app, true),
          });
          return;
        }
        throw new Error(error);
      }
      const row = await res.json() as {
        id: string; manifest: any; enabled: boolean; scan?: ScanReport;
      };
      if (row.scan?.findings?.length) {
        // Installed, but with findings worth reading before the app is opened.
        setScanState({ report: row.scan, target: app.name, blocked: false, retry: () => {} });
        setCatalog(prev => prev.map(p => (p.slug === app.slug ? { ...p, installed: true } : p)));
        onInstalled(normalizeApp(row), 'store');
        return;
      }
      message.success(`Đã cài ${row.manifest?.name ?? app.name}`);
      finish(row, 'store');
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Cài app từ cửa hàng thất bại');
    } finally {
      setBusySlug(null);
    }
  };

  const installZip = async (file: File, force = false) => {
    setInstallingZip(true);
    try {
      const formData = new FormData();
      formData.append('file', file);
      // A Space App's `runtime.start` runs at install time, so an override has
      // to be an explicit act — never a silent retry.
      if (force) formData.append('force', 'true');
      const res = await fetch('/api/space/apps/install-zip', { method: 'POST', body: formData });
      if (!res.ok) {
        const { blocked, error, scan } = await readScanError(res);
        if (blocked && scan) {
          setScanState({
            report: scan,
            target: file.name,
            blocked: true,
            retry: () => void installZip(file, true),
          });
          return;
        }
        throw new Error(error);
      }
      const row = await res.json() as {
        id: string; manifest: any; enabled: boolean; scan?: ScanReport;
      };
      if (row.scan?.findings?.length) {
        setScanState({ report: row.scan, target: file.name, blocked: false, retry: () => {} });
        onInstalled(normalizeApp(row), 'zip');
        return;
      }
      message.success(`Đã cài ${row.manifest?.name ?? row.id}`);
      finish(row, 'zip');
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Cài từ ZIP thất bại');
    } finally {
      setInstallingZip(false);
    }
  };

  const register = async () => {
    let vals: { manifest_url: string };
    try {
      vals = await form.validateFields();
    } catch {
      return; // validation error — the form shows it
    }
    setRegistering(true);
    try {
      const res = await fetch('/api/space/apps/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ manifest_url: vals.manifest_url }),
      });
      if (!res.ok) throw new Error((await readScanError(res)).error);
      const row = await res.json() as { id: string; manifest: any; enabled: boolean };
      form.resetFields();
      message.success(`Đã đăng ký ${row.manifest?.name ?? row.id}`);
      finish(row, 'url');
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Đăng ký app thất bại');
    } finally {
      setRegistering(false);
    }
  };

  const visible = catalog.filter(app =>
    appMatches({ name: app.name, description: app.description, id: app.slug }, storeQuery),
  );

  const storeTab = (
    <div className="flex flex-col" style={{ minHeight: 320 }}>
      <div className="flex items-center gap-2 mb-3">
        <Input
          allowClear
          value={storeQuery}
          onChange={e => setStoreQuery(e.target.value)}
          placeholder="Tìm app trong cửa hàng…"
          prefix={<SearchOutlined style={{ color: token.colorTextQuaternary }} />}
        />
        <Tooltip title="Tải lại danh mục">
          <Button
            icon={<ReloadOutlined />}
            loading={loadingCatalog}
            onClick={() => void loadCatalog()}
          />
        </Tooltip>
      </div>

      {catalogError && (
        <Alert
          type="warning"
          showIcon
          className="mb-3"
          title="Không đọc được danh mục cửa hàng"
          description={catalogError}
        />
      )}

      {loadingCatalog ? (
        <div className="flex-1 flex items-center justify-center py-8">
          <Spin />
        </div>
      ) : !visible.length ? (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          className="py-8"
          description={
            catalog.length
              ? 'Không có app nào khớp từ khóa'
              : 'Cửa hàng chưa có app — thêm nguồn hub ở Plugins → Marketplace rồi đồng bộ'
          }
        />
      ) : (
        <List
          size="small"
          dataSource={visible}
          style={{ maxHeight: 380, overflowY: 'auto' }}
          renderItem={app => (
            <List.Item
              actions={[
                app.installed && !app.updateAvailable ? (
                  <Tag key="installed" color="green">Đã cài</Tag>
                ) : (
                  <Button
                    key="install"
                    type="primary"
                    size="small"
                    icon={<CloudDownloadOutlined />}
                    loading={busySlug === app.slug}
                    onClick={() => void installFromStore(app)}
                  >
                    {app.updateAvailable ? 'Cập nhật' : 'Cài'}
                  </Button>
                ),
              ]}
            >
              <List.Item.Meta
                title={
                  <div className="flex items-center gap-2">
                    <span>{app.name}</span>
                    {app.version && <Tag>{app.version}</Tag>}
                    {app.updateAvailable && app.installedVersion && (
                      <Tag color="orange">đang dùng {app.installedVersion}</Tag>
                    )}
                  </div>
                }
                description={
                  <div>
                    <Paragraph type="secondary" className="mb-1 text-xs" ellipsis={{ rows: 2 }}>
                      {app.description || '—'}
                    </Paragraph>
                    <Text type="secondary" className="text-xs">
                      {app.slug}
                      {app.author ? ` · ${app.author}` : ''}
                      {typeof app.downloads === 'number' ? ` · ${app.downloads} lượt tải` : ''}
                    </Text>
                  </div>
                }
              />
            </List.Item>
          )}
        />
      )}
    </div>
  );

  const zipTab = (
    <div style={{ minHeight: 320 }}>
      <Alert
        type="warning"
        showIcon
        className="mb-3"
        title="App chạy mã ngay trên máy bạn"
        description="Lệnh runtime.start của app chạy ngay khi cài — chỉ cài ZIP từ nguồn bạn tin tưởng."
      />
      <Upload.Dragger
        accept=".zip"
        showUploadList={false}
        disabled={installingZip}
        beforeUpload={file => {
          void installZip(file);
          return false;
        }}
      >
        <p className="ant-upload-drag-icon">
          {installingZip ? <Spin /> : <InboxOutlined />}
        </p>
        <p className="ant-upload-text">Kéo file .zip vào đây hoặc bấm để chọn</p>
        <p className="ant-upload-hint">Bundle Space App có senclaw-manifest.json ở gốc.</p>
      </Upload.Dragger>
    </div>
  );

  const urlTab = (
    <div style={{ minHeight: 320 }}>
      <Alert
        type="warning"
        className="mb-3"
        showIcon
        title="App sẽ được nhúng qua iframe — đảm bảo tin tưởng nguồn gốc trước khi đăng ký."
      />
      <Form form={form} layout="vertical" onFinish={() => void register()}>
        <Form.Item
          name="manifest_url"
          label="URL Manifest"
          tooltip="App cần serve file senclaw-manifest.json tại endpoint này"
          rules={[{ required: true, type: 'url', message: 'Nhập URL hợp lệ' }]}
        >
          <Input
            placeholder="http://localhost:3100/senclaw-manifest.json"
            prefix={<LinkOutlined />}
          />
        </Form.Item>
        <Button type="primary" loading={registering} onClick={() => void register()}>
          Đăng ký
        </Button>
      </Form>
    </div>
  );

  return (
    <>
      <Modal
        title="Cài app mới"
        open={open}
        onCancel={onClose}
        footer={null}
        width={640}
        destroyOnHidden
      >
        <Tabs
          activeKey={tab}
          onChange={k => setTab(k as InstallSource)}
          items={[
            { key: 'store', label: <span><ShopOutlined /> Cửa hàng</span>, children: storeTab },
            { key: 'zip', label: <span><InboxOutlined /> Tệp ZIP</span>, children: zipTab },
            { key: 'url', label: <span><LinkOutlined /> Manifest URL</span>, children: urlTab },
          ]}
        />
      </Modal>

      <ScanReportDialog
        open={!!scanState}
        report={scanState?.report}
        target={scanState?.target}
        blocked={!!scanState?.blocked}
        busy={installingZip || !!busySlug}
        onCancel={() => setScanState(null)}
        onForceInstall={() => {
          const retry = scanState?.retry;
          setScanState(null);
          retry?.();
        }}
      />
    </>
  );
}

export default AppInstallDialog;
