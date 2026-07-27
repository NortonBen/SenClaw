import { useEffect, useMemo, useState } from "react";
import { Card, Col, Empty, Row, Spin, Statistic, Table, Tag, Typography } from "antd";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Legend,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { api } from "../api";
import {
  pagedEntityQuery,
  type BrowserProfile,
  type DailyRunCount,
  type DashboardRunStats,
  type Flow,
  type FlowRun,
  type PagedList,
  type RunsListResponse,
  type TikTokAccount,
} from "../types/api";

const { Text } = Typography;

const STATUS_VI: Record<string, string> = {
  done: "Thành công",
  failed: "Thất bại",
  running: "Đang chạy",
  queued: "Hàng đợi",
};

const STATUS_COLOR: Record<string, string> = {
  done: "#52c41a",
  failed: "#ff4d4f",
  running: "#1677ff",
  queued: "#faad14",
};

const PIE_FALLBACK = "#94a3b8";

function shortDayLabel(isoDate: string): string {
  const parts = isoDate.split("-").map(Number);
  const y = parts[0];
  const m = parts[1];
  const d = parts[2];
  if (!y || !m || !d) return isoDate;
  return `${String(d).padStart(2, "0")}/${String(m).padStart(2, "0")}`;
}

function emptyWeekDays(): DailyRunCount[] {
  const out: DailyRunCount[] = [];
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  for (let i = 6; i >= 0; i -= 1) {
    const d = new Date(today);
    d.setDate(d.getDate() - i);
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    out.push({ date: `${y}-${m}-${day}`, done: 0, failed: 0, running: 0, queued: 0, total: 0 });
  }
  return out;
}

function normalizeRunStats(st: DashboardRunStats | null | undefined): DashboardRunStats {
  if (st && Array.isArray(st.last7Days) && st.last7Days.length === 7) {
    return {
      last7Days: st.last7Days,
      statusTotals7d: st.statusTotals7d ?? {},
      topFlows7d: st.topFlows7d ?? [],
    };
  }
  return {
    last7Days: emptyWeekDays(),
    statusTotals7d: st?.statusTotals7d ?? {},
    topFlows7d: st?.topFlows7d ?? [],
  };
}

export function DashboardPage() {
  const [accountTotal, setAccountTotal] = useState(0);
  const [profileTotal, setProfileTotal] = useState(0);
  const [flows, setFlows] = useState<Flow[]>([]);
  const [runs, setRuns] = useState<FlowRun[]>([]);
  const [runStats, setRunStats] = useState<DashboardRunStats | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setLoading(true);
      try {
        const [a, bp, f, r, st] = await Promise.all([
          api<PagedList<TikTokAccount>>(`/api/accounts?${pagedEntityQuery(1, 1)}`),
          api<PagedList<BrowserProfile>>(`/api/browser-profiles?${pagedEntityQuery(1, 1)}`),
          api<Flow[]>("/api/flows"),
          api<RunsListResponse>("/api/runs?page=1&pageSize=10"),
          api<DashboardRunStats>("/api/dashboard/run-stats").catch(() => null),
        ]);
        if (cancelled) return;
        setAccountTotal(typeof a?.total === "number" ? a.total : Array.isArray(a?.items) ? a.items.length : 0);
        setProfileTotal(typeof bp?.total === "number" ? bp.total : Array.isArray(bp?.items) ? bp.items.length : 0);
        setFlows(Array.isArray(f) ? f : []);
        setRuns(Array.isArray(r?.items) ? r.items : []);
        setRunStats(normalizeRunStats(st));
      } catch {
        if (!cancelled) {
          setAccountTotal(0);
          setProfileTotal(0);
          setFlows([]);
          setRuns([]);
          setRunStats(normalizeRunStats(null));
        }
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const flowName = (id: string) => flows.find((x) => x.id === id)?.name ?? id;

  const barData = useMemo(
    () =>
      (runStats?.last7Days ?? []).map((d) => ({
        label: shortDayLabel(d.date),
        date: d.date,
        "Thành công": d.done,
        "Thất bại": d.failed,
        "Đang chạy": d.running,
        "Hàng đợi": d.queued,
      })),
    [runStats?.last7Days]
  );

  const pieData = useMemo(() => {
    const raw = runStats?.statusTotals7d ?? {};
    return Object.entries(raw)
      .filter(([, v]) => v > 0)
      .map(([key, value]) => ({
        key,
        name: STATUS_VI[key] ?? key,
        value,
      }));
  }, [runStats?.statusTotals7d]);

  const topFlowBarData = useMemo(
    () =>
      (runStats?.topFlows7d ?? []).map((row) => ({
        name:
          flowName(row.flowId).length > 36
            ? `${flowName(row.flowId).slice(0, 34)}…`
            : flowName(row.flowId),
        count: row.count,
        flowId: row.flowId,
      })),
    [runStats?.topFlows7d, flows]
  );

  const hasAnyRunInWeek = (runStats?.last7Days ?? []).some((d) => d.total > 0);

  return (
    <div className="page dashboard-page">
      <Row gutter={[16, 16]}>
        <Col xs={24} sm={12} md={8}>
          <Card>
            <Statistic title="Accounts" value={accountTotal} loading={loading} />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={8}>
          <Card>
            <Statistic title="Profiles" value={profileTotal} loading={loading} />
          </Card>
        </Col>
        <Col xs={24} sm={12} md={8}>
          <Card>
            <Statistic title="Flows" value={flows.length} loading={loading} />
          </Card>
        </Col>
      </Row>

      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col xs={24} lg={16}>
          <Card
            title="Lượt chạy 7 ngày gần nhất"
            extra={<Text type="secondary">Xếp chồng theo trạng thái (theo ngày bắt đầu run)</Text>}
          >
            <Spin spinning={loading}>
              {!hasAnyRunInWeek && !loading ? (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có run trong 7 ngày" />
              ) : (
                <div className="dashboard-chart-wrap">
                  <ResponsiveContainer width="100%" height="100%">
                    <BarChart data={barData} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
                      <CartesianGrid strokeDasharray="3 3" vertical={false} />
                      <XAxis dataKey="label" tick={{ fontSize: 11 }} />
                      <YAxis allowDecimals={false} tick={{ fontSize: 11 }} width={36} />
                      <Tooltip
                        contentStyle={{ borderRadius: 8 }}
                        labelFormatter={(_, payload) => {
                          const p = payload?.[0]?.payload as { date?: string; label?: string } | undefined;
                          return p?.date ?? p?.label ?? "";
                        }}
                      />
                      <Legend />
                      <Bar dataKey="Thành công" stackId="a" fill={STATUS_COLOR.done} />
                      <Bar dataKey="Thất bại" stackId="a" fill={STATUS_COLOR.failed} />
                      <Bar dataKey="Đang chạy" stackId="a" fill={STATUS_COLOR.running} />
                      <Bar dataKey="Hàng đợi" stackId="a" fill={STATUS_COLOR.queued} />
                    </BarChart>
                  </ResponsiveContainer>
                </div>
              )}
            </Spin>
          </Card>
        </Col>
        <Col xs={24} lg={8}>
          <Card title="Trạng thái (7 ngày)" extra={<Text type="secondary">Tổng theo status</Text>}>
            <Spin spinning={loading}>
              {pieData.length === 0 && !loading ? (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Không có dữ liệu" />
              ) : (
                <div className="dashboard-chart-wrap">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie
                        data={pieData}
                        dataKey="value"
                        nameKey="name"
                        cx="50%"
                        cy="50%"
                        innerRadius={52}
                        outerRadius={88}
                        paddingAngle={2}
                        labelLine={false}
                        label={({ name, percent }) => `${name} ${((percent ?? 0) * 100).toFixed(0)}%`}
                      >
                        {pieData.map((entry) => (
                          <Cell key={entry.key} fill={STATUS_COLOR[entry.key] ?? PIE_FALLBACK} stroke="transparent" />
                        ))}
                      </Pie>
                      <Tooltip formatter={(v: number) => [v, "Lượt"]} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
              )}
            </Spin>
          </Card>
        </Col>
      </Row>

      <Card title="Flow chạy nhiều nhất (7 ngày)" style={{ marginTop: 16 }} extra={<Text type="secondary">Top 8 theo số run</Text>}>
        <Spin spinning={loading}>
          {topFlowBarData.length === 0 && !loading ? (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có run trong 7 ngày" />
          ) : (
            <div className="dashboard-chart-wrap dashboard-chart-wrap--tall">
              <ResponsiveContainer width="100%" height="100%">
                <BarChart layout="vertical" data={topFlowBarData} margin={{ top: 8, right: 24, left: 8, bottom: 8 }}>
                  <CartesianGrid strokeDasharray="3 3" horizontal />
                  <XAxis type="number" allowDecimals={false} tick={{ fontSize: 11 }} />
                  <YAxis type="category" dataKey="name" width={148} tick={{ fontSize: 11 }} />
                  <Tooltip
                    formatter={(v: number) => [v, "Lượt chạy"]}
                    labelFormatter={(_, payload) => {
                      const row = payload?.[0]?.payload as { flowId?: string } | undefined;
                      return row?.flowId ? `ID: ${row.flowId}` : "";
                    }}
                  />
                  <Bar dataKey="count" name="Lượt chạy" fill="#722ed1" radius={[0, 6, 6, 0]} maxBarSize={28} />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}
        </Spin>
      </Card>

      <Card title="Lịch sử chạy gần đây" style={{ marginTop: 16 }}>
        <Table
          size="small"
          rowKey="id"
          pagination={false}
          loading={loading}
          dataSource={runs}
          columns={[
            { title: "Run ID", dataIndex: "id", key: "id", ellipsis: true },
            { title: "Account", dataIndex: "accountId", key: "accountId", ellipsis: true },
            { title: "Flow", dataIndex: "flowId", key: "flowId", ellipsis: true, render: (fid: string) => flowName(fid) },
            {
              title: "Status",
              dataIndex: "status",
              key: "status",
              render: (s: string) => {
                const color =
                  s === "done" ? "green" : s === "failed" ? "red" : s === "running" ? "blue" : "default";
                return <Tag color={color}>{s}</Tag>;
              },
            },
          ]}
        />
      </Card>
    </div>
  );
}
