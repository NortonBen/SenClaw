import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, Button, Card, Form, Input, Space, Table, Tag, Typography } from "antd";
import { useMemo, useState } from "react";
import type { BashProcessListResponse } from "@/lib/api/client";
import { api } from "@/lib/api/client";

export function BashProcessesPage() {
  const qc = useQueryClient();
  const [command, setCommand] = useState("echo \"Flow Kit\"\ndate");
  const [cwd, setCwd] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const q = useQuery<BashProcessListResponse>({
    queryKey: ["bash-processes"],
    queryFn: () => api.listBashProcesses(),
    staleTime: 5_000,
    refetchInterval: (query) => {
      const running = query.state.data?.processes?.some(
        (p) => p.status === "running"
      );
      return running ? 1200 : false;
    },
  });

  const processes = q.data?.processes ?? [];
  const selected = useMemo(() => {
    if (!selectedId) return null;
    return processes.find((p) => p.id === selectedId) ?? null;
  }, [processes, selectedId]);

  const startM = useMutation({
    mutationFn: () =>
      api.startBashProcess({
        command: command.trimEnd(),
        cwd: cwd.trim() || undefined,
      }),
    onSuccess: (row) => {
      setErr(null);
      setSelectedId(row.id);
      void qc.invalidateQueries({ queryKey: ["bash-processes"] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const killM = useMutation({
    mutationFn: (id: string) => api.killBashProcess(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["bash-processes"] }),
    onError: (e: Error) => setErr(e.message),
  });

  return (
    <div className="layout layout-wide">
      <Space direction="vertical" size={16} style={{ width: "100%" }}>
        <Typography.Title level={3} style={{ margin: 0 }}>
          Tiến trình bash
        </Typography.Title>
        {err && <Alert type="error" message={err} showIcon />}
        <Card title="Lệnh mới">
          <Form layout="vertical">
            <Form.Item label="Script / lệnh">
              <Input.TextArea rows={8} className="mono" value={command} onChange={(e) => setCommand(e.target.value)} />
            </Form.Item>
            <Form.Item label="Thư mục làm việc (tuỳ chọn)">
              <Input value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="/path/to/repo" />
            </Form.Item>
            <Button type="primary" loading={startM.isPending} disabled={!command.trim()} onClick={() => startM.mutate()}>
              Chạy
            </Button>
          </Form>
        </Card>
        <Card
          title="Danh sách"
          extra={
            <Button onClick={() => void q.refetch()}>
              Làm mới
            </Button>
          }
        >
          <Table
            rowKey="id"
            loading={q.isLoading}
            dataSource={processes}
            onRow={(row) => ({
              onClick: () => setSelectedId((prev) => (prev === row.id ? null : row.id)),
            })}
            columns={[
              {
                title: "Trạng thái",
                dataIndex: "status",
                key: "status",
                render: (v: string, row) => (
                  <Space>
                    <Tag color={v === "running" ? "gold" : v === "killed" ? "red" : "green"}>{v}</Tag>
                    {row.exit_code != null ? <Typography.Text type="secondary">({row.exit_code})</Typography.Text> : null}
                  </Space>
                ),
              },
              { title: "PID", dataIndex: "pid", key: "pid" },
              {
                title: "Lệnh",
                dataIndex: "command",
                key: "command",
                render: (v: string, row) => (
                  <div>
                    <Typography.Text code>{v.slice(0, 200)}{v.length > 200 ? "…" : ""}</Typography.Text>
                    {row.cwd ? <div className="sub">cwd: {row.cwd}</div> : null}
                  </div>
                ),
              },
              {
                title: "Bắt đầu",
                dataIndex: "started_at",
                key: "started_at",
                render: (v: string) => new Date(v).toLocaleString(),
              },
              {
                title: "",
                key: "actions",
                render: (_: unknown, row) =>
                  row.status === "running" ? (
                    <Button
                      danger
                      loading={killM.isPending}
                      onClick={(e) => {
                        e.stopPropagation();
                        killM.mutate(row.id);
                      }}
                    >
                      Dừng
                    </Button>
                  ) : null,
              },
            ]}
          />
        </Card>
        {selected && (
          <Card title="Đầu ra">
            <Typography.Paragraph>
              <Typography.Text code>id: {selected.id}</Typography.Text>
            </Typography.Paragraph>
            <pre
              className="mono"
              style={{
                margin: 0,
                padding: 12,
                borderRadius: 8,
                border: "1px solid var(--border)",
                background: "var(--bg)",
                maxHeight: 360,
                overflow: "auto",
                fontSize: "0.8rem",
                whiteSpace: "pre-wrap",
              }}
            >
              {selected.output_tail || "—"}
            </pre>
          </Card>
        )}
      </Space>
    </div>
  );
}
