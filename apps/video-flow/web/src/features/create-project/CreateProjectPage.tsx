import { useMutation, useQuery } from "@tanstack/react-query";
import { Alert, Button, Col, Form, Input, Row, Select, Space, Typography } from "antd";
import { useEffect, useMemo, useState } from "react";
import { api, type MaterialEntry } from "@/lib/api/client";

const FALLBACK_MATERIAL_IDS = ["realistic", "3d_pixar", "anime", "stop_motion", "minecraft", "oil_painting"] as const;
const DRAFT_KEY = "flowkit:create-project:draft:v3";
const NEXT_MATERIAL_KEY = "flowkit:create-project:next-material";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

export type CreateProjectSuccess = {
  projectId: string;
  sceneHints: [];
};

type Props = {
  onSuccess?: (payload: CreateProjectSuccess) => void;
  onCancel?: () => void;
};

export function CreateProjectPage({ onSuccess, onCancel }: Props) {
  const [name, setName] = useState("");
  const [story, setStory] = useState("");
  const [material, setMaterial] = useState<string>(FALLBACK_MATERIAL_IDS[0]);
  const [language, setLanguage] = useState("vi");
  const [aiPrompt, setAiPrompt] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [okMsg, setOkMsg] = useState<string | null>(null);

  // Load draft on mount
  useEffect(() => {
    try {
      const raw = localStorage.getItem(DRAFT_KEY);
      if (raw) {
        const d = JSON.parse(raw) as Record<string, string>;
        if (d.name)     setName(d.name);
        if (d.story)    setStory(d.story);
        if (d.material) setMaterial(d.material);
        if (d.language) setLanguage(d.language);
        if (d.aiPrompt) setAiPrompt(d.aiPrompt);
      }
    } catch { /* ignore */ }
  }, []);

  const materialsQ = useQuery({
    queryKey: ["materials"],
    queryFn: () => api.listMaterials(),
    staleTime: 120_000,
  });

  const materialOptions = useMemo(() => {
    const rows = materialsQ.data?.materials;
    if (rows?.length) return rows.map((m: MaterialEntry) => ({ label: `${m.name} (${m.id})`, value: m.id }));
    return FALLBACK_MATERIAL_IDS.map((id) => ({ label: id, value: id }));
  }, [materialsQ.data]);

  const materialIds = useMemo(
    () => materialsQ.data?.materials?.map((m) => m.id) ?? [...FALLBACK_MATERIAL_IDS],
    [materialsQ.data]
  );

  // Apply next-material preference from Settings
  useEffect(() => {
    const preferred = localStorage.getItem(NEXT_MATERIAL_KEY)?.trim();
    if (preferred && materialIds.includes(preferred)) {
      setMaterial(preferred);
      localStorage.removeItem(NEXT_MATERIAL_KEY);
    }
  }, [materialIds]);

  // AI suggest → autofill name, story, material, language
  const suggestM = useMutation({
    mutationFn: () => api.suggestProject({ prompt: aiPrompt.trim() }),
    onSuccess: (data) => {
      const s = data.suggestion;
      if (s.name)     setName(s.name);
      if (s.story)    setStory(s.story);
      const mat = s.material?.trim();
      if (mat && materialIds.includes(mat)) setMaterial(mat);
      const lang = s.language?.trim().toLowerCase();
      if (lang === "vi" || lang === "en") setLanguage(lang);
      setOkMsg(`Gợi ý AI (${data.provider}): đã điền thông tin project.`);
      setErr(null);
    },
    onError: (e: Error) => { setErr(e.message); setOkMsg(null); },
  });

  // Create project
  const createM = useMutation({
    mutationFn: () =>
      api.createProject({
        name: name.trim(),
        story: story.trim() || null,
        story_original: story.trim() || null,
        material,
        language,
      }),
    onSuccess: (row) => {
      localStorage.removeItem(DRAFT_KEY);
      setErr(null);
      onSuccess?.({ projectId: str(row.id), sceneHints: [] });
    },
    onError: (e: Error) => { setErr(e.message); setOkMsg(null); },
  });

  const saveDraft = () => {
    localStorage.setItem(DRAFT_KEY, JSON.stringify({ name, story, material, language, aiPrompt }));
    setOkMsg("Đã lưu nháp.");
  };

  return (
    <div className="layout" style={{ maxWidth: 680 }}>
      <Space direction="vertical" size={20} style={{ width: "100%" }}>
        <Space style={{ width: "100%", justifyContent: "space-between" }}>
          <div>
            <Typography.Title level={3} style={{ margin: 0 }}>Tạo project mới</Typography.Title>
            <Typography.Text type="secondary">Điền thông tin cơ bản — Characters và Scenes quản lý trong Project Detail.</Typography.Text>
          </div>
          {onCancel && <Button onClick={onCancel}>Quay lại</Button>}
        </Space>

        {err   && <Alert type="error"   message={err}   showIcon closable onClose={() => setErr(null)} />}
        {okMsg && <Alert type="success" message={okMsg} showIcon closable onClose={() => setOkMsg(null)} />}

        {/* AI Autofill */}
        <Form layout="vertical">
          <Form.Item
            label="Gợi ý bằng AI (tùy chọn)"
            extra="Mô tả ý tưởng — AI sẽ gợi ý tên, story, material, ngôn ngữ."
          >
            <Space.Compact style={{ width: "100%" }}>
              <Input
                value={aiPrompt}
                onChange={(e) => setAiPrompt(e.target.value)}
                placeholder="Một đoạn phim ngắn về chiến binh trong tương lai..."
                onPressEnter={() => aiPrompt.trim() && suggestM.mutate()}
              />
              <Button
                type="default"
                loading={suggestM.isPending}
                disabled={!aiPrompt.trim()}
                onClick={() => suggestM.mutate()}
              >
                Gợi ý
              </Button>
            </Space.Compact>
          </Form.Item>

          <Row gutter={12}>
            <Col span={24}>
              <Form.Item label="Tên project" required>
                <Input
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="Tên project..."
                />
              </Form.Item>
            </Col>
          </Row>

          <Row gutter={12}>
            <Col span={14}>
              <Form.Item label="Material (phong cách hình ảnh)">
                <Select
                  value={material}
                  onChange={setMaterial}
                  options={materialOptions}
                  loading={materialsQ.isPending}
                  showSearch
                  optionFilterProp="label"
                />
              </Form.Item>
            </Col>
            <Col span={10}>
              <Form.Item label="Ngôn ngữ">
                <Select value={language} onChange={setLanguage}>
                  <Select.Option value="vi">Tiếng Việt (vi)</Select.Option>
                  <Select.Option value="en">English (en)</Select.Option>
                  <Select.Option value="ja">日本語 (ja)</Select.Option>
                  <Select.Option value="ko">한국어 (ko)</Select.Option>
                  <Select.Option value="zh">中文 (zh)</Select.Option>
                </Select>
              </Form.Item>
            </Col>
          </Row>

          <Form.Item
            label="Story (tùy chọn)"
            extra="Bối cảnh và mạch nội dung. Sẽ được dùng trong Pipeline để phân tích script."
          >
            <Input.TextArea
              value={story}
              onChange={(e) => setStory(e.target.value)}
              autoSize={{ minRows: 4, maxRows: 10 }}
              placeholder="Mô tả bối cảnh, nhân vật, và mạch câu chuyện..."
            />
          </Form.Item>

          <Space>
            <Button
              type="primary"
              size="large"
              loading={createM.isPending}
              disabled={!name.trim()}
              onClick={() => createM.mutate()}
            >
              Tạo Project
            </Button>
            <Button onClick={saveDraft}>Lưu nháp</Button>
          </Space>
        </Form>

        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          Sau khi tạo: thêm Characters trong tab Characters · tạo Video và chọn Manual hoặc Smart Pipeline trong tab Videos.
        </Typography.Text>
      </Space>
    </div>
  );
}
