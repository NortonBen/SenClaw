// Plugins → Widget — quản lý widget chat/dashboard + luồng mặc định.
//
// Hai phần:
//  1. Danh mục widget (GET /api/widgets): nguồn builtin | app:<id> | plugin:<name>,
//     surface, bật/tắt từng widget (PUT /api/widgets/:id).
//  2. Luồng mặc định (GET/PUT /api/defaults): mở link / media / search / note.

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert, Card, Select, Space, Switch, Table, Tag, Typography, message, theme,
} from 'antd';
import { AppstoreOutlined, SettingOutlined } from '@ant-design/icons';
import type { FlowDefaults, WidgetCatalogEntry } from '../../types';
import { invalidateFlowDefaults, invalidateWidgetCatalog } from '../../utils/flowDefaults';

const { Text, Title } = Typography;

function sourceTag(source: string) {
  if (source === 'builtin') return <Tag color="blue">builtin</Tag>;
  if (source.startsWith('app:')) return <Tag color="green">{source}</Tag>;
  if (source.startsWith('plugin:')) return <Tag color="purple">{source}</Tag>;
  return <Tag>{source}</Tag>;
}

export default function WidgetsPanel() {
  const { token } = theme.useToken();
  const [widgets, setWidgets] = useState<WidgetCatalogEntry[]>([]);
  const [defaults, setDefaults] = useState<FlowDefaults | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [installedApps, setInstalledApps] = useState<string[]>([]);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [wRes, dRes, aRes] = await Promise.all([
        fetch('/api/widgets'),
        fetch('/api/defaults'),
        fetch('/api/space/apps'),
      ]);
      const wJson = wRes.ok ? await wRes.json() : null;
      const dJson = dRes.ok ? await dRes.json() : null;
      const aJson = aRes.ok ? await aRes.json() : null;
      // An old daemon serves the SPA page for unknown /api routes — verify shape.
      if (!wJson || !Array.isArray(wJson.widgets) || !dJson || typeof dJson.openLink !== 'string') {
        setLoadError('Daemon chưa hỗ trợ /api/widgets — cần build lại và khởi động daemon mới.');
        setWidgets([]);
        setDefaults(null);
      } else {
        setWidgets(wJson.widgets);
        setDefaults(dJson);
      }
      if (Array.isArray(aJson)) {
        setInstalledApps(
          aJson
            .filter((a: { enabled?: boolean }) => a?.enabled !== false)
            .map((a: { id?: string }) => String(a?.id ?? '')),
        );
      }
    } catch (e) {
      setLoadError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const toggleWidget = async (row: WidgetCatalogEntry, enabled: boolean) => {
    // Optimistic flip; revert on failure.
    setWidgets((ws) => ws.map((w) => (w.id === row.id ? { ...w, enabled } : w)));
    try {
      const res = await fetch(`/api/widgets/${encodeURIComponent(row.id)}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled }),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      invalidateWidgetCatalog();
      message.success(`${row.id}: ${enabled ? 'đã bật' : 'đã tắt'}`);
    } catch (e) {
      setWidgets((ws) => ws.map((w) => (w.id === row.id ? { ...w, enabled: !enabled } : w)));
      message.error(`Không lưu được: ${e}`);
    }
  };

  const saveDefault = async (key: keyof FlowDefaults, value: string) => {
    if (!defaults) return;
    const prev = defaults;
    setDefaults({ ...defaults, [key]: value });
    try {
      const res = await fetch('/api/defaults', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ [key]: value }),
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const merged: FlowDefaults = await res.json();
      setDefaults(merged);
      invalidateFlowDefaults();
      message.success('Đã lưu mặc định');
    } catch (e) {
      setDefaults(prev);
      message.error(`Không lưu được: ${e}`);
    }
  };

  // Availability comes from the installed-apps list, not the widget catalog —
  // an app can be installed without declaring any widget.
  const hasMiniBrowser = useMemo(() => installedApps.includes('mini-browser'), [installedApps]);
  const hasSearchApp = useMemo(() => installedApps.includes('search'), [installedApps]);

  const columns = [
    {
      title: 'Widget',
      dataIndex: 'name',
      key: 'name',
      render: (_: string, row: WidgetCatalogEntry) => (
        <div>
          <div>
            <Text strong>{row.name}</Text>{' '}
            <Text type="secondary" style={{ fontSize: 12 }}>({row.id})</Text>
          </div>
          {row.description ? (
            <Text type="secondary" style={{ fontSize: 12 }}>{row.description}</Text>
          ) : null}
        </div>
      ),
    },
    {
      title: 'Nguồn',
      dataIndex: 'source',
      key: 'source',
      width: 160,
      render: (source: string) => sourceTag(source),
      filters: [
        { text: 'builtin', value: 'builtin' },
        { text: 'app', value: 'app:' },
        { text: 'plugin', value: 'plugin:' },
      ],
      onFilter: (v: unknown, row: WidgetCatalogEntry) =>
        v === 'builtin' ? row.source === 'builtin' : row.source.startsWith(String(v)),
    },
    {
      title: 'Surface',
      dataIndex: 'surfaces',
      key: 'surfaces',
      width: 170,
      render: (surfaces: string[]) => (
        <Space size={4}>
          {(surfaces ?? []).map((s) => (
            <Tag key={s} color={s === 'chat' ? 'cyan' : undefined}>{s}</Tag>
          ))}
        </Space>
      ),
    },
    {
      title: 'Bật',
      dataIndex: 'enabled',
      key: 'enabled',
      width: 70,
      render: (_: boolean, row: WidgetCatalogEntry) => (
        <Switch
          size="small"
          checked={row.enabled}
          onChange={(v) => void toggleWidget(row, v)}
        />
      ),
    },
  ];

  const selectStyle = { width: 220 };

  return (
    <div style={{ padding: 24, maxWidth: 1080 }}>
      <Title level={4} style={{ marginTop: 0 }}>
        <AppstoreOutlined /> Widget
      </Title>
      <Text type="secondary">
        Widget hiển thị trong ô chat (qua <code>emit_widget</code>) và trên Dashboard.
        Space App khai báo widget trong <code>senclaw-manifest.json → widgets[]</code>;
        plugin trong <code>widgets/widgets.json</code>.
      </Text>

      {loadError ? (
        <Alert style={{ marginTop: 16 }} type="warning" showIcon message={loadError} />
      ) : null}

      <Card size="small" style={{ marginTop: 16 }} title="Danh mục widget">
        <Table<WidgetCatalogEntry>
          rowKey="id"
          size="small"
          loading={loading}
          dataSource={widgets}
          columns={columns}
          // Narrow panes (docked window, small display) otherwise squeeze the
          // name column until it wraps one character per line — scroll instead.
          scroll={{ x: 640 }}
          pagination={widgets.length > 20 ? { pageSize: 20 } : false}
        />
      </Card>

      <Card
        size="small"
        style={{ marginTop: 16 }}
        title={<span><SettingOutlined /> Luồng mặc định</span>}
      >
        {defaults ? (
          <Space direction="vertical" size={14} style={{ width: '100%' }}>
            <Space wrap>
              <Text style={{ width: 110, display: 'inline-block' }}>Mở link</Text>
              <Select
                style={selectStyle}
                value={defaults.openLink}
                onChange={(v) => void saveDefault('openLink', v)}
                options={[
                  { value: 'system-browser', label: 'Trình duyệt hệ thống' },
                  { value: 'new-tab', label: 'Tab mới (web UI)' },
                  {
                    value: 'mini-browser',
                    label: 'Mini Browser (trong SenClaw)',
                    disabled: !hasMiniBrowser,
                  },
                ]}
              />
              {!hasMiniBrowser ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  (cài app mini-browser để mở trong SenClaw)
                </Text>
              ) : null}
            </Space>
            <Space wrap>
              <Text style={{ width: 110, display: 'inline-block' }}>Media</Text>
              <Select
                style={selectStyle}
                value={defaults.media}
                onChange={(v) => void saveDefault('media', v)}
                options={[
                  { value: 'inline-widget', label: 'Phát ngay trong chat (widget)' },
                  { value: 'mini-browser', label: 'Mini Browser', disabled: !hasMiniBrowser },
                  { value: 'system-browser', label: 'Trình duyệt hệ thống' },
                ]}
              />
            </Space>
            <Space wrap>
              <Text style={{ width: 110, display: 'inline-block' }}>Search</Text>
              <Select
                style={selectStyle}
                value={defaults.search}
                onChange={(v) => void saveDefault('search', v)}
                options={[
                  { value: 'browser', label: 'browser_search (SERP)' },
                  {
                    value: 'search-app',
                    label: 'App Search (federated)',
                    disabled: !hasSearchApp,
                  },
                ]}
              />
              <Select
                style={{ width: 120 }}
                value={defaults.searchEngine}
                onChange={(v) => void saveDefault('searchEngine', v)}
                options={[
                  { value: 'google', label: 'Google' },
                  { value: 'bing', label: 'Bing' },
                ]}
              />
              {!hasSearchApp ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  (cài app search để dùng federated search)
                </Text>
              ) : null}
            </Space>
            <Space wrap>
              <Text style={{ width: 110, display: 'inline-block' }}>Ghi chú</Text>
              <Select
                style={selectStyle}
                value={defaults.note}
                onChange={(v) => void saveDefault('note', v)}
                options={[
                  { value: 'space-notes', label: 'Space Notes' },
                  { value: 'wiki', label: 'Wiki (wiki_write)' },
                  { value: 'memory', label: 'Memory (memory_save)' },
                ]}
              />
            </Space>
            <Text type="secondary" style={{ fontSize: 12, color: token.colorTextTertiary }}>
              Các mặc định này được đưa vào system prompt của agent (mục “User defaults”)
              và điều khiển hành vi click link trên UI. Kênh nhắn tin (Telegram/Zalo…)
              luôn nhận bản tóm tắt text thay cho widget.
            </Text>
          </Space>
        ) : (
          <Text type="secondary">Chưa tải được cài đặt mặc định.</Text>
        )}
      </Card>
    </div>
  );
}
