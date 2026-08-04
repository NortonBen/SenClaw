import { useCallback, useEffect, useRef, useState } from "react";
import { Header } from "./components/Header";
import { SettingsModal } from "./components/SettingsModal";
import { ProjectBar } from "./components/ProjectBar";
import { ControlsPanel } from "./components/ControlsPanel";
import { ResultsPanel } from "./components/ResultsPanel";
import { HistoryPanel } from "./components/HistoryPanel";
import { ExportPanel } from "./components/ExportPanel";
import {
  api,
  type Character,
  type CloneConfig,
  type Job,
  type Presets,
  type Project,
} from "./lib/api";
import { useDashboardWS } from "./lib/ws";

export default function App() {
  const [presets, setPresets] = useState<Presets | null>(null);
  const [settings, setSettings] = useState({
    has_api_key: false,
    api_key_from_env: false,
    default_model: "gemini-3-flash-preview",
  });
  const [settingsOpen, setSettingsOpen] = useState(false);

  const [projects, setProjects] = useState<Project[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [project, setProject] = useState<Project | null>(null);
  const [characters, setCharacters] = useState<Character[]>([]);
  const [scenesText, setScenesText] = useState("");
  const [sceneCount, setSceneCount] = useState(0);
  const [running, setRunning] = useState(false);
  const [job, setJob] = useState<Job | null>(null);

  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /// Bumped whenever the scene list changes, so the history panel refetches.
  const [sceneVersion, setSceneVersion] = useState(0);

  const [youtube, setYoutube] = useState<{ available: boolean | null; hint: string }>({
    available: null,
    hint: "",
  });
  const [importing, setImporting] = useState(false);
  const [importMessage, setImportMessage] = useState<string | null>(null);
  const activeImportId = useRef<number | null>(null);

  const refreshSettings = useCallback(async () => {
    try {
      setSettings(await api.settings());
    } catch {
      /* status banner already covers this */
    }
  }, []);

  const refreshProjects = useCallback(async () => {
    try {
      const { projects } = await api.listProjects();
      setProjects(projects);
      setActiveId((current) => {
        if (current && projects.some((p) => p.id === current)) return current;
        return projects[0]?.id ?? null;
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  const refreshActive = useCallback(async (id: number) => {
    try {
      const [detail, scenes] = await Promise.all([api.getProject(id), api.scenes(id)]);
      setProject(detail.project);
      setSceneCount(detail.scene_count);
      setRunning(detail.running);
      setJob(detail.latest_job);
      setCharacters(scenes.characters);
      setScenesText(scenes.text);
      setSceneVersion((v) => v + 1);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, []);

  useEffect(() => {
    void refreshSettings();
    void refreshProjects();
    api.presets().then(setPresets).catch(() => setPresets(null));
    api
      .youtubeAvailable()
      .then((r) => setYoutube({ available: r.available, hint: r.install_hint ?? "" }))
      .catch(() => setYoutube({ available: false, hint: "brew install yt-dlp" }));
  }, [refreshSettings, refreshProjects]);

  // Reflect a download's progress, and adopt the new project when it lands.
  const onImportProgress = useCallback(
    (data: { id?: number; status?: string; message?: string; project_id?: number | null }) => {
      if (activeImportId.current == null || data.id !== activeImportId.current) return;
      setImportMessage(data.message ?? null);
      if (data.status === "completed") {
        setImporting(false);
        activeImportId.current = null;
        void refreshProjects();
        if (typeof data.project_id === "number") setActiveId(data.project_id);
      } else if (data.status === "failed") {
        setImporting(false);
        activeImportId.current = null;
        setError(data.message ?? "Tải video thất bại");
      }
    },
    [refreshProjects],
  );

  useEffect(() => {
    if (activeId == null) {
      setProject(null);
      setScenesText("");
      setSceneCount(0);
      setCharacters([]);
      setJob(null);
      setRunning(false);
      return;
    }
    void refreshActive(activeId);
  }, [activeId, refreshActive]);

  // Live updates. The analysis worker pushes here when a run finishes, so the
  // UI does not have to poll aggressively.
  const activeRef = useRef<number | null>(null);
  activeRef.current = activeId;

  useDashboardWS((event) => {
    if (event.type === "youtube:progress") {
      onImportProgress(event.data as Parameters<typeof onImportProgress>[0]);
      return;
    }
    const projectId = event.data?.project_id as number | undefined;
    if (event.type === "project:created" || event.type === "project:deleted") {
      void refreshProjects();
      return;
    }
    if (projectId != null && projectId === activeRef.current) {
      void refreshActive(projectId);
    }
    if (event.type === "job:completed" || event.type === "job:failed") {
      void refreshProjects();
    }
  });

  // Slow safety net: if the socket drops mid-run the poll still notices the end.
  useEffect(() => {
    if (!running || activeId == null) return;
    const timer = window.setInterval(() => void refreshActive(activeId), 15000);
    return () => window.clearInterval(timer);
  }, [running, activeId, refreshActive]);

  const upload = async (file: File) => {
    setUploading(true);
    setError(null);
    try {
      const form = new FormData();
      form.append("video", file);
      form.append("name", file.name);
      form.append("model", settings.default_model);
      const { project } = await api.createProject(form);
      await refreshProjects();
      setActiveId(project.id);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setUploading(false);
    }
  };

  const importYoutube = async (url: string) => {
    setError(null);
    setImporting(true);
    setImportMessage("đang lấy thông tin video");
    try {
      const { import_id } = await api.youtubeImport({ url });
      activeImportId.current = import_id;
    } catch (e) {
      setImporting(false);
      activeImportId.current = null;
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // Fallback poll: if the WS drops mid-download, still notice the outcome.
  useEffect(() => {
    if (!importing || activeImportId.current == null) return;
    const timer = window.setInterval(async () => {
      const id = activeImportId.current;
      if (id == null) return;
      try {
        onImportProgress(await api.youtubeImportStatus(id));
      } catch {
        /* transient; keep polling */
      }
    }, 4000);
    return () => window.clearInterval(timer);
  }, [importing, onImportProgress]);

  const analyze = async (mode: "start" | "continue" | "regenerate", cfg: CloneConfig) => {
    if (activeId == null) return;
    if (!settings.has_api_key) {
      setSettingsOpen(true);
      return;
    }
    setError(null);
    try {
      await api.analyze(activeId, { ...cfg, mode });
      setRunning(true);
      void refreshActive(activeId);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="min-h-screen flex flex-col bg-slate-950 text-slate-200">
      <Header hasApiKey={settings.has_api_key} onOpenSettings={() => setSettingsOpen(true)} />

      <main className="flex-grow max-w-7xl mx-auto w-full px-6 py-8">
        {!settings.has_api_key && (
          <div className="mb-6 p-4 bg-amber-950/20 border border-amber-500/30 rounded-2xl flex items-center justify-between gap-4">
            <p className="text-[11px] text-amber-300 font-bold leading-relaxed">
              Chưa có Gemini API key. Video Cloner gọi thẳng Gemini để gửi kèm file video, nên cần
              key riêng trước khi phân tích được.
            </p>
            <button
              onClick={() => setSettingsOpen(true)}
              className="shrink-0 bg-amber-500 hover:bg-amber-400 text-slate-950 px-4 py-2 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all"
            >
              Nhập key
            </button>
          </div>
        )}

        {error && (
          <div className="mb-6 p-4 bg-red-950/30 border border-red-500/30 rounded-2xl text-red-400 text-[11px] font-bold flex items-center justify-between gap-4">
            <span className="leading-relaxed">{error}</span>
            <button
              onClick={() => setError(null)}
              className="shrink-0 text-red-500/60 hover:text-red-300"
              aria-label="Đóng lỗi"
            >
              ✕
            </button>
          </div>
        )}

        <div className="grid grid-cols-1 lg:grid-cols-12 gap-8">
          <div className="lg:col-span-4 space-y-6">
            <ProjectBar
              projects={projects}
              activeId={activeId}
              uploading={uploading}
              importing={importing}
              importMessage={importMessage}
              youtubeAvailable={youtube.available}
              youtubeHint={youtube.hint}
              onSelect={setActiveId}
              onUpload={(f) => void upload(f)}
              onImportYoutube={(u) => void importYoutube(u)}
              onDeleted={() => void refreshProjects()}
            />

            {project && (
              <ControlsPanel
                project={project}
                presets={presets}
                sceneCount={sceneCount}
                running={running}
                onAnalyze={(mode, cfg) => void analyze(mode, cfg)}
                onChanged={() => activeId != null && void refreshActive(activeId)}
              />
            )}

            {project && <ExportPanel projectId={project.id} sceneCount={sceneCount} />}

            {project && (
              <HistoryPanel
                projectId={project.id}
                running={running}
                version={sceneVersion}
                onRestored={() => activeId != null && void refreshActive(activeId)}
              />
            )}

            <div className="bg-slate-900/50 border border-slate-800 p-6 rounded-3xl">
              <h3 className="text-indigo-400 font-black text-[10px] uppercase tracking-widest mb-2 italic">
                Thông tin công cụ
              </h3>
              <p className="text-[11px] text-slate-500 leading-relaxed">
                Hệ thống chia video thành các phân đoạn 8 giây để phân tích chi tiết nhân vật, bối
                cảnh, hành động và phong cách hình ảnh.
                <br />
                <br />
                Mỗi lần chạy sinh ra một đoạn; dùng{" "}
                <span className="text-white">Tạo tiếp</span> để phân tích 8 giây kế tiếp. Kết quả
                được lưu lại nên đóng trình duyệt cũng không mất.
              </p>
            </div>
          </div>

          <div className="lg:col-span-8 flex flex-col">
            {project ? (
              <ResultsPanel
                project={project}
                scenesText={scenesText}
                sceneCount={sceneCount}
                characters={characters}
                running={running}
                job={job}
                onChanged={() => activeId != null && void refreshActive(activeId)}
                onContinue={() =>
                  void analyze("continue", {
                    style: project.style,
                    model: project.model,
                  })
                }
              />
            ) : (
              <div className="bg-slate-900 rounded-3xl border border-slate-800 shadow-2xl flex flex-col items-center justify-center min-h-[600px] text-slate-700 space-y-4">
                <div className="p-8 bg-slate-950 rounded-full border border-slate-900">
                  <svg
                    className="w-16 h-16 opacity-10"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      strokeLinecap="round"
                      strokeLinejoin="round"
                      strokeWidth="1"
                      d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z"
                    />
                  </svg>
                </div>
                <p className="text-[10px] font-black uppercase tracking-widest opacity-20">
                  Tải một video lên để bắt đầu
                </p>
              </div>
            )}
          </div>
        </div>
      </main>

      <footer className="py-6 text-center border-t border-slate-900 bg-slate-950">
        <p className="text-[9px] font-black text-slate-700 uppercase tracking-[0.4em]">
          Engineered for Content Re-creation • SenClaw Space App
        </p>
      </footer>

      {settingsOpen && (
        <SettingsModal
          presets={presets}
          hasApiKey={settings.has_api_key}
          apiKeyFromEnv={settings.api_key_from_env}
          defaultModel={settings.default_model}
          onClose={() => setSettingsOpen(false)}
          onSaved={() => void refreshSettings()}
        />
      )}
    </div>
  );
}
