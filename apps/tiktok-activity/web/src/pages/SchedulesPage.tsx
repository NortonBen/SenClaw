import { useEffect, useMemo, useState } from "react";
import {
  Button,
  Card,
  Col,
  DatePicker,
  Divider,
  Form,
  Input,
  Popconfirm,
  Row,
  Segmented,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  TimePicker,
  Typography,
  message,
} from "antd";
import {
  ArrowLeftOutlined,
  CalendarOutlined,
  ClockCircleOutlined,
  PlusOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import dayjs, { type Dayjs } from "dayjs";
import customParseFormat from "dayjs/plugin/customParseFormat";
import { api } from "../api";
import {
  ENTITY_LIST_MAX_PAGE_SIZE,
  pagedEntityQuery,
  type Flow,
  type PagedList,
  type Schedule,
  type ScheduleType,
  type TikTokAccount,
} from "../types/api";

dayjs.extend(customParseFormat);

const SCHEDULE_TYPES: { value: ScheduleType; label: string; hint: string }[] = [
  { value: "daily_at", label: "Hằng ngày", hint: "Chạy mỗi ngày vào giờ cố định (theo múi giờ)" },
  { value: "once_at", label: "Một lần", hint: "Chạy đúng một lần vào ngày giờ bạn chọn" },
  { value: "run_now", label: "Chạy ngay", hint: "Lưu xong sẽ dispatch ngay (schedule tự tắt sau đó)" },
];

const COMMON_TIMEZONES = [
  "Asia/Ho_Chi_Minh",
  "Asia/Bangkok",
  "Asia/Singapore",
  "Asia/Tokyo",
  "Asia/Seoul",
  "Asia/Shanghai",
  "Asia/Hong_Kong",
  "Asia/Kolkata",
  "Europe/London",
  "Europe/Paris",
  "Europe/Berlin",
  "America/New_York",
  "America/Los_Angeles",
  "America/Chicago",
  "Australia/Sydney",
  "UTC",
];

type FormValues = {
  name: string;
  flowId: string;
  enabled: boolean;
  allAccounts: boolean;
  type: ScheduleType;
  dailyAtTime: Dayjs | null;
  onceAtDate: Dayjs | null;
  timezoneId: string;
  accountIds: string[];
};

function dailyAtStringToDayjs(s: string): Dayjs | null {
  const t = dayjs(s.trim(), "H:mm", true);
  if (t.isValid()) return t;
  const t2 = dayjs(s.trim(), "HH:mm", true);
  return t2.isValid() ? t2 : null;
}

function dayjsToDailyAt(d: Dayjs | null): string {
  if (!d || !d.isValid()) return "";
  return `${String(d.hour()).padStart(2, "0")}:${String(d.minute()).padStart(2, "0")}`;
}

function onceAtToRFC3339(d: Dayjs | null): string {
  if (!d || !d.isValid()) return "";
  return d.format("YYYY-MM-DDTHH:mm:ssZ");
}

export function SchedulesPage() {
  const [form] = Form.useForm<FormValues>();
  const [items, setItems] = useState<Schedule[]>([]);
  const [flows, setFlows] = useState<Flow[]>([]);
  const [accounts, setAccounts] = useState<TikTokAccount[]>([]);
  const [editing, setEditing] = useState<Schedule | null>(null);
  const [mode, setMode] = useState<"list" | "form">("list");
  const [q, setQ] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [scheduleParamRows, setScheduleParamRows] = useState<Array<{ key: string; value: string }>>([]);

  const scheduleType = Form.useWatch("type", form);
  const allAccounts = Form.useWatch("allAccounts", form);
  const flowIdWatch = Form.useWatch("flowId", form);

  const asArray = <T,>(v: T[] | null | undefined): T[] => (Array.isArray(v) ? v : []);
  const mapToRows = (m?: Record<string, string>) => Object.entries(m ?? {}).map(([key, value]) => ({ key, value: String(value ?? "") }));
  const rowsToMap = (rows: Array<{ key: string; value: string }>) => {
    const out: Record<string, string> = {};
    for (const r of rows) {
      const k = r.key.trim();
      if (!k) continue;
      out[k] = r.value;
    }
    return out;
  };

  const refresh = async () => {
    try {
      const [sRaw, fRaw, aRaw] = await Promise.all([
        api<Schedule[] | null>("/api/schedules"),
        api<Flow[] | null>("/api/flows"),
        api<PagedList<TikTokAccount> | null>(`/api/accounts?${pagedEntityQuery(1, ENTITY_LIST_MAX_PAGE_SIZE)}`),
      ]);
      setItems(asArray(sRaw));
      setFlows(asArray(fRaw));
      setAccounts(asArray(aRaw?.items));
    } catch (err) {
      setError(String(err));
      setItems([]);
      setFlows([]);
      setAccounts([]);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    if (!s) return items;
    return items.filter(
      (x) =>
        x.name.toLowerCase().includes(s) ||
        x.id.toLowerCase().includes(s) ||
        x.flowId.toLowerCase().includes(s)
    );
  }, [items, q]);

  const flowName = (id: string) => flows.find((f) => f.id === id)?.name ?? id;
  const fmt = (v?: string) => (v ? new Date(v).toLocaleString() : "");

  const typeLabel = (t: ScheduleType) => SCHEDULE_TYPES.find((x) => x.value === t)?.label ?? t;

  const openForm = (row: Schedule | null) => {
    setEditing(row);
    setMode("form");
    const tz = row?.timezoneId?.trim() || "Asia/Ho_Chi_Minh";
    const daily = row?.dailyAt ? dailyAtStringToDayjs(row.dailyAt) : dayjs().hour(9).minute(0).second(0);
    const once = row?.onceAt ? dayjs(row.onceAt) : null;
    form.setFieldsValue({
      name: row?.name ?? "",
      flowId: row?.flowId ?? flows[0]?.id ?? "",
      enabled: row?.enabled ?? true,
      allAccounts: row?.allAccounts ?? true,
      type: row?.type ?? "daily_at",
      dailyAtTime: daily && daily.isValid() ? daily : dayjs().hour(9).minute(0).second(0),
      onceAtDate: once && once.isValid() ? once : null,
      timezoneId: tz,
      accountIds: row?.accountIds ?? [],
    });
    setScheduleParamRows(mapToRows(row?.params));
  };

  useEffect(() => {
    if (mode !== "form" || flows.length === 0) return;
    const fid = form.getFieldValue("flowId");
    if (!fid && flows[0]) form.setFieldValue("flowId", flows[0].id);
  }, [mode, flows, form]);

  useEffect(() => {
    if (mode !== "form") return;
    if (editing) return;
    if (scheduleParamRows.length > 0) return;
    const f = flows.find((x) => x.id === flowIdWatch);
    if (!f?.params) return;
    if (Object.keys(f.params).length === 0) return;
    setScheduleParamRows(mapToRows(f.params));
  }, [mode, editing, scheduleParamRows.length, flows, flowIdWatch]);

  const onFinish = async (values: FormValues) => {
    const type = values.type;
    const dailyAt = type === "daily_at" ? dayjsToDailyAt(values.dailyAtTime) : "";
    const onceAt = type === "once_at" ? onceAtToRFC3339(values.onceAtDate) : "";
    const body: Schedule = {
      id: editing?.id ?? "",
      name: values.name.trim(),
      enabled: values.enabled,
      flowId: values.flowId,
      params: rowsToMap(scheduleParamRows),
      allAccounts: values.allAccounts,
      accountIds: values.allAccounts ? [] : (values.accountIds ?? []),
      type,
      dailyAt,
      onceAt,
      timezoneId: type === "daily_at" ? values.timezoneId.trim() : values.timezoneId.trim(),
      lastRunAt: editing?.lastRunAt,
      nextRunAt: editing?.nextRunAt,
      createdAt: editing?.createdAt,
      updatedAt: editing?.updatedAt,
    };
    try {
      setSubmitting(true);
      setError(null);
      await api<Schedule>("/api/schedules", "POST", body);
      message.success(editing ? "Đã cập nhật schedule" : "Đã tạo schedule");
      setMode("list");
      setEditing(null);
      form.resetFields();
      await refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  };

  const toggleSchedule = async (id: string) => {
    try {
      await api<Schedule>(`/api/schedules/${id}/toggle`, "POST");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const runNow = async (id: string) => {
    try {
      await api<{ msg: string }>(`/api/schedules/${id}/run-now`, "POST");
      message.success("Đã gửi lệnh chạy");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const del = async (id: string) => {
    try {
      await api<{ msg: string }>(`/api/schedules/${id}`, "DELETE");
      message.success("Đã xóa schedule");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const typeHint = SCHEDULE_TYPES.find((x) => x.value === scheduleType)?.hint ?? "";

  return (
    <div className="page">
      {error && (
        <Card size="small" style={{ marginBottom: 12, borderColor: "var(--card-border)" }}>
          <Typography.Text type="danger">{error}</Typography.Text>
        </Card>
      )}

      {mode === "list" ? (
        <Card
          title={
            <div>
              <div style={{ fontWeight: 700 }}>Lịch chạy</div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Tự động chạy flow theo giờ hoặc một lần — gắn với lịch sử run
              </Typography.Text>
            </div>
          }
          extra={
            <Space wrap>
              <Input
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder="Tìm theo tên, ID, flow…"
                style={{ width: 240 }}
                allowClear
              />
              <Button type="primary" icon={<PlusOutlined />} onClick={() => openForm(null)}>
                Thêm lịch
              </Button>
            </Space>
          }
        >
          <Table
            rowKey="id"
            pagination={false}
            dataSource={filtered}
            columns={[
              { title: "Tên", dataIndex: "name", ellipsis: true },
              {
                title: "Flow",
                ellipsis: true,
                render: (_, r) => flowName(r.flowId),
              },
              {
                title: "Đích",
                render: (_, r) =>
                  r.allAccounts ? (
                    <Tag color="blue">Tất cả account</Tag>
                  ) : (
                    <Tag>{r.accountIds.length} account</Tag>
                  ),
              },
              {
                title: "Params",
                render: (_, r) => <Tag>{Object.keys(r.params ?? {}).length}</Tag>,
              },
              {
                title: "Kiểu",
                render: (_, r) => <Tag icon={<ClockCircleOutlined />}>{typeLabel(r.type)}</Tag>,
              },
              {
                title: "Bật",
                width: 88,
                render: (_, r) => <Switch checked={r.enabled} onChange={() => void toggleSchedule(r.id)} />,
              },
              { title: "Lần chạy tới", render: (_, r) => fmt(r.nextRunAt) || "—" },
              { title: "Lần chạy trước", render: (_, r) => fmt(r.lastRunAt) || "—" },
              {
                title: "Thao tác",
                width: 220,
                fixed: "right" as const,
                render: (_, r) => (
                  <Space size={0} wrap>
                    <Button type="link" size="small" onClick={() => openForm(r)}>
                      Sửa
                    </Button>
                    <Button
                      type="link"
                      size="small"
                      icon={<ThunderboltOutlined />}
                      onClick={() => void runNow(r.id)}
                      disabled={!r.enabled}
                    >
                      Chạy ngay
                    </Button>
                    <Popconfirm title="Xóa schedule này?" okText="Xóa" cancelText="Hủy" onConfirm={() => void del(r.id)}>
                      <Button type="link" size="small" danger>
                        Xóa
                      </Button>
                    </Popconfirm>
                  </Space>
                ),
              },
            ]}
            locale={{ emptyText: "Chưa có lịch nào" }}
            scroll={{ x: 900 }}
          />
        </Card>
      ) : (
        <Card
          title={
            <Space>
              <Button type="text" icon={<ArrowLeftOutlined />} onClick={() => { setMode("list"); setEditing(null); form.resetFields(); }}>
                Quay lại
              </Button>
              <Divider type="vertical" />
              <span style={{ fontWeight: 700 }}>{editing ? "Sửa lịch chạy" : "Thêm lịch chạy"}</span>
            </Space>
          }
        >
          <Typography.Paragraph type="secondary" style={{ marginTop: 0, marginBottom: 20, maxWidth: 640 }}>
            Chọn flow và thời điểm. Với <b>hằng ngày</b>, giờ được hiểu theo múi giờ bạn chọn. Với <b>một lần</b>, chọn ngày giờ theo máy bạn (gửi lên server dạng RFC3339).
          </Typography.Paragraph>

          <Form<FormValues>
            form={form}
            layout="vertical"
            onFinish={onFinish}
            initialValues={{
              name: "",
              flowId: "",
              enabled: true,
              allAccounts: true,
              type: "daily_at" as ScheduleType,
              dailyAtTime: dayjs().hour(9).minute(0).second(0),
              onceAtDate: null,
              timezoneId: "Asia/Ho_Chi_Minh",
              accountIds: [],
            }}
            requiredMark="optional"
            style={{ maxWidth: 720 }}
          >
            <Form.Item
              name="name"
              label="Tên lịch"
              rules={[{ required: true, message: "Nhập tên lịch" }]}
            >
              <Input placeholder="Ví dụ: Share buổi sáng" size="large" />
            </Form.Item>

            <Row gutter={16}>
              <Col xs={24} md={14}>
                <Form.Item
                  name="flowId"
                  label="Flow"
                  rules={[{ required: true, message: "Chọn flow" }]}
                >
                  <Select
                    placeholder="Chọn flow"
                    size="large"
                    options={flows.map((f) => ({ value: f.id, label: f.name }))}
                    showSearch
                    optionFilterProp="label"
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={10}>
                <Form.Item name="enabled" label="Kích hoạt" valuePropName="checked">
                  <Switch checkedChildren="Bật" unCheckedChildren="Tắt" />
                </Form.Item>
              </Col>
            </Row>
            <Form.Item label="Flow params khi schedule chạy (key-value)">
              <Typography.Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 8 }}>
                Flow mặc định cần params:{" "}
                {Object.keys(flows.find((f) => f.id === flowIdWatch)?.params ?? {}).join(", ") || "(không có)"}.
                Schedule params sẽ override flow defaults khi chạy.
              </Typography.Text>
              <Space direction="vertical" style={{ width: "100%" }}>
                {scheduleParamRows.map((row, idx) => (
                  <Space key={`sp-${idx}`} wrap>
                    <Input
                      placeholder="key"
                      value={row.key}
                      onChange={(e) =>
                        setScheduleParamRows((prev) => prev.map((x, i) => (i === idx ? { ...x, key: e.target.value } : x)))
                      }
                      style={{ width: 200 }}
                    />
                    <Input
                      placeholder="value"
                      value={row.value}
                      onChange={(e) =>
                        setScheduleParamRows((prev) => prev.map((x, i) => (i === idx ? { ...x, value: e.target.value } : x)))
                      }
                      style={{ width: 260 }}
                    />
                    <Button danger onClick={() => setScheduleParamRows((prev) => prev.filter((_, i) => i !== idx))}>
                      Xóa
                    </Button>
                  </Space>
                ))}
                <Button onClick={() => setScheduleParamRows((prev) => [...prev, { key: "", value: "" }])}>+ Thêm param</Button>
              </Space>
            </Form.Item>

            <Form.Item name="allAccounts" label="Chạy trên">
              <Segmented
                options={[
                  { label: "Tất cả account", value: true },
                  { label: "Chọn account", value: false },
                ]}
              />
            </Form.Item>

            {!allAccounts ? (
              <Form.Item
                name="accountIds"
                label="Account được chọn"
                rules={[{ required: true, type: "array", min: 1, message: "Chọn ít nhất một account" }]}
              >
                <Select
                  mode="multiple"
                  placeholder="Chọn một hoặc nhiều account"
                  options={accounts.map((a) => ({ value: a.id, label: `${a.username} (${a.id})` }))}
                  showSearch
                  optionFilterProp="label"
                  size="large"
                />
              </Form.Item>
            ) : null}

            <Divider orientation="left" plain>
              <Space>
                <CalendarOutlined />
                Loại lịch
              </Space>
            </Divider>

            <Form.Item name="type" label="Kiểu lịch">
              <Segmented
                options={SCHEDULE_TYPES.map((x) => ({ label: x.label, value: x.value }))}
                block
              />
            </Form.Item>
            {typeHint ? (
              <Typography.Text type="secondary" style={{ display: "block", marginBottom: 16 }}>
                {typeHint}
              </Typography.Text>
            ) : null}

            {scheduleType === "daily_at" ? (
              <Row gutter={16}>
                <Col xs={24} sm={12}>
                  <Form.Item
                    name="dailyAtTime"
                    label="Giờ chạy mỗi ngày"
                    rules={[{ required: true, message: "Chọn giờ" }]}
                  >
                    <TimePicker format="HH:mm" minuteStep={5} style={{ width: "100%" }} size="large" />
                  </Form.Item>
                </Col>
                <Col xs={24} sm={12}>
                  <Form.Item
                    name="timezoneId"
                    label="Múi giờ"
                    rules={[{ required: true, message: "Chọn múi giờ" }]}
                  >
                    <Select
                      showSearch
                      size="large"
                      placeholder="IANA timezone"
                      options={COMMON_TIMEZONES.map((z) => ({ value: z, label: z }))}
                      dropdownStyle={{ minWidth: 280 }}
                    />
                  </Form.Item>
                </Col>
              </Row>
            ) : null}

            {scheduleType === "once_at" ? (
              <Form.Item
                name="onceAtDate"
                label="Ngày và giờ chạy"
                rules={[{ required: true, message: "Chọn ngày giờ" }]}
              >
                <DatePicker
                  showTime
                  format="DD/MM/YYYY HH:mm"
                  style={{ width: "100%" }}
                  size="large"
                  disabledDate={(current) => current && current.isBefore(dayjs().startOf("day"))}
                />
              </Form.Item>
            ) : null}

            {scheduleType === "run_now" ? (
              <Typography.Paragraph type="secondary" style={{ marginBottom: 16 }}>
                Lưu form sẽ tạo lịch và hệ thống chạy ngay; schedule sẽ tự tắt sau khi dispatch.
              </Typography.Paragraph>
            ) : null}

            <Form.Item style={{ marginBottom: 0, marginTop: 8 }}>
              <Space wrap>
                <Button type="primary" htmlType="submit" size="large" loading={submitting}>
                  {editing ? "Lưu thay đổi" : "Tạo lịch"}
                </Button>
                <Button size="large" onClick={() => { setMode("list"); setEditing(null); form.resetFields(); }}>
                  Hủy
                </Button>
              </Space>
            </Form.Item>
          </Form>
        </Card>
      )}
    </div>
  );
}
