import { useMemo, useState, type ReactNode } from "react";
import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  ReactFlow,
  Position,
  type Connection,
  type Edge,
  type Node,
  type NodeProps,
  type NodeTypes,
  type OnConnect,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { CharacterRow, SceneRow } from "@/lib/api/client";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

function charNamesFromScene(row: SceneRow): string[] {
  const raw = row.character_names;
  if (Array.isArray(raw)) {
    return raw.map((x) => str(x).trim()).filter(Boolean);
  }
  const s = str(raw).trim();
  if (s.startsWith("[")) {
    try {
      const parsed = JSON.parse(s);
      if (Array.isArray(parsed)) return parsed.map((x) => str(x).trim()).filter(Boolean);
    } catch {}
  }
  return s.split(",").map((x) => x.trim()).filter(Boolean);
}

// namesFromScene returns all entity names relevant to this scene.
// Characters/creatures: character_names on the scene (explicit refs).
// Locations / visual_asset: name in scene text OR explicitly listed in character_names
// (kéo nối trên flow chỉ cập nhật character_names — phải tính cả nhánh này để edge/cảnh báo khớp).
function namesFromScene(row: SceneRow, entities: CharacterRow[]): string[] {
  const charSet = new Set(charNamesFromScene(row).map((n) => n.toLowerCase()));
  const sceneText = [
    str(row.prompt),
    str(row.video_prompt),
    str(row.action_sequence),
    str(row.narrator_text),
  ].join(" ").toLowerCase();

  const seen = new Set<string>();
  const out: string[] = [];
  for (const e of entities) {
    const name = str(e.name).trim();
    if (!name) continue;
    const key = name.toLowerCase();
    const eType = str(e.entity_type).toLowerCase();
    let include = false;
    if (eType === "location" || eType === "visual_asset") {
      include = sceneText.includes(key) || charSet.has(key);
    } else {
      include = charSet.has(key);
    }
    if (include && !seen.has(key)) {
      seen.add(key);
      out.push(name);
    }
  }
  return out;
}

function edgeColor(entityType: string): string {
  const t = entityType.toLowerCase().trim();
  if (t === "character") return "#5b8def";
  if (t === "location") return "#3dcca8";
  if (t === "visual_asset") return "#e8b84a";
  if (t === "creature") return "#b98cf3";
  return "#8b95a8";
}

function sceneOrderValue(scene: SceneRow, fallbackIdx: number): number {
  const raw = scene.display_order;
  if (typeof raw === "number" && Number.isFinite(raw)) return raw;
  const n = Number(raw);
  if (Number.isFinite(n)) return n;
  return fallbackIdx + 1;
}

function hashString(input: string): number {
  let h = 2166136261;
  for (let i = 0; i < input.length; i += 1) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function jitterByKey(key: string, spread: number): number {
  const n = hashString(key) % 1000;
  const normalized = n / 999;
  return (normalized * 2 - 1) * spread;
}

type Props = {
  entities: CharacterRow[];
  scenes: SceneRow[];
  orientation: "VERTICAL" | "HORIZONTAL";
  onConnectEntityScene: (sceneId: string, entityName: string) => void;
  onUnlinkEntityScene: (sceneId: string, entityName: string) => void;
};

function EntityFlowNode({ data }: NodeProps) {
  return (
    <div
      style={{
        position: "relative",
        minWidth: 200,
        padding: "8px 10px",
        borderRadius: 10,
        border: "1.5px solid var(--border)",
        background: "var(--panel)",
        boxShadow: "0 1px 0 rgba(0,0,0,0.03)",
      }}
    >
      <Handle
        type="source"
        position={Position.Right}
        style={{ width: 8, height: 8, background: "#6f93ff", border: "1px solid var(--panel)" }}
      />
      {data?.label as ReactNode}
    </div>
  );
}

function SceneNode({ data }: NodeProps) {
  return (
    <div
      style={{
        position: "relative",
        minWidth: 210,
        padding: "10px 10px 12px",
        borderRadius: 10,
        border: "1.5px solid var(--border)",
        background: "var(--panel)",
        boxShadow: "0 1px 0 rgba(0,0,0,0.03)",
      }}
    >
      <Handle
        type="target"
        id="top-chain"
        position={Position.Top}
        style={{ width: 8, height: 8, background: "#a3abb9" }}
      />
      <Handle
        type="target"
        id="left-entity"
        position={Position.Left}
        style={{ width: 8, height: 8, background: "#6f93ff" }}
      />
      <Handle
        type="source"
        id="bottom-chain"
        position={Position.Bottom}
        style={{ width: 8, height: 8, background: "#a3abb9" }}
      />
      {data?.label as ReactNode}
    </div>
  );
}

const nodeTypes: NodeTypes = {
  entityNode: EntityFlowNode,
  sceneNode: SceneNode,
};

export function EntitySceneFlow({
  entities,
  scenes,
  orientation,
  onConnectEntityScene,
  onUnlinkEntityScene,
}: Props) {
  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState("all");
  const filteredEntities = useMemo(() => {
    const q = query.trim().toLowerCase();
    return entities.filter((e) => {
      const t = str(e.entity_type) || "entity";
      if (typeFilter !== "all" && t !== typeFilter) return false;
      if (!q) return true;
      return str(e.name).toLowerCase().includes(q) || t.toLowerCase().includes(q);
    });
  }, [entities, query, typeFilter]);

  const entityTypes = useMemo(() => {
    const set = new Set(filteredEntities.map((e) => str(e.entity_type) || "entity"));
    return Array.from(set);
  }, [filteredEntities]);
  const sceneWarnings = useMemo(() => {
    const entityByName = new Map(
      entities.map((e) => [str(e.name).trim(), e] as const).filter(([name]) => !!name)
    );
    return scenes.map((s) => {
      const sceneId = str(s.id);
      const sceneLabel = sceneId.slice(0, 8);
      const names = namesFromScene(s, entities);
      if (!names.length) {
        return {
          sceneId,
          sceneLabel,
          missingEntities: true,
          missingRefs: [] as string[],
        };
      }
      const missingRefs = names.filter((name) => {
        const entity = entityByName.get(name);
        if (!entity) return true;
        return !str(entity.media_id).trim();
      });
      return {
        sceneId,
        sceneLabel,
        missingEntities: false,
        missingRefs,
      };
    });
  }, [entities, scenes]);

  const { nodes, edges } = useMemo(() => {
    const ns: Node[] = [];
    const es: Edge[] = [];
    const sceneIndexById = new Map<string, number>();
    scenes.forEach((s, idx) => {
      sceneIndexById.set(str(s.id), idx);
    });
    const byName = new Set(filteredEntities.map((e) => str(e.name).trim()).filter(Boolean));
    const linkedSceneIndexesByEntity = new Map<string, number[]>();
    filteredEntities.forEach((e) => {
      linkedSceneIndexesByEntity.set(str(e.id), []);
    });
    scenes.forEach((s) => {
      const idx = sceneIndexById.get(str(s.id)) ?? 0;
      namesFromScene(s, entities).forEach((name) => {
        const entity = filteredEntities.find((x) => str(x.name).trim() === name);
        if (!entity) return;
        const arr = linkedSceneIndexesByEntity.get(str(entity.id));
        if (arr) arr.push(idx);
      });
    });
    const orderedEntities = [...filteredEntities].sort((a, b) => {
      const aid = str(a.id);
      const bid = str(b.id);
      const aIdx = linkedSceneIndexesByEntity.get(aid) ?? [];
      const bIdx = linkedSceneIndexesByEntity.get(bid) ?? [];
      const aCenter = aIdx.length ? aIdx.reduce((x, y) => x + y, 0) / aIdx.length : Number.MAX_SAFE_INTEGER;
      const bCenter = bIdx.length ? bIdx.reduce((x, y) => x + y, 0) / bIdx.length : Number.MAX_SAFE_INTEGER;
      if (aCenter !== bCenter) return aCenter - bCenter;
      return str(a.name).localeCompare(str(b.name));
    });
    const entityGap = 108;
    const sceneGap = 188;
    const entityXBase = 48;
    const sceneXBase = 560;

    orderedEntities.forEach((e, idx) => {
      const id = `entity:${str(e.id)}`;
      const label = str(e.name) || `Entity ${idx + 1}`;
      const thumb = str(e.reference_image_url);
      const jitterX = jitterByKey(id + ":x", 42);
      const jitterY = jitterByKey(id + ":y", 10);
      ns.push({
        id,
        type: "entityNode",
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
        position: { x: entityXBase + jitterX, y: idx * entityGap + 24 + jitterY },
        data: {
          label: (
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              {thumb ? (
                <img
                  src={thumb}
                  alt={label}
                  style={{ width: 36, height: 36, borderRadius: 6, objectFit: "cover" }}
                />
              ) : (
                <div
                  style={{
                    width: 36,
                    height: 36,
                    borderRadius: 6,
                    border: "1px solid var(--border)",
                  }}
                />
              )}
              <div style={{ display: "grid", lineHeight: 1.2 }}>
                <strong>{label}</strong>
                <span className="sub mono" style={{ fontSize: 11 }}>
                  {str(e.entity_type) || "entity"}
                </span>
              </div>
            </div>
          ),
        },
      });
    });

    const orderedScenes = [...scenes].sort((a, b) => {
      const ao = sceneOrderValue(a, 0);
      const bo = sceneOrderValue(b, 0);
      if (ao !== bo) return ao - bo;
      return str(a.id).localeCompare(str(b.id));
    });

    orderedScenes.forEach((s, idx) => {
      const sid = str(s.id);
      const id = `scene:${sid}`;
      const warn = sceneWarnings.find((x) => x.sceneId === sid);
      const jitterX = jitterByKey(id + ":x", 54);
      const jitterY = jitterByKey(id + ":y", 24);
      const img =
        orientation === "HORIZONTAL"
          ? str(s.horizontal_image_url)
          : str(s.vertical_image_url);
      const hasWarn = !!warn && (warn.missingEntities || warn.missingRefs.length > 0);
      ns.push({
        id,
        type: "sceneNode",
        sourcePosition: Position.Right,
        targetPosition: Position.Left,
        position: { x: sceneXBase + jitterX, y: idx * sceneGap + 24 + jitterY },
        style: hasWarn ? { borderColor: "#e8b84a", background: "rgba(232, 184, 74, 0.08)" } : undefined,
        data: {
          label: (
            <div style={{ display: "grid", gap: 4 }}>
              <strong>Scene {idx + 1}</strong>
              <span className="sub mono" style={{ fontSize: 11 }}>
                {sid.slice(0, 8)}... {str(s.chain_type)}
              </span>
              {img ? (
                <img
                  src={img}
                  alt={`Scene ${idx + 1}`}
                  style={{
                    width: 140,
                    height: 78,
                    borderRadius: 6,
                    objectFit: "cover",
                    border: "1px solid var(--border)",
                  }}
                />
              ) : null}
              {warn?.missingEntities ? (
                <span className="sub" style={{ color: "#e8b84a", fontSize: 11 }}>
                  Chưa gán entity
                </span>
              ) : null}
              {!warn?.missingEntities && (warn?.missingRefs.length ?? 0) > 0 ? (
                <span className="sub" style={{ color: "#e8b84a", fontSize: 11 }}>
                  Thiếu ref: {warn?.missingRefs.join(", ")}
                </span>
              ) : null}
            </div>
          ),
        },
      });

      namesFromScene(s, entities).forEach((name) => {
        if (!byName.has(name)) return;
        const entity = entities.find((x) => str(x.name).trim() === name);
        if (!entity) return;
        const eType = str(entity.entity_type) || "entity";
        const stroke = edgeColor(eType);
        es.push({
          id: `edge:${str(entity.id)}:${sid}`,
          source: `entity:${str(entity.id)}`,
          target: id,
          targetHandle: "left-entity",
          type: "bezier",
          markerEnd: { type: MarkerType.ArrowClosed, color: stroke },
          style: { stroke, strokeWidth: 2.4, opacity: 0.9 },
          animated: true,
          zIndex: 1,
        });
      });
    });

    orderedScenes.forEach((scene, idx) => {
      if (idx === orderedScenes.length - 1) return;
      const curId = str(scene.id);
      const nextId = str(orderedScenes[idx + 1].id);
      if (!curId || !nextId) return;
      es.push({
        id: `edge:scene-chain:${curId}:${nextId}`,
        source: `scene:${curId}`,
        target: `scene:${nextId}`,
        sourceHandle: "bottom-chain",
        targetHandle: "top-chain",
        type: "smoothstep",
        markerEnd: { type: MarkerType.ArrowClosed, color: "#a3abb9" },
        style: { stroke: "#a3abb9", strokeWidth: 1.6, opacity: 0.8 },
        zIndex: 0,
      });
    });
    return { nodes: ns, edges: es };
  }, [filteredEntities, scenes, orientation, entities, sceneWarnings]);

  const onConnect = useMemo<OnConnect>(
    () => (conn: Connection) => {
      const src = str(conn.source);
      const tgt = str(conn.target);
      const sourceHandle = str(conn.sourceHandle);
      const targetHandle = str(conn.targetHandle);
      if (src.startsWith("scene:") || tgt.startsWith("scene:")) {
        const sceneToScene =
          src.startsWith("scene:") &&
          tgt.startsWith("scene:") &&
          sourceHandle === "bottom-chain" &&
          targetHandle === "top-chain";
        const entityToSceneLeft =
          src.startsWith("entity:") &&
          tgt.startsWith("scene:") &&
          targetHandle === "left-entity";
        if (!sceneToScene && !entityToSceneLeft) return;
      }
      if (!src.startsWith("entity:") || !tgt.startsWith("scene:")) return;
      const eid = src.replace("entity:", "");
      const sid = tgt.replace("scene:", "");
      const entity = entities.find((e) => str(e.id) === eid);
      if (!entity) return;
      onConnectEntityScene(sid, str(entity.name));
    },
    [entities, onConnectEntityScene]
  );

  if (!entities.length || !scenes.length) {
    return (
      <p className="sub" style={{ marginTop: 0 }}>
        Cần có cả entity và scene để hiển thị sơ đồ liên kết.
      </p>
    );
  }

  return (
    <div>
      <div className="row" style={{ marginBottom: "0.5rem" }}>
        <div className="field" style={{ maxWidth: 320 }}>
          <label>Tìm entity</label>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Lọc theo tên hoặc loại..."
          />
        </div>
        <div className="field" style={{ maxWidth: 220 }}>
          <label>Lọc loại</label>
          <select
            value={typeFilter}
            onChange={(e) => setTypeFilter(e.target.value)}
          >
            <option value="all">all</option>
            {Array.from(new Set(entities.map((e) => str(e.entity_type) || "entity"))).map(
              (t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              )
            )}
          </select>
        </div>
        <div className="sub" style={{ alignSelf: "flex-end", marginBottom: 6 }}>
          hiển thị {filteredEntities.length}/{entities.length} entities
        </div>
      </div>
      <div style={{ marginBottom: 8, display: "flex", gap: 10, flexWrap: "wrap" }}>
        {entityTypes.map((t) => (
          <span key={t} className="mono" style={{ fontSize: 11, color: edgeColor(t) }}>
            ■ {t}
          </span>
        ))}
      </div>
      <div className="sub" style={{ marginBottom: 8 }}>
        Scene cảnh báo:{" "}
        {sceneWarnings.filter((x) => x.missingEntities || x.missingRefs.length > 0).length}/
        {sceneWarnings.length}
      </div>
      {sceneWarnings.some((x) => x.missingEntities || x.missingRefs.length > 0) ? (
        <div
          style={{
            marginBottom: 10,
            padding: "8px 10px",
            borderRadius: 8,
            border: "1px solid rgba(232, 184, 74, 0.4)",
            background: "rgba(232, 184, 74, 0.08)",
          }}
        >
          {sceneWarnings
            .filter((x) => x.missingEntities || x.missingRefs.length > 0)
            .slice(0, 6)
            .map((x) => (
              <div key={x.sceneId} className="sub" style={{ marginTop: 0 }}>
                Scene {x.sceneLabel}...:{" "}
                {x.missingEntities
                  ? "chưa gán entity"
                  : `entity chưa có ref media_id (${x.missingRefs.join(", ")})`}
              </div>
            ))}
        </div>
      ) : null}
      <div style={{ height: 560, border: "1px solid var(--border)", borderRadius: 10 }}>
        <ReactFlow
          fitView
          fitViewOptions={{ padding: 0.16 }}
          nodeTypes={nodeTypes}
          nodes={nodes}
          edges={edges}
          nodesDraggable
          onConnect={onConnect}
          onEdgeDoubleClick={(_, edge) => {
            const src = str(edge.source);
            const tgt = str(edge.target);
            if (!src.startsWith("entity:") || !tgt.startsWith("scene:")) return;
            const eid = src.replace("entity:", "");
            const sid = tgt.replace("scene:", "");
            const ent = entities.find((e) => str(e.id) === eid);
            if (ent) onUnlinkEntityScene(sid, str(ent.name));
          }}
        >
          <MiniMap />
          <Controls />
          <Background />
        </ReactFlow>
      </div>
    </div>
  );
}
