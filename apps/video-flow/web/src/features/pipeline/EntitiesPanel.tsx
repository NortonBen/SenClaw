import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Avatar,
  Button,
  Card,
  Checkbox,
  Divider,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Tag,
  Typography,
} from "antd";
import {
  PlusOutlined,
  RobotOutlined,
  SyncOutlined,
  UserOutlined,
} from "@ant-design/icons";
import { useCallback, useMemo, useState } from "react";
import { api, type AISuggestEntityItem, type CharacterRow, type RequestRow } from "@/lib/api/client";

const { Text } = Typography;
const { TextArea } = Input;

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

type Draft = {
  name: string;
  entity_type: string;
  description: string;
  voice_description: string;
  image_prompt: string;
};

const ENTITY_TYPES = [
  { value: "character", label: "Character" },
  { value: "location", label: "Location" },
  { value: "creature", label: "Creature" },
  { value: "visual_asset", label: "Visual Asset" },
  { value: "generic_troop", label: "Generic Troop" },
  { value: "faction", label: "Faction" },
];

const emptyDraft = (): Draft => ({
  name: "",
  entity_type: "character",
  description: "",
  voice_description: "",
  image_prompt: "",
});

function rowToDraft(row: CharacterRow): Draft {
  return {
    name: str(row.name),
    entity_type: str(row.entity_type) || "character",
    description: str(row.description),
    voice_description: str(row.voice_description),
    image_prompt: str(row.image_prompt),
  };
}

const JOB_STATUS_COLOR: Record<string, "default" | "processing" | "success" | "error"> = {
  PENDING: "default",
  PROCESSING: "processing",
  COMPLETED: "success",
  FAILED: "error",
};

type Props = {
  projectId: string;
  rows: CharacterRow[] | undefined;
  isLoading: boolean;
  orientation: "VERTICAL" | "HORIZONTAL";
  onGenRefQueued?: () => void;
};

export function EntitiesPanel({ projectId, rows, isLoading, orientation, onGenRefQueued }: Props) {
  const qc = useQueryClient();

  // Fetch project for story context
  const projectQ = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => api.getProject(projectId),
    enabled: !!projectId,
    staleTime: 60_000,
  });

  const refJobsQ = useQuery({
    queryKey: ["requests", "character-ref-jobs", projectId],
    queryFn: () => api.listRequests({ project_id: projectId }),
    enabled: !!projectId.trim(),
    refetchInterval: 3500,
  });

  const latestRefJobByCharacter = useMemo(() => {
    const raw = (refJobsQ.data ?? []) as RequestRow[];
    const types = new Set(["GENERATE_CHARACTER_IMAGE", "REGENERATE_CHARACTER_IMAGE", "EDIT_CHARACTER_IMAGE"]);
    const m = new Map<string, RequestRow>();
    for (const r of raw) {
      const cid = str(r.character_id);
      if (!cid || !types.has(str(r.type))) continue;
      if (!m.has(cid)) m.set(cid, r);
    }
    return m;
  }, [refJobsQ.data]);

  const [err, setErr] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  // Add entity manually
  const [showAdd, setShowAdd] = useState(false);
  const [newDraft, setNewDraft] = useState<Draft>(emptyDraft);

  // Edit entity
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(emptyDraft);

  // AI Generate entities
  const [showAISuggest, setShowAISuggest] = useState(false);
  const [aiPrompt, setAiPrompt] = useState("");
  const [aiSuggestions, setAiSuggestions] = useState<AISuggestEntityItem[]>([]);
  const [selectedSuggestions, setSelectedSuggestions] = useState<Set<number>>(new Set());

  // ---------- mutations ----------

  const createM = useMutation({
    mutationFn: () =>
      api.createProjectCharacter(projectId, {
        name: newDraft.name.trim(),
        entity_type: newDraft.entity_type.trim() || "character",
        description: newDraft.description.trim() || null,
        voice_description: newDraft.voice_description.trim() || null,
      }),
    onSuccess: () => {
      setErr(null); setOk("Đã thêm entity."); setNewDraft(emptyDraft()); setShowAdd(false);
      void qc.invalidateQueries({ queryKey: ["characters", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests", "character-ref-jobs", projectId] });
    },
    onError: (e: Error) => { setOk(null); setErr(e.message); },
  });

  const patchM = useMutation({
    mutationFn: (args: { cid: string; body: Draft }) =>
      api.patchProjectCharacter(projectId, args.cid, {
        name: args.body.name.trim(),
        entity_type: args.body.entity_type.trim() || "character",
        description: args.body.description.trim(),
        voice_description: args.body.voice_description.trim(),
        image_prompt: args.body.image_prompt.trim(),
      }),
    onSuccess: () => {
      setErr(null); setOk("Đã cập nhật entity."); setEditingId(null);
      void qc.invalidateQueries({ queryKey: ["characters", projectId] });
    },
    onError: (e: Error) => { setOk(null); setErr(e.message); },
  });

  const queueGenRef = useMutation({
    mutationFn: (characterId: string) =>
      api.createRequestBatch([{ type: "GENERATE_CHARACTER_IMAGE", character_id: characterId, project_id: projectId, orientation }]),
    onSuccess: () => {
      setErr(null); setOk("Đã gửi gen ảnh tham chiếu.");
      onGenRefQueued?.();
      void qc.invalidateQueries({ queryKey: ["characters", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests", "character-ref-jobs", projectId] });
    },
    onError: (e: Error) => { setOk(null); setErr(e.message); },
  });

  const queueRegenRef = useMutation({
    mutationFn: (characterId: string) =>
      api.createRequestBatch([{ type: "REGENERATE_CHARACTER_IMAGE", character_id: characterId, project_id: projectId, orientation }]),
    onSuccess: () => {
      setErr(null); setOk("Đã gửi gen lại ảnh tham chiếu.");
      onGenRefQueued?.();
      void qc.invalidateQueries({ queryKey: ["characters", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests", "character-ref-jobs", projectId] });
    },
    onError: (e: Error) => { setOk(null); setErr(e.message); },
  });

  const unlinkM = useMutation({
    mutationFn: (args: { cid: string; deleteRow: boolean }) =>
      api.unlinkProjectCharacter(projectId, args.cid, args.deleteRow),
    onSuccess: (data, variables) => {
      setErr(null); setEditingId(null);
      if (variables.deleteRow && data.character_deleted) {
        setOk("Đã gỡ và xóa bản ghi character.");
      } else if (variables.deleteRow && data.still_linked_to_other_projects) {
        setOk("Đã gỡ khỏi project này — entity vẫn gắn project khác.");
      } else {
        setOk("Đã gỡ entity khỏi project.");
      }
      void qc.invalidateQueries({ queryKey: ["characters", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests", "character-ref-jobs", projectId] });
    },
    onError: (e: Error) => { setOk(null); setErr(e.message); },
  });

  const suggestEntitiesM = useMutation({
    mutationFn: () => {
      const proj = projectQ.data as Record<string, unknown> | undefined;
      const story = str(proj?.story).trim();
      return api.suggestEntities({ story: story || undefined, prompt: aiPrompt.trim() || undefined, project_id: projectId });
    },
    onSuccess: (data) => {
      setAiSuggestions(data.entities ?? []);
      // Pre-select all
      setSelectedSuggestions(new Set((data.entities ?? []).map((_, i) => i)));
      setErr(null);
    },
    onError: (e: Error) => { setErr(e.message); },
  });

  const addSelectedM = useMutation({
    mutationFn: async () => {
      const toAdd = aiSuggestions.filter((_, i) => selectedSuggestions.has(i));
      for (const e of toAdd) {
        await api.createProjectCharacter(projectId, {
          name: e.name.trim(),
          entity_type: e.entity_type || "character",
          description: e.description.trim() || null,
        });
      }
      return { added: toAdd.length };
    },
    onSuccess: ({ added }) => {
      setOk(`Đã thêm ${added} entity từ AI.`);
      setShowAISuggest(false);
      setAiSuggestions([]);
      setSelectedSuggestions(new Set());
      setAiPrompt("");
      void qc.invalidateQueries({ queryKey: ["characters", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests", "character-ref-jobs", projectId] });
    },
    onError: (e: Error) => { setErr(e.message); },
  });

  const startEdit = useCallback((row: CharacterRow) => {
    setEditingId(str(row.id));
    setDraft(rowToDraft(row));
    setErr(null); setOk(null);
  }, []);

  const toggleSuggestion = (i: number) => {
    setSelectedSuggestions((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i); else next.add(i);
      return next;
    });
  };

  const list = rows ?? [];
  const projectStory = str((projectQ.data as Record<string, unknown> | undefined)?.story);

  const columns = useMemo(() => [
    {
      title: "Entity",
      key: "entity",
      render: (_: unknown, row: CharacterRow) => {
        const imgUrl = str(row.reference_image_url);
        return (
          <Space>
            {imgUrl
              ? <Avatar src={imgUrl} size={36} />
              : <Avatar icon={<UserOutlined />} size={36} />
            }
            <div>
              <Text strong style={{ fontSize: 13 }}>{str(row.name)}</Text>
              <br />
              <Text type="secondary" style={{ fontSize: 11 }}>{str(row.description).slice(0, 60)}{str(row.description).length > 60 ? "…" : ""}</Text>
            </div>
          </Space>
        );
      },
    },
    {
      title: "Loại",
      dataIndex: "entity_type",
      key: "type",
      width: 110,
      render: (v: unknown) => <Tag>{str(v) || "character"}</Tag>,
    },
    {
      title: "Ref image",
      key: "ref",
      width: 84,
      render: (_: unknown, row: CharacterRow) => {
        const url = str(row.reference_image_url);
        return url
          ? <a href={url} target="_blank" rel="noreferrer"><img src={url} alt="ref" style={{ width: 56, height: 56, objectFit: "cover", borderRadius: 6, border: "1px solid var(--border)" }} /></a>
          : <Text type="secondary">—</Text>;
      },
    },
    {
      title: "Job ref",
      key: "job",
      width: 110,
      render: (_: unknown, row: CharacterRow) => {
        const req = latestRefJobByCharacter.get(str(row.id));
        if (!req) return <Text type="secondary">—</Text>;
        const st = str(req.status);
        return <Tag color={JOB_STATUS_COLOR[st] ?? "default"}>{st}</Tag>;
      },
    },
    {
      title: "Hành động",
      key: "actions",
      width: 200,
      render: (_: unknown, row: CharacterRow) => {
        const id = str(row.id);
        const hasRef = !!str(row.media_id);
        return (
          <Space size={4} wrap>
            <Button size="small" onClick={() => startEdit(row)}>Sửa</Button>
            {!hasRef
              ? <Button size="small" icon={<SyncOutlined />} onClick={() => queueGenRef.mutate(id)} loading={queueGenRef.isPending}>Gen ref</Button>
              : <Button size="small" icon={<SyncOutlined />} onClick={() => queueRegenRef.mutate(id)} loading={queueRegenRef.isPending}>Gen lại</Button>
            }
            <Popconfirm
              title={`Gỡ «${str(row.name)}» khỏi project?`}
              description="Chỉ gỡ liên kết, bản ghi character giữ trong DB."
              onConfirm={() => unlinkM.mutate({ cid: id, deleteRow: false })}
            >
              <Button size="small" danger>Gỡ</Button>
            </Popconfirm>
          </Space>
        );
      },
    },
  ], [latestRefJobByCharacter, startEdit, queueGenRef, queueRegenRef, unlinkM]);

  return (
    <Space direction="vertical" size={16} style={{ width: "100%" }}>
      {err && <Alert type="error" message={err} showIcon closable onClose={() => setErr(null)} />}
      {ok && <Alert type="success" message={ok} showIcon closable onClose={() => setOk(null)} />}

      <Card
        size="small"
        extra={
          <Space>
            <Button
              icon={<RobotOutlined />}
              size="small"
              onClick={() => { setShowAISuggest(true); setErr(null); setOk(null); }}
            >
              AI Generate
            </Button>
            <Button
              type="primary"
              icon={<PlusOutlined />}
              size="small"
              onClick={() => { setShowAdd(true); setErr(null); setOk(null); }}
            >
              Thêm entity
            </Button>
          </Space>
        }
      >
        <Table<CharacterRow>
          dataSource={list}
          columns={columns}
          rowKey={(r) => str(r.id)}
          size="small"
          loading={isLoading}
          pagination={false}
          locale={{ emptyText: "Chưa có entity trong project này." }}
        />
      </Card>

      {/* Modal: AI Generate Entities */}
      <Modal
        title={<Space><RobotOutlined />AI Generate Characters & Entities</Space>}
        open={showAISuggest}
        onCancel={() => { setShowAISuggest(false); setAiSuggestions([]); setSelectedSuggestions(new Set()); setAiPrompt(""); }}
        width={640}
        footer={
          <Space>
            <Button onClick={() => { setShowAISuggest(false); setAiSuggestions([]); setSelectedSuggestions(new Set()); }}>
              Đóng
            </Button>
            <Button
              icon={<RobotOutlined />}
              loading={suggestEntitiesM.isPending}
              disabled={!projectStory.trim() && !aiPrompt.trim()}
              onClick={() => suggestEntitiesM.mutate()}
            >
              Generate
            </Button>
            {aiSuggestions.length > 0 && (
              <Button
                type="primary"
                loading={addSelectedM.isPending}
                disabled={selectedSuggestions.size === 0}
                onClick={() => addSelectedM.mutate()}
              >
                {`Thêm ${selectedSuggestions.size} entity đã chọn`}
              </Button>
            )}
          </Space>
        }
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          {projectStory && (
            <Card size="small" style={{ background: "rgba(91,141,239,0.06)", border: "1px dashed var(--border)" }}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                <strong>Story project:</strong> {projectStory.slice(0, 200)}{projectStory.length > 200 ? "…" : ""}
              </Text>
            </Card>
          )}

          <Form.Item
            label="Hướng dẫn thêm (tùy chọn)"
            style={{ marginBottom: 0 }}
            help="AI sẽ dùng story của project + hướng dẫn này để gợi ý nhân vật, địa điểm, sinh vật…"
          >
            <TextArea
              value={aiPrompt}
              onChange={(e) => setAiPrompt(e.target.value)}
              placeholder="VD: Tập trung vào nhân vật chính và phản diện. Thêm 2-3 địa điểm quan trọng."
              autoSize={{ minRows: 2, maxRows: 5 }}
            />
          </Form.Item>

          {aiSuggestions.length > 0 && (
            <>
              <Divider style={{ margin: "12px 0" }}>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {`${aiSuggestions.length} entity được gợi ý · ${selectedSuggestions.size} đã chọn`}
                </Text>
              </Divider>
              <Space style={{ marginBottom: 8 }}>
                <Button
                  size="small"
                  onClick={() => setSelectedSuggestions(new Set(aiSuggestions.map((_, i) => i)))}
                >
                  Chọn tất cả
                </Button>
                <Button
                  size="small"
                  onClick={() => setSelectedSuggestions(new Set())}
                >
                  Bỏ chọn tất cả
                </Button>
              </Space>
              <Space direction="vertical" size={8} style={{ width: "100%" }}>
                {aiSuggestions.map((entity, i) => (
                  <Card
                    key={i}
                    size="small"
                    style={{
                      cursor: "pointer",
                      borderColor: selectedSuggestions.has(i) ? "#5b8def" : undefined,
                      background: selectedSuggestions.has(i) ? "rgba(91,141,239,0.06)" : undefined,
                    }}
                    onClick={() => toggleSuggestion(i)}
                  >
                    <Space>
                      <Checkbox checked={selectedSuggestions.has(i)} onChange={() => toggleSuggestion(i)} onClick={(e) => e.stopPropagation()} />
                      <Avatar icon={<UserOutlined />} size={28} />
                      <div>
                        <Space size={6}>
                          <Text strong style={{ fontSize: 13 }}>{entity.name}</Text>
                          <Tag style={{ margin: 0 }}>{entity.entity_type}</Tag>
                        </Space>
                        {entity.description && (
                          <div>
                            <Text type="secondary" style={{ fontSize: 12 }}>{entity.description}</Text>
                          </div>
                        )}
                      </div>
                    </Space>
                  </Card>
                ))}
              </Space>
            </>
          )}
        </Space>
      </Modal>

      {/* Modal: Add Entity */}
      <Modal
        title="Thêm entity"
        open={showAdd}
        onCancel={() => { setShowAdd(false); setNewDraft(emptyDraft()); }}
        onOk={() => createM.mutate()}
        confirmLoading={createM.isPending}
        okButtonProps={{ disabled: !newDraft.name.trim() }}
        okText="Tạo entity"
      >
        <Form layout="vertical" style={{ marginTop: 8 }}>
          <Form.Item label="Tên" required>
            <Input
              value={newDraft.name}
              onChange={(e) => setNewDraft((d) => ({ ...d, name: e.target.value }))}
              placeholder="Hero, Castle, …"
            />
          </Form.Item>
          <Form.Item label="Loại">
            <Select
              value={newDraft.entity_type}
              onChange={(v) => setNewDraft((d) => ({ ...d, entity_type: v }))}
              options={ENTITY_TYPES}
            />
          </Form.Item>
          <Form.Item label="Mô tả (ngoại hình / bối cảnh)">
            <TextArea
              value={newDraft.description}
              onChange={(e) => setNewDraft((d) => ({ ...d, description: e.target.value }))}
              autoSize={{ minRows: 3, maxRows: 6 }}
            />
          </Form.Item>
          <Form.Item label="Voice description (tuỳ chọn)" style={{ marginBottom: 0 }}>
            <Input
              value={newDraft.voice_description}
              onChange={(e) => setNewDraft((d) => ({ ...d, voice_description: e.target.value }))}
              placeholder="Giọng đọc…"
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* Modal: Edit Entity */}
      <Modal
        title="Sửa entity"
        open={!!editingId}
        onCancel={() => setEditingId(null)}
        onOk={() => editingId && patchM.mutate({ cid: editingId, body: draft })}
        confirmLoading={patchM.isPending}
        okButtonProps={{ disabled: !draft.name.trim() }}
        okText="Lưu"
      >
        <Form layout="vertical" style={{ marginTop: 8 }}>
          <Form.Item label="Tên" required>
            <Input value={draft.name} onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))} />
          </Form.Item>
          <Form.Item label="Loại">
            <Select value={draft.entity_type} onChange={(v) => setDraft((d) => ({ ...d, entity_type: v }))} options={ENTITY_TYPES} />
          </Form.Item>
          <Form.Item label="Mô tả">
            <TextArea value={draft.description} onChange={(e) => setDraft((d) => ({ ...d, description: e.target.value }))} autoSize={{ minRows: 3, maxRows: 6 }} />
          </Form.Item>
          <Form.Item label="Voice description">
            <Input value={draft.voice_description} onChange={(e) => setDraft((d) => ({ ...d, voice_description: e.target.value }))} />
          </Form.Item>
          <Form.Item label="Image prompt (override)" style={{ marginBottom: 0 }}>
            <TextArea value={draft.image_prompt} onChange={(e) => setDraft((d) => ({ ...d, image_prompt: e.target.value }))} autoSize={{ minRows: 2, maxRows: 5 }} placeholder="Prompt ảnh tham chiếu entity…" />
          </Form.Item>
        </Form>
      </Modal>
    </Space>
  );
}
