import React, { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Checkbox,
  Divider,
  Input,
  Modal,
  Radio,
  Select,
  Space,
  Switch,
  Tag,
  Tooltip,
  Typography,
  message,
} from 'antd';
import { DeleteOutlined, PlusOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

type NetMode = 'off' | 'all' | 'hosts';
type ReadMode = 'open' | 'strict' | 'allowlist';

interface AppFolder {
  path: string;
  readOnly: boolean;
}

export interface AppSandboxConfig {
  enabled: boolean;
  readMode: ReadMode;
  folders: AppFolder[];
  network: NetMode;
  hosts: string[];
  daemonApi: boolean;
  loopback: number[];
}

interface Effective {
  isolation: string;
  enforceable: boolean;
  networkEnforceable: boolean;
  note?: string | null;
  alwaysGranted: string[];
  daemonPort: number;
}

interface ProxyState {
  port: number;
  stats: { allowed: number; denied: number; recentDenied: string[] };
}

const EMPTY: AppSandboxConfig = {
  enabled: false,
  readMode: 'open',
  folders: [],
  network: 'all',
  hosts: [],
  daemonApi: true,
  loopback: [],
};

/**
 * Per-app sandbox settings: does this app run confined, which folders it gets,
 * and how much of the network.
 *
 * The dialog deliberately shows what this machine will *actually* enforce
 * (`effective`) next to what is being asked for. On Linux the network mode is
 * not enforceable for a served app and on Windows nothing is, so a dialog that
 * only showed the stored settings would be promising isolation the machine is
 * not providing.
 */
export const SpaceAppSandboxModal: React.FC<{
  appId: string | null;
  appName?: string;
  open: boolean;
  onClose: () => void;
}> = ({ appId, appName, open, onClose }) => {
  const [cfg, setCfg] = useState<AppSandboxConfig>(EMPTY);
  const [eff, setEff] = useState<Effective | null>(null);
  const [proxy, setProxy] = useState<ProxyState | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [newFolder, setNewFolder] = useState('');
  const [newFolderRo, setNewFolderRo] = useState(false);
  const [newHost, setNewHost] = useState('');

  useEffect(() => {
    if (!open || !appId) return;
    setLoading(true);
    fetch(`/api/space/apps/${appId}/sandbox`)
      .then(r => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then(d => {
        setCfg({ ...EMPTY, ...(d.config ?? {}) });
        setEff(d.effective ?? null);
        setProxy(d.proxy ?? null);
      })
      .catch(e => message.error(`Cannot load sandbox settings: ${e}`))
      .finally(() => setLoading(false));
  }, [open, appId]);

  const save = async (restart: boolean) => {
    if (!appId) return;
    setSaving(true);
    try {
      const r = await fetch(`/api/space/apps/${appId}/sandbox`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(cfg),
      });
      const d = await r.json().catch(() => ({}));
      if (!r.ok) throw new Error(d.error ?? d.message ?? `HTTP ${r.status}`);
      if (restart) {
        await fetch(`/api/space/apps/${appId}/restart`, { method: 'POST' });
        message.success('Saved — app restarted with the new sandbox');
      } else {
        message.success(
          d.hostsAppliedLive
            ? 'Saved. The site list applies immediately; other changes need a restart.'
            : 'Saved. Restart the app for it to take effect.',
        );
      }
      onClose();
    } catch (e: any) {
      // The backend refuses rather than silently repairing (a bad folder, a host
      // that is really this machine), so its reason is the useful message.
      message.error(String(e.message ?? e));
    } finally {
      setSaving(false);
    }
  };

  const addFolder = () => {
    const p = newFolder.trim();
    if (!p) return;
    setCfg(c => ({ ...c, folders: [...c.folders, { path: p, readOnly: newFolderRo }] }));
    setNewFolder('');
    setNewFolderRo(false);
  };

  const addHost = () => {
    const h = newHost.trim();
    if (!h) return;
    setCfg(c => ({ ...c, hosts: [...c.hosts, h] }));
    setNewHost('');
  };

  const notEnforceable = eff && !eff.enforceable;

  return (
    <Modal
      open={open}
      onCancel={onClose}
      width={720}
      title={`Sandbox — ${appName ?? appId ?? ''}`}
      footer={[
        <Button key="cancel" onClick={onClose}>
          Cancel
        </Button>,
        <Button key="save" onClick={() => save(false)} loading={saving}>
          Save
        </Button>,
        <Button key="restart" type="primary" onClick={() => save(true)} loading={saving}>
          Save & restart app
        </Button>,
      ]}
      confirmLoading={loading}
    >
      {notEnforceable && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 12 }}
          message="This machine cannot confine a Space App"
          description={`Isolation available: ${eff?.isolation}. The app will keep running, unconfined — the switch below is stored but not enforced.`}
        />
      )}
      {eff?.note && (
        <Alert type="warning" showIcon style={{ marginBottom: 12 }} message={eff.note} />
      )}

      <Space align="start">
        <Switch
          checked={cfg.enabled}
          onChange={v => setCfg(c => ({ ...c, enabled: v }))}
        />
        <div>
          <Text strong>Run this app inside the sandbox</Text>
          <Paragraph type="secondary" style={{ marginBottom: 0, fontSize: 12 }}>
            The app may only write its own folder and its own data folder. Everything
            below applies while this is on.{' '}
            {eff?.isolation && <Tag>{eff.isolation}</Tag>}
          </Paragraph>
        </div>
      </Space>

      <Divider titlePlacement="start" plain style={{ marginTop: 16 }}>
        Folders
      </Divider>
      <Space direction="vertical" style={{ width: '100%' }} size={8}>
        <Space>
          <Text>May read:</Text>
          <Select
            size="small"
            style={{ width: 320 }}
            disabled={!cfg.enabled}
            value={cfg.readMode}
            onChange={v => setCfg(c => ({ ...c, readMode: v }))}
            options={[
              { value: 'open', label: 'Everything except credentials (default)' },
              { value: 'strict', label: 'Only its own + granted folders' },
              { value: 'allowlist', label: 'Only its own + granted folders (same, explicit)' },
            ]}
          />
        </Space>
        <Text type="secondary" style={{ fontSize: 12 }}>
          Always granted, read and write:
        </Text>
        <div>
          {(eff?.alwaysGranted ?? []).map(p => (
            <Tag key={p} style={{ marginBottom: 4 }}>
              <code>{p}</code>
            </Tag>
          ))}
        </div>
        {cfg.folders.map((f, i) => (
          <Space key={`${f.path}-${i}`}>
            <Tag color={f.readOnly ? 'blue' : 'green'}>{f.readOnly ? 'read-only' : 'read+write'}</Tag>
            <code style={{ fontSize: 12 }}>{f.path}</code>
            <Button
              type="text"
              size="small"
              danger
              icon={<DeleteOutlined />}
              onClick={() => setCfg(c => ({ ...c, folders: c.folders.filter((_, j) => j !== i) }))}
            />
          </Space>
        ))}
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="/absolute/path/to/folder"
            value={newFolder}
            disabled={!cfg.enabled}
            onChange={e => setNewFolder(e.target.value)}
            onPressEnter={addFolder}
          />
          <Tooltip title="Grant read-only">
            <Button
              disabled={!cfg.enabled}
              type={newFolderRo ? 'primary' : 'default'}
              onClick={() => setNewFolderRo(v => !v)}
            >
              read-only
            </Button>
          </Tooltip>
          <Button disabled={!cfg.enabled} icon={<PlusOutlined />} onClick={addFolder}>
            Add
          </Button>
        </Space.Compact>
        <Text type="secondary" style={{ fontSize: 12 }}>
          If the app stores data somewhere else, add that folder here — otherwise it
          will fail to write and say so in its log.
        </Text>
      </Space>

      <Divider titlePlacement="start" plain>
        Network
      </Divider>
      <Radio.Group
        disabled={!cfg.enabled}
        value={cfg.network}
        onChange={e => setCfg(c => ({ ...c, network: e.target.value }))}
      >
        <Space direction="vertical">
          <Radio value="all">Everything (like an app outside the sandbox)</Radio>
          <Radio value="hosts">Only these sites</Radio>
          <Radio value="off">No network at all</Radio>
        </Space>
      </Radio.Group>

      {cfg.network === 'hosts' && (
        <div style={{ marginTop: 12 }}>
          {eff && !eff.networkEnforceable && (
            <Alert
              type="warning"
              showIcon
              style={{ marginBottom: 8 }}
              message="On this platform the site list is not enforced — only the folder rules are."
            />
          )}
          <Space direction="vertical" style={{ width: '100%' }} size={6}>
            <div>
              {cfg.hosts.map((h, i) => (
                <Tag
                  key={`${h}-${i}`}
                  closable
                  onClose={() => setCfg(c => ({ ...c, hosts: c.hosts.filter((_, j) => j !== i) }))}
                >
                  {h}
                </Tag>
              ))}
              {cfg.hosts.length === 0 && (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  No site listed yet — the app can reach nothing.
                </Text>
              )}
            </div>
            <Space.Compact style={{ width: '100%' }}>
              <Input
                placeholder="api.openai.com  or  *.example.com"
                value={newHost}
                onChange={e => setNewHost(e.target.value)}
                onPressEnter={addHost}
              />
              <Button icon={<PlusOutlined />} onClick={addHost}>
                Add site
              </Button>
            </Space.Compact>
            <Text type="secondary" style={{ fontSize: 12 }}>
              Enforced by SenClaw's allowlist proxy on loopback: the app gets no direct
              way out, so a request to anything not listed fails. Traffic stays
              end-to-end encrypted — only the destination is checked.
            </Text>
            {proxy && (
              <Text type="secondary" style={{ fontSize: 12 }}>
                Proxy live on 127.0.0.1:{proxy.port} — {proxy.stats.allowed} allowed,{' '}
                {proxy.stats.denied} refused
                {proxy.stats.recentDenied.length > 0 && (
                  <>
                    {' · wanted: '}
                    {proxy.stats.recentDenied.map(h => (
                      <Tag
                        key={h}
                        color="orange"
                        style={{ cursor: 'pointer' }}
                        onClick={() => setCfg(c => ({ ...c, hosts: [...c.hosts, h] }))}
                      >
                        + {h}
                      </Tag>
                    ))}
                  </>
                )}
              </Text>
            )}
          </Space>
        </div>
      )}

      <Divider titlePlacement="start" plain>
        This machine
      </Divider>
      <Space direction="vertical" size={4}>
        <Checkbox
          disabled={!cfg.enabled}
          checked={cfg.daemonApi}
          onChange={e => setCfg(c => ({ ...c, daemonApi: e.target.checked }))}
        >
          May call SenClaw's own API on 127.0.0.1:{eff?.daemonPort ?? 18788}
        </Checkbox>
        <Text type="secondary" style={{ fontSize: 12 }}>
          Required by the AI bridge, which is what most apps use for anything
          intelligent. It is also SenClaw's unauthenticated local API — uncheck it for
          an app that does not need AI. Every other local service stays closed.
        </Text>
        <Space.Compact>
          <Input
            style={{ width: 320 }}
            disabled={!cfg.enabled}
            placeholder="other local ports, e.g. 5432, 3000"
            value={cfg.loopback.join(', ')}
            onChange={e =>
              setCfg(c => ({
                ...c,
                loopback: e.target.value
                  .split(/[,\s]+/)
                  .map(x => parseInt(x, 10))
                  .filter(n => Number.isFinite(n) && n > 0),
              }))
            }
          />
        </Space.Compact>
      </Space>
    </Modal>
  );
};

export default SpaceAppSandboxModal;
