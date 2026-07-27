import { useCallback, useEffect, useState } from "react";
import {
  ArrowLeftOutlined,
  DesktopOutlined,
  LockOutlined,
  ReloadOutlined,
  SafetyCertificateOutlined,
  UserOutlined,
} from "@ant-design/icons";
import {
  Button,
  Card,
  Divider,
  Form,
  Input,
  Row,
  Col,
  Select,
  Space,
  Table,
  Typography,
  message,
} from "antd";
import { api } from "../api";
import {
  ENTITY_LIST_MAX_PAGE_SIZE,
  pagedEntityQuery,
  type BrowserProfile,
  type PagedList,
  type TikTokAccount,
} from "../types/api";

type AccountFormValues = {
  username: string;
  password: string;
  browserProfileId?: string;
  profilePath?: string;
  userAgent?: string;
};

const SEARCH_DEBOUNCE_MS = 400;

export function AccountsPage() {
  const [items, setItems] = useState<TikTokAccount[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [listLoading, setListLoading] = useState(false);

  const [profiles, setProfiles] = useState<BrowserProfile[]>([]);
  const [metaLoading, setMetaLoading] = useState(false);

  const [searchInput, setSearchInput] = useState("");
  const [searchQ, setSearchQ] = useState("");

  const [editing, setEditing] = useState<TikTokAccount | null>(null);
  const [mode, setMode] = useState<"list" | "form">("list");
  const [error, setError] = useState<string | null>(null);
  const [accountForm] = Form.useForm<AccountFormValues>();
  const [messageApi, messageContextHolder] = message.useMessage();

  const loadPickerMeta = useCallback(async () => {
    setMetaLoading(true);
    try {
      const bpRaw = await api<PagedList<BrowserProfile>>(
        `/api/browser-profiles?${pagedEntityQuery(1, ENTITY_LIST_MAX_PAGE_SIZE)}`,
      );
      setProfiles(Array.isArray(bpRaw?.items) ? bpRaw.items : []);
    } catch (err) {
      setError(String(err));
      setProfiles([]);
    } finally {
      setMetaLoading(false);
    }
  }, []);

  const loadAccountRows = useCallback(async () => {
    setListLoading(true);
    try {
      setError(null);
      const res = await api<PagedList<TikTokAccount>>(`/api/accounts?${pagedEntityQuery(page, pageSize, searchQ)}`);
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
    void loadPickerMeta();
  }, [loadPickerMeta]);

  useEffect(() => {
    if (mode === "list") void loadAccountRows();
  }, [mode, loadAccountRows]);

  useEffect(() => {
    if (mode !== "form") return;
    accountForm.setFieldsValue({
      username: editing?.username ?? "",
      password: editing?.password ?? "",
      browserProfileId: editing?.browserProfileId || undefined,
      profilePath: editing?.profilePath ?? "",
      userAgent: editing?.userAgent ?? "",
    });
  }, [mode, editing, accountForm]);

  const onAccountFinish = async (values: AccountFormValues) => {
    try {
      setError(null);
      await api<TikTokAccount>("/api/accounts", "POST", {
        id: editing?.id ?? "",
        username: (values.username ?? "").trim(),
        password: values.password ?? "",
        browserProfileId: (values.browserProfileId ?? "").trim(),
        profilePath: (values.profilePath ?? "").trim(),
        userAgent: (values.userAgent ?? "").trim(),
      });
      setEditing(null);
      accountForm.resetFields();
      await Promise.all([loadPickerMeta(), loadAccountRows()]);
      messageApi.success("Đã lưu account");
      setMode("list");
    } catch (err) {
      setError(String(err));
    }
  };

  const profileLabel = (id?: string) => profiles.find((p) => p.id === id)?.name ?? "";

  return (
    <div className="page">
      {messageContextHolder}
      {error && <pre className="error">{error}</pre>}

      {mode === "list" ? (
        <Card
          title={
            <div>
              <div style={{ fontWeight: 700 }}>Accounts</div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Quản lý tài khoản TikTok — lọc & phân trang từ server
              </Typography.Text>
            </div>
          }
          extra={
            <Space>
              <Input
                value={searchInput}
                onChange={(e) => setSearchInput(e.target.value)}
                placeholder="Lọc username, id, profile…"
                style={{ width: 260 }}
                allowClear
              />
              <Button icon={<ReloadOutlined />} onClick={() => void loadAccountRows()} disabled={listLoading}>
                Làm mới
              </Button>
              <Button disabled>Add Filter</Button>
              <Button
                type="primary"
                onClick={() => {
                  setEditing(null);
                  setMode("form");
                }}
              >
                + Add Account
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
              showTotal: (t) => `${t} account`,
              onChange: (p, ps) => {
                setPage(p);
                setPageSize(ps);
              },
            }}
            columns={[
              { title: "Username", dataIndex: "username" },
              { title: "Profile", render: (_, r) => (r.browserProfileId ? profileLabel(r.browserProfileId) || r.browserProfileId : "") },
              {
                title: "Actions",
                render: (_, r) => (
                  <Button
                    type="link"
                    onClick={() => {
                      setEditing(r);
                      setMode("form");
                    }}
                  >
                    Edit
                  </Button>
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
              <div style={{ fontWeight: 700 }}>{editing ? "Chỉnh sửa account" : "Thêm account"}</div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                TikTok login và browser profile gắn với account
              </Typography.Text>
            </div>
          }
          extra={
            <Button
              icon={<ArrowLeftOutlined />}
              onClick={() => {
                setMode("list");
                setEditing(null);
                accountForm.resetFields();
              }}
            >
              Quay lại
            </Button>
          }
        >
          <Form<AccountFormValues>
            form={accountForm}
            layout="vertical"
            requiredMark="optional"
            onFinish={(v) => void onAccountFinish(v)}
            className="entity-editor-form"
            size="middle"
          >
            <Divider orientation="left" plain style={{ marginTop: 0 }}>
              <Space size={8}>
                <UserOutlined />
                Đăng nhập TikTok
              </Space>
            </Divider>
            <Row gutter={16}>
              <Col xs={24} md={12}>
                <Form.Item
                  name="username"
                  label="Username"
                  rules={[{ required: true, message: "Nhập username" }]}
                >
                  <Input placeholder="Tên đăng nhập TikTok" allowClear prefix={<UserOutlined style={{ color: "var(--muted-text)" }} />} />
                </Form.Item>
              </Col>
              <Col xs={24} md={12}>
                <Form.Item
                  name="password"
                  label="Mật khẩu"
                  rules={[{ required: true, message: "Nhập mật khẩu" }]}
                >
                  <Input.Password placeholder="Mật khẩu" prefix={<LockOutlined style={{ color: "var(--muted-text)" }} />} />
                </Form.Item>
              </Col>
            </Row>

            <Divider orientation="left" plain>
              <Space size={8}>
                <DesktopOutlined />
                Trình duyệt & fingerprint
              </Space>
            </Divider>
            <Form.Item
              name="browserProfileId"
              label="Browser profile đã lưu"
              extra="Gắn profile Chrome đã cấu hình ở trang Profiles."
            >
              <Select
                allowClear
                placeholder="Không gắn profile"
                options={profiles.map((p) => ({
                  value: p.id,
                  label: `${p.name} (${p.userDataDir})`,
                }))}
                showSearch
                optionFilterProp="label"
                loading={metaLoading}
              />
            </Form.Item>
            <Form.Item
              name="profilePath"
              label="UserDataDir thủ công (legacy)"
              extra="Ghi đè đường dẫn thư mục khi không dùng profile đã lưu."
            >
              <Input placeholder="./profiles/acc_01" allowClear />
            </Form.Item>
            <Form.Item name="userAgent" label="User-Agent (legacy)">
              <Input placeholder="Ghi đè User-Agent khi cần" allowClear prefix={<SafetyCertificateOutlined style={{ color: "var(--muted-text)" }} />} />
            </Form.Item>

            <Form.Item style={{ marginBottom: 0, marginTop: 8 }}>
              <Button type="primary" htmlType="submit" block size="large">
                {editing ? "Lưu thay đổi" : "Thêm account"}
              </Button>
            </Form.Item>
          </Form>
        </Card>
      )}
    </div>
  );
}
