import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Col,
  Empty,
  Form,
  Input,
  Modal,
  Popconfirm,
  Row,
  Select,
  Space,
  Table,
  Tabs,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import {
  ClearOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  PictureOutlined,
  PlayCircleOutlined,
  PlusOutlined,
  RobotOutlined,
  SyncOutlined,
  ThunderboltOutlined,
  UserOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  type AISceneHint,
  type BatchStatus,
  type CharacterRow,
  type CreativePlanResponse,
  type ProjectRow,
  type SceneRow,
  type VideoRow,
} from "@/lib/api/client";
import { EntitiesPanel } from "@/features/pipeline/EntitiesPanel";
import { EntitySceneFlow } from "@/features/pipeline/EntitySceneFlow";

const { Text, Title } = Typography;
const { TextArea } = Input;

export type PipelineAppProps = {
  initialProjectId?: string;
  initialSceneHints?: AISceneHint[];
  initialVideoId?: string;
  openNonce?: number;
  onOpenIntentConsumed?: () => void;
};

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

function csvToNames(csv: string): string[] {
  return csv.split(",").map((s) => s.trim()).filter(Boolean);
}

function parseCharacterNames(raw: unknown): string[] {
  if (Array.isArray(raw)) return raw.map((x) => str(x).trim()).filter(Boolean);
  const s = str(raw).trim();
  if (s.startsWith("[")) {
    try {
      const parsed = JSON.parse(s);
      if (Array.isArray(parsed)) return parsed.map((x) => str(x).trim()).filter(Boolean);
    } catch {}
  }
  return s.split(",").map((x) => x.trim()).filter(Boolean);
}

function sceneImageURL(row: SceneRow, orientation: "VERTICAL" | "HORIZONTAL"): string {
  return str(row[orientation === "HORIZONTAL" ? "horizontal_image_url" : "vertical_image_url"]);
}

function sceneVideoURL(row: SceneRow, orientation: "VERTICAL" | "HORIZONTAL"): string {
  return str(row[orientation === "HORIZONTAL" ? "horizontal_video_url" : "vertical_video_url"]);
}

function sceneImageMediaID(row: SceneRow, orientation: "VERTICAL" | "HORIZONTAL"): string {
  return str(row[orientation === "HORIZONTAL" ? "horizontal_image_media_id" : "vertical_image_media_id"]);
}

function sceneVideoMediaID(row: SceneRow, orientation: "VERTICAL" | "HORIZONTAL"): string {
  return str(row[orientation === "HORIZONTAL" ? "horizontal_video_media_id" : "vertical_video_media_id"]);
}

function sceneImageStatus(row: SceneRow, orientation: "VERTICAL" | "HORIZONTAL"): string {
  return str(row[orientation === "HORIZONTAL" ? "horizontal_image_status" : "vertical_image_status"]) || "PENDING";
}

function sceneVideoStatus(row: SceneRow, orientation: "VERTICAL" | "HORIZONTAL"): string {
  return str(row[orientation === "HORIZONTAL" ? "horizontal_video_status" : "vertical_video_status"]) || "PENDING";
}

const STATUS_COLOR: Record<string, "default" | "processing" | "success" | "error"> = {
  PENDING: "default",
  PROCESSING: "processing",
  COMPLETED: "success",
  FAILED: "error",
};

export function PipelineApp({
  initialProjectId = "",
  initialSceneHints,
  initialVideoId = "",
  openNonce = 0,
  onOpenIntentConsumed,
}: PipelineAppProps = {}) {
  const qc = useQueryClient();

  const [err, setErr] = useState<string | null>(null);
  const [okMsg, setOkMsg] = useState<string | null>(null);

  const [projectId, setProjectId] = useState("");
  const [videoId, setVideoId] = useState("");
  const [videoTitle, setVideoTitle] = useState("Video 1");
  const [orientation, setOrientation] = useState<"VERTICAL" | "HORIZONTAL">("VERTICAL");

  const [scenePrompt, setScenePrompt] = useState("");
  const [sceneVideoPrompt, setSceneVideoPrompt] = useState("");
  const [sceneTransitionPrompt, setSceneTransitionPrompt] = useState("");
  const [sceneNarratorText, setSceneNarratorText] = useState("");
  const [sceneAiPrompt, setSceneAiPrompt] = useState("");
  const [sceneCharNames, setSceneCharNames] = useState("");
  const [sceneChain, setSceneChain] = useState<"ROOT" | "CONTINUATION">("ROOT");
  const [showAddSceneModal, setShowAddSceneModal] = useState(false);
  const [showAISceneModal, setShowAISceneModal] = useState(false);
  const [selectedAiHintIndexes, setSelectedAiHintIndexes] = useState<Set<number>>(new Set());
  const [editingSceneId, setEditingSceneId] = useState("");
  const [editScenePrompt, setEditScenePrompt] = useState("");
  const [editSceneVideoPrompt, setEditSceneVideoPrompt] = useState("");
  const [editSceneTransitionPrompt, setEditSceneTransitionPrompt] = useState("");
  const [editSceneNarratorText, setEditSceneNarratorText] = useState("");
  const [editSceneCharNames, setEditSceneCharNames] = useState("");
  const [editSceneChain, setEditSceneChain] = useState<"ROOT" | "CONTINUATION">("CONTINUATION");
  const [aiSceneHints, setAiSceneHints] = useState<AISceneHint[]>([]);
  const [pollType, setPollType] = useState<string | null>(null);
  const [previewMedia, setPreviewMedia] = useState<{ url: string; type: "image" | "video" } | null>(null);

  useEffect(() => { setAiSceneHints(initialSceneHints ?? []); }, [initialSceneHints]);

  // ---------- queries ----------

  const projectsQ = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
    staleTime: 30_000,
  });

  const materialsQ = useQuery({
    queryKey: ["materials"],
    queryFn: () => api.listMaterials(),
    staleTime: 120_000,
  });

  const projectRowQ = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => api.getProject(projectId),
    enabled: !!projectId.trim(),
  });

  const [projName, setProjName] = useState("");
  const [projStory, setProjStory] = useState("");

  useEffect(() => {
    const r = projectRowQ.data as Record<string, unknown> | undefined;
    if (!r) return;
    setProjName(str(r.name));
    setProjStory(str(r.story));
  }, [projectRowQ.data]);

  const [hydrateRev, setHydrateRev] = useState(0);

  useEffect(() => {
    const id = initialProjectId?.trim();
    if (!id) return;
    setProjectId(id);
    setOkMsg(null);
    setErr(null);
    setHydrateRev((r) => r + 1);
  }, [initialProjectId, openNonce]);

  const videosQ = useQuery({
    queryKey: ["videos", projectId],
    queryFn: () => api.listVideos(projectId),
    enabled: !!projectId.trim(),
  });

  const sortedProjectVideos = useMemo(() => {
    const rows = (videosQ.data ?? []) as VideoRow[];
    return [...rows].sort((a, b) => Number(a.display_order ?? 0) - Number(b.display_order ?? 0));
  }, [videosQ.data]);

  const applyVideoRow = useCallback((v: VideoRow) => {
    setVideoId(str(v.id));
    setVideoTitle(str(v.title) || "Video");
    const o = str(v.orientation).toUpperCase();
    if (o === "VERTICAL" || o === "HORIZONTAL") setOrientation(o as "VERTICAL" | "HORIZONTAL");
  }, []);

  useEffect(() => {
    if (hydrateRev === 0) return;
    const id = initialProjectId?.trim();
    if (!id || projectId !== id) return;
    if (videosQ.isLoading || !videosQ.isFetched) return;
    const sorted = sortedProjectVideos;
    if (sorted.length > 0) {
      const want = initialVideoId?.trim();
      const pick = want && sorted.some((v) => str(v.id) === want)
        ? sorted.find((v) => str(v.id) === want)!
        : sorted[0];
      applyVideoRow(pick);
      setOkMsg(`Đã mở project — video «${str(pick.title || pick.id)}» (${sorted.length} video).`);
    } else {
      setVideoId("");
    }
    setHydrateRev(0);
    onOpenIntentConsumed?.();
  }, [hydrateRev, initialProjectId, initialVideoId, projectId, videosQ.isLoading, videosQ.isFetched, sortedProjectVideos, applyVideoRow, onOpenIntentConsumed]);

  const healthQ = useQuery({
    queryKey: ["health"],
    queryFn: () => api.health(),
    refetchInterval: 15_000,
  });

  const pendingGlobalQ = useQuery({
    queryKey: ["requests-pending-global"],
    queryFn: () => api.listPendingRequests(),
    staleTime: 4000,
    refetchInterval: 8000,
  });

  const charactersQ = useQuery({
    queryKey: ["characters", projectId],
    queryFn: () => api.listProjectCharacters(projectId),
    enabled: !!projectId,
  });

  const projectEntityOptions = useMemo(
    () => ((charactersQ.data ?? []) as CharacterRow[])
      .map((c) => str(c.name).trim())
      .filter(Boolean)
      .map((name) => ({ value: name, label: name })),
    [charactersQ.data]
  );

  const scenesQ = useQuery({
    queryKey: ["scenes", videoId],
    queryFn: () => api.listScenes(videoId),
    enabled: !!videoId,
    refetchInterval: 3500,
  });

  const requestsQ = useQuery({
    queryKey: ["requests", "project", projectId],
    queryFn: () => api.listRequests({ project_id: projectId }),
    enabled: !!projectId.trim(),
    refetchInterval: 5000,
  });

  const batchPollQ = useQuery({
    queryKey: ["batch-status", videoId, projectId, pollType, orientation],
    queryFn: () => {
      const charOnly = pollType === "GENERATE_CHARACTER_IMAGE";
      return api.batchStatus({
        video_id: charOnly ? undefined : videoId || undefined,
        project_id: projectId || undefined,
        type: pollType ?? undefined,
        orientation,
      });
    },
    enabled: !!pollType && !!projectId && (pollType === "GENERATE_CHARACTER_IMAGE" || !!videoId),
    refetchInterval: (q) => {
      const d = q.state.data as BatchStatus | undefined;
      if (!d || d.done) return false;
      return 2000;
    },
  });

  // ---------- mutations ----------

  const ensureVideoM = useMutation({
    mutationFn: () => api.createVideo({ project_id: projectId, title: "Video 1", orientation, display_order: 0 }),
    onSuccess: (row) => {
      setErr(null);
      applyVideoRow(row as VideoRow);
      setOkMsg("Đã tạo video mặc định.");
      void qc.invalidateQueries({ queryKey: ["videos", projectId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const suggestScenesM = useMutation({
    mutationFn: async () => {
      const nameList = ((charactersQ.data ?? []) as CharacterRow[]).map((c) => str(c.name)).filter((x) => x.trim() !== "");
      const csvExtra = sceneCharNames.split(",").map((s) => s.trim()).filter(Boolean);
      const merged = [...new Set([...nameList, ...csvExtra])];
      const characters_hint = merged.length ? merged.join(", ") : undefined;
      const p = sceneAiPrompt.trim();
      const st = projStory.trim();
      if (!p && !st) throw new Error("Nhập prompt AI hoặc điền story cho project.");
      return api.suggestScenes({ prompt: p || undefined, story: st || undefined, characters_hint, project_id: projectId.trim() || undefined });
    },
    onSuccess: (data) => {
      const hints = [...(data.scene_hints ?? [])].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
      setAiSceneHints(hints);
      setSelectedAiHintIndexes(new Set(hints.map((_, i) => i)));
      setErr(null);
      setOkMsg(`AI gợi ý ${hints.length} scene.`);
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const createSceneM = useMutation({
    mutationFn: () => {
      const order = scenesQ.data?.length ?? 0;
      const names = sceneCharNames.split(",").map((s) => s.trim()).filter(Boolean);
      return api.createScene({
        video_id: videoId,
        display_order: order,
        prompt: scenePrompt.trim(),
        video_prompt: sceneVideoPrompt.trim() || null,
        transition_prompt: sceneTransitionPrompt.trim() || null,
        narrator_text: sceneNarratorText.trim() || null,
        character_names: names.length ? names : null,
        chain_type: order === 0 ? "ROOT" : sceneChain,
      });
    },
    onSuccess: () => {
      setErr(null);
      setOkMsg("Đã thêm scene.");
      setScenePrompt(""); setSceneVideoPrompt(""); setSceneTransitionPrompt("");
      setSceneNarratorText(""); setSceneCharNames(""); setShowAddSceneModal(false);
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const bulkCreateScenesM = useMutation({
    mutationFn: async (hints: AISceneHint[]) => {
      const baseOrder = scenesQ.data?.length ?? 0;
      for (let i = 0; i < hints.length; i++) {
        const hint = hints[i];
        const order = baseOrder + i;
        const names = (hint.character_names ?? []).filter(Boolean);
        await api.createScene({
          video_id: videoId,
          display_order: order,
          prompt: str(hint.prompt).trim(),
          video_prompt: str(hint.video_prompt).trim() || null,
          transition_prompt: null,
          narrator_text: null,
          character_names: names.length ? names : null,
          chain_type: order === 0 ? "ROOT" : "CONTINUATION",
        });
      }
    },
    onSuccess: (_data, hints) => {
      setErr(null);
      setOkMsg(`Đã thêm ${hints.length} scene từ AI.`);
      setShowAISceneModal(false);
      setAiSceneHints([]);
      setSelectedAiHintIndexes(new Set());
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const patchSceneM = useMutation({
    mutationFn: (sid: string) => {
      const names = csvToNames(editSceneCharNames);
      return api.patchScene(sid, {
        prompt: editScenePrompt.trim(),
        video_prompt: editSceneVideoPrompt.trim() || null,
        transition_prompt: editSceneTransitionPrompt.trim() || null,
        narrator_text: editSceneNarratorText.trim() || null,
        character_names: names.length ? names : null,
        chain_type: editSceneChain,
      });
    },
    onSuccess: () => {
      setErr(null); setOkMsg("Đã cập nhật scene."); setEditingSceneId("");
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const deleteSceneM = useMutation({
    mutationFn: (sid: string) => api.deleteScene(sid),
    onSuccess: () => {
      setErr(null); setOkMsg("Đã xóa scene.");
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const creativeMixM = useMutation({
    mutationFn: async (sceneId: string) => {
      const plan: CreativePlanResponse = await api.planCreativeBreakdown(sceneId, { style: "cinematic_action", max_inserts: 4, pacing: "medium" });
      return api.applyCreativeBreakdown(sceneId, { plan: { root_scene_id: plan.root_scene_id, inserts: plan.inserts }, execution: { create_scenes: true, generate_images: true, generate_videos: true, auto_fix_closeup_anchor: true } });
    },
    onSuccess: (data) => {
      setErr(null);
      const created = data.created_scenes?.length ?? 0;
      setOkMsg(`Creative Mix: thêm ${created} scene, queue ${data.requests?.length ?? 0} request.`);
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const linkEntityToSceneM = useMutation({
    mutationFn: async (args: { sceneId: string; entityName: string; unlink?: boolean }) => {
      const current = ((scenesQ.data ?? []) as SceneRow[]).find((x) => str(x.id) === args.sceneId);
      if (!current) throw new Error("Không tìm thấy scene.");
      const rawNames = current.character_names;
      const names = parseCharacterNames(rawNames);
      const hasName = names.includes(args.entityName);
      const next = args.unlink ? names.filter((x) => x !== args.entityName) : hasName ? names : [...names, args.entityName];
      await api.patchScene(args.sceneId, { character_names: next.length ? next : null });
      return { linked: !args.unlink && !hasName, unlinked: !!args.unlink && hasName };
    },
    onSuccess: (result) => {
      setErr(null);
      setOkMsg(result.linked ? "Đã liên kết entity vào scene." : result.unlinked ? "Đã gỡ liên kết." : "Không thay đổi.");
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const batchRefsM = useMutation({
    mutationFn: async (): Promise<{ skipped: boolean; queued: number }> => {
      const chars = (charactersQ.data ?? []) as CharacterRow[];
      if (!chars.length) throw new Error("Chưa có entity.");
      const missing = chars.filter((c) => !str(c.media_id).trim());
      if (!missing.length) return { skipped: true, queued: 0 };
      const reqs = missing.map((c) => ({ type: "GENERATE_CHARACTER_IMAGE", character_id: str(c.id), project_id: projectId, orientation }));
      await api.createRequestBatch(reqs);
      return { skipped: false, queued: reqs.length };
    },
    onSuccess: (result) => {
      setErr(null);
      if (result.skipped) { setOkMsg("Mọi entity đã có media_id."); return; }
      setOkMsg(`Đã gửi batch ảnh ref cho ${result.queued} entity.`);
      setPollType("GENERATE_CHARACTER_IMAGE");
      void qc.invalidateQueries({ queryKey: ["batch-status"] });
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests-pending-global"] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const batchImagesM = useMutation({
    mutationFn: async () => {
      const initialScenes = (scenesQ.data ?? []) as SceneRow[];
      if (!initialScenes.length) throw new Error("Chưa có scene.");
      const statusKey = orientation === "HORIZONTAL" ? "horizontal_image_status" : "vertical_image_status";
      const mediaKey = orientation === "HORIZONTAL" ? "horizontal_image_media_id" : "vertical_image_media_id";
      const isImageReady = (row: SceneRow) => str(row[statusKey]) === "COMPLETED" && str(row[mediaKey]).trim() !== "";
      const waitTypeDone = async (type: string) => {
        for (let i = 0; i < 120; i++) {
          const st = await api.batchStatus({ video_id: videoId, project_id: projectId, type, orientation });
          if (st.done) return;
          await new Promise((resolve) => setTimeout(resolve, 2000));
        }
        throw new Error(`Timeout cho ${type}.`);
      };
      let scenes = initialScenes;
      let queuedTotal = 0;
      let waveCount = 0;
      const doneSceneIDs = new Set(scenes.filter((s) => isImageReady(s)).map((s) => str(s.id)));
      const allIDs = new Set(scenes.map((s) => str(s.id)));
      while (doneSceneIDs.size < allIDs.size) {
        const prevDone = doneSceneIDs.size;
        const waveReqs: Array<{ type: "GENERATE_IMAGE" | "EDIT_IMAGE"; scene_id: string; project_id: string; video_id: string; orientation: "VERTICAL" | "HORIZONTAL" }> = [];
        for (const s of scenes) {
          const sid = str(s.id);
          if (!sid || doneSceneIDs.has(sid)) continue;
          // The parent link is the source of truth, not `chain_type`: the
          // parser marks every non-first scene CONTINUATION but never fills
          // `parent_scene_id`, and stale ids survive a re-parse. So a scene is a
          // real continuation only when its parent still exists in this set;
          // otherwise (no link, or a dangling one) generate it fresh instead of
          // waiting forever on a parent that will never complete.
          const parentID = str(s.parent_scene_id).trim();
          const hasParent = !!parentID && allIDs.has(parentID);
          if (hasParent) {
            if (!doneSceneIDs.has(parentID)) continue; // wait for the parent's image
            waveReqs.push({ type: "EDIT_IMAGE", scene_id: sid, project_id: projectId, video_id: videoId, orientation });
          } else {
            waveReqs.push({ type: "GENERATE_IMAGE", scene_id: sid, project_id: projectId, video_id: videoId, orientation });
          }
        }
        if (!waveReqs.length) {
          // Every remaining scene points at a parent that never became ready —
          // a cycle or a failed parent. Name them instead of a vague hint.
          const stuck = scenes.filter((s) => !doneSceneIDs.has(str(s.id))).map((s) => str(s.display_order)).join(", ");
          throw new Error(`Còn cảnh chờ ảnh của cảnh cha chưa xong (cảnh: ${stuck}). Gen ảnh cho cảnh cha trước, hoặc bỏ liên kết parent.`);
        }
        const inserted = await api.createRequestBatch(waveReqs);
        queuedTotal += inserted.length;
        waveCount++;
        const types = new Set(waveReqs.map((r) => r.type));
        for (const type of types) await waitTypeDone(type);
        scenes = (await api.listScenes(videoId)) as SceneRow[];
        for (const s of scenes) { if (isImageReady(s)) doneSceneIDs.add(str(s.id)); }
        // A wave ran but nothing new became ready → the queued scenes all
        // failed. Stop instead of re-queuing the same failures forever.
        if (doneSceneIDs.size === prevDone) {
          const failed = scenes.filter((s) => !doneSceneIDs.has(str(s.id))).map((s) => str(s.display_order)).join(", ");
          throw new Error(`Gen ảnh thất bại ở cảnh: ${failed}. Kiểm tra extension/quota rồi thử lại.`);
        }
      }
      return { queuedTotal, waveCount };
    },
    onSuccess: ({ queuedTotal, waveCount }) => {
      setErr(null);
      setOkMsg(`Gen ảnh xong: ${waveCount} wave, ${queuedTotal} request.`);
      setPollType("GENERATE_IMAGE");
      void qc.invalidateQueries({ queryKey: ["batch-status"] });
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests-pending-global"] });
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  // Flow no longer returns video URLs from the generation API, so a rendered
  // clip can sit there with no link. This asks the extension to scrape the
  // Flow project page, then pulls the assets local.
  const fetchUrlsM = useMutation({
    mutationFn: () => api.fetchMediaUrls(projectId),
    onSuccess: (r) => {
      setErr(null);
      setOkMsg(
        r.scenes_still_without_url > 0
          ? `Đã lấy ${r.downloaded} link — còn ${r.scenes_still_without_url} cảnh chưa có (mở Google Flow trong extension rồi thử lại).`
          : `Đã lấy và tải về ${r.downloaded} file.`
      );
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const queueSceneImageM = useMutation({
    mutationFn: (args: { sceneId: string; reqType: "GENERATE_IMAGE" | "REGENERATE_IMAGE" }) =>
      api.createRequestBatch([{ type: args.reqType, scene_id: args.sceneId, project_id: projectId, video_id: videoId, orientation }]),
    onSuccess: (_, vars) => {
      setErr(null);
      setOkMsg(vars.reqType === "REGENERATE_IMAGE" ? "Đã gửi Gen lại ảnh scene." : "Đã gửi Gen ảnh scene.");
      setPollType(vars.reqType);
      void qc.invalidateQueries({ queryKey: ["batch-status"] });
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests-pending-global"] });
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const queueSceneVideoM = useMutation({
    mutationFn: (args: { sceneId: string; reqType: "GENERATE_VIDEO" | "REGENERATE_VIDEO" }) =>
      api.createRequestBatch([{ type: args.reqType, scene_id: args.sceneId, project_id: projectId, video_id: videoId, orientation }]),
    onSuccess: (_, vars) => {
      setErr(null);
      setOkMsg(vars.reqType === "REGENERATE_VIDEO" ? "Đã gửi Gen lại video scene." : "Đã gửi Gen video scene.");
      setPollType(vars.reqType);
      void qc.invalidateQueries({ queryKey: ["batch-status"] });
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests-pending-global"] });
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const batchVideosM = useMutation({
    mutationFn: async () => {
      const scenes = (scenesQ.data ?? []) as SceneRow[];
      const reqs = scenes.map((s) => ({ type: "GENERATE_VIDEO", scene_id: str(s.id), project_id: projectId, video_id: videoId, orientation }));
      if (!reqs.length) throw new Error("Chưa có scene.");
      return api.createRequestBatch(reqs);
    },
    onSuccess: () => {
      setErr(null);
      setOkMsg("Đã gửi batch video.");
      setPollType("GENERATE_VIDEO");
      void qc.invalidateQueries({ queryKey: ["batch-status"] });
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
      void qc.invalidateQueries({ queryKey: ["requests-pending-global"] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const deleteRequestM = useMutation({
    mutationFn: (id: string) => api.deleteRequest(id),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] }); },
  });

  const clearRequestsM = useMutation({
    mutationFn: () => api.clearRequests(videoId || undefined),
    onSuccess: () => {
      setOkMsg("Đã xóa log requests.");
      void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
    },
    onError: (e: Error) => { setErr(e.message); },
  });

  const importAIScenesM = useMutation({
    mutationFn: async () => {
      if (!videoId) throw new Error("Cần có video.");
      if (!aiSceneHints.length) throw new Error("Chưa có scene gợi ý từ AI.");
      const startOrder = scenesQ.data?.length ?? 0;
      const hints = [...aiSceneHints].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
      const existing = (scenesQ.data ?? []) as SceneRow[];
      let parentID: string | null = existing.length > 0 ? str(existing[existing.length - 1].id) : null;
      for (let i = 0; i < hints.length; i++) {
        const h = hints[i];
        const isRoot = startOrder === 0 && i === 0;
        const cn = h.character_names?.filter((x) => String(x ?? "").trim() !== "");
        const created = await api.createScene({ video_id: videoId, display_order: startOrder + i, prompt: h.prompt, video_prompt: h.video_prompt?.trim() || null, character_names: cn?.length ? cn : null, chain_type: isRoot ? "ROOT" : "CONTINUATION", parent_scene_id: isRoot ? null : parentID });
        parentID = str(created.id);
      }
    },
    onSuccess: () => {
      setErr(null); setOkMsg("Đã tạo scene từ gợi ý AI.");
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  const reviewUpdateScenesM = useMutation({
    mutationFn: async () => {
      const scenes = ((scenesQ.data ?? []) as SceneRow[]).slice().sort((a, b) => Number(a.display_order ?? 0) - Number(b.display_order ?? 0));
      if (!scenes.length) throw new Error("Chưa có scene.");
      const entityNames = ((charactersQ.data ?? []) as CharacterRow[]).map((c) => str(c.name).trim()).filter(Boolean);
      const projectRow = (projectRowQ.data ?? {}) as Record<string, unknown>;
      const projectContext = `PROJECT_NAME=${str(projectRow.name)}\nPROJECT_LANGUAGE=${str(projectRow.language) || "vi"}\nPROJECT_MATERIAL=${str(projectRow.material) || "realistic"}`;
      const storyOriginal = str(projectRow.story_original).trim();
      const storyCurrent = str(projectRow.story).trim() || projStory.trim();
      const fullStoryBlock = ["FULL_STORY_CONTEXT:", storyOriginal ? `STORY_ORIGINAL:\n${storyOriginal}` : "", storyCurrent ? `STORY_CURRENT:\n${storyCurrent}` : ""].filter(Boolean).join("\n\n");
      const sceneContext = scenes.map((s, idx) => [`Scene ${idx + 1}:`, `chain_type=${str(s.chain_type) || "ROOT"}`, `prompt=${str(s.prompt)}`, `video_prompt=${str(s.video_prompt)}`, `character_names=${Array.isArray(s.character_names) ? (s.character_names as unknown[]).map((x) => str(x)).join(", ") : str(s.character_names)}`].join("\n")).join("\n\n");
      const aiPrompt = [`You MUST return exactly ${scenes.length} scene hints in the same story order. Improve prompts for cinematic quality.`, "", projectContext, "", fullStoryBlock, "", "CURRENT_SCENES:", sceneContext].join("\n");
      const res = await api.suggestScenes({ prompt: aiPrompt, story: projStory.trim() || undefined, characters_hint: entityNames.join(", ") || undefined, project_id: projectId.trim() || undefined });
      const hints = [...(res.scene_hints ?? [])].sort((a, b) => (a.order ?? 0) - (b.order ?? 0));
      if (!hints.length) throw new Error("AI không trả về scene_hints.");
      const n = Math.min(scenes.length, hints.length);
      for (let i = 0; i < n; i++) {
        const scene = scenes[i];
        const hint = hints[i];
        const names = (hint.character_names ?? []).map((x) => str(x).trim()).filter(Boolean);
        const videoPrompt = str(hint.video_prompt).trim();
        await api.patchScene(str(scene.id), { prompt: str(hint.prompt).trim() || str(scene.prompt), video_prompt: videoPrompt || null, transition_prompt: videoPrompt || null, narrator_text: str(scene.narrator_text).trim() || (videoPrompt ? `${videoPrompt.split(".")[0]?.trim()}.` : null), character_names: names.length ? names : null });
      }
      return { updated: n };
    },
    onSuccess: ({ updated }) => {
      setErr(null); setOkMsg(`AI Review hoàn tất: đã cập nhật ${updated} scene.`);
      void qc.invalidateQueries({ queryKey: ["scenes", videoId] });
    },
    onError: (e: Error) => { setOkMsg(null); setErr(e.message); },
  });

  // ---------- derived ----------

  const materialIds = useMemo(() => {
    const rows = materialsQ.data?.materials;
    if (rows?.length) return rows.map((m) => m.id);
    return ["realistic", "3d_pixar", "anime", "stop_motion"];
  }, [materialsQ.data]);

  void materialIds; // referenced in case needed later

  const batch = batchPollQ.data;
  const pendingCount = pendingGlobalQ.data?.length ?? 0;
  const extensionConnected = (healthQ.data as { extension_connected?: boolean } | undefined)?.extension_connected;

  const projects = (projectsQ.data ?? []) as ProjectRow[];
  const projectOptions = projects.map((p) => ({ label: String(p.name ?? p.id), value: String(p.id) }));

  // ---------- scene table columns ----------

  const sceneColumns = useMemo(() => [
    {
      title: "#",
      dataIndex: "display_order",
      key: "order",
      width: 36,
      render: (_: unknown, __: unknown, idx: number) => idx,
    },
    {
      title: "Ảnh",
      key: "image",
      width: 84,
      render: (_: unknown, s: SceneRow) => {
        const url = sceneImageURL(s, orientation);
        const status = sceneImageStatus(s, orientation);
        if (url) {
          return (
            <img
              src={url}
              alt="scene"
              onClick={() => setPreviewMedia({ url, type: "image" })}
              style={{ width: 64, height: 64, objectFit: "cover", borderRadius: 6, border: "1px solid var(--border)", cursor: "zoom-in" }}
            />
          );
        }
        return <Tag color={STATUS_COLOR[status] ?? "default"}>{status}</Tag>;
      },
    },
    {
      title: "Video",
      key: "video",
      width: 80,
      render: (_: unknown, s: SceneRow) => {
        const url = sceneVideoURL(s, orientation);
        const status = sceneVideoStatus(s, orientation);
        // Plays straight from whatever URL we have — a local /api/media file or
        // a remote Flow URL. Nothing needs downloading first.
        if (url) {
          return (
            <Button size="small" type="link" icon={<PlayCircleOutlined />}
              onClick={() => setPreviewMedia({ url, type: "video" })}>
              Xem
            </Button>
          );
        }
        // Rendered but linkless: Flow stopped returning video URLs, so offer the
        // one action that can produce one instead of a dead status tag.
        if (status === "COMPLETED") {
          return (
            <Button size="small" type="link" loading={fetchUrlsM.isPending}
              onClick={() => fetchUrlsM.mutate()}>
              Lấy link
            </Button>
          );
        }
        return <Tag color={STATUS_COLOR[status] ?? "default"}>{status}</Tag>;
      },
    },
    {
      title: "Chain",
      dataIndex: "chain_type",
      key: "chain",
      width: 100,
      render: (v: unknown) => <Tag>{str(v) || "ROOT"}</Tag>,
    },
    {
      title: "Prompt",
      key: "prompt",
      render: (_: unknown, s: SceneRow) => (
        <Tooltip title={str(s.prompt)}>
          <Text ellipsis style={{ maxWidth: 260 }}>{str(s.prompt).slice(0, 80)}{str(s.prompt).length > 80 ? "…" : ""}</Text>
        </Tooltip>
      ),
    },
    {
      title: "Hành động",
      key: "actions",
      width: 300,
      render: (_: unknown, s: SceneRow) => (
        <Space size={4} wrap>
          <Button
            size="small"
            icon={<EditOutlined />}
            onClick={() => {
              setEditingSceneId(str(s.id));
              setEditScenePrompt(str(s.prompt));
              setEditSceneVideoPrompt(str(s.video_prompt));
              setEditSceneTransitionPrompt(str(s.transition_prompt));
              setEditSceneNarratorText(str(s.narrator_text));
              setEditSceneCharNames(parseCharacterNames(s.character_names).join(", "));
              setEditSceneChain(str(s.chain_type).toUpperCase() === "ROOT" ? "ROOT" : "CONTINUATION");
            }}
          >
            Sửa
          </Button>
          <Popconfirm title={`Xóa scene ${str(s.id).slice(0, 8)}…?`} onConfirm={() => deleteSceneM.mutate(str(s.id))}>
            <Button size="small" danger icon={<DeleteOutlined />}>Xóa</Button>
          </Popconfirm>
          <Popconfirm title="Chạy Creative Mix cho scene này?" onConfirm={() => creativeMixM.mutate(str(s.id))}>
            <Button size="small" loading={creativeMixM.isPending}>Mix</Button>
          </Popconfirm>
          <Button
            size="small"
            icon={<SyncOutlined />}
            loading={queueSceneImageM.isPending}
            onClick={() => queueSceneImageM.mutate({ sceneId: str(s.id), reqType: sceneImageMediaID(s, orientation) ? "REGENERATE_IMAGE" : "GENERATE_IMAGE" })}
          >
            {sceneImageMediaID(s, orientation) ? "Gen lại" : "Gen ảnh"}
          </Button>
          {/* Video needs a start image, so this is disabled until the image
              exists. Once a clip is rendered the same button regenerates it. */}
          <Tooltip title={sceneImageMediaID(s, orientation) ? "" : "Cần gen ảnh trước"}>
            <Button
              size="small"
              icon={<VideoCameraOutlined />}
              disabled={!sceneImageMediaID(s, orientation)}
              loading={queueSceneVideoM.isPending}
              onClick={() => queueSceneVideoM.mutate({ sceneId: str(s.id), reqType: sceneVideoMediaID(s, orientation) ? "REGENERATE_VIDEO" : "GENERATE_VIDEO" })}
            >
              {sceneVideoMediaID(s, orientation) ? "Gen lại video" : "Gen video"}
            </Button>
          </Tooltip>
        </Space>
      ),
    },
  ], [orientation, deleteSceneM, creativeMixM, queueSceneImageM, queueSceneVideoM]);

  // ---------- render ----------

  const scenes = (scenesQ.data ?? []) as SceneRow[];
  const chars = (charactersQ.data ?? []) as CharacterRow[];

  return (
    <div style={{ maxWidth: 960, margin: "0 auto", padding: "24px 16px 48px" }}>
      {/* Header */}
      <div style={{ marginBottom: 20 }}>
        <Space align="center" style={{ marginBottom: 4 }}>
          <Title level={3} style={{ margin: 0 }}>Manual Studio</Title>
          {healthQ.isSuccess && (
            <Tag color={pendingCount > 0 ? "processing" : "default"}>
              queue: {pendingCount}
            </Tag>
          )}
        </Space>
        <Text type="secondary">Kiểm soát thủ công từng bước: Characters → Scenes → Generate</Text>
      </div>

      {/* Config card */}
      <Card style={{ marginBottom: 16 }}>
        <Form layout="vertical" size="middle">
          <Row gutter={[16, 0]} align="bottom">
            <Col xs={24} sm={12}>
              <Form.Item label="Project" style={{ marginBottom: 0 }}>
                {projectId && !projects.find((p) => String(p.id) === projectId) ? (
                  <Space>
                    <Text strong>{projName || projectId.slice(0, 8) + "…"}</Text>
                    <Text type="secondary" style={{ fontSize: 12 }}>({projectId.slice(0, 8)}…)</Text>
                  </Space>
                ) : (
                  <Select
                    placeholder="Chọn project"
                    options={projectOptions}
                    value={projectId || undefined}
                    onChange={(val) => { setProjectId(val); setVideoId(""); }}
                    loading={projectsQ.isLoading}
                    showSearch
                    filterOption={(inp, opt) => (opt?.label ?? "").toLowerCase().includes(inp.toLowerCase())}
                    style={{ width: "100%" }}
                  />
                )}
              </Form.Item>
            </Col>
            <Col xs={16} sm={8}>
              <Form.Item label="Video" style={{ marginBottom: 0 }}>
                {sortedProjectVideos.length > 1 ? (
                  <Select
                    value={videoId || undefined}
                    placeholder="Chọn video"
                    options={sortedProjectVideos.map((v) => ({ label: `${str(v.title || v.id)} · ${str(v.orientation)}`, value: str(v.id) }))}
                    onChange={(val) => { const row = sortedProjectVideos.find((v) => str(v.id) === val); if (row) applyVideoRow(row); }}
                    style={{ width: "100%" }}
                  />
                ) : videoId ? (
                  <Text strong>{videoTitle} · {orientation}</Text>
                ) : projectId ? (
                  <Button size="small" onClick={() => ensureVideoM.mutate()} loading={ensureVideoM.isPending}>
                    Tạo Video mặc định
                  </Button>
                ) : (
                  <Text type="secondary">—</Text>
                )}
              </Form.Item>
            </Col>
            <Col xs={8} sm={4}>
              <Form.Item label="Orientation" style={{ marginBottom: 0 }}>
                <Select
                  value={orientation}
                  onChange={setOrientation}
                  options={[{ label: "Dọc (9:16)", value: "VERTICAL" }, { label: "Ngang (16:9)", value: "HORIZONTAL" }]}
                />
              </Form.Item>
            </Col>
          </Row>
        </Form>
      </Card>

      {/* Extension warning */}
      {healthQ.isSuccess && extensionConnected === false && (
        <Alert type="warning" message="Extension chưa kết nối WebSocket — worker không xử lý được job tới Google Flow." showIcon style={{ marginBottom: 12 }} />
      )}

      {err && <Alert type="error" message={err} showIcon closable onClose={() => setErr(null)} style={{ marginBottom: 12 }} />}
      {okMsg && <Alert type="success" message={okMsg} showIcon closable onClose={() => setOkMsg(null)} style={{ marginBottom: 12 }} />}

      {!projectId ? (
        <Card>
          <Empty description="Mở từ Projects → Detail → New Video → Manual Studio" />
        </Card>
      ) : (
        <Tabs
          items={[
            {
              key: "characters",
              label: <Space size={4}><UserOutlined />{`Characters (${chars.length})`}</Space>,
              children: (
                <EntitiesPanel
                  projectId={projectId}
                  rows={chars}
                  isLoading={charactersQ.isLoading}
                  orientation={orientation}
                  onGenRefQueued={() => {
                    setPollType("GENERATE_CHARACTER_IMAGE");
                    setOkMsg("Đã gửi gen ảnh tham chiếu.");
                    void qc.invalidateQueries({ queryKey: ["batch-status"] });
                    void qc.invalidateQueries({ queryKey: ["requests", "project", projectId] });
                  }}
                />
              ),
            },
            {
              key: "scenes",
              label: <Space size={4}><PictureOutlined />{`Scenes (${scenes.length})`}</Space>,
              children: (
                <Space direction="vertical" size={16} style={{ width: "100%" }}>
                  {!videoId ? (
                    <Card>
                      <Space>
                        <Text type="secondary">Cần có video để quản lý scenes.</Text>
                        <Button onClick={() => ensureVideoM.mutate()} loading={ensureVideoM.isPending}>
                          Tạo Video mặc định
                        </Button>
                      </Space>
                    </Card>
                  ) : (
                    <>
                      <Card size="small">
                        <Space wrap>
                          <Button icon={<RobotOutlined />} onClick={() => { setAiSceneHints([]); setSelectedAiHintIndexes(new Set()); setShowAISceneModal(true); }}>
                            AI Generate Scenes
                          </Button>
                          <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowAddSceneModal(true)}>
                            Thêm Scene
                          </Button>
                          {aiSceneHints.length > 0 && (
                            <Button
                              onClick={() => importAIScenesM.mutate()}
                              loading={importAIScenesM.isPending}
                            >
                              {`Tạo ${aiSceneHints.length} scene từ AI`}
                            </Button>
                          )}
                          <Button
                            icon={<SyncOutlined />}
                            disabled={scenes.length === 0}
                            loading={reviewUpdateScenesM.isPending}
                            onClick={() => {
                              if (confirm("Chạy AI Review & Update cho toàn bộ scenes?")) {
                                reviewUpdateScenesM.mutate();
                              }
                            }}
                          >
                            AI Review & Update
                          </Button>
                        </Space>
                      </Card>

                      <Table<SceneRow>
                        dataSource={scenes}
                        columns={sceneColumns}
                        rowKey={(r) => str(r.id)}
                        size="small"
                        pagination={false}
                        scroll={{ x: 700 }}
                        locale={{ emptyText: "Chưa có scene. Nhấn Thêm Scene để bắt đầu." }}
                      />

                      {scenes.length > 0 && (
                        <Card size="small" title="Entity ↔ Scene (kéo để liên kết)">
                          <EntitySceneFlow
                            entities={chars}
                            scenes={scenes}
                            orientation={orientation}
                            onConnectEntityScene={(sceneId, entityName) => linkEntityToSceneM.mutate({ sceneId, entityName })}
                            onUnlinkEntityScene={(sceneId, entityName) => linkEntityToSceneM.mutate({ sceneId, entityName, unlink: true })}
                          />
                        </Card>
                      )}
                    </>
                  )}
                </Space>
              ),
            },
            {
              key: "generate",
              label: <Space size={4}><ThunderboltOutlined />Generate</Space>,
              children: (
                <Space direction="vertical" size={16} style={{ width: "100%" }}>
                  {/* Batch actions */}
                  <Card title="Batch Generation" size="small">
                    <Space direction="vertical" size={12} style={{ width: "100%" }}>
                      <Space wrap>
                        <Button
                          icon={<UserOutlined />}
                          disabled={batchRefsM.isPending || !chars.length}
                          loading={batchRefsM.isPending}
                          onClick={() => batchRefsM.mutate()}
                        >
                          Gen ref images (entity thiếu ref)
                        </Button>
                        {videoId && (
                          <>
                            <Button
                              icon={<PictureOutlined />}
                              disabled={batchImagesM.isPending || !scenes.length}
                              loading={batchImagesM.isPending}
                              onClick={() => batchImagesM.mutate()}
                            >
                              Gen scene images
                            </Button>
                            <Button
                              icon={<ThunderboltOutlined />}
                              disabled={batchVideosM.isPending || !scenes.length}
                              loading={batchVideosM.isPending}
                              onClick={() => batchVideosM.mutate()}
                            >
                              Gen videos
                            </Button>
                          </>
                        )}
                        {pollType && (
                          <Button onClick={() => setPollType(null)}>Dừng poll</Button>
                        )}
                      </Space>

                      {!videoId && (
                        <Text type="secondary">Tạo video (tab Scenes) để gen ảnh scene và clip video.</Text>
                      )}

                      {pollType && batch && (
                        <Card size="small" style={{ background: "var(--bg-2, rgba(0,0,0,0.04))" }}>
                          <Text type="secondary" style={{ fontFamily: "var(--mono)", fontSize: 12 }}>
                            {`type=${pollType} · total=${batch.total} · pending=${batch.pending} · processing=${batch.processing} · completed=${batch.completed} · failed=${batch.failed} · done=${String(batch.done)}`}
                          </Text>
                        </Card>
                      )}
                    </Space>
                  </Card>

                  {/* Request log */}
                  <Card
                    size="small"
                    title={
                      <Space>
                        <span>Requests ({requestsQ.data?.length ?? 0})</span>
                        <Popconfirm
                          title="Xóa toàn bộ request log?"
                          description={videoId ? "Chỉ xóa log của video hiện tại." : "Xóa tất cả log trong project."}
                          onConfirm={() => clearRequestsM.mutate()}
                          okText="Xóa"
                          cancelText="Hủy"
                        >
                          <Button size="small" icon={<ClearOutlined />} loading={clearRequestsM.isPending} danger>
                            Xóa log
                          </Button>
                        </Popconfirm>
                      </Space>
                    }
                  >
                    <Table
                      dataSource={(requestsQ.data ?? []).slice(0, 60)}
                      rowKey={(r) => str((r as Record<string, unknown>).id)}
                      size="small"
                      pagination={false}
                      scroll={{ x: 520 }}
                      columns={[
                        {
                          title: "Type", dataIndex: "type", key: "type", width: 190,
                          render: (v: unknown) => {
                            const s = str(v).replace(/^GENERATE_/, "GEN_").replace(/^REGENERATE_/, "REGEN_");
                            return <Text code style={{ fontSize: 10 }}>{s}</Text>;
                          },
                        },
                        {
                          title: "Status", dataIndex: "status", key: "status", width: 110,
                          render: (v: unknown, r: unknown) => {
                            const row = r as Record<string, unknown>;
                            const s = str(v);
                            const color = s === "COMPLETED" ? "success" : s === "FAILED" ? "error" : "processing";
                            const errMsg = str(row.error_message);
                            const tag = <Tag color={color} style={{ fontSize: 10 }}>{s}</Tag>;
                            return errMsg ? <Tooltip title={errMsg}>{tag}</Tooltip> : tag;
                          },
                        },
                        {
                          title: "Target", key: "target", width: 100,
                          render: (_: unknown, r: unknown) => {
                            const row = r as Record<string, unknown>;
                            const sid = str(row.scene_id);
                            const cid = str(row.character_id);
                            if (sid) return <Text code style={{ fontSize: 10 }}>scene:{sid.slice(0, 6)}</Text>;
                            if (cid) return <Text code style={{ fontSize: 10 }}>char:{cid.slice(0, 6)}</Text>;
                            return "—";
                          },
                        },
                        {
                          title: "Output", key: "output", width: 100,
                          render: (_: unknown, r: unknown) => {
                            const row = r as Record<string, unknown>;
                            const url = str(row.output_url);
                            if (str(row.status) !== "COMPLETED" || !url) return "—";
                            const isVideo = url.toLowerCase().match(/\.(mp4|webm|mov)/) || str(row.type).includes("VIDEO");
                            const isImg = !isVideo && (url.toLowerCase().match(/\.(png|jpg|jpeg|webp)/) || url.includes("image") || str(row.type).includes("IMAGE"));
                            if (isImg) {
                              return (
                                <img
                                  src={url}
                                  style={{ width: 44, height: 44, objectFit: "cover", borderRadius: 4, cursor: "pointer" }}
                                  alt="out"
                                  onClick={() => setPreviewMedia({ url, type: "image" })}
                                />
                              );
                            }
                            if (isVideo) {
                              return (
                                <Button
                                  size="small"
                                  icon={<PlayCircleOutlined />}
                                  onClick={() => setPreviewMedia({ url, type: "video" })}
                                >
                                  Xem
                                </Button>
                              );
                            }
                            return <Button size="small" icon={<EyeOutlined />} href={url} target="_blank">Xem</Button>;
                          },
                        },
                        {
                          title: "", key: "actions", width: 40, fixed: "right",
                          render: (_: unknown, r: unknown) => {
                            const row = r as Record<string, unknown>;
                            const id = str(row.id);
                            return (
                              <Popconfirm title="Xóa request này?" onConfirm={() => deleteRequestM.mutate(id)} okText="Xóa" cancelText="Hủy">
                                <Button size="small" type="text" danger icon={<DeleteOutlined />} />
                              </Popconfirm>
                            );
                          },
                        },
                      ]}
                    />
                  </Card>

                  {/* Entity media_id status */}
                  <Card title={`Entities (${chars.length})`} size="small">
                    <Table
                      dataSource={chars}
                      rowKey={(r) => str(r.id)}
                      size="small"
                      pagination={false}
                      columns={[
                        { title: "Tên", dataIndex: "name", key: "name", render: (v: unknown) => str(v) },
                        { title: "media_id", dataIndex: "media_id", key: "mid", render: (v: unknown) => <Text code style={{ fontSize: 11 }}>{str(v) || "—"}</Text> },
                      ]}
                    />
                  </Card>
                </Space>
              ),
            },
          ]}
        />
      )}

      {/* Modal: Media preview */}
      <Modal
        open={!!previewMedia}
        onCancel={() => setPreviewMedia(null)}
        footer={null}
        centered
        width={previewMedia?.type === "video" ? 720 : 600}
        styles={{ body: { padding: 8, textAlign: "center" } }}
        title={previewMedia?.type === "video" ? "Video" : "Ảnh"}
      >
        {previewMedia?.type === "image" && (
          <img src={previewMedia.url} style={{ maxWidth: "100%", maxHeight: "80vh", borderRadius: 6 }} alt="preview" />
        )}
        {previewMedia?.type === "video" && (
          <video src={previewMedia.url} controls autoPlay style={{ maxWidth: "100%", maxHeight: "80vh", borderRadius: 6 }} />
        )}
      </Modal>

      {/* Modal: AI Generate Scenes */}
      <Modal
        title="Tạo scene bằng AI"
        open={showAISceneModal}
        onCancel={() => setShowAISceneModal(false)}
        footer={
          <Space>
            <Button onClick={() => setShowAISceneModal(false)}>Đóng</Button>
            <Button
              loading={suggestScenesM.isPending}
              disabled={!sceneAiPrompt.trim() && !projStory.trim()}
              onClick={() => suggestScenesM.mutate()}
            >
              Generate
            </Button>
            <Button
              type="primary"
              disabled={selectedAiHintIndexes.size === 0 || !videoId.trim()}
              loading={bulkCreateScenesM.isPending}
              onClick={() => {
                const hints = Array.from(selectedAiHintIndexes)
                  .sort((a, b) => a - b)
                  .map((i) => aiSceneHints[i]);
                bulkCreateScenesM.mutate(hints);
              }}
            >
              Thêm {selectedAiHintIndexes.size} scene đã chọn
            </Button>
          </Space>
        }
      >
        <Space direction="vertical" style={{ width: "100%" }}>
          <Form.Item label="Hướng dẫn cho AI" style={{ marginBottom: 0 }}>
            <TextArea
              value={sceneAiPrompt}
              onChange={(e) => setSceneAiPrompt(e.target.value)}
              placeholder="VD: Chuỗi 6 shot mở đầu — chiến binh rời bỏ làng..."
              autoSize={{ minRows: 3, maxRows: 6 }}
            />
          </Form.Item>
          {aiSceneHints.length > 0 && (
            <>
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 8 }}>
                <Text strong>Chọn scene AI để thêm ({selectedAiHintIndexes.size}/{aiSceneHints.length})</Text>
                <Space>
                  <Button size="small" onClick={() => setSelectedAiHintIndexes(new Set(aiSceneHints.map((_, i) => i)))}>Chọn tất cả</Button>
                  <Button size="small" onClick={() => setSelectedAiHintIndexes(new Set())}>Bỏ chọn</Button>
                </Space>
              </div>
              <Space direction="vertical" style={{ width: "100%" }}>
                {aiSceneHints.map((hint, i) => {
                  const checked = selectedAiHintIndexes.has(i);
                  return (
                    <Card
                      key={i}
                      size="small"
                      style={{
                        cursor: "pointer",
                        border: checked ? "1.5px solid #4096ff" : "1px solid #d9d9d9",
                        background: checked ? "rgba(64,150,255,0.06)" : undefined,
                      }}
                      onClick={() => {
                        setSelectedAiHintIndexes((prev) => {
                          const next = new Set(prev);
                          if (next.has(i)) next.delete(i); else next.add(i);
                          return next;
                        });
                      }}
                    >
                      <Space align="start">
                        <input type="checkbox" checked={checked} onChange={() => {}} style={{ marginTop: 3, flexShrink: 0 }} />
                        <div>
                          <Text strong style={{ fontSize: 13 }}>Scene #{i + 1}</Text>
                          <br />
                          <Text type="secondary" style={{ fontSize: 12 }}>
                            <strong>Prompt:</strong> {str(hint.prompt).slice(0, 120)}{str(hint.prompt).length > 120 ? "…" : ""}
                          </Text>
                          {str(hint.video_prompt) && (
                            <>
                              <br />
                              <Text type="secondary" style={{ fontSize: 12 }}>
                                <strong>Video:</strong> {str(hint.video_prompt).slice(0, 80)}{str(hint.video_prompt).length > 80 ? "…" : ""}
                              </Text>
                            </>
                          )}
                          {(hint.character_names ?? []).length > 0 && (
                            <>
                              <br />
                              <Text type="secondary" style={{ fontSize: 12 }}>
                                <strong>Characters:</strong> {(hint.character_names ?? []).join(", ")}
                              </Text>
                            </>
                          )}
                        </div>
                      </Space>
                    </Card>
                  );
                })}
              </Space>
            </>
          )}
        </Space>
      </Modal>

      {/* Modal: Add Scene */}
      <Modal
        title="Thêm scene"
        open={showAddSceneModal}
        onCancel={() => setShowAddSceneModal(false)}
        onOk={() => createSceneM.mutate()}
        confirmLoading={createSceneM.isPending}
        okButtonProps={{ disabled: !scenePrompt.trim() }}
        okText="Thêm scene"
      >
        <Space direction="vertical" style={{ width: "100%", marginTop: 8 }}>
          <Form.Item label="Prompt ảnh" required style={{ marginBottom: 8 }}>
            <TextArea value={scenePrompt} onChange={(e) => setScenePrompt(e.target.value)} autoSize={{ minRows: 3, maxRows: 6 }} />
          </Form.Item>
          <Form.Item label="Video prompt" style={{ marginBottom: 8 }}>
            <TextArea value={sceneVideoPrompt} onChange={(e) => setSceneVideoPrompt(e.target.value)} placeholder="0-3s: … 3-6s: …" autoSize={{ minRows: 2, maxRows: 5 }} />
          </Form.Item>
          <Form.Item label="Transition prompt" style={{ marginBottom: 8 }}>
            <TextArea value={sceneTransitionPrompt} onChange={(e) => setSceneTransitionPrompt(e.target.value)} autoSize={{ minRows: 2, maxRows: 4 }} />
          </Form.Item>
          <Form.Item label="Narrator text" style={{ marginBottom: 8 }}>
            <TextArea value={sceneNarratorText} onChange={(e) => setSceneNarratorText(e.target.value)} autoSize={{ minRows: 2, maxRows: 4 }} />
          </Form.Item>
          <Form.Item label="Characters" style={{ marginBottom: 8 }}>
            <Select mode="multiple" allowClear showSearch value={csvToNames(sceneCharNames)} options={projectEntityOptions} onChange={(vals) => setSceneCharNames((vals as string[]).join(", "))} placeholder="Chọn entity trong project" style={{ width: "100%" }} />
          </Form.Item>
          {scenes.length > 0 && (
            <Form.Item label="Chain" style={{ marginBottom: 0 }}>
              <Select value={sceneChain} onChange={setSceneChain} options={[{ value: "CONTINUATION", label: "CONTINUATION" }, { value: "ROOT", label: "ROOT" }]} />
            </Form.Item>
          )}
        </Space>
      </Modal>

      {/* Modal: Edit Scene */}
      <Modal
        title="Sửa scene"
        open={!!editingSceneId}
        onCancel={() => setEditingSceneId("")}
        onOk={() => patchSceneM.mutate(editingSceneId)}
        confirmLoading={patchSceneM.isPending}
        okButtonProps={{ disabled: !editScenePrompt.trim() }}
        okText="Lưu"
      >
        <Space direction="vertical" style={{ width: "100%", marginTop: 8 }}>
          <Form.Item label="Prompt ảnh" required style={{ marginBottom: 8 }}>
            <TextArea value={editScenePrompt} onChange={(e) => setEditScenePrompt(e.target.value)} autoSize={{ minRows: 3, maxRows: 6 }} />
          </Form.Item>
          <Form.Item label="Video prompt" style={{ marginBottom: 8 }}>
            <TextArea value={editSceneVideoPrompt} onChange={(e) => setEditSceneVideoPrompt(e.target.value)} autoSize={{ minRows: 2, maxRows: 5 }} />
          </Form.Item>
          <Form.Item label="Transition prompt" style={{ marginBottom: 8 }}>
            <TextArea value={editSceneTransitionPrompt} onChange={(e) => setEditSceneTransitionPrompt(e.target.value)} autoSize={{ minRows: 2, maxRows: 4 }} />
          </Form.Item>
          <Form.Item label="Narrator text" style={{ marginBottom: 8 }}>
            <TextArea value={editSceneNarratorText} onChange={(e) => setEditSceneNarratorText(e.target.value)} autoSize={{ minRows: 2, maxRows: 4 }} />
          </Form.Item>
          <Form.Item label="Characters" style={{ marginBottom: 8 }}>
            <Select mode="multiple" allowClear showSearch value={csvToNames(editSceneCharNames)} options={projectEntityOptions} onChange={(vals) => setEditSceneCharNames((vals as string[]).join(", "))} placeholder="Chọn entity" style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item label="Chain" style={{ marginBottom: 0 }}>
            <Select value={editSceneChain} onChange={setEditSceneChain} options={[{ value: "CONTINUATION", label: "CONTINUATION" }, { value: "ROOT", label: "ROOT" }]} />
          </Form.Item>
        </Space>
      </Modal>
    </div>
  );
}
