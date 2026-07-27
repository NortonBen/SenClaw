import { useCallback, useRef, useState } from "react";
import { Navigate, Route, Routes, useNavigate } from "react-router-dom";
import { AppShell } from "@/features/app/AppShell";
import { CharactersPage } from "@/features/content/CharactersPage";
import { ScenesPage } from "@/features/content/ScenesPage";
import {
  CreateProjectPage,
  type CreateProjectSuccess,
} from "@/features/create-project/CreateProjectPage";
import { DagPipelinePage } from "@/features/dag-pipeline/DagPipelinePage";
import { DashboardPage } from "@/features/dashboard/DashboardPage";
import { MediaManagerPage } from "@/features/media-manager/MediaManagerPage";
import { PipelineApp } from "@/features/pipeline/PipelineApp";
import { ProjectDetailPage } from "@/features/projects/ProjectDetailPage";
import { ProjectsPage } from "@/features/projects/ProjectsPage";
import { SettingsPage } from "@/features/settings/SettingsPage";

function AppRoutes() {
  const navigate = useNavigate();

  // --- Manual pipeline state ---
  const [pipelineProjectId, setPipelineProjectId] = useState("");
  const [pipelineInitialVideoId, setPipelineInitialVideoId] = useState("");
  const [pipelineOpenNonce, setPipelineOpenNonce] = useState(0);
  const pipelineProjectIdRef = useRef("");
  pipelineProjectIdRef.current = pipelineProjectId;

  // --- Smart pipeline state ---
  const [dagProjectId, setDagProjectId] = useState("");

  const consumePipelineOpenIntent = useCallback(() => {
    setPipelineInitialVideoId("");
  }, []);

  const goPipeline = useCallback(
    (projectIdArg?: string, opts?: { videoId?: string }) => {
      const vid = opts?.videoId?.trim() ?? "";
      setPipelineInitialVideoId(vid);

      const incoming = projectIdArg?.trim();
      const prev = pipelineProjectIdRef.current;
      const next = incoming || prev;
      const projectChanged = !!(incoming && incoming !== prev);

      if (incoming && incoming !== prev) {
        // Project changed — clear previous video intent
      }
      setPipelineProjectId(next);

      if (vid || projectChanged) {
        setPipelineOpenNonce((n) => n + 1);
      }

      navigate("/pipeline");
    },
    [navigate]
  );

  const goDagPipeline = useCallback(
    (projectId: string) => {
      setDagProjectId(projectId);
      navigate("/dag-pipeline");
    },
    [navigate]
  );

  // After Create Project → navigate to Project Detail
  const handleCreateProjectSuccess = useCallback(
    ({ projectId }: CreateProjectSuccess) => {
      navigate(`/projects/${projectId}`);
    },
    [navigate]
  );

  return (
    <Routes>
      <Route path="/" element={<AppShell />}>
        <Route index element={<DashboardPage onOpenPipeline={goPipeline} />} />

        {/* Smart Pipeline */}
        <Route
          path="dag-pipeline"
          element={<DagPipelinePage initialProjectId={dagProjectId} />}
        />

        {/* Manual Studio */}
        <Route
          path="pipeline"
          element={
            <PipelineApp
              initialProjectId={pipelineProjectId}
              initialSceneHints={[]}
              initialVideoId={pipelineInitialVideoId}
              openNonce={pipelineOpenNonce}
              onOpenIntentConsumed={consumePipelineOpenIntent}
            />
          }
        />

        {/* Projects list */}
        <Route
          path="projects"
          element={
            <ProjectsPage
              onOpenCreateProject={() => navigate("/projects/create")}
              onOpenPipeline={goPipeline}
              onOpenDetail={(id) => navigate(`/projects/${id}`)}
            />
          }
        />

        {/* Project Detail — hub page */}
        <Route
          path="projects/:id"
          element={
            <ProjectDetailPage
              onOpenPipeline={goPipeline}
              onOpenSmartPipeline={goDagPipeline}
            />
          }
        />

        {/* Create Project (must be before :id to avoid conflict) */}
        <Route
          path="projects/create"
          element={
            <CreateProjectPage
              onCancel={() => navigate("/projects")}
              onSuccess={handleCreateProjectSuccess}
            />
          }
        />

        <Route path="characters" element={<CharactersPage onOpenPipeline={goPipeline} />} />
        <Route path="scenes" element={<ScenesPage onOpenPipeline={goPipeline} />} />
        <Route path="media" element={<MediaManagerPage />} />
        <Route path="agents" element={<Navigate to="/settings" replace />} />
        <Route path="settings" element={<SettingsPage />} />
        <Route path="bash-processes" element={<Navigate to="/" replace />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}

export default function App() {
  return <AppRoutes />;
}
