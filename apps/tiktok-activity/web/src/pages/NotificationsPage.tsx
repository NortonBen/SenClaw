import { useEffect, useMemo, useState } from "react";
import { Button, Card, Form, Input, Select, Space, Switch, Table, Typography, message } from "antd";
import { api } from "../api";
import { ENTITY_LIST_MAX_PAGE_SIZE, pagedEntityQuery, type Flow, type PagedList, type TikTokAccount } from "../types/api";
import type { NotificationRule } from "../types/notifications";

export function NotificationsPage() {
  const [rules, setRules] = useState<NotificationRule[]>([]);
  const [accounts, setAccounts] = useState<TikTokAccount[]>([]);
  const [flows, setFlows] = useState<Flow[]>([]);
  const [loading, setLoading] = useState(false);
  const [form] = Form.useForm<NotificationRule>();
  const [editing, setEditing] = useState<NotificationRule | null>(null);
  const [mode, setMode] = useState<"list" | "form">("list");
  const [q, setQ] = useState("");

  const refresh = async () => {
    const [r, a, f] = await Promise.all([
      api<NotificationRule[]>("/api/notification-rules"),
      api<PagedList<TikTokAccount>>(`/api/accounts?${pagedEntityQuery(1, ENTITY_LIST_MAX_PAGE_SIZE)}`),
      api<Flow[]>("/api/flows"),
    ]);
    setRules(r);
    setAccounts(Array.isArray(a?.items) ? a.items : []);
    setFlows(f);
  };

  useEffect(() => {
    void refresh();
  }, []);

  const onSave = async () => {
    try {
      setLoading(true);
      const v = await form.validateFields();
      await api<NotificationRule>("/api/notification-rules", "POST", {
        ...v,
        id: editing?.id ?? "",
      });
      setEditing(null);
      form.resetFields();
      await refresh();
      message.success("Đã lưu rule");
      setMode("list");
    } finally {
      setLoading(false);
    }
  };

  const onDelete = async (id: string) => {
    const res = await fetch(`/api/notification-rules/${encodeURIComponent(id)}`, { method: "DELETE" });
    if (!res.ok) message.error(await res.text());
    else {
      message.success("Đã xóa");
      await refresh();
    }
  };

  const startEdit = (r: NotificationRule) => {
    setEditing(r);
    form.setFieldsValue(r);
    setMode("form");
  };

  const filtered = useMemo(() => {
    const s = q.trim().toLowerCase();
    if (!s) return rules;
    return rules.filter((r) =>
      [r.name, r.event, r.flowId ?? "", r.accountId ?? ""].some((v) => v.toLowerCase().includes(s))
    );
  }, [rules, q]);

  return (
    <div className="page">
      {mode === "list" ? (
        <Card
          title={
            <div>
              <div style={{ fontWeight: 700 }}>Notification Rules</div>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Configure in-app notification conditions
              </Typography.Text>
            </div>
          }
          extra={
            <Space>
              <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search" style={{ width: 220 }} allowClear />
              <Button disabled>Add Filter</Button>
              <Button type="primary" onClick={() => { setEditing(null); form.resetFields(); setMode("form"); }}>
                + Add Rule
              </Button>
            </Space>
          }
        >
          <Table
            rowKey="id"
            pagination={false}
            dataSource={filtered}
            columns={[
              { title: "Name", dataIndex: "name" },
              { title: "Event", dataIndex: "event" },
              { title: "Enabled", dataIndex: "enabled", render: (v: boolean) => (v ? "yes" : "no") },
              { title: "Flow", dataIndex: "flowId" },
              { title: "Account", dataIndex: "accountId" },
              {
                title: "Actions",
                render: (_, r) => (
                  <Space>
                    <Button type="link" onClick={() => startEdit(r)}>
                      Edit
                    </Button>
                    <Button type="link" danger onClick={() => void onDelete(r.id)}>
                      Delete
                    </Button>
                  </Space>
                ),
              },
            ]}
            locale={{ emptyText: "No data" }}
          />
        </Card>
      ) : (
        <Card
          title={editing ? "Edit Notification Rule" : "Add Notification Rule"}
          extra={<Button onClick={() => { setMode("list"); setEditing(null); form.resetFields(); }}>Back</Button>}
        >
          <Form form={form} layout="vertical" initialValues={{ enabled: true, event: "run_failed" }}>
            <Form.Item name="name" label="Tên rule" rules={[{ required: true }]}>
              <Input placeholder="Run failed alert" />
            </Form.Item>
            <Form.Item name="enabled" label="Enabled" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item name="event" label="Event" rules={[{ required: true }]}>
              <Select
                options={[
                  { value: "run_failed", label: "run_failed" },
                  { value: "run_done", label: "run_done" },
                  { value: "flow_action", label: "flow_action" },
                ]}
              />
            </Form.Item>
            <Form.Item name="flowId" label="Flow filter (optional)">
              <Select allowClear options={flows.map((f) => ({ value: f.id, label: f.name }))} />
            </Form.Item>
            <Form.Item name="accountId" label="Account filter (optional)">
              <Select allowClear options={accounts.map((a) => ({ value: a.id, label: a.username }))} />
            </Form.Item>
            <Form.Item name="messageTemplate" label="Message template (optional)">
              <Input.TextArea rows={3} placeholder="Nếu để trống sẽ dùng message mặc định." />
            </Form.Item>
            <div className="row row-wrap">
              <Button type="primary" onClick={() => void onSave()} loading={loading}>
                Lưu
              </Button>
            </div>
          </Form>
        </Card>
      )}
    </div>
  );
}

