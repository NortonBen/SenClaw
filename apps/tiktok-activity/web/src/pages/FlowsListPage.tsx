import { useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, Input, Modal, Select, Space, Table, Typography, message } from "antd";
import { Link } from "react-router-dom";
import { CopyOutlined, DragOutlined, DownloadOutlined, UploadOutlined, UnorderedListOutlined } from "@ant-design/icons";
import { api } from "../api";
import {
  ENTITY_LIST_MAX_PAGE_SIZE,
  pagedEntityQuery,
  type Flow,
  type FlowRun,
  type PagedList,
  type RunsListResponse,
  type TikTokAccount,
} from "../types/api";
import {
  downloadFlowJson,
  ensureStartStep,
  flowForNewDatabaseRow,
  parseFlowImportJSON,
} from "./flows/flowIo";

export function FlowsListPage() {
  const [flows, setFlows] = useState<Flow[]>([]);
  const [accounts, setAccounts] = useState<TikTokAccount[]>([]);
  const [runs, setRuns] = useState<FlowRun[]>([]);
  const [runAccountId, setRunAccountId] = useState("");
  const [runFlowId, setRunFlowId] = useState("");
  const [runParamRows, setRunParamRows] = useState<Array<{ key: string; value: string }>>([]);
  const [q, setQ] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [importBusy, setImportBusy] = useState(false);
  const fileImportRef = useRef<HTMLInputElement | null>(null);

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
      const [fRaw, aRaw, rRaw] = await Promise.all([
        api<Flow[] | null>("/api/flows"),
        api<PagedList<TikTokAccount> | null>(`/api/accounts?${pagedEntityQuery(1, ENTITY_LIST_MAX_PAGE_SIZE)}`),
        api<RunsListResponse | null>("/api/runs?page=1&pageSize=10"),
      ]);
      const f = asArray(fRaw);
      const a = asArray(aRaw?.items);
      const r = asArray(rRaw?.items);
      setFlows(f);
      setAccounts(a);
      setRuns(r);
      if (!runFlowId && f.length) setRunFlowId(f[0]!.id);
      if (!runAccountId && a.length) setRunAccountId(a[0]!.id);
    } catch (err) {
      setError(String(err));
      setFlows([]);
      setAccounts([]);
      setRuns([]);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    const selected = flows.find((f) => f.id === runFlowId);
    if (!selected) return;
    setRunParamRows(mapToRows(selected.params));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runFlowId, flows]);

  const filteredFlows = useMemo(() => {
    const s = q.trim().toLowerCase();
    if (!s) return flows;
    return flows.filter((f) => f.name.toLowerCase().includes(s) || f.id.toLowerCase().includes(s));
  }, [flows, q]);

  const startRun = async () => {
    try {
      setError(null);
      const params = rowsToMap(runParamRows);
      await api<FlowRun>("/api/runs/start", "POST", { accountId: runAccountId, flowId: runFlowId, params });
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const exportFlow = (f: Flow) => {
    downloadFlowJson(f, f.name);
    message.success("Đã tải file JSON");
  };

  const duplicateFlow = async (f: Flow) => {
    try {
      setError(null);
      const body = flowForNewDatabaseRow({ name: `${f.name} (bản sao)`, actions: f.actions ?? [] });
      body.actions = ensureStartStep(body.actions);
      await api<Flow>("/api/flows", "POST", body);
      message.success("Đã tạo bản sao flow");
      await refresh();
    } catch (err) {
      setError(String(err));
    }
  };

  const submitImport = async () => {
    try {
      setImportBusy(true);
      setError(null);
      const parsed = parseFlowImportJSON(importText);
      const body = flowForNewDatabaseRow(parsed);
      body.actions = ensureStartStep(body.actions);
      await api<Flow>("/api/flows", "POST", body);
      message.success("Đã import flow mới");
      setImportOpen(false);
      setImportText("");
      await refresh();
    } catch (err) {
      setError(String(err));
      message.error(String(err));
    } finally {
      setImportBusy(false);
    }
  };

  return (
    <div className="page" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      {error && <pre className="error">{error}</pre>}

      <Card
        title={
          <div>
            <div style={{ fontWeight: 700 }}>Flows</div>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              Quản lý flow và chạy nhanh
            </Typography.Text>
          </div>
        }
        extra={
          <Space wrap>
            <Link to="/flows/actions">
              <Button icon={<UnorderedListOutlined />}>Danh sách actions</Button>
            </Link>
            <Link to="/flows/actions/build">
              <Button icon={<DragOutlined />}>Tạo action (atomic)</Button>
            </Link>
            <Input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Tìm kiếm" allowClear style={{ width: 220 }} />
            <Button icon={<UploadOutlined />} onClick={() => setImportOpen(true)}>
              Import JSON
            </Button>
            <Link to="/flows/new">
              <Button type="primary">+ Thêm flow</Button>
            </Link>
          </Space>
        }
      >
        <Table
          rowKey="id"
          dataSource={filteredFlows}
          pagination={false}
          columns={[
            { title: "Tên", dataIndex: "name" },
            { title: "Steps", render: (_, r) => r.actions?.length ?? 0 },
            { title: "Flow ID", dataIndex: "id" },
            {
              title: "Thao tác",
              render: (_, r) => (
                <Space wrap size="small">
                  <Link to={`/flows/${encodeURIComponent(r.id)}/view`}>
                    <Button type="link" size="small">
                      Xem
                    </Button>
                  </Link>
                  <Link to={`/flows/${encodeURIComponent(r.id)}/edit`}>
                    <Button type="link" size="small">
                      Sửa
                    </Button>
                  </Link>
                  <Button type="link" size="small" icon={<DownloadOutlined />} onClick={() => exportFlow(r)}>
                    Export
                  </Button>
                  <Button type="link" size="small" icon={<CopyOutlined />} onClick={() => void duplicateFlow(r)}>
                    Nhân bản
                  </Button>
                </Space>
              ),
            },
          ]}
          locale={{ emptyText: "Chưa có flow" }}
        />

        <Card size="small" title="Chạy nhanh" style={{ marginTop: 12 }}>
          <Space wrap align="start">
            <Select
              value={runAccountId}
              onChange={setRunAccountId}
              style={{ width: 220 }}
              options={accounts.map((a) => ({ value: a.id, label: a.username }))}
            />
            <Select
              value={runFlowId}
              onChange={setRunFlowId}
              style={{ width: 220 }}
              options={flows.map((f) => ({ value: f.id, label: f.name }))}
            />
            <Button type="primary" onClick={() => void startRun()}>
              Start
            </Button>
            <div style={{ border: "1px solid var(--flow-panel-border)", borderRadius: 8, padding: 10, minWidth: 360 }}>
              <Typography.Text strong style={{ display: "block", marginBottom: 8 }}>
                Params khi chạy (key-value)
              </Typography.Text>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Flow cần params:{" "}
                {flows
                  .find((f) => f.id === runFlowId)
                  ?.params &&
                Object.keys(flows.find((f) => f.id === runFlowId)?.params ?? {}).length > 0
                  ? Object.keys(flows.find((f) => f.id === runFlowId)?.params ?? {}).join(", ")
                  : "(không có)"}
              </Typography.Text>
              <Space direction="vertical" style={{ width: "100%", marginTop: 8 }}>
                {runParamRows.map((row, idx) => (
                  <Space key={`runp-${idx}`} wrap>
                    <Input
                      placeholder="key"
                      value={row.key}
                      onChange={(e) =>
                        setRunParamRows((prev) => prev.map((x, i) => (i === idx ? { ...x, key: e.target.value } : x)))
                      }
                      style={{ width: 140 }}
                    />
                    <Input
                      placeholder="value"
                      value={row.value}
                      onChange={(e) =>
                        setRunParamRows((prev) => prev.map((x, i) => (i === idx ? { ...x, value: e.target.value } : x)))
                      }
                      style={{ width: 180 }}
                    />
                    <Button danger onClick={() => setRunParamRows((prev) => prev.filter((_, i) => i !== idx))}>
                      Xóa
                    </Button>
                  </Space>
                ))}
                <Button onClick={() => setRunParamRows((prev) => [...prev, { key: "", value: "" }])}>+ Thêm param</Button>
              </Space>
            </div>
          </Space>
        </Card>

        <Card size="small" title="Run gần đây" style={{ marginTop: 12 }}>
          <Table
            rowKey="id"
            pagination={false}
            dataSource={runs}
            columns={[
              { title: "Run ID", dataIndex: "id" },
              { title: "Trạng thái", dataIndex: "status" },
              { title: "Account", dataIndex: "accountId" },
            ]}
          />
        </Card>
      </Card>

      <input
        ref={fileImportRef}
        type="file"
        accept="application/json,.json"
        style={{ display: "none" }}
        onChange={(e) => {
          const f = e.target.files?.[0];
          if (!f) return;
          void f.text().then((t) => {
            setImportText(t);
            message.info("Đã đọc file — kiểm tra JSON rồi nhấn Import");
          });
          e.target.value = "";
        }}
      />

      <Modal
        title="Import flow từ JSON"
        open={importOpen}
        onCancel={() => {
          setImportOpen(false);
          setImportText("");
        }}
        onOk={() => void submitImport()}
        okText="Tạo flow mới"
        confirmLoading={importBusy}
        width={640}
        destroyOnClose
      >
        <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
          Dán nội dung JSON (có <code>name</code> và <code>actions</code>) hoặc chọn file. Luôn tạo <b>flow mới</b> (id step được
          map lại để không trùng nhánh).
        </Typography.Paragraph>
        <Space style={{ marginBottom: 8 }}>
          <Button icon={<UploadOutlined />} onClick={() => fileImportRef.current?.click()}>
            Chọn file .json
          </Button>
        </Space>
        <Input.TextArea rows={14} value={importText} onChange={(e) => setImportText(e.target.value)} placeholder='{"name":"...","actions":[...]}' />
      </Modal>
    </div>
  );
}
