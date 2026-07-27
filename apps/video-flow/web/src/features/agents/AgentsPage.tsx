import {
  AudioOutlined,
  BranchesOutlined,
  EyeOutlined,
  FileTextOutlined,
  RobotOutlined,
  ScissorOutlined,
  SearchOutlined,
  UserOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Input,
  Modal,
  Space,
  Spin,
  Tag,
  Typography,
  message,
} from "antd";
import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, type AgentInfo } from "@/lib/api/client";

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

const AGENT_ICON: Record<string, React.ReactNode> = {
  orchestrator: <BranchesOutlined />,
  script_parser: <FileTextOutlined />,
  character: <UserOutlined />,
  image: <RobotOutlined />,
  video: <VideoCameraOutlined />,
  audio: <AudioOutlined />,
  concat: <ScissorOutlined />,
};

// ---- AgentDetailDialog ----

function AgentDetailDialog({
  agent,
  onClose,
}: {
  agent: AgentInfo | null;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const saveMut = useMutation({
    mutationFn: () => api.putAgentSoul(agent!.type, draft),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["agents"] });
      setEditing(false);
      void message.success("Đã lưu soul");
    },
    onError: (e: Error) => void message.error(e.message),
  });

  if (!agent) return null;

  const icon = AGENT_ICON[agent.type] ?? <RobotOutlined />;
  const isBuiltin = agent.kind === "built-in";

  return (
    <Modal
      open={!!agent}
      title={
        <Space>
          <Text style={{ fontSize: 16 }}>{icon}</Text>
          <span>{agent.type}</span>
          <Tag color={isBuiltin ? "purple" : "blue"} style={{ fontSize: 11 }}>
            {isBuiltin ? `built-in${agent.name ? ` · ${agent.name}` : ""}` : "skill"}
          </Tag>
        </Space>
      }
      onCancel={() => {
        setEditing(false);
        onClose();
      }}
      width={780}
      footer={
        editing ? (
          <Space>
            <Button onClick={() => setEditing(false)}>Hủy</Button>
            <Button
              type="primary"
              loading={saveMut.isPending}
              onClick={() => saveMut.mutate()}
            >
              Lưu soul
            </Button>
          </Space>
        ) : (
          <Button
            onClick={() => {
              setDraft(agent.prompt ?? "");
              setEditing(true);
            }}
          >
            Sửa soul
          </Button>
        )
      }
    >
      {agent.description && (
        <Paragraph type="secondary" style={{ marginBottom: 16 }}>
          {agent.description}
        </Paragraph>
      )}
      {agent.kind === "built-in" && agent.soul_file && (
        <Paragraph type="secondary" style={{ marginBottom: 12, fontFamily: "var(--mono)", fontSize: 12 }}>
          File: <Text code>souls/{agent.soul_file}</Text>
        </Paragraph>
      )}

      {editing ? (
        <TextArea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={20}
          style={{ fontFamily: "var(--mono)", fontSize: 12 }}
        />
      ) : (
        <div
          style={{
            background: "var(--bg)",
            border: "1px solid var(--border)",
            borderRadius: 8,
            padding: "16px 20px",
            maxHeight: "60vh",
            overflowY: "auto",
          }}
        >
          {agent.prompt ? (
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {agent.prompt}
            </ReactMarkdown>
          ) : (
            <Text type="secondary" style={{ fontStyle: "italic" }}>
              Chưa có soul. Nhấn "Sửa soul" để thêm.
            </Text>
          )}
        </div>
      )}
    </Modal>
  );
}

// ---- AgentRow ----

function AgentRow({
  agent,
  onView,
}: {
  agent: AgentInfo;
  onView: () => void;
}) {
  const icon = AGENT_ICON[agent.type] ?? <RobotOutlined />;
  const isBuiltin = agent.kind === "built-in";

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "10px 14px",
        borderBottom: "1px solid var(--border)",
        flexWrap: "wrap",
      }}
    >
      {/* Icon + name */}
      <Space size={6} style={{ minWidth: 160, flex: "1 1 160px" }}>
        <Text style={{ fontSize: 15, color: "var(--muted)" }}>{icon}</Text>
        <Text strong style={{ fontSize: 13 }}>
          {agent.type}
        </Text>
      </Space>

      {/* Description */}
      <Text
        type="secondary"
        style={{
          fontSize: 12,
          flex: "2 1 200px",
          overflow: "hidden",
          textOverflow: "ellipsis",
          whiteSpace: "nowrap",
        }}
      >
        {agent.description || agent.soul_summary || "—"}
      </Text>

      {/* Tags */}
      <Space size={4} style={{ flexShrink: 0 }}>
        {agent.kind === "built-in" && agent.soul_file ? (
          <Tag style={{ fontSize: 10, margin: 0 }} color="default">
            {agent.soul_file}
          </Tag>
        ) : (
          <Tag style={{ fontSize: 10, margin: 0 }} color="default">
            {agent.type}
          </Tag>
        )}
        <Tag
          style={{ fontSize: 10, margin: 0 }}
          color={isBuiltin ? "purple" : "blue"}
        >
          {isBuiltin ? `built-in${agent.name ? ` · ${agent.name}` : ""}` : "skill"}
        </Tag>
        <Tag style={{ fontSize: 10, margin: 0 }} color="green">
          enabled
        </Tag>
      </Space>

      {/* Actions */}
      <Button
        size="small"
        icon={<EyeOutlined />}
        onClick={onView}
        style={{ flexShrink: 0 }}
      >
        Chi tiết
      </Button>
    </div>
  );
}

// ---- AgentsPage ----

export function AgentsPage() {
  const [filter, setFilter] = useState("");
  const [viewing, setViewing] = useState<AgentInfo | null>(null);

  const agentsQ = useQuery({
    queryKey: ["agents"],
    queryFn: () => api.listAgents(),
    staleTime: 60_000,
  });

  const agents = agentsQ.data ?? [];

  const filtered = useMemo(() => {
    if (!filter.trim()) return agents;
    const q = filter.toLowerCase();
    return agents.filter(
      (a) =>
        a.type.toLowerCase().includes(q) ||
        (a.description ?? "").toLowerCase().includes(q)
    );
  }, [agents, filter]);

  const builtins = filtered.filter((a) => a.kind === "built-in");
  const skills = filtered.filter((a) => a.kind !== "built-in");

  return (
    <div style={{ maxWidth: 960, margin: "0 auto", padding: "24px 16px 48px" }}>
      {/* Header */}
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          flexWrap: "wrap",
          gap: 12,
          marginBottom: 20,
        }}
      >
        <div>
          <Title level={3} style={{ margin: 0 }}>
            Quản lý Agents
          </Title>
          <Text type="secondary">
            {agents.length} agent đã đăng ký
          </Text>
        </div>
        <Input
          prefix={<SearchOutlined />}
          placeholder="Tìm agent…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{ width: 220 }}
          allowClear
        />
      </div>

      {agentsQ.isLoading && (
        <div style={{ textAlign: "center", padding: 60 }}>
          <Spin size="large" />
        </div>
      )}

      {/* Built-in agents */}
      {builtins.length > 0 && (
        <div
          style={{
            border: "1px solid var(--border)",
            borderRadius: 8,
            overflow: "hidden",
            marginBottom: 20,
          }}
        >
          <div
            style={{
              padding: "8px 14px",
              background: "var(--surface)",
              borderBottom: "1px solid var(--border)",
            }}
          >
            <Text strong style={{ fontSize: 12, color: "var(--muted)" }}>
              BUILT-IN ({builtins.length})
            </Text>
          </div>
          {builtins.map((a) => (
            <AgentRow key={a.type} agent={a} onView={() => setViewing(a)} />
          ))}
        </div>
      )}

      {/* Skill agents */}
      {skills.length > 0 && (
        <div
          style={{
            border: "1px solid var(--border)",
            borderRadius: 8,
            overflow: "hidden",
          }}
        >
          <div
            style={{
              padding: "8px 14px",
              background: "var(--surface)",
              borderBottom: "1px solid var(--border)",
            }}
          >
            <Text strong style={{ fontSize: 12, color: "var(--muted)" }}>
              SKILL AGENTS ({skills.length})
            </Text>
          </div>
          {skills.map((a) => (
            <AgentRow key={a.type} agent={a} onView={() => setViewing(a)} />
          ))}
        </div>
      )}

      {!agentsQ.isLoading && filtered.length === 0 && (
        <Text type="secondary">
          {filter ? `Không tìm thấy agent nào cho "${filter}"` : "Chưa có agent nào."}
        </Text>
      )}

      <AgentDetailDialog agent={viewing} onClose={() => setViewing(null)} />
    </div>
  );
}
