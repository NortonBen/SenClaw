import { useCallback, useEffect, useState } from "react";
import {
  Button,
  Card,
  Col,
  Divider,
  Form,
  Input,
  InputNumber,
  Modal,
  Row,
  Select,
  Space,
  Table,
  Typography,
  message,
} from "antd";
import {
  ArrowLeftOutlined,
  ClockCircleOutlined,
  FolderOpenOutlined,
  GlobalOutlined,
  IdcardOutlined,
  LinkOutlined,
  MonitorOutlined,
  ReloadOutlined,
} from "@ant-design/icons";
import { api } from "../api";
import {
  ENTITY_LIST_MAX_PAGE_SIZE,
  pagedEntityQuery,
  type BrowserProfile,
  type PagedList,
  type TikTokAccount,
} from "../types/api";

type ProfileFormValues = {
  name: string;
  userDataDir: string;
  userAgent?: string;
  viewportWidth?: number | null;
  viewportHeight?: number | null;
  locale?: string;
  timezoneId?: string;
  accountId?: string;
  notes?: string;
};

const SEARCH_DEBOUNCE_MS = 400;

export function ProfilesPage() {
  const [items, setItems] = useState<BrowserProfile[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [listLoading, setListLoading] = useState(false);
  const [searchInput, setSearchInput] = useState("");
  const [searchQ, setSearchQ] = useState("");

  const [accounts, setAccounts] = useState<TikTokAccount[]>([]);
  const [accountsLoading, setAccountsLoading] = useState(false);

  const [editing, setEditing] = useState<BrowserProfile | null>(null);
  const [mode, setMode] = useState<"list" | "form">("list");
  const [error, setError] = useState<string | null>(null);
  const [aiOpen, setAiOpen] = useState(false);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiForm] = Form.useForm<{ note: string; accountId: string; quantity: number }>();
  const [profileForm] = Form.useForm<ProfileFormValues>();
  const [messageApi, messageContextHolder] = message.useMessage();

  const loadAccountsForPickers = useCallback(async () => {
    setAccountsLoading(true);
    try {
      const aRaw = await api<PagedList<TikTokAccount>>(`/api/accounts?${pagedEntityQuery(1, ENTITY_LIST_MAX_PAGE_SIZE)}`);
      setAccounts(Array.isArray(aRaw?.items) ? aRaw.items : []);
    } catch (err) {
      setError(String(err));
      setAccounts([]);
    } finally {
      setAccountsLoading(false);
    }
  }, []);

  const loadProfileRows = useCallback(async () => {
    setListLoading(true);
    try {
      setError(null);
      const res = await api<PagedList<BrowserProfile>>(`/api/browser-profiles?${pagedEntityQuery(page, pageSize, searchQ)}`);
      setItems(Array.isArray(res.items) ? res.items : []);
      setTotal(typeof res.total === "number" ? res.total : 0);
    } catch (err) {
      setError(String(err));
      setItems([]);
      setTotal(0);
    } finally {
      setListLoading(false);
    }
  }, [page, pageSize, searchQ]);

  useEffect(() => {
    const t = window.setTimeout(() => setSearchQ(searchInput.trim()), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [searchInput]);

  useEffect(() => {
    setPage(1);
  }, [searchQ]);

  useEffect(() => {
    void loadAccountsForPickers();
  }, [loadAccountsForPickers]);

  useEffect(() => {
    if (mode === "list") void loadProfileRows();
  }, [mode, loadProfileRows]);

  useEffect(() => {
    if (mode !== "form") return;
    profileForm.setFieldsValue({
      name: editing?.name ?? "",
      userDataDir: editing?.userDataDir ?? "",
      userAgent: editing?.userAgent ?? "",
      viewportWidth: editing?.viewportWidth ?? undefined,
      viewportHeight: editing?.viewportHeight ?? undefined,
      locale: editing?.locale ?? "",
      timezoneId: editing?.timezoneId ?? "",
      accountId: editing?.accountId || undefined,
      notes: editing?.notes ?? "",
    });
  }, [mode, editing, profileForm]);

  const onProfileFinish = async (values: ProfileFormValues) => {
    const vw = Number(values.viewportWidth);
    const vh = Number(values.viewportHeight);
    try {
      setError(null);
      await api<BrowserProfile>("/api/browser-profiles", "POST", {
        id: editing?.id ?? "",
        name: (values.name ?? "").trim(),
        userDataDir: (values.userDataDir ?? "").trim(),
        userAgent: (values.userAgent ?? "").trim(),
        viewportWidth: Number.isFinite(vw) ? vw : 0,
        viewportHeight: Number.isFinite(vh) ? vh : 0,
        locale: (values.locale ?? "").trim(),
        timezoneId: (values.timezoneId ?? "").trim(),
        accountId: (values.accountId ?? "").trim(),
        notes: (values.notes ?? "").trim(),
      });
      setEditing(null);
      profileForm.resetFields();
      await Promise.all([loadProfileRows(), loadAccountsForPickers()]);
      messageApi.success("Đã lưu profile");
      setMode("list");
    } catch (err) {
      setError(String(err));
    }
  };

  const onDelete = async (id: string) => {
    try {
      setError(null);
      const res = await fetch(`/api/browser-profiles/${encodeURIComponent(id)}`, {
        method: "DELETE",
      });
      if (!res.ok) throw new Error(await res.text());
      await loadProfileRows();
      messageApi.success("Đã xóa");
    } catch (err) {
      setError(String(err));
    }
  };

  const onGenerateWithAI = async () => {
    try {
      setError(null);
      const values = await aiForm.validateFields();
      const qty = Math.max(1, Math.min(20, Number(values.quantity) || 1));
      const accountId = (values.accountId ?? "").trim();
      const note = (values.note ?? "").trim();
      setAiLoading(true);

      if (qty === 1) {
        const draft = await api<BrowserProfile>("/api/agent/profiles/generate", "POST", { accountId, note });
        setEditing({ ...draft, id: "" });
        setMode("form");
        setAiOpen(false);
        messageApi.success("Đã tạo draft profile bằng AI");
        return;
      }

      for (let i = 0; i < qty; i += 1) {
        const draft = await api<BrowserProfile>("/api/agent/profiles/generate", "POST", { accountId, note });
        const suffix = String(i + 1).padStart(2, "0");
        await api<BrowserProfile>("/api/browser-profiles", "POST", {
          ...draft,
          id: "",
          name: `${draft.name || "AI Profile"} ${suffix}`,
          userDataDir: `${draft.userDataDir || "./profiles/ai_profile"}_${suffix}`,
        });
      }
      setAiOpen(false);
      await Promise.all([loadProfileRows(), loadAccountsForPickers()]);
      messageApi.success(`Đã tạo ${qty} profiles bằng AI`);
    } catch (err) {
      setError(String(err));
    } finally {
      setAiLoading(false);
    }
  };

  return (
    <div className="page">
      {messageContextHolder}
      {error && <pre className="error">{error}</pre>}

      {mode === "list" ? (
        <Card
          title={
            <div>
              <div style={{ fontWeight: 700 }}>Profiles</div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Lọc & phân trang từ server
              </Typography.Text>
            </div>
          }
          extra={
            <Space>
              <Input
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                placeholder="Lọc tên, thư mục, account…"
                style={{ width: 240 }}
                allowClear
              />
              <Button icon={<ReloadOutlined />} onClick={() => void loadProfileRows()} disabled={listLoading}>
                Làm mới
              </Button>
              <Button disabled>Add Filter</Button>
              <Button
                onClick={() => {
                  aiForm.setFieldsValue({ note: "", accountId: accounts[0]?.id ?? "", quantity: 1 });
                  setAiOpen(true);
                }}
              >
                AI Generate
              </Button>
              <Button
                type="primary"
                onClick={() => {
                  setEditing(null);
                  setMode("form");
                }}
              >
                + Add Profile
              </Button>
            </Space>
          }
        >
          <Table
            rowKey="id"
            dataSource={items}
            loading={listLoading}
            pagination={{
              current: page,
              pageSize,
              total,
              showSizeChanger: true,
              pageSizeOptions: [10, 20, 50, 100],
              showTotal: (t) => `${t} profile`,
              onChange: (p, ps) => {
                setPage(p);
                setPageSize(ps);
              },
            }}
            columns={[
              { title: "Name", dataIndex: "name" },
              { title: "UserDataDir", dataIndex: "userDataDir" },
              { title: "Account", dataIndex: "accountId" },
              {
                title: "Viewport",
                render: (_, r) => (r.viewportWidth || r.viewportHeight ? `${r.viewportWidth}×${r.viewportHeight}` : ""),
              },
              {
                title: "Actions",
                render: (_, r) => (
                  <Space>
                    <Button
                      type="link"
                      onClick={() => {
                        setEditing(r);
                        setMode("form");
                      }}
                    >
                      Edit
                    </Button>
                    <Button type="link" danger onClick={() => void onDelete(r.id)}>
                      Delete
                    </Button>
                  </Space>
                ),
              },
            ]}
            locale={{ emptyText: listLoading ? "Đang tải…" : "No data" }}
          />
        </Card>
      ) : (
        <Card
          className="entity-editor-card"
          title={
            <div>
              <div style={{ fontWeight: 700 }}>{editing ? "Chỉnh sửa profile" : "Thêm profile"}</div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Đường dẫn dữ liệu Chrome, viewport và fingerprint cho phiên chạy
              </Typography.Text>
            </div>
          }
          extra={
            <Button
              icon={<ArrowLeftOutlined />}
              onClick={() => {
                setMode("list");
                setEditing(null);
                profileForm.resetFields();
              }}
            >
              Quay lại
            </Button>
          }
        >
          <Form<ProfileFormValues>
            form={profileForm}
            layout="vertical"
            requiredMark="optional"
            onFinish={(v) => void onProfileFinish(v)}
            className="entity-editor-form"
            size="middle"
          >
            <Divider orientation="left" plain style={{ marginTop: 0 }}>
              <Space size={8}>
                <IdcardOutlined />
                Thông tin chính
              </Space>
            </Divider>
            <Form.Item name="name" label="Tên profile" rules={[{ required: true, message: "Nhập tên profile" }]}>
              <Input placeholder="Ví dụ: AI Profile 10" allowClear />
            </Form.Item>
            <Form.Item
              name="userDataDir"
              label="Thư mục User Data"
              extra="Đường dẫn thư mục profile Chrome (tương đối hoặc tuyệt đối)."
              rules={[{ required: true, message: "Nhập đường dẫn thư mục" }]}
            >
              <Input placeholder="./profiles/profile_ai_10" prefix={<FolderOpenOutlined style={{ color: "var(--muted-text)" }} />} />
            </Form.Item>

            <Divider orientation="left" plain>
              <Space size={8}>
                <MonitorOutlined />
                Trình duyệt & viewport
              </Space>
            </Divider>
            <Form.Item name="userAgent" label="User-Agent">
              <Input placeholder="Tuỳ chọn — để trống dùng mặc định hệ thống" allowClear />
            </Form.Item>
            <Form.Item label="Kích thước viewport (px)" style={{ marginBottom: 0 }}>
              <Row gutter={16}>
                <Col xs={24} sm={12}>
                  <Form.Item name="viewportWidth" noStyle>
                    <InputNumber placeholder="Rộng (vd. 1366)" min={0} style={{ width: "100%" }} controls />
                  </Form.Item>
                </Col>
                <Col xs={24} sm={12}>
                  <Form.Item name="viewportHeight" noStyle>
                    <InputNumber placeholder="Cao (vd. 768)" min={0} style={{ width: "100%" }} controls />
                  </Form.Item>
                </Col>
              </Row>
            </Form.Item>

            <Divider orientation="left" plain>
              <Space size={8}>
                <GlobalOutlined />
                Ngôn ngữ & múi giờ
              </Space>
            </Divider>
            <Row gutter={16}>
              <Col xs={24} md={12}>
                <Form.Item name="locale" label="Locale">
                  <Input placeholder="en_US, vi-VN…" prefix={<GlobalOutlined style={{ color: "var(--muted-text)" }} />} allowClear />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item name="timezoneId" label="Timezone (IANA)">
                  <Input
                    placeholder="Asia/Ho_Chi_Minh, Asia/Bangkok…"
                    prefix={<ClockCircleOutlined style={{ color: "var(--muted-text)" }} />}
                    allowClear
                  />
                </Form.Item>
              </Col>
            </Row>

            <Divider orientation="left" plain>
              <Space size={8}>
                <LinkOutlined />
                Liên kết & ghi chú
              </Space>
            </Divider>
            <Form.Item name="accountId" label="Gắn TikTok account">
              <Select
                allowClear
                placeholder="Tuỳ chọn — không gắn account"
                options={accounts.map((a) => ({ value: a.id, label: `${a.username} (${a.id})` }))}
                optionFilterProp="label"
                showSearch
                loading={accountsLoading}
              />
            </Form.Item>
            <Form.Item name="notes" label="Ghi chú">
              <Input.TextArea rows={3} placeholder="Mô tả nhanh: thiết bị, mục đích test…" allowClear />
            </Form.Item>

            <Form.Item style={{ marginBottom: 0, marginTop: 8 }}>
              <Button type="primary" htmlType="submit" block size="large">
                {editing ? "Lưu thay đổi" : "Thêm profile"}
              </Button>
            </Form.Item>
          </Form>
        </Card>
      )}
      <Modal
        title="AI Generate Profiles"
        open={aiOpen}
        onCancel={() => setAiOpen(false)}
        onOk={() => void onGenerateWithAI()}
        confirmLoading={aiLoading}
        okText="Generate"
      >
        <Form form={aiForm} layout="vertical" initialValues={{ note: "", accountId: "", quantity: 1 }}>
          <Form.Item name="note" label="Mô tả nhanh profile (optional)">
            <Input.TextArea rows={3} placeholder="Ví dụ: Android profile, nhẹ, timezone VN..." />
          </Form.Item>
          <Form.Item name="accountId" label="Account tối ưu theo (optional)">
            <Select
              allowClear
              placeholder="Chọn account (tuỳ chọn)"
              options={accounts.map((a) => ({ value: a.id, label: `${a.username} (${a.id})` }))}
              loading={accountsLoading}
            />
          </Form.Item>
          <Form.Item name="quantity" label="Số lượng profile" rules={[{ required: true }]}>
            <InputNumber min={1} max={20} style={{ width: "100%" }} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
