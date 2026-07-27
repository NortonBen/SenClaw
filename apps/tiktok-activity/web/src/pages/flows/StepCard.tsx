import { Button } from "antd";
import { DeleteOutlined } from "@ant-design/icons";
import type { FlowAction } from "../../types/api";
import { branchSummary } from "./branchUtils";
import { ui } from "./constants";

export function StepCard({
  step,
  index,
  selected,
  onClick,
  onDragStart,
  onDropBefore,
  onDropAfter,
  onDelete,
}: {
  step: FlowAction;
  index: number;
  selected: boolean;
  onClick: () => void;
  onDragStart: (e: React.DragEvent) => void;
  onDropBefore: (e: React.DragEvent) => void;
  onDropAfter: (e: React.DragEvent) => void;
  onDelete: () => void;
}) {
  return (
    <div
      style={{
        ...ui.stepCard,
        outline: selected ? "2px solid #1677ff" : "none",
        position: "relative",
      }}
      draggable
      onDragStart={onDragStart}
      onClick={onClick}
      onDragOver={(e) => e.preventDefault()}
    >
      <div style={{ ...ui.stepCardHeader, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
        <span>{index + 1}. {step.name}</span>
        <Button
          type="text"
          size="small"
          danger
          icon={<DeleteOutlined />}
          onClick={(e) => {
            e.stopPropagation();
            onDelete();
          }}
          aria-label="Delete step"
        />
      </div>
      <div style={ui.stepCardBody}>{step.type}</div>
      {step.type === "playwright_atomics" ? (
        <div style={{ ...ui.stepCardBody, paddingTop: 0 }}>
          {(step.atomics?.length ?? 0)} atomic
        </div>
      ) : null}
      {branchSummary(step) ? (
        <div style={{ ...ui.stepCardBody, paddingTop: 0, color: "var(--muted-text)" }}>{branchSummary(step)}</div>
      ) : null}
      <div
        onDrop={onDropBefore}
        style={{ position: "absolute", width: 12, height: "100%", left: -6, top: 0 }}
      />
      <div
        onDrop={onDropAfter}
        style={{ position: "absolute", width: 12, height: "100%", right: -6, top: 0 }}
      />
    </div>
  );
}

