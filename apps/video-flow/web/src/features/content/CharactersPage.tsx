import { useQuery } from "@tanstack/react-query";
import { Button, Card, Select, Space, Typography } from "antd";
import { useState } from "react";
import type { OpenPipelineProp } from "@/features/content/contentTypes";
import { EntitiesPanel } from "@/features/pipeline/EntitiesPanel";
import { api, type CharacterRow, type ProjectRow } from "@/lib/api/client";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

export function CharactersPage({ onOpenPipeline }: OpenPipelineProp = {}) {
  const [projectId, setProjectId] = useState("");
  const [orientation, setOrientation] = useState<"VERTICAL" | "HORIZONTAL">(
    "VERTICAL"
  );

  const projectsQ = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
  });

  const charactersQ = useQuery({
    queryKey: ["characters", projectId],
    queryFn: () => api.listProjectCharacters(projectId),
    enabled: !!projectId.trim(),
    refetchInterval: 3500,
  });

  const projectRows = (projectsQ.data ?? []) as ProjectRow[];

  return (
    <div className="layout layout-wide">
      <Space direction="vertical" size={16} style={{ width: "100%" }}>
        <Typography.Title level={3} style={{ margin: 0 }}>
          Nhân vật
        </Typography.Title>
        <Typography.Text type="secondary">
          Entity / nhân vật · địa điểm — cùng API project characters với Pipeline.
        </Typography.Text>
        <Card>
          <Space wrap align="end">
            <div style={{ minWidth: 300 }}>
              <Typography.Text strong>Project</Typography.Text>
              <Select
                style={{ width: "100%" }}
                placeholder="— Chọn project —"
                value={projectId || undefined}
                onChange={(v) => setProjectId(v)}
                options={projectRows.map((p) => ({
                  value: str(p.id),
                  label: str(p.name) || str(p.id),
                }))}
              />
            </div>
            {projectId.trim() && onOpenPipeline && (
              <Button onClick={() => onOpenPipeline(projectId.trim())}>
                Mở trong Pipeline
              </Button>
            )}
          </Space>
        </Card>

        {projectId.trim() && (
          <>
            <Card title="Hướng gen ref">
              <Space direction="vertical" size={8} style={{ width: "100%", maxWidth: 260 }}>
                <Typography.Text type="secondary">
                  Dùng cho nút Gen ref / Gen lại ref trong bảng entity.
                </Typography.Text>
                <Select
                  value={orientation}
                  onChange={(v) => setOrientation(v)}
                  options={[
                    { value: "VERTICAL", label: "VERTICAL" },
                    { value: "HORIZONTAL", label: "HORIZONTAL" },
                  ]}
                />
              </Space>
            </Card>
            <EntitiesPanel
              projectId={projectId}
              rows={charactersQ.data as CharacterRow[] | undefined}
              isLoading={charactersQ.isLoading}
              orientation={orientation}
            />
          </>
        )}
      </Space>
    </div>
  );
}
