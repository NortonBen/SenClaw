// App.tsx — one-page shell: connection hero, stats, service tabs
// (Gmail / Calendar / Drive / Activity), plus Connect & Settings modals.
// All data comes from the Rust backend via api.ts.

import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  App as AntApp,
  Badge,
  Button,
  Card,
  Checkbox,
  Col,
  ConfigProvider,
  Drawer,
  Empty,
  Form,
  Input,
  InputNumber,
  Modal,
  Row,
  Space,
  Statistic,
  Table,
  Tabs,
  Tag,
  Timeline,
  Typography,
} from "antd";
import {
  ApiOutlined,
  CalendarOutlined,
  CheckCircleFilled,
  CloudSyncOutlined,
  CloudUploadOutlined,
  DisconnectOutlined,
  FileOutlined,
  GoogleOutlined,
  KeyOutlined,
  MailOutlined,
  PlusOutlined,
  ReloadOutlined,
  SendOutlined,
  SettingOutlined,
  SyncOutlined,
} from "@ant-design/icons";

import * as api from "./api";
import type { CalEvent, DriveFile, EmailFull, EmailMeta, Settings, SyncRun } from "./api";
import { openExternal } from "./openExternal";

const { Text, Title, Paragraph } = Typography;

const DEFAULT_SETTINGS: Settings = {
  clientId: "",
  clientSecret: "",
  days: 7,
  services: ["gmail", "calendar", "drive"],
  connected: false,
  hasRefreshToken: false,
  tokenExpiresAt: 0,
};

const SERVICE_META: Record<string, { label: string; color: string }> = {
  gmail: { label: "Gmail", color: "#ea4335" },
  calendar: { label: "Calendar", color: "#1a73e8" },
  drive: { label: "Drive", color: "#fbbc04" },
};

function fmtTime(unixSecs?: number) {
  if (!unixSecs) return "—";
  return new Date(unixSecs * 1000).toLocaleString("vi-VN");
}

function useSettings() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [lastRun, setLastRun] = useState<SyncRun | null>(null);
  const reload = useCallback(async () => {
    const r = await api.getSettings();
    if (r.ok) {
      setSettings({ ...DEFAULT_SETTINGS, ...(r.settings as Settings) });
      setLastRun((r.lastRun as SyncRun) ?? null);
    }
    return r;
  }, []);
  useEffect(() => {
    void reload();
  }, [reload]);
  return { settings, lastRun, reload };
}

// ── Gmail tab ───────────────────────────────────────────────────────────────

function GmailTab({ connected, notify }: { connected: boolean; notify: (m: string, ok?: boolean) => void }) {
  const [emails, setEmails] = useState<EmailMeta[]>([]);
  const [q, setQ] = useState("");
  const [loading, setLoading] = useState(false);
  const [detail, setDetail] = useState<EmailFull | null>(null);
  const [composeOpen, setComposeOpen] = useState(false);
  const [sending, setSending] = useState(false);
  const [compose] = Form.useForm();

  const load = useCallback(async (query = "") => {
    setLoading(true);
    const r = await api.listEmails(20, query);
    setLoading(false);
    if (r.ok) setEmails((r.emails as EmailMeta[]) ?? []);
    else notify(r.error ?? "Lỗi tải email");
  }, [notify]);

  useEffect(() => {
    if (connected) void load();
  }, [connected, load]);

  const open = async (id: string) => {
    const r = await api.readEmail(id);
    if (r.ok) setDetail(r.email as EmailFull);
    else notify(r.error ?? "Không đọc được email");
  };

  const send = async () => {
    const v = await compose.validateFields();
    setSending(true);
    const r = await api.sendEmail(v.to, v.subject, v.body);
    setSending(false);
    if (r.ok) {
      setComposeOpen(false);
      compose.resetFields();
      notify("Đã gửi email.", true);
    } else notify(r.error ?? "Gửi thất bại");
  };

  if (!connected) return <Empty description="Kết nối Google để xem Gmail" />;

  return (
    <>
      <Space style={{ marginBottom: 12, width: "100%", justifyContent: "space-between" }}>
        <Input.Search
          placeholder="Gmail query — vd: is:unread, from:sếp@cty.vn"
          style={{ width: 360 }}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          onSearch={(v) => void load(v)}
          allowClear
        />
        <Space>
          <Button icon={<ReloadOutlined />} onClick={() => void load(q)} loading={loading}>
            Tải lại
          </Button>
          <Button type="primary" icon={<SendOutlined />} onClick={() => setComposeOpen(true)}>
            Soạn email
          </Button>
        </Space>
      </Space>
      <Table<EmailMeta>
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={emails}
        pagination={false}
        onRow={(rec) => ({ onClick: () => void open(rec.id), style: { cursor: "pointer" } })}
        columns={[
          {
            title: "Tiêu đề",
            dataIndex: "subject",
            render: (s: string, rec) => (
              <div>
                <Text strong>{s || "(không tiêu đề)"}</Text>
                <div>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {rec.snippet}
                  </Text>
                </div>
              </div>
            ),
          },
          { title: "Từ", dataIndex: "from", width: 260, ellipsis: true },
          { title: "Ngày", dataIndex: "date", width: 200, ellipsis: true },
        ]}
      />

      <Drawer
        title={detail?.subject ?? "Email"}
        open={!!detail}
        onClose={() => setDetail(null)}
        width={640}
      >
        {detail ? <EmailDetail email={detail} /> : null}
      </Drawer>

      <Modal
        title="Soạn email"
        open={composeOpen}
        onCancel={() => setComposeOpen(false)}
        onOk={() => void send()}
        okText="Gửi"
        confirmLoading={sending}
        okButtonProps={{ icon: <SendOutlined /> }}
      >
        <Form form={compose} layout="vertical">
          <Form.Item name="to" label="Đến" rules={[{ required: true, message: "Nhập người nhận" }]}>
            <Input placeholder="ai-do@example.com" />
          </Form.Item>
          <Form.Item name="subject" label="Tiêu đề" rules={[{ required: true, message: "Nhập tiêu đề" }]}>
            <Input />
          </Form.Item>
          <Form.Item name="body" label="Nội dung" rules={[{ required: true, message: "Nhập nội dung" }]}>
            <Input.TextArea rows={8} placeholder="Hỗ trợ HTML hoặc text thuần" />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}

function EmailDetail({ email }: { email: EmailFull }) {
  return (
    <Space direction="vertical" style={{ width: "100%" }}>
      <Text type="secondary">Từ: {email.from ?? "—"}</Text>
      <Text type="secondary">Đến: {email.to ?? "—"}</Text>
      <Text type="secondary">Ngày: {email.date ?? "—"}</Text>
      {email.attachments && email.attachments.length > 0 && (
        <Space wrap>
          {email.attachments.map((a, i) => (
            <Tag key={i} icon={<FileOutlined />}>
              {a.filename}
            </Tag>
          ))}
        </Space>
      )}
      <Card size="small" style={{ marginTop: 8 }}>
        {email.bodyText ? (
          <pre style={{ whiteSpace: "pre-wrap", margin: 0, fontFamily: "inherit" }}>{email.bodyText}</pre>
        ) : email.bodyHtml ? (
          // Emails render in a sandboxed iframe so foreign HTML can't touch the app.
          <iframe
            sandbox=""
            srcDoc={email.bodyHtml}
            style={{ width: "100%", minHeight: 360, border: "none" }}
            title="email-body"
          />
        ) : (
          <Text type="secondary">{email.snippet ?? "(trống)"}</Text>
        )}
      </Card>
    </Space>
  );
}

// ── Calendar tab ────────────────────────────────────────────────────────────

function CalendarTab({ connected, notify }: { connected: boolean; notify: (m: string, ok?: boolean) => void }) {
  const [events, setEvents] = useState<CalEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [createOpen, setCreateOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [form] = Form.useForm();

  const load = useCallback(async () => {
    setLoading(true);
    const r = await api.listEvents(20);
    setLoading(false);
    if (r.ok) setEvents((r.events as CalEvent[]) ?? []);
    else notify(r.error ?? "Lỗi tải sự kiện");
  }, [notify]);

  useEffect(() => {
    if (connected) void load();
  }, [connected, load]);

  const create = async () => {
    const v = await form.validateFields();
    setCreating(true);
    const r = await api.createEvent({
      summary: v.summary,
      description: v.description ?? "",
      startTime: v.startTime,
      endTime: v.endTime,
    });
    setCreating(false);
    if (r.ok) {
      setCreateOpen(false);
      form.resetFields();
      notify("Đã tạo sự kiện.", true);
      void load();
    } else notify(r.error ?? "Tạo sự kiện thất bại");
  };

  if (!connected) return <Empty description="Kết nối Google để xem Calendar" />;

  return (
    <>
      <Space style={{ marginBottom: 12 }}>
        <Button icon={<ReloadOutlined />} onClick={() => void load()} loading={loading}>
          Tải lại
        </Button>
        <Button type="primary" icon={<PlusOutlined />} onClick={() => setCreateOpen(true)}>
          Tạo sự kiện
        </Button>
      </Space>
      <Table<CalEvent>
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={events}
        pagination={false}
        columns={[
          { title: "Sự kiện", dataIndex: "summary", render: (s: string) => <Text strong>{s || "(không tên)"}</Text> },
          { title: "Bắt đầu", dataIndex: "start", width: 220 },
          { title: "Kết thúc", dataIndex: "end", width: 220 },
          {
            title: "",
            dataIndex: "htmlLink",
            width: 80,
            render: (l: string) =>
              l ? (
                <a href={l} target="_blank" rel="noreferrer">
                  Mở
                </a>
              ) : null,
          },
        ]}
      />
      <Modal
        title="Tạo sự kiện"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => void create()}
        okText="Tạo"
        confirmLoading={creating}
      >
        <Form form={form} layout="vertical">
          <Form.Item name="summary" label="Tên sự kiện" rules={[{ required: true, message: "Nhập tên" }]}>
            <Input />
          </Form.Item>
          <Form.Item name="description" label="Mô tả">
            <Input.TextArea rows={2} />
          </Form.Item>
          <Row gutter={12}>
            <Col span={12}>
              <Form.Item
                name="startTime"
                label="Bắt đầu"
                rules={[{ required: true, message: "VD 2026-07-30T15:00" }]}
                extra="YYYY-MM-DDTHH:MM (giờ local) hoặc RFC3339"
              >
                <Input placeholder="2026-07-30T15:00" />
              </Form.Item>
            </Col>
            <Col span={12}>
              <Form.Item
                name="endTime"
                label="Kết thúc"
                rules={[{ required: true, message: "VD 2026-07-30T16:00" }]}
              >
                <Input placeholder="2026-07-30T16:00" />
              </Form.Item>
            </Col>
          </Row>
        </Form>
      </Modal>
    </>
  );
}

// ── Drive tab ───────────────────────────────────────────────────────────────

function DriveTab({ connected, notify }: { connected: boolean; notify: (m: string, ok?: boolean) => void }) {
  const [files, setFiles] = useState<DriveFile[]>([]);
  const [loading, setLoading] = useState(false);
  const [uploadOpen, setUploadOpen] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [form] = Form.useForm();

  const load = useCallback(async () => {
    setLoading(true);
    const r = await api.listFiles(20);
    setLoading(false);
    if (r.ok) setFiles((r.files as DriveFile[]) ?? []);
    else notify(r.error ?? "Lỗi tải danh sách file");
  }, [notify]);

  useEffect(() => {
    if (connected) void load();
  }, [connected, load]);

  const upload = async () => {
    const v = await form.validateFields();
    setUploading(true);
    const r = await api.uploadFile(v.name, v.mimeType || "text/plain", v.textContent);
    setUploading(false);
    if (r.ok) {
      setUploadOpen(false);
      form.resetFields();
      notify("Đã tải file lên Drive.", true);
      void load();
    } else notify(r.error ?? "Upload thất bại");
  };

  if (!connected) return <Empty description="Kết nối Google để xem Drive" />;

  return (
    <>
      <Space style={{ marginBottom: 12 }}>
        <Button icon={<ReloadOutlined />} onClick={() => void load()} loading={loading}>
          Tải lại
        </Button>
        <Button type="primary" icon={<CloudUploadOutlined />} onClick={() => setUploadOpen(true)}>
          Tải file text lên
        </Button>
      </Space>
      <Table<DriveFile>
        rowKey="id"
        size="small"
        loading={loading}
        dataSource={files}
        pagination={false}
        columns={[
          {
            title: "Tên",
            dataIndex: "name",
            render: (n: string, rec) =>
              rec.webViewLink ? (
                <a href={rec.webViewLink} target="_blank" rel="noreferrer">
                  {n}
                </a>
              ) : (
                n
              ),
          },
          { title: "Loại", dataIndex: "mimeType", width: 260, ellipsis: true },
          { title: "Sửa lúc", dataIndex: "modifiedTime", width: 220 },
        ]}
      />
      <Modal
        title="Tải file text lên Drive"
        open={uploadOpen}
        onCancel={() => setUploadOpen(false)}
        onOk={() => void upload()}
        okText="Tải lên"
        confirmLoading={uploading}
      >
        <Form form={form} layout="vertical">
          <Form.Item name="name" label="Tên file" rules={[{ required: true, message: "Nhập tên file" }]}>
            <Input placeholder="ghi-chu.md" />
          </Form.Item>
          <Form.Item name="mimeType" label="MIME type">
            <Input placeholder="text/plain" />
          </Form.Item>
          <Form.Item name="textContent" label="Nội dung" rules={[{ required: true, message: "Nhập nội dung" }]}>
            <Input.TextArea rows={6} />
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}

// ── Activity tab ────────────────────────────────────────────────────────────

function ActivityTab({ refreshKey }: { refreshKey: number }) {
  const [runs, setRuns] = useState<SyncRun[]>([]);
  useEffect(() => {
    void api.getRuns().then((r) => {
      if (r.ok) setRuns((r.runs as SyncRun[]) ?? []);
    });
  }, [refreshKey]);

  if (!runs.length) return <Empty description="Chưa có hoạt động nào" />;
  return (
    <Timeline
      items={runs.map((run) => ({
        color: run.status === "error" ? "red" : "green",
        children: (
          <Space direction="vertical" size={0}>
            <Space>
              <Text strong>{SERVICE_META[run.service]?.label ?? run.service}</Text>
              <Tag color={run.status === "error" ? "error" : "success"}>{run.status}</Tag>
            </Space>
            {run.detail && (
              <Text type="secondary" style={{ fontSize: 12, wordBreak: "break-all" }}>
                {run.detail.length > 160 ? `${run.detail.slice(0, 160)}…` : run.detail}
              </Text>
            )}
            <Text type="secondary" style={{ fontSize: 12 }}>
              {fmtTime(run.created_at)}
            </Text>
          </Space>
        ),
      }))}
    />
  );
}

// ── main app ────────────────────────────────────────────────────────────────

function Shell() {
  const { message, modal } = AntApp.useApp();
  const { settings, lastRun, reload } = useSettings();
  const [banner, setBanner] = useState<string | null>(null);
  const [connectOpen, setConnectOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [settingsForm] = Form.useForm();
  const [tokenForm] = Form.useForm();

  const notify = useCallback(
    (m: string, ok = false) => {
      if (ok) void message.success(m);
      else setBanner(m);
    },
    [message],
  );

  // While the connect modal is open (OAuth runs in a new tab), poll until connected.
  useEffect(() => {
    if (!connectOpen || settings.connected) return;
    const t = setInterval(() => {
      void reload().then((r) => {
        if (r.ok && (r.settings as Settings).connected) {
          setConnectOpen(false);
          void message.success("Đã kết nối Google.");
        }
      });
    }, 3000);
    return () => clearInterval(t);
  }, [connectOpen, settings.connected, reload, message]);

  const sync = async () => {
    setSyncing(true);
    setBanner(null);
    const r = await api.runSync();
    setSyncing(false);
    setRefreshKey((k) => k + 1);
    if (!r.ok) {
      notify(r.error ?? "Sync thất bại");
      return;
    }
    const results = (r.results ?? {}) as Record<string, { status?: string; error?: string }>;
    const failed = Object.entries(results).filter(([, v]) => v.status === "error");
    if (failed.length) {
      notify(`Sync xong nhưng có lỗi: ${failed.map(([k, v]) => `${k}: ${v.error}`).join("; ")}`);
    } else {
      void message.success("Sync hoàn tất.");
    }
  };

  const connectOauth = async () => {
    // Primary path: backend → daemon /api/ui/open-url → HOST system browser.
    // Google rejects embedded webviews (disallowed_useragent) and the OAuth
    // callback lives on the daemon machine, so the browser must open there.
    const r = await api.openAuthInBrowser();
    if (r.ok) {
      void message.success("Đã mở trình duyệt — đồng ý quyền Google rồi quay lại đây.");
      return;
    }
    // Daemon path failed but we still got the URL (e.g. standalone/dev run):
    // fall back to the client-side bridge / window.open.
    if (typeof r.url === "string" && r.url) {
      openExternal(r.url);
      return;
    }
    // Config errors must surface ABOVE the modal (the page banner sits under it).
    void message.error(r.error ?? "Chưa cấu hình OAuth client — mở Cài đặt.");
  };

  const connectToken = async () => {
    const v = await tokenForm.validateFields();
    const r = await api.connectWithToken(v.accessToken.trim(), v.refreshToken?.trim());
    if (r.ok) {
      setConnectOpen(false);
      tokenForm.resetFields();
      await reload();
      void message.success("Đã lưu token — kết nối thành công.");
    } else notify(r.error ?? "Lưu token thất bại");
  };

  const disconnectNow = () => {
    modal.confirm({
      title: "Ngắt kết nối Google?",
      content: "Token lưu cục bộ sẽ bị xoá. Có thể kết nối lại bất cứ lúc nào.",
      okText: "Ngắt kết nối",
      okButtonProps: { danger: true },
      onOk: async () => {
        await api.disconnect();
        await reload();
      },
    });
  };

  const saveSettings = async () => {
    const v = await settingsForm.validateFields();
    const r = await api.saveSettings({
      clientId: v.clientId ?? "",
      clientSecret: v.clientSecret ?? "",
      days: v.days ?? 7,
      services: v.services ?? [],
    });
    if (r.ok) {
      setSettingsOpen(false);
      await reload();
      void message.success("Đã lưu cài đặt.");
    } else notify(r.error ?? "Lưu cài đặt thất bại");
  };

  return (
    <main style={{ minHeight: "100vh", background: "#f5f7fb", padding: "20px 24px 40px" }}>
          <div style={{ maxWidth: 1100, margin: "0 auto" }}>
            {/* Header */}
            <Row align="middle" justify="space-between" style={{ marginBottom: 16 }}>
              <Col>
                <Space size={12} align="center">
                  <div
                    style={{
                      width: 42,
                      height: 42,
                      borderRadius: 12,
                      background: "#fff",
                      border: "1px solid rgba(15,23,42,0.08)",
                      display: "grid",
                      placeItems: "center",
                      fontSize: 20,
                      color: "#1a73e8",
                    }}
                  >
                    <GoogleOutlined />
                  </div>
                  <div>
                    <Title level={4} style={{ margin: 0 }}>
                      Google Workspace
                    </Title>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      Gmail · Calendar · Drive — chạy trong Space, token lưu cục bộ
                    </Text>
                  </div>
                </Space>
              </Col>
              <Col>
                <Space>
                  <Tag icon={<ApiOutlined />} color="geekblue">
                    MCP google-workspace-mcp
                  </Tag>
                  <Button
                    icon={<SettingOutlined />}
                    onClick={() => {
                      settingsForm.setFieldsValue({
                        clientId: settings.clientId,
                        clientSecret: settings.clientSecret,
                        days: settings.days,
                        services: settings.services,
                      });
                      setSettingsOpen(true);
                    }}
                  >
                    Cài đặt
                  </Button>
                </Space>
              </Col>
            </Row>

            {/* Connection hero */}
            <Card
              style={{ marginBottom: 16, overflow: "hidden" }}
              styles={{ body: { padding: 0 } }}
            >
              <div
                style={{
                  background: settings.connected
                    ? "linear-gradient(135deg, #0f172a 0%, #1a73e8 100%)"
                    : "linear-gradient(135deg, #1a73e8 0%, #6366f1 60%, #8b5cf6 100%)",
                  color: "#fff",
                  padding: "20px 22px",
                }}
              >
                <Row align="middle" justify="space-between" gutter={[12, 12]}>
                  <Col>
                    <Space size={12}>
                      {settings.connected ? (
                        <CheckCircleFilled style={{ fontSize: 34 }} />
                      ) : (
                        <GoogleOutlined style={{ fontSize: 34 }} />
                      )}
                      <div>
                        <div style={{ fontSize: 17, fontWeight: 600 }}>
                          {settings.connected ? "Đã kết nối Google" : "Chưa kết nối"}
                        </div>
                        <Text style={{ color: "rgba(255,255,255,0.75)", fontSize: 12 }}>
                          {settings.connected
                            ? settings.hasRefreshToken
                              ? "Có refresh token — tự gia hạn khi hết hạn"
                              : "Token dán tay — hết hạn sau ~1 giờ"
                            : "Kết nối bằng OAuth hoặc dán access token"}
                          {" · "}sync {settings.days} ngày · {settings.services.length} dịch vụ
                        </Text>
                      </div>
                    </Space>
                  </Col>
                  <Col>
                    <Space wrap>
                      {settings.connected ? (
                        <>
                          <Button
                            icon={<DisconnectOutlined />}
                            onClick={disconnectNow}
                            style={{
                              background: "rgba(255,255,255,0.12)",
                              border: "1px solid rgba(255,255,255,0.3)",
                              color: "#fff",
                            }}
                          >
                            Ngắt kết nối
                          </Button>
                          <Button
                            size="large"
                            icon={syncing ? <SyncOutlined spin /> : <CloudSyncOutlined />}
                            loading={syncing}
                            onClick={() => void sync()}
                            style={{ background: "#fff", color: "#1a73e8", fontWeight: 600 }}
                          >
                            Sync ngay
                          </Button>
                        </>
                      ) : (
                        <Button
                          size="large"
                          icon={<GoogleOutlined />}
                          onClick={() => setConnectOpen(true)}
                          style={{ background: "#fff", color: "#1a73e8", fontWeight: 600 }}
                        >
                          Kết nối Google
                        </Button>
                      )}
                    </Space>
                  </Col>
                </Row>
              </div>
            </Card>

            {banner && (
              <Alert
                type="error"
                showIcon
                closable
                style={{ marginBottom: 16 }}
                message={banner}
                onClose={() => setBanner(null)}
              />
            )}

            {/* Stats */}
            <Row gutter={[12, 12]} style={{ marginBottom: 16 }}>
              <Col xs={12} md={6}>
                <Card size="small">
                  <Statistic
                    title="Dịch vụ bật"
                    value={settings.services.length}
                    suffix="/ 3"
                    prefix={<ApiOutlined style={{ color: "#1a73e8" }} />}
                  />
                </Card>
              </Col>
              <Col xs={12} md={6}>
                <Card size="small">
                  <Statistic
                    title="Cửa sổ sync"
                    value={settings.days}
                    suffix="ngày"
                    prefix={<CalendarOutlined style={{ color: "#ea4335" }} />}
                  />
                </Card>
              </Col>
              <Col xs={12} md={6}>
                <Card size="small">
                  <Statistic
                    title="Lần sync gần nhất"
                    value={lastRun ? fmtTime(lastRun.created_at) : "chưa có"}
                    prefix={<CloudSyncOutlined style={{ color: "#34a853" }} />}
                    valueStyle={{ fontSize: 15 }}
                  />
                </Card>
              </Col>
              <Col xs={12} md={6}>
                <Card size="small">
                  <Statistic
                    title="Trạng thái"
                    value={settings.connected ? "Connected" : "Offline"}
                    prefix={
                      <Badge status={settings.connected ? "success" : "default"} />
                    }
                    valueStyle={{ fontSize: 15 }}
                  />
                </Card>
              </Col>
            </Row>

            {/* Service tabs */}
            <Card>
              <Tabs
                defaultActiveKey="gmail"
                items={[
                  {
                    key: "gmail",
                    label: (
                      <span>
                        <MailOutlined /> Gmail
                      </span>
                    ),
                    children: <GmailTab connected={settings.connected} notify={notify} />,
                  },
                  {
                    key: "calendar",
                    label: (
                      <span>
                        <CalendarOutlined /> Calendar
                      </span>
                    ),
                    children: <CalendarTab connected={settings.connected} notify={notify} />,
                  },
                  {
                    key: "drive",
                    label: (
                      <span>
                        <FileOutlined /> Drive
                      </span>
                    ),
                    children: <DriveTab connected={settings.connected} notify={notify} />,
                  },
                  {
                    key: "activity",
                    label: (
                      <span>
                        <SyncOutlined /> Hoạt động
                      </span>
                    ),
                    children: <ActivityTab refreshKey={refreshKey} />,
                  },
                ]}
              />
            </Card>

            <div style={{ textAlign: "center", marginTop: 20 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                SenClaw · Google Workspace Space App (Rust)
              </Text>
            </div>
          </div>

          {/* Connect modal */}
          <Modal
            title={
              <Space>
                <GoogleOutlined style={{ color: "#1a73e8" }} />
                Kết nối Google
              </Space>
            }
            open={connectOpen}
            onCancel={() => setConnectOpen(false)}
            footer={null}
            width={560}
          >
            <Paragraph type="secondary" style={{ marginTop: 4 }}>
              Cách 1 — OAuth chuẩn (khuyên dùng, tự gia hạn token): cần Client ID/Secret trong
              Cài đặt, bấm nút dưới để mở màn hình cấp quyền của Google ở tab mới.
            </Paragraph>
            <Button type="primary" icon={<GoogleOutlined />} onClick={() => void connectOauth()} block>
              Mở trang cấp quyền Google (OAuth)
            </Button>
            <Paragraph type="secondary" style={{ marginTop: 16 }}>
              Cách 2 — dán access token (<code>ya29.…</code>) lấy từ OAuth Playground. Token hết hạn
              sau ~1 giờ trừ khi kèm refresh token.
            </Paragraph>
            <Form form={tokenForm} layout="vertical">
              <Form.Item
                name="accessToken"
                label="Access token"
                rules={[{ required: true, message: "Dán access token" }]}
              >
                <Input.Password placeholder="ya29.a0..." autoComplete="off" />
              </Form.Item>
              <Form.Item name="refreshToken" label="Refresh token (tuỳ chọn)">
                <Input.Password placeholder="1//..." autoComplete="off" />
              </Form.Item>
              <Button icon={<KeyOutlined />} onClick={() => void connectToken()} block>
                Lưu token & kết nối
              </Button>
            </Form>
          </Modal>

          {/* Settings modal */}
          <Modal
            title={
              <Space>
                <SettingOutlined />
                Cài đặt Google Workspace
              </Space>
            }
            open={settingsOpen}
            onCancel={() => setSettingsOpen(false)}
            onOk={() => void saveSettings()}
            okText="Lưu"
            width={620}
          >
            <Form form={settingsForm} layout="vertical" style={{ marginTop: 8 }}>
              <Paragraph type="secondary" style={{ fontSize: 12 }}>
                OAuth client (Google Cloud Console → APIs &amp; Services → Credentials, loại{" "}
                <b>Web application</b>) với redirect URI:{" "}
                <Text code copyable>
                  http://127.0.0.1:4310/api/auth/callback
                </Text>
              </Paragraph>
              <Row gutter={12}>
                <Col span={12}>
                  <Form.Item name="clientId" label="Client ID">
                    <Input placeholder="xxxx.apps.googleusercontent.com" />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item name="clientSecret" label="Client Secret">
                    <Input.Password placeholder="GOCSPX-..." autoComplete="off" />
                  </Form.Item>
                </Col>
              </Row>
              <Row gutter={12}>
                <Col span={12}>
                  <Form.Item name="days" label="Cửa sổ sync (ngày)">
                    <InputNumber min={1} max={90} style={{ width: "100%" }} />
                  </Form.Item>
                </Col>
                <Col span={12}>
                  <Form.Item name="services" label="Dịch vụ sync">
                    <Checkbox.Group
                      options={[
                        { label: "Gmail", value: "gmail" },
                        { label: "Calendar", value: "calendar" },
                        { label: "Drive", value: "drive" },
                      ]}
                    />
                  </Form.Item>
                </Col>
              </Row>
            </Form>
          </Modal>
    </main>
  );
}

export default function App() {
  return (
    // motion:false — the app lives in an iframe where rc-motion's appear
    // animation can stall at opacity 0 (modals never become visible).
    <ConfigProvider
      theme={{ token: { colorPrimary: "#1a73e8", borderRadius: 10, motion: false } }}
    >
      <AntApp>
        <Shell />
      </AntApp>
    </ConfigProvider>
  );
}
