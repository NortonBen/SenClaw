import { useCallback, useState } from "react";
import { Button, Input, Modal, Typography, message } from "antd";
import type { FlowAtomic } from "../../types/api";
import { ATOMIC_PALETTE, MIME_ATOMIC_REORDER, MIME_ATOMIC_TEMPLATE, type AtomicPaletteItem } from "./atomicPalette";
import { ui } from "./constants";
import { parseAtomicsImportPayload } from "./flowIo";
import { FillValueSourceFields } from "./FillValueSourceFields";
import { GotoURLSourceFields } from "./GotoURLSourceFields";

function newAtomicId(): string {
  return `atom_${Date.now()}_${Math.random().toString(16).slice(2)}`;
}

function cloneParams(p: Record<string, string>): Record<string, string> {
  return { ...p };
}

function paramsKeysForKind(kind: string): { key: string; label: string; multiline?: boolean }[] {
  switch (kind) {
    case "click":
      return [
        { key: "selector", label: "selector (một)" },
        { key: "selectors", label: "selectors (nhiều dòng / ||)", multiline: true },
        { key: "timeout_ms", label: "timeout_ms" },
        { key: "click_timeout_ms", label: "click_timeout_ms" },
      ];
    case "click_unless_contains":
      return [
        { key: "selectors", label: "selectors (nhiều dòng, giống click)", multiline: true },
        {
          key: "unless_substrings",
          label: "unless_substrings (mỗi dòng một chuỗi; nếu InnerText nút chứa thì bỏ qua click; alias skip_if_contains)",
          multiline: true,
        },
        { key: "timeout_ms", label: "timeout_ms" },
        { key: "click_timeout_ms", label: "click_timeout_ms" },
      ];
    case "click_button_text":
      return [
        { key: "text", label: "text (chuỗi con trong nút / accessible name)" },
        { key: "mode", label: "mode: role (mặc định) | nested" },
        { key: "role", label: "role (khi mode=role): button, link, …" },
        { key: "match", label: "match: contains (mặc định) | exact | regex" },
        { key: "base_selector", label: "base_selector (khi mode=nested, CSS OR)" },
        { key: "timeout_ms", label: "timeout_ms" },
        { key: "click_timeout_ms", label: "click_timeout_ms" },
      ];
    case "fill":
      return [
        { key: "selector", label: "selector" },
        { key: "selectors", label: "selectors (một dòng)", multiline: true },
        { key: "timeout_ms", label: "timeout_ms" },
        { key: "fill_timeout_ms", label: "fill_timeout_ms" },
      ];
    case "press":
      return [
        { key: "selector", label: "selector (focus, tùy chọn)" },
        { key: "key", label: "key (Enter, …)" },
        { key: "timeout_ms", label: "timeout_ms" },
      ];
    case "wait_ms":
      return [{ key: "ms", label: "ms" }];
    case "wait_load":
      return [
        { key: "state", label: "state (load | domcontentloaded | networkidle)" },
        { key: "timeout_ms", label: "timeout_ms" },
      ];
    case "goto":
      return [
        { key: "wait_until", label: "wait_until" },
        { key: "timeout_ms", label: "timeout_ms" },
      ];
    case "scroll":
      return [
        { key: "delta_x", label: "delta_x (px, ngang)" },
        { key: "delta_y", label: "delta_y (px, dọc)" },
        { key: "method", label: "method (wheel | scroll_by)" },
        { key: "selector", label: "selector (tùy chọn — scroll trong phần tử)" },
        { key: "selectors", label: "selectors (nhiều dòng)", multiline: true },
        { key: "timeout_ms", label: "timeout_ms (khi có selector)" },
      ];
    case "assert":
      return [
        { key: "expect", label: "expect (visible|hidden|url_contains|url_regex|text_contains)" },
        { key: "selector", label: "selector (visible|hidden|text_contains)" },
        { key: "selectors", label: "selectors (visible|hidden)", multiline: true },
        { key: "value", label: "value (url_contains / url_regex / text_contains)" },
        { key: "pattern", label: "pattern (url_regex, nếu khác value)" },
        { key: "text", label: "text (alias value cho text_contains)" },
        { key: "timeout_ms", label: "timeout_ms" },
      ];
    default:
      return [];
  }
}

export function AtomicChainEditor({
  atomics,
  onChange,
}: {
  atomics: FlowAtomic[];
  onChange: (next: FlowAtomic[]) => void;
}) {
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");

  const list = atomics ?? [];

  const patchAtomic = useCallback(
    (index: number, patch: Partial<FlowAtomic>) => {
      const next = list.map((a, i) => (i === index ? { ...a, ...patch, params: patch.params ?? a.params } : a));
      onChange(next);
    },
    [list, onChange]
  );

  const patchParam = useCallback(
    (index: number, key: string, value: string) => {
      const next = list.map((a, i) => {
        if (i !== index) return a;
        const params = { ...(a.params ?? {}) };
        params[key] = value;
        return { ...a, params };
      });
      onChange(next);
    },
    [list, onChange]
  );

  const insertAt = useCallback(
    (index: number, item: AtomicPaletteItem) => {
      const row: FlowAtomic = {
        id: newAtomicId(),
        kind: item.kind,
        params: cloneParams(item.defaultParams),
      };
      const next = [...list.slice(0, index), row, ...list.slice(index)];
      onChange(next);
    },
    [list, onChange]
  );

  const removeAt = useCallback(
    (index: number) => {
      onChange(list.filter((_, i) => i !== index));
    },
    [list, onChange]
  );

  const move = useCallback(
    (from: number, dir: -1 | 1) => {
      const to = from + dir;
      if (to < 0 || to >= list.length) return;
      const a = [...list];
      const [x] = a.splice(from, 1);
      if (x === undefined) return;
      a.splice(to, 0, x);
      onChange(a);
    },
    [list, onChange]
  );

  const onPaletteDragStart = (e: React.DragEvent, item: AtomicPaletteItem) => {
    e.dataTransfer.setData(MIME_ATOMIC_TEMPLATE, JSON.stringify(item));
    e.dataTransfer.effectAllowed = "copy";
  };

  const onReorderDragStart = (e: React.DragEvent, index: number) => {
    e.dataTransfer.setData(MIME_ATOMIC_REORDER, String(index));
    e.dataTransfer.effectAllowed = "move";
  };

  const handleStripDrop = useCallback(
    (e: React.DragEvent, insertIndex: number) => {
      e.preventDefault();
      e.stopPropagation();
      const rawT = e.dataTransfer.getData(MIME_ATOMIC_TEMPLATE);
      if (rawT) {
        const item = JSON.parse(rawT) as AtomicPaletteItem;
        insertAt(insertIndex, item);
        return;
      }
      const rawM = e.dataTransfer.getData(MIME_ATOMIC_REORDER);
      if (rawM === "") return;
      const from = Number(rawM);
      if (!Number.isFinite(from)) return;
      if (from === insertIndex || from + 1 === insertIndex) return;
      const copy = [...list];
      const [x] = copy.splice(from, 1);
      if (x === undefined) return;
      let i = insertIndex;
      if (from < insertIndex) i = insertIndex - 1;
      copy.splice(i, 0, x);
      onChange(copy);
    },
    [insertAt, list, onChange]
  );

  const exportJson = useCallback(() => {
    const payload = { atomics: list };
    const s = JSON.stringify(payload, null, 2);
    void navigator.clipboard.writeText(s).then(
      () => message.success("Đã copy JSON atomics"),
      () => {
        message.info("Không copy được; mở console để xem");
        console.warn(s);
      }
    );
  }, [list]);

  const doImport = useCallback(() => {
    try {
      const { atomics: rows, flowName, sourceHint } = parseAtomicsImportPayload(importText);
      const normalized = rows.map((r, i) => ({
        id: r.id || `atom_import_${i}_${newAtomicId()}`,
        name: r.name,
        kind: r.kind,
        params: { ...(r.params ?? {}) },
      }));
      onChange(normalized);
      setImportOpen(false);
      setImportText("");
      const extra = [flowName ? `flow: ${flowName}` : "", sourceHint ?? ""].filter(Boolean).join(" — ");
      message.success(extra ? `Đã nhập chuỗi atomic (${extra})` : "Đã nhập chuỗi atomic");
    } catch (e) {
      message.error(e instanceof Error ? e.message : "JSON không hợp lệ");
    }
  }, [importText, onChange]);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <Typography.Paragraph type="secondary" style={{ marginBottom: 0 }}>
        Kéo <b>atomic</b> từ cột trái vào vạch thả (trên / giữa / cuối chuỗi). Kéo từng dòng để đổi thứ tự.
      </Typography.Paragraph>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(220px, 280px) minmax(0, 1fr)",
          gap: 16,
          alignItems: "start",
        }}
      >
        <div style={{ ...ui.leftPanel, maxHeight: "min(560px, 65vh)", overflow: "auto" }}>
          <div style={{ ...ui.leftPanelHeader, padding: "8px 10px" }}>
            <div style={{ fontWeight: 700, fontSize: 12 }}>Atomic palette</div>
          </div>
          <div style={{ ...ui.paletteList, maxHeight: "min(480px, 55vh)" }}>
            {ATOMIC_PALETTE.map((item) => (
              <div
                key={item.id}
                draggable
                onDragStart={(e) => onPaletteDragStart(e, item)}
                style={{ ...ui.paletteItem, padding: "8px 10px" }}
              >
                <div style={{ fontWeight: 700, fontSize: 12 }}>{item.label}</div>
                <div style={{ fontSize: 11, color: "var(--muted-text)" }}>{item.kind}</div>
              </div>
            ))}
          </div>
        </div>

        <div
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 6,
            minWidth: 0,
            maxHeight: "min(620px, 68vh)",
            overflowY: "auto",
            paddingRight: 4,
          }}
        >
          <div
            onDragOver={(e) => {
              e.preventDefault();
              e.dataTransfer.dropEffect = e.dataTransfer.types.includes(MIME_ATOMIC_REORDER) ? "move" : "copy";
            }}
            onDrop={(e) => handleStripDrop(e, 0)}
            style={{ height: 10, borderRadius: 4, background: "var(--flow-dropzone-bg)" }}
            title="Thả để chèn đầu chuỗi"
          />

          {list.map((a, index) => {
            const paramFields = paramsKeysForKind(a.kind);
            return (
              <div key={a.id ?? `idx_${index}`}>
                <div
                  draggable
                  onDragStart={(e) => onReorderDragStart(e, index)}
                  style={{
                    border: "1px solid var(--flow-chain-card-border)",
                    borderRadius: 10,
                    padding: "10px 12px",
                    background: "var(--flow-chain-card-bg)",
                  }}
                >
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, marginBottom: 8 }}>
                    <span style={{ fontWeight: 700, fontSize: 12, color: "var(--text)" }}>
                      {index + 1}. {a.name?.trim() ? `${a.name.trim()} · ` : ""}
                      {a.kind}
                    </span>
                    <div style={{ display: "flex", gap: 4 }}>
                      <Button size="small" onClick={() => move(index, -1)} disabled={index === 0}>
                        ↑
                      </Button>
                      <Button size="small" onClick={() => move(index, 1)} disabled={index === list.length - 1}>
                        ↓
                      </Button>
                      <Button size="small" danger onClick={() => removeAt(index)}>
                        Xóa
                      </Button>
                    </div>
                  </div>
                  <div style={{ display: "grid", gap: 6 }}>
                    <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
                      name (nhãn, tùy chọn)
                      <Input
                        size="small"
                        value={a.name ?? ""}
                        onChange={(e) => patchAtomic(index, { name: e.target.value })}
                        placeholder="vd: Ch�� load trang"
                        style={{ marginTop: 4 }}
                      />
                    </label>
                    <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
                      kind
                      <Input
                        size="small"
                        value={a.kind}
                        onChange={(e) => patchAtomic(index, { kind: e.target.value })}
                        style={{ marginTop: 4 }}
                      />
                    </label>
                    {a.kind === "fill" ? (
                      <FillValueSourceFields
                        params={a.params ?? {}}
                        onPatch={(next) => patchAtomic(index, { params: { ...(a.params ?? {}), ...next } })}
                      />
                    ) : null}
                    {a.kind === "goto" ? (
                      <GotoURLSourceFields
                        params={a.params ?? {}}
                        onPatch={(next) => patchAtomic(index, { params: { ...(a.params ?? {}), ...next } })}
                      />
                    ) : null}
                    {paramFields.length > 0 ? (
                      paramFields.map((f) => (
                        <label key={f.key} style={{ fontSize: 11, color: "var(--muted-text)" }}>
                          {f.label}
                          {f.multiline ? (
                            <Input.TextArea
                              size="small"
                              rows={f.key === "selectors" ? 3 : 2}
                              value={(a.params ?? {})[f.key] ?? ""}
                              onChange={(e) => patchParam(index, f.key, e.target.value)}
                              style={{ marginTop: 4 }}
                            />
                          ) : (
                            <Input
                              size="small"
                              value={(a.params ?? {})[f.key] ?? ""}
                              onChange={(e) => patchParam(index, f.key, e.target.value)}
                              style={{ marginTop: 4 }}
                            />
                          )}
                        </label>
                      ))
                    ) : (
                      <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                        Kind tùy chỉnh — thêm params bằng JSON trong Export/Import hoặc đổi kind thành click | click_unless_contains | click_button_text | fill | press | wait_ms | wait_load | goto | scroll | assert.
                      </Typography.Text>
                    )}
                  </div>
                </div>
                <div
                  onDragOver={(e) => {
                    e.preventDefault();
                    e.dataTransfer.dropEffect = e.dataTransfer.types.includes(MIME_ATOMIC_REORDER) ? "move" : "copy";
                  }}
                  onDrop={(e) => handleStripDrop(e, index + 1)}
                  style={{ height: 8, marginTop: 4, borderRadius: 4, background: "var(--flow-dropzone-bg)" }}
                  title="Thả để chèn sau bước này"
                />
              </div>
            );
          })}

          {list.length === 0 ? (
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              Chưa có atomic — kéo từ palette vào vạch xám phía trên.
            </Typography.Text>
          ) : null}
        </div>
      </div>

      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        <Button size="small" onClick={exportJson}>
          Export JSON (clipboard)
        </Button>
        <Button size="small" onClick={() => setImportOpen(true)}>
          Import JSON
        </Button>
      </div>

      <Modal title="Import atomics JSON" open={importOpen} onCancel={() => setImportOpen(false)} onOk={doImport} okText="Nhập">
        <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
          Hỗ trợ <code>{`{ "atomics": [ ... ] }`}</code> hoặc file export flow{" "}
          <code>{`{ "actions": [ { "type": "playwright_atomics", "atomics": [...] } ] }`}</code>
          — chỉ lấy chuỗi từ các step <code>playwright_atomics</code>.
        </Typography.Paragraph>
        <Input.TextArea rows={10} value={importText} onChange={(e) => setImportText(e.target.value)} placeholder='{"atomics":[]} hoặc {"actions":[...]}' />
      </Modal>
    </div>
  );
}
