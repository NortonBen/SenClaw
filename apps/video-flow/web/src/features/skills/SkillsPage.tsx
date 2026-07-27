import { BookOutlined, SearchOutlined } from "@ant-design/icons";
import { useQuery } from "@tanstack/react-query";
import {
  Badge,
  Card,
  Col,
  Input,
  Modal,
  Row,
  Space,
  Spin,
  Tag,
  Typography,
} from "antd";
import { useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, type SkillEntry } from "@/lib/api/client";

const { Title, Text, Paragraph } = Typography;

function SkillCard({ skill, onClick }: { skill: SkillEntry; onClick: () => void }) {
  const isNew =
    skill.id.includes("pipeline") ||
    skill.id.includes("parse-script") ||
    skill.id.includes("agent-status");

  return (
    <Card
      hoverable
      size="small"
      onClick={onClick}
      style={{ height: "100%", cursor: "pointer" }}
      styles={{ body: { padding: "12px 14px" } }}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        <BookOutlined style={{ color: "var(--accent)", marginTop: 2, flexShrink: 0 }} />
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
            <Text strong style={{ fontSize: 13 }}>
              {skill.name || skill.id}
            </Text>
            {isNew && (
              <Badge
                count="NEW"
                style={{ backgroundColor: "#5b8def", fontSize: 10, padding: "0 5px" }}
              />
            )}
          </div>
          <Text
            type="secondary"
            style={{ fontSize: 11, display: "block", marginTop: 2 }}
            ellipsis={{ tooltip: skill.description }}
          >
            {skill.description || skill.id}
          </Text>
          <Tag style={{ marginTop: 6, fontSize: 10 }}>/{skill.id}</Tag>
        </div>
      </div>
    </Card>
  );
}

function SkillModal({
  skill,
  onClose,
}: {
  skill: SkillEntry | null;
  onClose: () => void;
}) {
  if (!skill) return null;
  return (
    <Modal
      open={!!skill}
      title={
        <Space>
          <BookOutlined />
          <span>{skill.name || skill.id}</span>
          <Tag style={{ fontSize: 11 }}>/{skill.id}</Tag>
        </Space>
      }
      onCancel={onClose}
      footer={null}
      width={760}
    >
      {skill.description && (
        <Paragraph type="secondary" style={{ marginBottom: 16 }}>
          {skill.description}
        </Paragraph>
      )}
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
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{skill.body}</ReactMarkdown>
      </div>
    </Modal>
  );
}


export function SkillsPage({ embedded = false }: { embedded?: boolean }) {
  const [filter, setFilter] = useState("");
  const [viewing, setViewing] = useState<SkillEntry | null>(null);

  const skillsQ = useQuery({
    queryKey: ["skill-catalog"],
    queryFn: () => api.listSkillCatalog(),
    staleTime: 60_000,
  });

  const skills = skillsQ.data ?? [];

  const filtered = useMemo(() => {
    if (!filter.trim()) return skills;
    const q = filter.toLowerCase();
    return skills.filter(
      (s) =>
        s.id.toLowerCase().includes(q) ||
        s.name.toLowerCase().includes(q) ||
        s.description.toLowerCase().includes(q)
    );
  }, [skills, filter]);

  const newSkills = filtered.filter(
    (s) =>
      s.id.includes("pipeline") ||
      s.id.includes("parse-script") ||
      s.id.includes("agent-status")
  );
  const otherSkills = filtered.filter(
    (s) =>
      !s.id.includes("pipeline") &&
      !s.id.includes("parse-script") &&
      !s.id.includes("agent-status")
  );

  return (
    <div style={{ maxWidth: 1000, margin: "0 auto", padding: embedded ? "8px 0 32px" : "24px 16px 48px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          flexWrap: "wrap",
          gap: 12,
          marginBottom: 16,
        }}
      >
        {!embedded && (
          <div>
            <Title level={3} style={{ margin: 0 }}>
              Skills
            </Title>
            <Text type="secondary">
              {skills.length} skill từ{" "}
              <code style={{ fontSize: 12 }}>skills/</code>
            </Text>
          </div>
        )}
        {embedded && (
          <Text type="secondary">
            {skills.length} skill từ <code style={{ fontSize: 12 }}>skills/</code>
          </Text>
        )}
        <Input
          prefix={<SearchOutlined />}
          placeholder="Tìm skill…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          style={{ width: 220 }}
          allowClear
        />
      </div>

      {skillsQ.isLoading && (
        <div style={{ textAlign: "center", padding: 60 }}>
          <Spin size="large" />
        </div>
      )}

      {!skillsQ.isLoading && filtered.length === 0 && (
        <Card>
          <Text type="secondary">
            {filter ? `Không tìm thấy skill nào cho "${filter}"` : "Chưa có skill nào."}
          </Text>
        </Card>
      )}

      {newSkills.length > 0 && (
        <>
          <div style={{ marginBottom: 10 }}>
            <Text strong style={{ fontSize: 13, color: "var(--accent)" }}>
              Multi-Agent Skills (mới)
            </Text>
          </div>
          <Row gutter={[12, 12]} style={{ marginBottom: 24 }}>
            {newSkills.map((s) => (
              <Col key={s.id} xs={24} sm={12} md={8}>
                <SkillCard skill={s} onClick={() => setViewing(s)} />
              </Col>
            ))}
          </Row>
        </>
      )}

      {otherSkills.length > 0 && (
        <>
          <div style={{ marginBottom: 10 }}>
            <Text strong style={{ fontSize: 13, color: "var(--muted)" }}>
              Tất cả skills ({otherSkills.length})
            </Text>
          </div>
          <Row gutter={[12, 12]}>
            {otherSkills.map((s) => (
              <Col key={s.id} xs={24} sm={12} md={8} lg={6}>
                <SkillCard skill={s} onClick={() => setViewing(s)} />
              </Col>
            ))}
          </Row>
        </>
      )}

      <SkillModal skill={viewing} onClose={() => setViewing(null)} />
    </div>
  );
}
