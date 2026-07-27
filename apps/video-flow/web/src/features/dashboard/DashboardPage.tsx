import {
  ApiOutlined,
  BranchesOutlined,
  FolderOutlined,
  PlayCircleOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import { Badge, Button, Card, Col, Row, Statistic, Tag, Typography } from "antd";
import { useNavigate } from "react-router-dom";
import { api, type ProjectRow } from "@/lib/api/client";

const { Title, Text } = Typography;

function HealthCard() {
  const q = useQuery({
    queryKey: ["health"],
    queryFn: () => api.health(),
    refetchInterval: 5_000,
  });

  const data = q.data as Record<string, unknown> | undefined;
  const extConnected = !!data?.extension_connected;
  const ok = q.isSuccess && extConnected;

  return (
    <Card
      style={{ marginBottom: 16 }}
      styles={{ body: { padding: "16px 20px" } }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
        <Badge status={ok ? "success" : q.isLoading ? "processing" : "error"} />
        <div>
          <Text strong style={{ fontSize: 15 }}>
            {q.isLoading
              ? "Đang kết nối…"
              : ok
              ? "Server & Extension đã kết nối"
              : "Extension chưa kết nối"}
          </Text>
          <br />
          <Text type="secondary" style={{ fontSize: 12 }}>
            {`Backend :4460 · Extension WebSocket :9222`}
          </Text>
        </div>
        {data && (
          <Tag color={extConnected ? "green" : "orange"} style={{ marginLeft: "auto" }}>
            {extConnected ? "ONLINE" : "OFFLINE"}
          </Tag>
        )}
      </div>
    </Card>
  );
}

function StatsRow() {
  const projectsQ = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
    staleTime: 15_000,
  });

  const pendingQ = useQuery({
    queryKey: ["pending-requests"],
    queryFn: () => api.listPendingRequests(),
    refetchInterval: 8_000,
  });

  const agentsQ = useQuery({
    queryKey: ["agents"],
    queryFn: () => api.listAgents(),
    staleTime: 60_000,
  });

  return (
    <Row gutter={[16, 16]} style={{ marginBottom: 24 }}>
      <Col xs={12} sm={6}>
        <Card styles={{ body: { padding: "16px 20px" } }}>
          <Statistic
            title="Projects"
            value={projectsQ.data?.length ?? "—"}
            prefix={<FolderOutlined />}
          />
        </Card>
      </Col>
      <Col xs={12} sm={6}>
        <Card styles={{ body: { padding: "16px 20px" } }}>
          <Statistic
            title="Hàng đợi"
            value={pendingQ.data?.length ?? "—"}
            prefix={<PlayCircleOutlined />}
            valueStyle={
              (pendingQ.data?.length ?? 0) > 0 ? { color: "#d97706" } : undefined
            }
          />
        </Card>
      </Col>
      <Col xs={12} sm={6}>
        <Card styles={{ body: { padding: "16px 20px" } }}>
          <Statistic
            title="Agents"
            value={agentsQ.data?.length ?? "—"}
            prefix={<BranchesOutlined />}
          />
        </Card>
      </Col>
      <Col xs={12} sm={6}>
        <Card styles={{ body: { padding: "16px 20px" } }}>
          <Statistic
            title="Backend"
            value="v2 Multi-Agent"
            prefix={<ApiOutlined />}
            valueStyle={{ fontSize: 14 }}
          />
        </Card>
      </Col>
    </Row>
  );
}

function QuickActions({ onOpenPipeline }: { onOpenPipeline?: () => void }) {
  const navigate = useNavigate();
  return (
    <Card title="Quick Actions" style={{ marginBottom: 24 }}>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
        <Button
          type="primary"
          icon={<ThunderboltOutlined />}
          onClick={() => navigate("/dag-pipeline")}
        >
          Smart Pipeline
        </Button>
        <Button icon={<PlayCircleOutlined />} onClick={onOpenPipeline}>
          Studio Pipeline
        </Button>
        <Button icon={<FolderOutlined />} onClick={() => navigate("/projects")}>
          Projects
        </Button>
        <Button onClick={() => navigate("/projects/create")}>
          + Tạo project
        </Button>
      </div>
    </Card>
  );
}

function RecentProjects({ onOpenPipeline }: { onOpenPipeline?: (id: string) => void }) {
  const navigate = useNavigate();
  const q = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
    staleTime: 15_000,
  });

  const projects = (q.data ?? []).slice(0, 6) as ProjectRow[];

  return (
    <Card title="Projects gần đây">
      {projects.length === 0 && !q.isLoading && (
        <Text type="secondary">Chưa có project nào. Tạo project mới để bắt đầu.</Text>
      )}
      <Row gutter={[12, 12]}>
        {projects.map((p) => {
          const pid = String(p.id ?? "");
          const name = String(p.name ?? "Untitled");
          const material = String(p.material ?? "");
          return (
            <Col xs={24} sm={12} md={8} key={pid}>
              <Card
                size="small"
                hoverable
                onClick={() => {
                  if (onOpenPipeline) onOpenPipeline(pid);
                  else navigate("/pipeline");
                }}
                styles={{ body: { padding: "10px 14px" } }}
              >
                <Text strong style={{ fontSize: 13 }}>
                  {name}
                </Text>
                <br />
                <Text type="secondary" style={{ fontSize: 11 }}>
                  {material || "no material"} · {pid.slice(0, 8)}
                </Text>
              </Card>
            </Col>
          );
        })}
      </Row>
    </Card>
  );
}

type Props = {
  onOpenPipeline?: (projectId?: string) => void;
};

export function DashboardPage({ onOpenPipeline }: Props) {
  return (
    <div style={{ maxWidth: 900, margin: "0 auto", padding: "24px 16px 48px" }}>
      <div style={{ marginBottom: 24 }}>
        <Title level={3} style={{ margin: 0 }}>
          Flow Agent Dashboard
        </Title>
        <Text type="secondary">Veo3 Multi-Agent Video Production</Text>
      </div>

      <HealthCard />
      <StatsRow />
      <QuickActions onOpenPipeline={() => onOpenPipeline?.()} />
      <RecentProjects onOpenPipeline={onOpenPipeline} />
    </div>
  );
}
