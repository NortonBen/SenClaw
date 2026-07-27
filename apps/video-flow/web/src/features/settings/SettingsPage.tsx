import { EyeOutlined, PlusOutlined, RobotOutlined } from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, Button, Card, Form, Input, Modal, Popconfirm, Select, Space, Switch, Tabs, Tag, Typography } from "antd";
import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { api, type AgentInfo } from "@/lib/api/client";
import { SkillsPage } from "@/features/skills/SkillsPage";

const MATERIAL_PRESETS = [
  {
    id: "watercolor_soft",
    name: "Watercolor Soft",
    style_instruction:
      "Soft watercolor painting on textured paper, loose wet brushwork, translucent color washes, delicate ink outlines, dreamy artistic mood.",
    negative_prompt: "NOT photorealistic, NOT 3D render, NOT anime, NOT hard-edged vector style.",
    scene_prefix: "Watercolor style, soft translucent washes, gentle natural light.",
    lighting: "Soft diffused daylight",
  },
  {
    id: "cyberpunk_neon",
    name: "Cyberpunk Neon",
    style_instruction:
      "High-detail cyberpunk art with neon accents, rainy futuristic city atmosphere, reflective surfaces, cinematic depth, moody contrast.",
    negative_prompt: "NOT pastel watercolor, NOT low-detail sketch, NOT flat cartoon style.",
    scene_prefix: "Cyberpunk neon style, futuristic city mood, dramatic contrast lighting.",
    lighting: "Neon rim light with cinematic shadows",
  },
  {
    id: "comic_bold",
    name: "Comic Bold",
    style_instruction:
      "Comic book illustration with bold ink outlines, flat-shaded colors, halftone accents, dynamic composition, high visual readability.",
    negative_prompt: "NOT photorealistic, NOT painterly oil texture, NOT 3D CGI.",
    scene_prefix: "Comic-book style, bold outlines, dramatic framing.",
    lighting: "Graphic high-contrast key light",
  },
] as const;
const NEXT_PROJECT_MATERIAL_KEY = "flowkit:create-project:next-material";

// ---- AgentsTab ----

function AgentsTab() {
  const qc = useQueryClient();
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<AgentInfo | null>(null);
  const [formName, setFormName] = useState("");
  const [formSkillIDs, setFormSkillIDs] = useState<string[]>([]);
  const [formPrompt, setFormPrompt] = useState("");
  const [editPrompt, setEditPrompt] = useState("");
  const [formEditSkillIDs, setFormEditSkillIDs] = useState<string[]>([]);
  const [editSoulTarget, setEditSoulTarget] = useState<AgentInfo | null>(null);
  const [editSoulText, setEditSoulText] = useState("");
  const [viewSoulTarget, setViewSoulTarget] = useState<AgentInfo | null>(null);

  const agentsQ = useQuery({
    queryKey: ["agents"],
    queryFn: () => api.listAgents(),
    staleTime: 30_000,
  });
  const skillsQ = useQuery({
    queryKey: ["skill-catalog"],
    queryFn: () => api.listSkillCatalog(),
    staleTime: 60_000,
  });

  const agents = agentsQ.data ?? [];
  const builtins = agents.filter((a) => a.kind === "built-in");
  const skillAgents = agents.filter((a) => a.kind === "skill");

  const resetCreate = () => {
    setFormName("");
    setFormSkillIDs([]);
    setFormPrompt("");
  };

  const createM = useMutation({
    mutationFn: () =>
      api.createSkillAgent({ name: formName, skill_ids: formSkillIDs, prompt: formPrompt }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["agents"] });
      setCreateOpen(false);
      resetCreate();
    },
  });

  const updateM = useMutation({
    mutationFn: ({
      id,
      patch,
    }: {
      id: string;
      patch: { name?: string; prompt?: string; enabled?: boolean; skill_ids?: string[] };
    }) => api.updateSkillAgent(id, patch),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["agents"] }),
  });

  const deleteM = useMutation({
    mutationFn: (id: string) => api.deleteSkillAgent(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["agents"] }),
  });

  const putSoulM = useMutation({
    mutationFn: ({ agentType, soul }: { agentType: string; soul: string }) =>
      api.putAgentSoul(agentType, soul),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["agents"] });
      setEditSoulTarget(null);
    },
  });

  const toggleBuiltinM = useMutation({
    mutationFn: ({ agentType, enabled }: { agentType: string; enabled: boolean }) =>
      api.patchBuiltinAgent(agentType, enabled),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["agents"] }),
  });

  const rowStyle: React.CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "10px 14px",
    borderBottom: "1px solid var(--border)",
    flexWrap: "wrap",
  };

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      {/* Built-in section */}
      <div>
        <Typography.Text strong style={{ fontSize: 12, color: "var(--muted)", textTransform: "uppercase" }}>
          Built-in ({builtins.length})
        </Typography.Text>
        <div style={{ border: "1px solid var(--border)", borderRadius: 8, overflow: "hidden", marginTop: 8 }}>
          {builtins.map((a) => (
            <div key={a.type} style={rowStyle}>
              <RobotOutlined style={{ color: "var(--muted)", flexShrink: 0 }} />
              <div style={{ minWidth: 120, flex: "1 1 140px" }}>
                <Typography.Text strong>{a.type}</Typography.Text>
                {a.soul_file && (
                  <Typography.Text
                    type="secondary"
                    style={{ display: "block", fontSize: 10, fontFamily: "var(--mono)", marginTop: 2 }}
                  >
                    souls/{a.soul_file}
                  </Typography.Text>
                )}
              </div>
              <Typography.Text type="secondary" style={{ fontSize: 12, flex: "2 1 160px" }}>
                {a.description}
              </Typography.Text>
              {a.soul_summary && (
                <Typography.Text type="secondary" style={{ fontSize: 11, flex: "3 1 200px", fontFamily: "var(--mono)", opacity: 0.6 }}>
                  {a.soul_summary}
                </Typography.Text>
              )}
              <Tag color="purple" style={{ fontSize: 10, margin: 0 }}>
                built-in{a.name ? ` · ${a.name}` : ""}
              </Tag>
              <Tag color={a.enabled ? "green" : "default"} style={{ fontSize: 10, margin: 0 }}>
                {a.enabled ? "enabled" : "disabled"}
              </Tag>
              <Switch
                size="small"
                checked={!!a.enabled}
                loading={toggleBuiltinM.isPending && toggleBuiltinM.variables?.agentType === a.type}
                onChange={(checked) => toggleBuiltinM.mutate({ agentType: a.type, enabled: checked })}
              />
              <Space size={4} wrap>
                <Button size="small" icon={<EyeOutlined />} onClick={() => setViewSoulTarget(a)}>
                  Xem soul
                </Button>
                <Button
                  size="small"
                  onClick={() => {
                    setEditSoulTarget(a);
                    setEditSoulText(a.prompt ?? "");
                  }}
                >
                  Sửa soul
                </Button>
              </Space>
            </div>
          ))}
        </div>
      </div>

      {/* Skill agents section */}
      <div>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
          <Typography.Text strong style={{ fontSize: 12, color: "var(--muted)", textTransform: "uppercase" }}>
            Skill Agents ({skillAgents.length})
          </Typography.Text>
          <Button size="small" icon={<PlusOutlined />} type="primary" onClick={() => setCreateOpen(true)}>
            Thêm Agent
          </Button>
        </div>

        {skillAgents.length === 0 ? (
          <Typography.Text type="secondary">
            Chưa có skill agent nào. Nhấn "Thêm Agent" để tạo.
          </Typography.Text>
        ) : (
          <div style={{ border: "1px solid var(--border)", borderRadius: 8, overflow: "hidden" }}>
            {skillAgents.map((a) => (
              <div key={a.type} style={rowStyle}>
                <RobotOutlined style={{ color: "var(--muted)", flexShrink: 0 }} />
                <div style={{ flex: "1 1 160px" }}>
                  <Typography.Text strong>{a.name || a.type}</Typography.Text>
                  <Typography.Text type="secondary" style={{ fontSize: 11, marginLeft: 8 }}>
                    {a.skill_ids?.length
                      ? a.skill_ids.join(", ")
                      : a.skill_id && a.skill_id !== "-"
                        ? a.skill_id
                        : "—"}
                  </Typography.Text>
                </div>
                <Switch
                  size="small"
                  checked={!!a.enabled}
                  loading={updateM.isPending}
                  onChange={(checked) =>
                    updateM.mutate({ id: a.type, patch: { enabled: checked } })
                  }
                />
                <Tag color={a.enabled ? "green" : "default"} style={{ fontSize: 10, margin: 0 }}>
                  {a.enabled ? "enabled" : "disabled"}
                </Tag>
                <Button
                  size="small"
                  onClick={() => {
                    setEditTarget(a);
                    setEditPrompt(a.prompt ?? "");
                    setFormEditSkillIDs(
                      a.skill_ids?.length
                        ? [...a.skill_ids]
                        : a.skill_id && a.skill_id !== "-"
                          ? [a.skill_id]
                          : []
                    );
                  }}
                >
                  Sửa
                </Button>
                <Popconfirm title="Xóa agent này?" onConfirm={() => deleteM.mutate(a.type)}>
                  <Button danger size="small" loading={deleteM.isPending}>
                    Xóa
                  </Button>
                </Popconfirm>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Create modal */}
      <Modal
        title="Thêm Skill Agent"
        open={createOpen}
        onCancel={() => { setCreateOpen(false); resetCreate(); }}
        onOk={() => createM.mutate()}
        confirmLoading={createM.isPending}
        okText="Tạo Agent"
        okButtonProps={{
          disabled:
            !formName.trim() || (formSkillIDs.length === 0 && !formPrompt.trim()),
        }}
        destroyOnClose
      >
        <Form layout="vertical" style={{ marginTop: 12 }}>
          <Form.Item label="Tên agent">
            <Input
              value={formName}
              onChange={(e) => setFormName(e.target.value)}
              placeholder="VD: Viết lại cảnh theo phong cách anime"
            />
          </Form.Item>
          <Form.Item
            label="Skill từ catalog"
            extra="Có thể chọn nhiều hoặc để trống — khi đó bắt buộc điền prompt bên dưới."
          >
            <Select
              mode="multiple"
              allowClear
              placeholder="Chọn một hoặc nhiều skill…"
              value={formSkillIDs}
              onChange={(v) => setFormSkillIDs(v)}
              options={(skillsQ.data ?? []).map((s) => ({
                label: `${s.name} (${s.id})`,
                value: s.id,
              }))}
              showSearch
              filterOption={(input, opt) =>
                String(opt?.label ?? "").toLowerCase().includes(input.toLowerCase())
              }
              style={{ width: "100%" }}
              maxTagCount="responsive"
            />
          </Form.Item>
          <Form.Item label="Prompt bổ sung (tuỳ chọn)">
            <Input.TextArea
              value={formPrompt}
              onChange={(e) => setFormPrompt(e.target.value)}
              rows={4}
              placeholder="Hướng dẫn thêm riêng cho agent này — bắt buộc nếu không chọn skill catalog."
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* Edit skill agent modal */}
      <Modal
        title={`Sửa skill agent: ${editTarget?.name || editTarget?.type}`}
        open={!!editTarget}
        onCancel={() => setEditTarget(null)}
        onOk={() => {
          if (!editTarget) return;
          updateM.mutate(
            {
              id: editTarget.type,
              patch: { prompt: editPrompt, skill_ids: formEditSkillIDs },
            },
            { onSuccess: () => setEditTarget(null) }
          );
        }}
        confirmLoading={updateM.isPending}
        okText="Lưu"
        okButtonProps={{
          disabled: formEditSkillIDs.length === 0 && !editPrompt.trim(),
        }}
        destroyOnClose
      >
        <Form layout="vertical" style={{ marginTop: 12 }}>
          <Form.Item
            label="Skill từ catalog"
            extra="Để trống chỉ hợp lệ khi có prompt bổ sung."
          >
            <Select
              mode="multiple"
              allowClear
              placeholder="Chọn skill…"
              value={formEditSkillIDs}
              onChange={(v) => setFormEditSkillIDs(v)}
              options={(skillsQ.data ?? []).map((s) => ({
                label: `${s.name} (${s.id})`,
                value: s.id,
              }))}
              showSearch
              filterOption={(input, opt) =>
                String(opt?.label ?? "").toLowerCase().includes(input.toLowerCase())
              }
              style={{ width: "100%" }}
              maxTagCount="responsive"
            />
          </Form.Item>
          <Form.Item label="Prompt bổ sung">
            <Input.TextArea
              value={editPrompt}
              onChange={(e) => setEditPrompt(e.target.value)}
              rows={8}
              style={{ fontFamily: "var(--mono)", fontSize: 12 }}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* View soul (markdown preview + raw) */}
      <Modal
        title={
          <Space direction="vertical" size={0}>
            <Typography.Text strong>{`Xem soul: ${viewSoulTarget?.type ?? ""}`}</Typography.Text>
            {viewSoulTarget?.soul_file && (
              <Typography.Text type="secondary" style={{ fontSize: 11, fontFamily: "var(--mono)" }}>
                backend/souls/{viewSoulTarget.soul_file}
              </Typography.Text>
            )}
          </Space>
        }
        open={!!viewSoulTarget}
        onCancel={() => setViewSoulTarget(null)}
        footer={<Button onClick={() => setViewSoulTarget(null)}>Đóng</Button>}
        width={920}
        styles={{ body: { maxHeight: "78vh", overflowY: "auto", paddingTop: 8 } }}
        destroyOnClose
      >
        {!viewSoulTarget?.prompt?.trim() ? (
          <Typography.Text type="secondary">Chưa có nội dung soul (fallback trong code).</Typography.Text>
        ) : (
          <Tabs
            defaultActiveKey="md"
            items={[
              {
                key: "md",
                label: "Markdown",
                children: (
                  <div
                    style={{
                      background: "var(--bg)",
                      border: "1px solid var(--border)",
                      borderRadius: 8,
                      padding: "16px 20px",
                      fontSize: 13,
                      lineHeight: 1.6,
                      maxHeight: "62vh",
                      overflowY: "auto",
                    }}
                  >
                    <ReactMarkdown remarkPlugins={[remarkGfm]}>{viewSoulTarget.prompt ?? ""}</ReactMarkdown>
                  </div>
                ),
              },
              {
                key: "raw",
                label: "Raw",
                children: (
                  <pre
                    style={{
                      margin: 0,
                      padding: "14px 16px",
                      borderRadius: 8,
                      border: "1px solid var(--border)",
                      fontSize: 11,
                      fontFamily: "var(--mono)",
                      whiteSpace: "pre-wrap",
                      wordBreak: "break-word",
                      maxHeight: "62vh",
                      overflowY: "auto",
                      background: "var(--bg)",
                    }}
                  >
                    {viewSoulTarget.prompt}
                  </pre>
                ),
              },
            ]}
          />
        )}
      </Modal>

      {/* Edit soul modal (built-in agents) */}
      <Modal
        title={`Sửa soul: ${editSoulTarget?.type}`}
        open={!!editSoulTarget}
        onCancel={() => setEditSoulTarget(null)}
        onOk={() => {
          if (!editSoulTarget) return;
          putSoulM.mutate({ agentType: editSoulTarget.type, soul: editSoulText });
        }}
        confirmLoading={putSoulM.isPending}
        okText="Lưu soul"
        width={680}
        destroyOnClose
      >
        {editSoulTarget?.soul_file && (
          <Typography.Text code style={{ fontSize: 11, display: "block", marginBottom: 10 }}>
            backend/souls/{editSoulTarget.soul_file}
          </Typography.Text>
        )}
        <Typography.Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 12 }}>
          Soul là system prompt của agent. Lưu sẽ ghi đúng file trên và áp dụng cho pipeline sau.
        </Typography.Text>
        <Input.TextArea
          value={editSoulText}
          onChange={(e) => setEditSoulText(e.target.value)}
          rows={16}
          style={{ fontFamily: "var(--mono)", fontSize: 12 }}
          placeholder={`You are the ${editSoulTarget?.type} agent in the Flow Agent Video pipeline...`}
        />
      </Modal>
    </Space>
  );
}

// ---- SettingsPage ----

export function SettingsPage() {
  const qc = useQueryClient();
  const [profile, setProfile] = useState("");
  const [videoModel, setVideoModel] = useState("auto");
  const [materialID, setMaterialID] = useState("");
  const [materialName, setMaterialName] = useState("");
  const [styleInstruction, setStyleInstruction] = useState("");
  const [negativePrompt, setNegativePrompt] = useState("");
  const [scenePrefix, setScenePrefix] = useState("");
  const [lighting, setLighting] = useState("");
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [nextProjectMaterialID, setNextProjectMaterialID] = useState("");
  const [materialModalOpen, setMaterialModalOpen] = useState(false);

  const q = useQuery({
    queryKey: ["llm-settings"],
    queryFn: () => api.getLLMSettings(),
    staleTime: 15_000,
  });
  const materialsQ = useQuery({
    queryKey: ["materials"],
    queryFn: () => api.listMaterials(),
    staleTime: 30_000,
  });
  const toolsQ = useQuery({
    queryKey: ["tools-settings"],
    queryFn: () => api.getToolsSettings(),
    staleTime: 30_000,
  });
  useEffect(() => {
    const d = q.data;
    if (!d) return;
    setProfile(d.profile ?? "");
    setVideoModel(d.video_model ?? "auto");
  }, [q.data]);
  useEffect(() => {
    setNextProjectMaterialID(
      localStorage.getItem(NEXT_PROJECT_MATERIAL_KEY)?.trim() ?? ""
    );
  }, []);

  const saveM = useMutation({
    mutationFn: () => api.putLLMSettings({ profile, video_model: videoModel }),
    onSuccess: () => {
      setErr(null);
      setMsg("Đã lưu. Toàn bộ pipeline AI (suggest-project/suggest-scenes) sẽ dùng LLM profile này.");
      void qc.invalidateQueries({ queryKey: ["llm-settings"] });
    },
    onError: (e: Error) => {
      setMsg(null);
      setErr(e.message);
    },
  });
  const createMaterialM = useMutation({
    mutationFn: () =>
      api.createMaterial({
        id: materialID.trim(),
        name: materialName.trim(),
        style_instruction: styleInstruction.trim(),
        negative_prompt: negativePrompt.trim() || undefined,
        scene_prefix: scenePrefix.trim() || undefined,
        lighting: lighting.trim() || undefined,
      }),
    onSuccess: () => {
      setErr(null);
      setMsg("Đã tạo custom material.");
      setMaterialModalOpen(false);
      setMaterialID("");
      setMaterialName("");
      setStyleInstruction("");
      setNegativePrompt("");
      setScenePrefix("");
      setLighting("");
      void qc.invalidateQueries({ queryKey: ["materials"] });
    },
    onError: (e: Error) => {
      setMsg(null);
      setErr(e.message);
    },
  });
  const deleteMaterialM = useMutation({
    mutationFn: (materialId: string) => api.deleteMaterial(materialId),
    onSuccess: () => {
      setErr(null);
      setMsg("Đã xóa custom material.");
      void qc.invalidateQueries({ queryKey: ["materials"] });
    },
    onError: (e: Error) => {
      setMsg(null);
      setErr(e.message);
    },
  });
  const importMaterialFileM = useMutation({
    mutationFn: async (file: File) => {
      const jsonContent = await file.text();
      return api.importMaterialsFromJSON(jsonContent);
    },
    onSuccess: (res) => {
      setErr(null);
      setMsg(res.message);
      void qc.invalidateQueries({ queryKey: ["materials"] });
    },
    onError: (e: Error) => {
      setMsg(null);
      setErr(e.message);
    },
  });

  const profileOptions = [
    { label: "— dùng model đang active của SenClaw —", value: "" },
    ...(q.data?.profiles ?? []).map((p) => {
      const value = p.id ?? p.label ?? p.model ?? "";
      const name = p.label ?? p.id ?? p.model ?? "";
      return {
        label: p.model && p.model !== name ? `${name} (${p.model})` : name,
        value,
      };
    }),
  ];
  const applyPreset = (preset: (typeof MATERIAL_PRESETS)[number]) => {
    setMaterialID(preset.id);
    setMaterialName(preset.name);
    setStyleInstruction(preset.style_instruction);
    setNegativePrompt(preset.negative_prompt);
    setScenePrefix(preset.scene_prefix);
    setLighting(preset.lighting);
    setErr(null);
    setMsg(`Đã nạp preset: ${preset.name}`);
  };
  const chooseForNextProject = (materialId: string, materialNameText: string) => {
    localStorage.setItem(NEXT_PROJECT_MATERIAL_KEY, materialId);
    setNextProjectMaterialID(materialId);
    setErr(null);
    setMsg(`Đã chọn material mặc định cho lần tạo project kế tiếp: ${materialNameText} (${materialId})`);
  };

  return (
    <div className="layout layout-wide">
      <Space direction="vertical" size={16} style={{ width: "100%" }}>
        <Typography.Title level={3} style={{ margin: 0 }}>
          Cài đặt
        </Typography.Title>
        {err && <Alert type="error" message={err} showIcon />}
        {msg && <Alert type="success" message={msg} showIcon />}
        <Tabs
          defaultActiveKey="provider"
          items={[
            {
              key: "provider",
              label: "Provider",
              children: (
                <Card>
                  <Typography.Text type="secondary">
                    LLM chạy qua SenClaw daemon — chọn profile từ danh sách model đã cấu hình trong SenClaw.
                  </Typography.Text>
                  <Form layout="vertical" style={{ marginTop: 12 }}>
                    <Form.Item
                      label="LLM profile"
                      extra={
                        q.data?.model ? (
                          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                            Model hiện tại: {q.data.model}
                          </Typography.Text>
                        ) : null
                      }
                    >
                      <Select
                        value={profile}
                        onChange={(v) => setProfile(v ?? "")}
                        options={profileOptions}
                        showSearch
                        filterOption={(input, opt) =>
                          String(opt?.label ?? "").toLowerCase().includes(input.toLowerCase())
                        }
                      />
                    </Form.Item>

                    <Form.Item
                      label="Model video (Google Flow)"
                      extra={
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          {videoModel === "lite"
                            ? "Veo 3.1 Lite: 0 credit, chạy trên mọi tier — nên chọn cái này nếu Fast báo done mà không ra video."
                            : videoModel === "fast"
                            ? "Veo 3.1 Fast: chất lượng cao hơn nhưng tốn credit và cần service tier cao (tier thấp sẽ 200 mà không ra clip)."
                            : `Tự động: dùng đúng model Flow đang chọn${q.data?.video_model_learned ? ` (đã học: ${q.data.video_model_learned})` : ""}.`}
                        </Typography.Text>
                      }
                    >
                      <Select
                        value={videoModel}
                        onChange={(v) => setVideoModel(v ?? "auto")}
                        options={[
                          { label: "Tự động (học từ Flow)", value: "auto" },
                          { label: "Veo 3.1 Lite — 0 credit, mọi tier", value: "lite" },
                          { label: "Veo 3.1 Fast — chất lượng cao, tốn credit", value: "fast" },
                        ]}
                      />
                    </Form.Item>

                    <Button type="primary" loading={saveM.isPending} onClick={() => saveM.mutate()}>
                      Lưu cài đặt
                    </Button>
                  </Form>
                </Card>
              ),
            },
            {
              key: "material",
              label: "Material",
              children: (
                <Card>
                  <Space direction="vertical" size={12} style={{ width: "100%" }}>
                    <Typography.Text type="secondary">
                      Quản lý visual style cho project. Tạo custom material bằng dialog.
                    </Typography.Text>
                    <Space>
                      <Button type="primary" onClick={() => setMaterialModalOpen(true)}>
                        Thêm Material
                      </Button>
                      <Button
                        loading={importMaterialFileM.isPending}
                        onClick={() => document.getElementById("material-json-file-input")?.click()}
                      >
                        Import file JSON
                      </Button>
                      <input
                        id="material-json-file-input"
                        type="file"
                        accept=".json,application/json"
                        style={{ display: "none" }}
                        onChange={(e) => {
                          const file = e.target.files?.[0];
                          if (file) {
                            importMaterialFileM.mutate(file);
                          }
                          e.currentTarget.value = "";
                        }}
                      />
                    </Space>
                    <Typography.Text strong>Danh sách material</Typography.Text>
                    <div style={{ marginTop: 4 }}>
                      {(materialsQ.data?.materials ?? []).map((m) => (
                        <Card
                          key={m.id}
                          size="small"
                          style={{ marginBottom: 8 }}
                          title={
                            <Space>
                              <span>{m.name}</span>
                              <Tag>{m.id}</Tag>
                              {m.is_builtin ? (
                                <Tag color="blue">built-in{m.name ? ` · ${m.name}` : ""}</Tag>
                              ) : (
                                <Tag color="green">custom</Tag>
                              )}
                              {nextProjectMaterialID === m.id ? (
                                <Tag color="gold">mặc định lần tạo kế tiếp</Tag>
                              ) : null}
                            </Space>
                          }
                          extra={
                            <Space>
                              <Button
                                size="small"
                                onClick={() => chooseForNextProject(m.id, m.name)}
                              >
                                Dùng cho project kế tiếp
                              </Button>
                              {m.is_builtin ? null : (
                                <Popconfirm
                                  title="Xóa material này?"
                                  description="Không thể hoàn tác. Nếu đang có project dùng material này, thao tác sẽ bị chặn."
                                  onConfirm={() => deleteMaterialM.mutate(m.id)}
                                >
                                  <Button danger size="small" loading={deleteMaterialM.isPending}>
                                    Xóa
                                  </Button>
                                </Popconfirm>
                              )}
                            </Space>
                          }
                        >
                          <Typography.Paragraph style={{ marginBottom: 6 }}>
                            {m.style_instruction}
                          </Typography.Paragraph>
                          {m.scene_prefix && (
                            <Typography.Text type="secondary">
                              scene_prefix: {m.scene_prefix}
                            </Typography.Text>
                          )}
                        </Card>
                      ))}
                    </div>
                  </Space>
                </Card>
              ),
            },
            {
              key: "skills",
              label: "Skills",
              children: <SkillsPage embedded />,
            },
            {
              key: "agents",
              label: "Agents",
              children: (
                <Card>
                  <AgentsTab />
                </Card>
              ),
            },
            {
              key: "tools",
              label: "Tools",
              children: (
                <Card>
                  <Space direction="vertical" size={12} style={{ width: "100%" }}>
                    <Typography.Text type="secondary">
                      Danh sách tools backend đã đăng ký cho agent. Tool trong `executor.go` sẽ hiện ở đây với tên `execute_cmd`.
                    </Typography.Text>
                    {(toolsQ.data?.tools ?? []).map((tool) => (
                      <Card
                        key={tool.name}
                        size="small"
                        title={
                          <Space>
                            <Typography.Text strong>{tool.name}</Typography.Text>
                            <Tag color="purple">tool</Tag>
                          </Space>
                        }
                      >
                        <Typography.Paragraph style={{ marginBottom: 8 }}>
                          {tool.description}
                        </Typography.Paragraph>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          Input schema
                        </Typography.Text>
                        <pre
                          style={{
                            marginTop: 6,
                            marginBottom: 0,
                            padding: 10,
                            borderRadius: 8,
                            background: "var(--bg)",
                            color: "var(--text)",
                            overflowX: "auto",
                            fontSize: 12,
                            border: "1px solid var(--border)",
                          }}
                        >
                          {JSON.stringify(tool.input_schema ?? {}, null, 2)}
                        </pre>
                      </Card>
                    ))}
                    {(toolsQ.data?.tools ?? []).length === 0 && (
                      <Typography.Text type="secondary">
                        Chưa có tool nào được đăng ký.
                      </Typography.Text>
                    )}
                  </Space>
                </Card>
              ),
            },
          ]}
        />
      </Space>
      <Modal
        title="Thêm Material"
        open={materialModalOpen}
        onCancel={() => setMaterialModalOpen(false)}
        onOk={() => createMaterialM.mutate()}
        okText="Tạo custom material"
        confirmLoading={createMaterialM.isPending}
        okButtonProps={{
          disabled: !materialID.trim() || !materialName.trim() || styleInstruction.trim().length < 10,
        }}
      >
        <Form layout="vertical">
          <Typography.Text type="secondary">
            ID phải viết thường và dùng dấu gạch dưới.
          </Typography.Text>
          <div style={{ margin: "8px 0 12px" }}>
            <Typography.Text type="secondary">Preset nhanh:</Typography.Text>
            <Space wrap style={{ marginLeft: 8 }}>
              {MATERIAL_PRESETS.map((preset) => (
                <Button key={preset.id} size="small" onClick={() => applyPreset(preset)}>
                  {preset.name}
                </Button>
              ))}
            </Space>
          </div>
          <Form.Item label="Material ID">
            <Input
              value={materialID}
              onChange={(e) => setMaterialID(e.target.value)}
              placeholder="vd: watercolor_soft"
            />
          </Form.Item>
          <Form.Item label="Tên hiển thị">
            <Input
              value={materialName}
              onChange={(e) => setMaterialName(e.target.value)}
              placeholder="Watercolor Soft"
            />
          </Form.Item>
          <Form.Item label="Style instruction">
            <Input.TextArea
              value={styleInstruction}
              onChange={(e) => setStyleInstruction(e.target.value)}
              autoSize={{ minRows: 2, maxRows: 6 }}
              placeholder="Mô tả style chính..."
            />
          </Form.Item>
          <Form.Item label="Negative prompt (tuỳ chọn)">
            <Input
              value={negativePrompt}
              onChange={(e) => setNegativePrompt(e.target.value)}
              placeholder="NOT photorealistic, NOT 3D render..."
            />
          </Form.Item>
          <Form.Item label="Scene prefix (tuỳ chọn)">
            <Input
              value={scenePrefix}
              onChange={(e) => setScenePrefix(e.target.value)}
              placeholder="Watercolor style, soft wash lighting..."
            />
          </Form.Item>
          <Form.Item label="Lighting (tuỳ chọn)">
            <Input
              value={lighting}
              onChange={(e) => setLighting(e.target.value)}
              placeholder="Soft diffused natural light"
            />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
