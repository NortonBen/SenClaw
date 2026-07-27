import { useCallback, useEffect, useState } from "react";
import { Button, Card, Drawer, Input, Space, Spin, Table, Typography } from "antd";
import { ReloadOutlined } from "@ant-design/icons";
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

const SEARCH_DEBOUNCE_MS = 400;

export function HistoryPage() {
  const [runs, setRuns] = useState<FlowRun[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(10);
  const [loading, setLoading] = useState(false);
  const [searchInput, setSearchInput] = useState("");
  const [searchQ, setSearchQ] = useState("");

  const [accounts, setAccounts] = useState<TikTokAccount[]>([]);
  const [flows, setFlows] = useState<Flow[]>([]);
  const [expandedRunId, setExpandedRunId] = useState<string | null>(null);
  const [runDetail, setRunDetail] = useState<FlowRun | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const refreshMeta = useCallback(async () => {
    const [a, f] = await Promise.all([
      api<PagedList<TikTokAccount>>(`/api/accounts?${pagedEntityQuery(1, ENTITY_LIST_MAX_PAGE_SIZE)}`),
      api<Flow[]>("/api/flows"),
    ]);
    setAccounts(Array.isArray(a?.items) ? a.items : []);
    setFlows(Array.isArray(f) ? f : []);
  }, []);

  useEffect(() => {
    void refreshMeta();
  }, [refreshMeta]);

  useEffect(() => {
    const t = window.setTimeout(() => setSearchQ(searchInput.trim()), SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(t);
  }, [searchInput]);

  useEffect(() => {
    setPage(1);
  }, [searchQ]);

  const loadRuns = useCallback(async () => {
    setLoading(true);
    try {
      const params = new URLSearchParams({
        page: String(page),
        pageSize: String(pageSize),
      });
      if (searchQ) params.set("q", searchQ);
      const res = await api<RunsListResponse>(`/api/runs?${params.toString()}`);
      setRuns(Array.isArray(res.items) ? res.items : []);
      setTotal(typeof res.total === "number" ? res.total : 0);
    } catch {
      setRuns([]);
      setTotal(0);
    } finally {
      setLoading(false);
    }
  }, [page, pageSize, searchQ]);

  useEffect(() => {
    void loadRuns();
  }, [loadRuns]);

  useEffect(() => {
    if (!expandedRunId) {
      setRunDetail(null);
      setDetailLoading(false);
      return;
    }
    setRunDetail(null);
    setDetailLoading(true);
    let cancelled = false;
    const fetchRun = async () => {
      try {
        const r = await api<FlowRun>(`/api/runs/${encodeURIComponent(expandedRunId)}`);
        if (!cancelled) {
          setRunDetail(r);
          setDetailLoading(false);
        }
      } catch {
        if (!cancelled) setDetailLoading(false);
      }
    };
    void fetchRun();
    const t = window.setInterval(() => {
      void fetchRun();
    }, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, [expandedRunId]);

  const accountName = (id: string) => accounts.find((a) => a.id === id)?.username ?? id;
  const flowName = (id: string) => flows.find((f) => f.id === id)?.name ?? id;

  return (
    <div className="page">
      <Card
        title={
          <div>
            <div style={{ fontWeight: 700 }}>Run History</div>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              Theo dõi trạng thái và log — lọc & phân trang từ server
            </Typography.Text>
          </div>
        }
        extra={
          <Space>
            <Input
              value={searchInput}
              onChange={(e) => setSearchInput(e.target.value)}
              placeholder="Lọc theo run id, status, account id, flow id…"
              allowClear
              style={{ width: 280 }}
            />
            <Button icon={<ReloadOutlined />} onClick={() => void loadRuns()} disabled={loading}>
              Làm mới
            </Button>
            <Button disabled>Add Filter</Button>
          </Space>
        }
      >
        <Table
          rowKey="id"
          dataSource={runs}
          loading={loading}
          pagination={{
            current: page,
            pageSize,
            total,
            showSizeChanger: true,
            pageSizeOptions: [10, 20, 50, 100],
            showTotal: (t) => `${t} lượt chạy`,
            onChange: (p, ps) => {
              setPage(p);
              setPageSize(ps);
            },
          }}
          columns={[
            { title: "Status", dataIndex: "status" },
            { title: "Account", render: (_, r) => accountName(r.accountId) },
            { title: "Flow", render: (_, r) => flowName(r.flowId) },
            { title: "Run ID", dataIndex: "id" },
            {
              title: "Actions",
              render: (_, r) => (
                <Button type="link" onClick={() => setExpandedRunId(r.id)}>
                  View Logs
                </Button>
              ),
            },
          ]}
          locale={{ emptyText: loading ? "Đang tải…" : "Không có dữ liệu" }}
        />
      </Card>
      <Drawer title="Run Logs" open={!!expandedRunId} onClose={() => setExpandedRunId(null)} width={720}>
        {detailLoading && (!runDetail || runDetail.id !== expandedRunId) ? (
          <div style={{ display: "flex", justifyContent: "center", padding: 48 }}>
            <Spin />
          </div>
        ) : runDetail?.id === expandedRunId ? (
          <pre className="run-logs">{(runDetail.logs ?? []).join("\n") || "…"}</pre>
        ) : (
          <Typography.Text type="secondary">Không tải được log.</Typography.Text>
        )}
      </Drawer>
    </div>
  );
}
