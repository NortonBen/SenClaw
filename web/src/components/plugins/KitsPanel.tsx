// Plugins → Kits — cài/gỡ Zen Kit.
//
// Daemon sở hữu toàn bộ việc cài (thứ tự, luật không-ghi-đè, sổ biên nhận), nên
// trang này chỉ liệt kê và gọi endpoint. Xem docs/zen-kits.md.
//   GET    /api/kits            → kit đã cài (đọc từ sổ biên nhận)
//   GET    /api/kits/available  → kit các marketplace source đang chào
//   DELETE /api/kits/:id        → gỡ đúng những gì kit đã tạo
//
// Việc cài nằm trong KitInstallDialog: mọi nguồn (tệp .zip/.json, dán manifest,
// marketplace) đều phải qua một bước xem trước trước khi ghi bất cứ thứ gì.

import { useCallback, useEffect, useState } from 'react';
import {
  Alert, Button, Card, Empty, Modal, Popconfirm, Space, Table, Tabs, Tag,
  Tooltip, Typography, message,
} from 'antd';
import {
  AppstoreOutlined, CloudDownloadOutlined, DeleteOutlined, GiftOutlined,
  ReloadOutlined, RocketOutlined,
} from '@ant-design/icons';
import type {
  AvailableKit, KitItemRecord, KitReceipt, KitRemoveOutcome, KitUninstallReport,
} from '../../types';
import KitInstallDialog, { type KitPickSource } from './KitInstallDialog';
import { KIND_LABEL, REMOVE_STATUS, errorText, kindTag } from './kitCommon';

const { Text, Title, Paragraph } = Typography;

/** Gom các mục trong sổ biên nhận thành "2 Persona · 1 Skill" cho gọn. */
function itemSummary(items: KitItemRecord[]) {
  const counts = new Map<string, number>();
  for (const i of items) counts.set(i.type, (counts.get(i.type) ?? 0) + 1);
  return [...counts.entries()].map(([kind, n]) => (
    <Tag key={kind}>
      {n} {KIND_LABEL[kind] ?? kind}
    </Tag>
  ));
}

export default function KitsPanel() {
  const [kits, setKits] = useState<KitReceipt[]>([]);
  const [available, setAvailable] = useState<AvailableKit[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);

  const [dialog, setDialog] = useState<KitPickSource | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [installedRes, availableRes] = await Promise.all([
        fetch('/api/kits'),
        fetch('/api/kits/available'),
      ]);
      const installed = installedRes.ok ? await installedRes.json() : null;
      // Daemon cũ trả trang SPA cho /api lạ → không phải object có `kits`.
      if (!installed || !Array.isArray(installed.kits)) {
        setLoadError(
          'Daemon này chưa phục vụ /api/kits — cần build lại và khởi động daemon mới.',
        );
        setKits([]);
      } else {
        setKits(installed.kits);
      }
      // Marketplace là phần thêm: hỏng thì danh sách đã cài vẫn phải dùng được.
      const offered = availableRes.ok ? await availableRes.json() : null;
      setAvailable(Array.isArray(offered?.kits) ? offered.kits : []);
    } catch (e) {
      setLoadError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const uninstall = useCallback(
    async (id: string) => {
      setRemoving(id);
      try {
        const res = await fetch(`/api/kits/${encodeURIComponent(id)}`, { method: 'DELETE' });
        const data = await res.json().catch(() => null);
        if (!res.ok || !data?.report) {
          message.error(data?.error ?? (await errorText(res)));
          return;
        }
        showRemoveReport(data.report as KitUninstallReport, data.ok === true);
        await load();
      } catch (e) {
        message.error(String(e));
      } finally {
        setRemoving(null);
      }
    },
    [load],
  );

  return (
    <div style={{ padding: 24, maxWidth: 1080 }}>
      <Space style={{ width: '100%', justifyContent: 'space-between' }} align="start">
        <div>
          <Title level={4} style={{ marginTop: 0, marginBottom: 4 }}>
            <GiftOutlined /> Kits
          </Title>
          <Paragraph type="secondary" style={{ maxWidth: 720, marginBottom: 0 }}>
            Một kit cài trọn bộ trong một lần: persona, skill, workflow, hook và lịch chạy.
            Dạng <code>.json</code> chỉ mang được phần khai báo; dạng <code>.zip</code> mang
            thêm tệp thật của skill/workflow và cả Space App. Daemon giữ luật không ghi đè
            (trùng tên thì bỏ qua) và một sổ biên nhận, nên gỡ kit chỉ xoá đúng thứ nó đã tạo.
          </Paragraph>
        </div>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={() => void load()} loading={loading}>
            Làm mới
          </Button>
          <Button
            type="primary"
            icon={<RocketOutlined />}
            onClick={() => setDialog({ kind: 'local' })}
          >
            Cài kit
          </Button>
        </Space>
      </Space>

      {loadError ? (
        <Alert style={{ marginTop: 16 }} type="warning" showIcon message={loadError} />
      ) : null}

      <Tabs
        style={{ marginTop: 8 }}
        items={[
          {
            key: 'installed',
            label: `Đã cài${kits.length ? ` (${kits.length})` : ''}`,
            children: (
              <InstalledTable
                kits={kits}
                loading={loading}
                removing={removing}
                onUninstall={uninstall}
              />
            ),
          },
          {
            key: 'market',
            label: `Marketplace${available.length ? ` (${available.length})` : ''}`,
            children: (
              <MarketTable
                kits={available}
                loading={loading}
                onInstall={(k) =>
                  setDialog({
                    kind: 'market',
                    sourceId: k.sourceId,
                    sourceName: k.sourceName,
                    name: k.name,
                  })
                }
              />
            ),
          },
        ]}
      />

      <KitInstallDialog
        open={dialog !== null}
        source={dialog ?? { kind: 'local' }}
        onClose={() => setDialog(null)}
        onInstalled={() => void load()}
      />
    </div>
  );
}

// ─── Kit đã cài ───────────────────────────────────────────────────────────────

function InstalledTable({
  kits,
  loading,
  removing,
  onUninstall,
}: {
  kits: KitReceipt[];
  loading: boolean;
  removing: string | null;
  onUninstall: (id: string) => void;
}) {
  if (!loading && kits.length === 0) {
    return (
      <Card size="small">
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description="Chưa cài kit nào — bấm “Cài kit” để bắt đầu."
        />
      </Card>
    );
  }
  return (
    <Card size="small" styles={{ body: { padding: 0 } }}>
      <Table<KitReceipt>
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={kits}
        pagination={false}
        scroll={{ x: 640 }}
        // Sổ biên nhận CHỈ ghi thứ kit tự tạo. Mục trùng tên bị bỏ qua lúc cài
        // không nằm ở đây — đó là lý do danh sách này có thể ngắn hơn những gì
        // bản xem trước hứa, và cũng là lý do gỡ kit không đụng tới chúng.
        expandable={{
          expandedRowRender: (row) => <CreatedItems items={row.items} />,
          rowExpandable: (row) => row.items.length > 0,
          defaultExpandAllRows: true,
        }}
        columns={[
          {
            title: 'Kit',
            dataIndex: 'name',
            render: (name: string, row) => (
              <Space direction="vertical" size={0}>
                <Text strong>{name || row.id}</Text>
                <Text type="secondary" style={{ fontSize: 11 }}>
                  <code>{row.id}</code> · v{row.version}
                </Text>
                {row.description ? (
                  <Text type="secondary" style={{ fontSize: 12, marginTop: 2 }}>
                    {row.description}
                  </Text>
                ) : null}
              </Space>
            ),
          },
          {
            title: 'Đã tạo',
            dataIndex: 'items',
            width: 300,
            render: (items: KitItemRecord[]) =>
              items.length ? (
                <Space size={4} wrap>
                  {itemSummary(items)}
                </Space>
              ) : (
                <Text type="secondary">—</Text>
              ),
          },
          {
            title: 'Cài lúc',
            dataIndex: 'installedAt',
            width: 150,
            render: (at: string) => (
              <Tooltip title={at}>
                <Text style={{ fontSize: 12 }}>{new Date(at).toLocaleString()}</Text>
              </Tooltip>
            ),
          },
          {
            title: '',
            key: 'actions',
            width: 90,
            render: (_, row) => (
              <Popconfirm
                title="Gỡ kit này?"
                description="Chỉ xoá những mục kit đã tạo. Mục trùng tên bị bỏ qua lúc cài sẽ được giữ nguyên."
                okText="Gỡ"
                okButtonProps={{ danger: true }}
                cancelText="Huỷ"
                onConfirm={() => onUninstall(row.id)}
              >
                <Button danger size="small" icon={<DeleteOutlined />} loading={removing === row.id}>
                  Gỡ
                </Button>
              </Popconfirm>
            ),
          },
        ]}
      />
    </Card>
  );
}

/** Từng mục kit đã tạo, kèm đường dẫn / id engine để đi kiểm chứng được. */
function CreatedItems({ items }: { items: KitItemRecord[] }) {
  return (
    <Space direction="vertical" size={4} style={{ width: '100%' }}>
      {items.map((i, n) => (
        <Space key={`${i.type}-${i.name}-${n}`} size={8} align="start">
          <span style={{ minWidth: 92, display: 'inline-block' }}>{kindTag(i.type)}</span>
          <Space direction="vertical" size={0}>
            <Text style={{ fontSize: 12 }}>{i.name}</Text>
            {i.path || i.engineRef ? (
              <Text
                type="secondary"
                copyable={{ text: i.path ?? i.engineRef ?? '' }}
                style={{ fontSize: 11, fontFamily: 'ui-monospace, monospace' }}
              >
                {i.path ?? i.engineRef}
              </Text>
            ) : null}
          </Space>
        </Space>
      ))}
    </Space>
  );
}

// ─── Kit trên marketplace ─────────────────────────────────────────────────────

function MarketTable({
  kits,
  loading,
  onInstall,
}: {
  kits: AvailableKit[];
  loading: boolean;
  onInstall: (kit: AvailableKit) => void;
}) {
  if (!loading && kits.length === 0) {
    return (
      <Card size="small">
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={
            <Space direction="vertical" size={2}>
              <Text type="secondary">Chưa có marketplace source nào chào kit.</Text>
              <Text type="secondary" style={{ fontSize: 12 }}>
                Kit được khai báo trong mảng <code>kits[]</code> của <code>marketplace.json</code>{' '}
                ở source — thêm source tại trang Marketplace.
              </Text>
            </Space>
          }
        />
      </Card>
    );
  }
  return (
    <Card size="small" styles={{ body: { padding: 0 } }}>
      <Table<AvailableKit>
        rowKey={(r) => `${r.sourceId}:${r.name}`}
        size="small"
        loading={loading}
        dataSource={kits}
        pagination={kits.length > 20 ? { pageSize: 20 } : false}
        scroll={{ x: 680 }}
        columns={[
          {
            title: 'Kit',
            dataIndex: 'name',
            render: (name: string, row) => (
              <Space direction="vertical" size={0}>
                <Text strong>{name}</Text>
                {row.description ? (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {row.description}
                  </Text>
                ) : null}
              </Space>
            ),
          },
          {
            title: 'Nguồn',
            dataIndex: 'sourceName',
            width: 160,
            render: (s: string) => <Tag icon={<AppstoreOutlined />}>{s}</Tag>,
          },
          {
            title: 'Bản',
            dataIndex: 'version',
            width: 110,
            render: (v: string | null, row) =>
              row.installedVersion ? (
                <Tooltip title={`Đã cài v${row.installedVersion}`}>
                  <Tag color="green">đã cài</Tag>
                </Tooltip>
              ) : (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {v ?? '—'}
                </Text>
              ),
          },
          {
            title: '',
            key: 'actions',
            width: 110,
            render: (_, row) => (
              <Tooltip
                title={row.installable ? undefined : 'Mục trong catalog không khai báo tệp để tải.'}
              >
                <Button
                  size="small"
                  icon={<CloudDownloadOutlined />}
                  disabled={!row.installable}
                  onClick={() => onInstall(row)}
                >
                  {row.installedVersion ? 'Cài lại' : 'Cài'}
                </Button>
              </Tooltip>
            ),
          },
        ]}
      />
    </Card>
  );
}

// ─── Báo cáo gỡ ───────────────────────────────────────────────────────────────

/** Hiện kết quả gỡ trong một modal thông tin.
 *
 * `missing` không phải lỗi — người dùng đã tự xoá bằng tay từ trước. Còn khi có
 * mục lỗi thì daemon GIỮ LẠI sổ biên nhận, vì nó là bản ghi duy nhất về những
 * thứ còn sót; nói rõ điều đó thay vì để người dùng tưởng kit đã sạch.
 */
function showRemoveReport(report: KitUninstallReport, ok: boolean) {
  const rows = report.items;
  const failed = rows.filter((i) => i.status === 'failed');

  Modal[ok ? 'success' : 'warning']({
    title: ok ? `Đã gỡ kit ${report.kitId}` : `Gỡ kit ${report.kitId} chưa trọn`,
    width: 560,
    content: (
      <Space direction="vertical" size={10} style={{ width: '100%', marginTop: 8 }}>
        {failed.length > 0 ? (
          <Alert
            type="warning"
            showIcon
            message="Sổ biên nhận được giữ lại"
            description="Vẫn còn mục chưa gỡ được, và sổ là bản ghi duy nhất về chúng — gỡ lại sau khi xử lý xong."
          />
        ) : null}
        <Table<KitRemoveOutcome>
          rowKey={(r) => `${r.type}-${r.name}`}
          size="small"
          pagination={false}
          dataSource={rows}
          columns={[
            { title: 'Loại', dataIndex: 'type', width: 100, render: kindTag },
            { title: 'Tên', dataIndex: 'name', ellipsis: true },
            {
              title: 'Kết quả',
              dataIndex: 'status',
              width: 100,
              render: (s: keyof typeof REMOVE_STATUS) => (
                <Tag color={REMOVE_STATUS[s].color}>{REMOVE_STATUS[s].label}</Tag>
              ),
            },
          ]}
        />
      </Space>
    ),
  });
}
