import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Alert, Button, Card, Input, Modal, Select, Space, Typography } from "antd";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  type AISceneHint,
  type ProjectRow,
  type SceneRow,
  type VideoRow,
} from "@/lib/api/client";
import type { OpenPipelineProp } from "@/features/content/contentTypes";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

function characterNamesToCsv(names: unknown): string {
  if (names == null) return "";
  if (Array.isArray(names)) {
    return names.map((x) => String(x ?? "").trim()).filter(Boolean).join(", ");
  }
  const s = str(names).trim();
  if (s.startsWith("[")) {
    try {
      const parsed = JSON.parse(s);
      if (Array.isArray(parsed)) return parsed.map((x) => String(x ?? "").trim()).filter(Boolean).join(", ");
    } catch {}
  }
  return s;
}

function csvToNames(csv: string): string[] {
  return csv
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

type SceneDraft = {
  prompt: string;
  video_prompt: string;
  image_prompt: string;
  transition_prompt: string;
  narrator_text: string;
  character_names_csv: string;
  chain_type: "ROOT" | "CONTINUATION";
  display_order: number;
};

type NewSceneForm = {
  prompt: string;
  image_prompt: string;
  video_prompt: string;
  transition_prompt: string;
  narrator_text: string;
  character_names_csv: string;
  chain_type: "ROOT" | "CONTINUATION";
};

function rowToDraft(row: SceneRow, fallbackOrder: number): SceneDraft {
  return {
    prompt: str(row.prompt),
    video_prompt: str(row.video_prompt),
    image_prompt: str(row.image_prompt),
    transition_prompt: str(row.transition_prompt),
    narrator_text: str(row.narrator_text),
    character_names_csv: characterNamesToCsv(row.character_names),
    chain_type:
      str(row.chain_type).toUpperCase() === "ROOT" ? "ROOT" : "CONTINUATION",
    display_order: Number(row.display_order ?? fallbackOrder),
  };
}

export function ScenesPage({ onOpenPipeline }: OpenPipelineProp = {}) {
  const qc = useQueryClient();
  const [projectId, setProjectId] = useState("");
  const [videoId, setVideoId] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [okMsg, setOkMsg] = useState<string | null>(null);

  const [editingSceneId, setEditingSceneId] = useState<string | null>(null);
  const [draft, setDraft] = useState<SceneDraft | null>(null);

  const [showAddSceneModal, setShowAddSceneModal] = useState(false);
  const [showAIDialog, setShowAIDialog] = useState(false);
  const [aiScenePrompt, setAiScenePrompt] = useState("");
  const [aiStoryPrompt, setAiStoryPrompt] = useState("");
  const [aiCharactersHint, setAiCharactersHint] = useState("");
  const [aiHints, setAiHints] = useState<AISceneHint[]>([]);
  const [selectedAiHintIndexes, setSelectedAiHintIndexes] = useState<Set<number>>(new Set());

  const resetNewScene = useCallback(() => {
    setNewScene({
      prompt: "",
      image_prompt: "",
      video_prompt: "",
      transition_prompt: "",
      narrator_text: "",
      character_names_csv: "",
      chain_type: "CONTINUATION" as "ROOT" | "CONTINUATION",
    });
  }, []);

const [newScene, setNewScene] = useState<NewSceneForm>({
    prompt: "",
    image_prompt: "",
    video_prompt: "",
    transition_prompt: "",
    narrator_text: "",
    character_names_csv: "",
    chain_type: "CONTINUATION" as "ROOT" | "CONTINUATION",
  });

  const projectsQ = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
  });

  const videosQ = useQuery({
    queryKey: ["videos", projectId],
    queryFn: () => api.listVideos(projectId),
    enabled: !!projectId.trim(),
  });

  const scenesQ = useQuery({
    queryKey: ["scenes", videoId],
    queryFn: () => api.listScenes(videoId),
    enabled: !!videoId.trim(),
  });
  const entitiesQ = useQuery({
    queryKey: ["project-characters", projectId],
    queryFn: () => api.listProjectCharacters(projectId),
    enabled: !!projectId.trim(),
  });

  const sortedVideos = useMemo(() => {
    const rows = (videosQ.data ?? []) as VideoRow[];
    return [...rows].sort(
      (a, b) =>
        Number(a.display_order ?? 0) - Number(b.display_order ?? 0)
    );
  }, [videosQ.data]);

  const sortedScenes = useMemo(() => {
    const rows = (scenesQ.data ?? []) as SceneRow[];
    return [...rows].sort(
      (a, b) =>
        Number(a.display_order ?? 0) - Number(b.display_order ?? 0)
    );
  }, [scenesQ.data]);

  useEffect(() => {
    if (!projectId.trim()) {
      setVideoId("");
      return;
    }
    if (videosQ.isLoading || !videosQ.isFetched) return;
    if (sortedVideos.length === 0) {
      setVideoId("");
      return;
    }
    const ok = sortedVideos.some((v) => str(v.id) === videoId);
    if (!videoId.trim() || !ok) {
      setVideoId(str(sortedVideos[0].id));
    }
  }, [
    projectId,
    videoId,
    sortedVideos,
    videosQ.isLoading,
    videosQ.isFetched,
  ]);

  const startEditScene = useCallback(
    (row: SceneRow, index: number) => {
      const id = str(row.id);
      setEditingSceneId(id);
      setDraft(rowToDraft(row, index));
      setErr(null);
      setOkMsg(null);
    },
    []
  );

  const patchSceneM = useMutation({
    mutationFn: (args: { sid: string; body: SceneDraft }) => {
      const names = args.body.character_names_csv
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      return api.patchScene(args.sid, {
        prompt: args.body.prompt.trim(),
        video_prompt: args.body.video_prompt.trim() || null,
        image_prompt: args.body.image_prompt.trim() || null,
        character_names: names.length ? names : null,
        transition_prompt: args.body.transition_prompt.trim() || null,
        narrator_text: args.body.narrator_text.trim() || null,
        chain_type: args.body.chain_type,
        display_order: args.body.display_order,
      });
    },
    onSuccess: () => {
      setErr(null);
      setOkMsg("Đã cập nhật scene.");
      setEditingSceneId(null);
      setDraft(null);
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => {
      setOkMsg(null);
      setErr(e.message);
    },
  });

  const deleteSceneM = useMutation({
    mutationFn: (sid: string) => api.deleteScene(sid),
    onSuccess: () => {
      setErr(null);
      setOkMsg("Đã xóa scene.");
      setEditingSceneId(null);
      setDraft(null);
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => {
      setOkMsg(null);
      setErr(e.message);
    },
  });

  const createSceneM = useMutation({
    mutationFn: () => {
      const order = sortedScenes.length;
      const names = newScene.character_names_csv
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      const chain =
        order === 0 ? "ROOT" : newScene.chain_type;
      return api.createScene({
        video_id: videoId,
        display_order: order,
        prompt: newScene.prompt.trim(),
        image_prompt: newScene.image_prompt.trim() || null,
        video_prompt: newScene.video_prompt.trim() || null,
        transition_prompt: newScene.transition_prompt.trim() || null,
        narrator_text: newScene.narrator_text.trim() || null,
        character_names: names.length ? names : null,
        chain_type: chain,
      });
    },
    onSuccess: () => {
      setErr(null);
      setOkMsg("Đã thêm scene.");
      resetNewScene();
      setShowAddSceneModal(false);
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => {
      setOkMsg(null);
      setErr(e.message);
    },
  });

  const suggestSceneM = useMutation({
    mutationFn: () =>
      api.suggestScenes({
        prompt: aiScenePrompt.trim() || undefined,
        story: aiStoryPrompt.trim() || undefined,
        characters_hint: aiCharactersHint.trim() || undefined,
        project_id: projectId.trim() || undefined,
      }),
    onSuccess: (data) => {
      const hints = data.scene_hints ?? [];
      if (!hints.length) {
        setOkMsg(null);
        setErr("AI chưa trả về scene nào. Hãy thử prompt cụ thể hơn.");
        return;
      }
      setErr(null);
      setAiHints(hints);
      setSelectedAiHintIndexes(new Set(hints.map((_, i) => i)));
      setOkMsg(`AI đã gợi ý ${hints.length} scene. Chọn các scene muốn thêm.`);
    },
    onError: (e: Error) => {
      setOkMsg(null);
      setErr(e.message);
    },
  });

  const bulkCreateScenesM = useMutation({
    mutationFn: async (hints: AISceneHint[]) => {
      for (let i = 0; i < hints.length; i++) {
        const hint = hints[i];
        const order = sortedScenes.length + i;
        const names = (hint.character_names ?? []).filter(Boolean);
        const chain = order === 0 ? "ROOT" : "CONTINUATION";
        await api.createScene({
          video_id: videoId,
          display_order: order,
          prompt: str(hint.prompt).trim(),
          image_prompt: null,
          video_prompt: str(hint.video_prompt).trim() || null,
          transition_prompt: null,
          narrator_text: null,
          character_names: names.length ? names : null,
          chain_type: chain,
        });
      }
    },
    onSuccess: (_data, hints) => {
      setErr(null);
      setOkMsg(`Đã thêm ${hints.length} scene từ AI.`);
      setShowAIDialog(false);
      setAiHints([]);
      setSelectedAiHintIndexes(new Set());
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => {
      setOkMsg(null);
      setErr(e.message);
    },
  });

  const projectRows = (projectsQ.data ?? []) as ProjectRow[];
  const entityOptions = useMemo(
    () =>
      ((entitiesQ.data ?? []) as Array<Record<string, unknown>>)
        .map((e) => str(e.name).trim())
        .filter(Boolean)
        .map((name) => ({ value: name, label: name })),
    [entitiesQ.data]
  );

  return (
    <div className="layout layout-wide">
      <Space direction="vertical" size={16} style={{ width: "100%" }}>
        <Typography.Title level={3} style={{ margin: 0 }}>
          Scenes
        </Typography.Title>

        <Card>
          <Space wrap align="end">
            <div style={{ minWidth: 320 }}>
              <Typography.Text strong>Project</Typography.Text>
              <Select
                style={{ width: "100%" }}
                value={projectId || undefined}
                placeholder="— Chọn project —"
                onChange={(v) => {
                  setProjectId(v);
                  setEditingSceneId(null);
                  setDraft(null);
                  setErr(null);
                  setOkMsg(null);
                }}
                options={projectRows.map((p) => ({
                  value: str(p.id),
                  label: str(p.name) || str(p.id),
                }))}
              />
            </div>
            {projectId.trim() && onOpenPipeline && (
              <Button
                onClick={() =>
                  onOpenPipeline(
                    projectId.trim(),
                    videoId.trim() ? { videoId: videoId.trim() } : undefined
                  )
                }
              >
                Mở trong Pipeline
              </Button>
            )}
          </Space>
        </Card>

        {err && <Alert type="error" message={err} showIcon />}
        {okMsg && <Alert type="success" message={okMsg} showIcon />}

      {projectId.trim() && (
        <>
          <Card title="Video">
            {!sortedVideos.length && !videosQ.isLoading && (
              <p className="sub">
                Project chưa có video — tạo trong Pipeline hoặc API{" "}
                <code className="mono">POST /api/videos</code>.
              </p>
            )}
            {sortedVideos.length > 0 && (
              <div style={{ maxWidth: 420 }}>
                <Select
                  style={{ width: "100%" }}
                  value={videoId}
                  onChange={(v) => {
                    setVideoId(v);
                    setEditingSceneId(null);
                    setDraft(null);
                  }}
                  options={sortedVideos.map((v) => ({
                    value: str(v.id),
                    label: `${str(v.title || v.id)} · ${str(v.orientation)}`,
                  }))}
                />
              </div>
            )}
          </Card>

          {!!videoId.trim() && (
            <>
              <Card>
                <h2 style={{ marginTop: 0 }}>
                  Danh sách scene ({sortedScenes.length})
                </h2>
                {scenesQ.isLoading && <p className="sub">Đang tải scenes…</p>}
                {!scenesQ.isLoading && sortedScenes.length === 0 && (
                  <p className="sub">Chưa có scene — thêm ở form bên dưới.</p>
                )}
                {sortedScenes.length > 0 && (
                  <div className="table-wrap">
                    <table>
                      <thead>
                        <tr>
                          <th>#</th>
                          <th>id</th>
                          <th>chain</th>
                          <th>prompt (rút gọn)</th>
                          <th />
                        </tr>
                      </thead>
                      <tbody>
                        {sortedScenes.map((s, i) => {
                          const id = str(s.id);
                          const open = editingSceneId === id;
                          return (
                            <tr key={id}>
                              <td>{i}</td>
                              <td className="mono">{id.slice(0, 8)}…</td>
                              <td>{str(s.chain_type)}</td>
                              <td style={{ maxWidth: 280 }}>
                                {open && draft ? (
                                  <div
                                    style={{
                                      display: "grid",
                                      gap: "0.5rem",
                                      minWidth: 260,
                                    }}
                                  >
                                    <div className="field" style={{ margin: 0 }}>
                                      <label>prompt</label>
                                      <textarea
                                        value={draft.prompt}
                                        onChange={(e) =>
                                          setDraft((d) =>
                                            d
                                              ? {
                                                  ...d,
                                                  prompt: e.target.value,
                                                }
                                              : d
                                          )
                                        }
                                        style={{ minHeight: 56 }}
                                      />
                                    </div>
                                    <div className="field" style={{ margin: 0 }}>
                                      <label>video_prompt</label>
                                      <textarea
                                        value={draft.video_prompt}
                                        onChange={(e) =>
                                          setDraft((d) =>
                                            d
                                              ? {
                                                  ...d,
                                                  video_prompt: e.target.value,
                                                }
                                              : d
                                          )
                                        }
                                        style={{ minHeight: 44 }}
                                      />
                                    </div>
                                    <div className="field" style={{ margin: 0 }}>
                                      <label>image_prompt</label>
                                      <textarea
                                        value={draft.image_prompt}
                                        onChange={(e) =>
                                          setDraft((d) =>
                                            d
                                              ? {
                                                  ...d,
                                                  image_prompt: e.target.value,
                                                }
                                              : d
                                          )
                                        }
                                        style={{ minHeight: 44 }}
                                      />
                                    </div>
                                    <div className="field" style={{ margin: 0 }}>
                                      <label>transition_prompt</label>
                                      <textarea
                                        value={draft.transition_prompt}
                                        onChange={(e) =>
                                          setDraft((d) =>
                                            d
                                              ? {
                                                  ...d,
                                                  transition_prompt: e.target.value,
                                                }
                                              : d
                                          )
                                        }
                                        style={{ minHeight: 44 }}
                                      />
                                    </div>
                                    <div className="field" style={{ margin: 0 }}>
                                      <label>narrator_text</label>
                                      <textarea
                                        value={draft.narrator_text}
                                        onChange={(e) =>
                                          setDraft((d) =>
                                            d
                                              ? {
                                                  ...d,
                                                  narrator_text: e.target.value,
                                                }
                                              : d
                                          )
                                        }
                                        style={{ minHeight: 44 }}
                                      />
                                    </div>
                                    <div className="field" style={{ margin: 0 }}>
                                      <label>character_names (CSV)</label>
                                      <input
                                        value={draft.character_names_csv}
                                        onChange={(e) =>
                                          setDraft((d) =>
                                            d
                                              ? {
                                                  ...d,
                                                  character_names_csv:
                                                    e.target.value,
                                                }
                                              : d
                                          )
                                        }
                                      />
                                    </div>
                                    <div className="row">
                                      <div className="field">
                                        <label>chain_type</label>
                                        <select
                                          value={draft.chain_type}
                                          onChange={(e) =>
                                            setDraft((d) =>
                                              d
                                                ? {
                                                    ...d,
                                                    chain_type: e.target
                                                      .value as SceneDraft["chain_type"],
                                                  }
                                                : d
                                            )
                                          }
                                        >
                                          <option value="ROOT">ROOT</option>
                                          <option value="CONTINUATION">
                                            CONTINUATION
                                          </option>
                                        </select>
                                      </div>
                                      <div className="field">
                                        <label>display_order</label>
                                        <input
                                          type="number"
                                          value={draft.display_order}
                                          onChange={(e) =>
                                            setDraft((d) =>
                                              d
                                                ? {
                                                    ...d,
                                                    display_order: Number(
                                                      e.target.value
                                                    ),
                                                  }
                                                : d
                                            )
                                          }
                                        />
                                      </div>
                                    </div>
                                    <div>
                                      <button
                                        type="button"
                                        className="btn secondary small"
                                        onClick={() => {
                                          setEditingSceneId(null);
                                          setDraft(null);
                                        }}
                                      >
                                        Huỷ
                                      </button>
                                      <button
                                        type="button"
                                        className="btn small"
                                        disabled={
                                          patchSceneM.isPending ||
                                          !draft.prompt.trim()
                                        }
                                        onClick={() =>
                                          patchSceneM.mutate({
                                            sid: id,
                                            body: draft,
                                          })
                                        }
                                      >
                                        Lưu
                                      </button>
                                    </div>
                                  </div>
                                ) : (
                                  <span title={str(s.prompt)}>
                                    {str(s.prompt).slice(0, 72)}
                                    {str(s.prompt).length > 72 ? "…" : ""}
                                  </span>
                                )}
                              </td>
                              <td style={{ whiteSpace: "nowrap" }}>
                                {!open && (
                                  <>
                                    <button
                                      type="button"
                                      className="btn secondary small"
                                      onClick={() => startEditScene(s, i)}
                                    >
                                      Sửa
                                    </button>
                                    <button
                                      type="button"
                                      className="btn secondary small"
                                      disabled={deleteSceneM.isPending}
                                      onClick={() => {
                                        if (
                                          confirm(
                                            `Xóa scene ${id.slice(0, 8)}…?`
                                          )
                                        ) {
                                          deleteSceneM.mutate(id);
                                        }
                                      }}
                                    >
                                      Xóa
                                    </button>
                                  </>
                                )}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                )}
                <Space style={{ marginTop: 12 }}>
                  <Button
                    onClick={() => {
                      setErr(null);
                      setOkMsg(null);
                      setAiHints([]);
                      setSelectedAiHintIndexes(new Set());
                      setShowAIDialog(true);
                    }}
                  >
                    AI Generate Scene
                  </Button>
                  <Button
                    type="primary"
                    onClick={() => {
                      setErr(null);
                      setOkMsg(null);
                      if (!newScene.prompt.trim()) resetNewScene();
                      setShowAddSceneModal(true);
                    }}
                  >
                    Thêm scene
                  </Button>
                </Space>
              </Card>
            </>
          )}
        </>
      )}
      <Modal
        title="Tạo scene bằng AI"
        open={showAIDialog}
        onCancel={() => setShowAIDialog(false)}
        footer={
          <Space>
            <Button onClick={() => setShowAIDialog(false)}>Đóng</Button>
            <Button
              loading={suggestSceneM.isPending}
              disabled={
                !projectId.trim() ||
                (!aiScenePrompt.trim() && !aiStoryPrompt.trim() && !aiCharactersHint.trim())
              }
              onClick={() => suggestSceneM.mutate()}
            >
              Generate
            </Button>
            <Button
              type="primary"
              disabled={selectedAiHintIndexes.size === 0 || !videoId.trim()}
              loading={bulkCreateScenesM.isPending}
              onClick={() => {
                const selectedHints = Array.from(selectedAiHintIndexes)
                  .sort((a, b) => a - b)
                  .map((i) => aiHints[i]);
                bulkCreateScenesM.mutate(selectedHints);
              }}
            >
              Thêm {selectedAiHintIndexes.size} scene đã chọn
            </Button>
          </Space>
        }
      >
        <Space direction="vertical" size={10} style={{ width: "100%" }}>
          <div className="field" style={{ margin: 0 }}>
            <label>Hướng dẫn cho AI (Prompt scene)</label>
            <Input.TextArea
              value={aiScenePrompt}
              onChange={(e) => setAiScenePrompt(e.target.value)}
              placeholder="Ví dụ: Chuỗi 6 shot mở đầu..."
              autoSize={{ minRows: 3, maxRows: 6 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>Story / Context (tuỳ chọn)</label>
            <Input.TextArea
              value={aiStoryPrompt}
              onChange={(e) => setAiStoryPrompt(e.target.value)}
              placeholder="Mô tả bối cảnh tổng thể để AI bám mạch"
              autoSize={{ minRows: 2, maxRows: 5 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>character_names hint (CSV, tuỳ chọn)</label>
            <Input
              value={aiCharactersHint}
              onChange={(e) => setAiCharactersHint(e.target.value)}
              placeholder="Luna, Candy Planet Surface"
            />
          </div>
          {aiHints.length > 0 && (
            <>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <label style={{ fontWeight: 500 }}>
                  Chọn scene AI để thêm ({selectedAiHintIndexes.size}/{aiHints.length})
                </label>
                <Space>
                  <Button
                    size="small"
                    onClick={() => setSelectedAiHintIndexes(new Set(aiHints.map((_, i) => i)))}
                  >
                    Chọn tất cả
                  </Button>
                  <Button
                    size="small"
                    onClick={() => setSelectedAiHintIndexes(new Set())}
                  >
                    Bỏ chọn
                  </Button>
                </Space>
              </div>
              {aiHints.map((hint, i) => {
                const checked = selectedAiHintIndexes.has(i);
                return (
                  <Card
                    key={i}
                    size="small"
                    style={{
                      cursor: "pointer",
                      border: checked ? "1.5px solid #4096ff" : "1px solid var(--border)",
                      background: checked ? "rgba(64, 150, 255, 0.06)" : undefined,
                    }}
                    onClick={() => {
                      setSelectedAiHintIndexes((prev) => {
                        const next = new Set(prev);
                        if (next.has(i)) next.delete(i);
                        else next.add(i);
                        return next;
                      });
                    }}
                  >
                    <Space align="start">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => {}}
                        style={{ marginTop: 2, flexShrink: 0 }}
                      />
                      <div>
                        <div style={{ fontWeight: 500, marginBottom: 2 }}>Scene #{i + 1}</div>
                        <div className="sub" style={{ marginBottom: 4 }}>
                          <strong>Prompt:</strong> {str(hint.prompt).slice(0, 120)}{str(hint.prompt).length > 120 ? "…" : ""}
                        </div>
                        {str(hint.video_prompt) && (
                          <div className="sub" style={{ marginBottom: 4 }}>
                            <strong>Video:</strong> {str(hint.video_prompt).slice(0, 80)}{str(hint.video_prompt).length > 80 ? "…" : ""}
                          </div>
                        )}
                        {(hint.character_names ?? []).length > 0 && (
                          <div className="sub">
                            <strong>Characters:</strong> {(hint.character_names ?? []).join(", ")}
                          </div>
                        )}
                      </div>
                    </Space>
                  </Card>
                );
              })}
            </>
          )}
        </Space>
      </Modal>
      <Modal
        title="Thêm scene"
        open={showAddSceneModal}
        onCancel={() => setShowAddSceneModal(false)}
        onOk={() => createSceneM.mutate()}
        okText="Thêm scene"
        confirmLoading={createSceneM.isPending}
        okButtonProps={{ disabled: !newScene.prompt.trim() }}
      >
        <p className="sub" style={{ marginTop: 0 }}>
          Scene đầu tiên dùng chain <strong>ROOT</strong> tự động; từ scene thứ hai dùng lựa
          chọn bên dưới.
        </p>
        <Space direction="vertical" size={10} style={{ width: "100%" }}>
          <div className="field" style={{ margin: 0 }}>
            <label>Prompt ảnh (action)</label>
            <Input.TextArea
              value={newScene.prompt}
              onChange={(e) =>
                setNewScene((x) => ({ ...x, prompt: e.target.value }))
              }
              autoSize={{ minRows: 3, maxRows: 6 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>Image prompt (tuỳ chọn)</label>
            <Input.TextArea
              value={newScene.image_prompt}
              onChange={(e) =>
                setNewScene((x) => ({ ...x, image_prompt: e.target.value }))
              }
              autoSize={{ minRows: 2, maxRows: 5 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>Video prompt (sub-clip, tuỳ chọn)</label>
            <Input.TextArea
              value={newScene.video_prompt}
              onChange={(e) =>
                setNewScene((x) => ({ ...x, video_prompt: e.target.value }))
              }
              autoSize={{ minRows: 2, maxRows: 5 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>Transition prompt (tuỳ chọn)</label>
            <Input.TextArea
              value={newScene.transition_prompt}
              onChange={(e) =>
                setNewScene((x) => ({ ...x, transition_prompt: e.target.value }))
              }
              autoSize={{ minRows: 2, maxRows: 5 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>Narrator text (tuỳ chọn)</label>
            <Input.TextArea
              value={newScene.narrator_text}
              onChange={(e) =>
                setNewScene((x) => ({ ...x, narrator_text: e.target.value }))
              }
              autoSize={{ minRows: 2, maxRows: 4 }}
            />
          </div>
          <div className="field" style={{ margin: 0 }}>
            <label>character_names (chọn từ Entities)</label>
            <Select
              mode="multiple"
              allowClear
              showSearch
              value={csvToNames(newScene.character_names_csv)}
              options={entityOptions}
              onChange={(vals) =>
                setNewScene((x) => ({
                  ...x,
                  character_names_csv: (vals as string[]).join(", "),
                }))
              }
              placeholder="Chọn entity trong project"
            />
          </div>
          {sortedScenes.length > 0 && (
            <div className="field" style={{ margin: 0 }}>
              <label>Chain (scene mới)</label>
              <Select
                value={newScene.chain_type}
                onChange={(v) =>
                  setNewScene((x) => ({
                    ...x,
                    chain_type: v as "ROOT" | "CONTINUATION",
                  }))
                }
                options={[
                  { value: "CONTINUATION", label: "CONTINUATION" },
                  { value: "ROOT", label: "ROOT" },
                ]}
              />
            </div>
          )}
        </Space>
      </Modal>
      </Space>
    </div>
  );
}
