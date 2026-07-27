import { useState } from "react";
import { Button, Input, Space, Typography } from "antd";
import { DeleteOutlined, PlusOutlined } from "@ant-design/icons";

export function StepParamsEditor({
  params,
  onChange,
  readOnly,
}: {
  params: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
  readOnly?: boolean;
}) {
  const p = params ?? {};
  const entries = Object.entries(p);
  const ro = Boolean(readOnly);
  const [newKey, setNewKey] = useState("");
  const [newVal, setNewVal] = useState("");

  const removeKey = (k: string) => {
    const next = { ...p };
    delete next[k];
    onChange(next);
  };

  const addPair = () => {
    const k = newKey.trim();
    if (!k) return;
    onChange({ ...p, [k]: newVal });
    setNewKey("");
    setNewVal("");
  };

  return (
    <div
      style={{
        marginBottom: 12,
        border: "1px solid var(--flow-panel-border)",
        borderRadius: 10,
        padding: "10px 12px",
        background: "var(--flow-panel-bg)",
      }}
    >
      <Typography.Text strong style={{ display: "block", marginBottom: 8 }}>
        Params của step (tùy chọn)
      </Typography.Text>
      <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
        Map key → chuỗi. Atomic <code>fill</code> / <code>goto</code> có thể lấy giá trị theo tên key (khác với{" "}
        <code>config</code> dùng cho stage / nhánh).
      </Typography.Paragraph>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {entries.map(([k, v]) => (
          <Space key={k} style={{ width: "100%" }} align="start">
            <Typography.Text code style={{ minWidth: 100, display: "inline-block" }}>
              {k}
            </Typography.Text>
            <Input
              size="small"
              style={{ flex: 1, minWidth: 120 }}
              value={v}
              readOnly={ro}
              disabled={ro}
              onChange={(e) => onChange({ ...p, [k]: e.target.value })}
            />
            {ro ? null : (
              <Button size="small" danger type="text" icon={<DeleteOutlined />} onClick={() => removeKey(k)} aria-label="Xóa" />
            )}
          </Space>
        ))}
        {ro ? null : (
          <Space wrap>
            <Input size="small" style={{ width: 140 }} value={newKey} onChange={(e) => setNewKey(e.target.value)} placeholder="key" />
            <Input size="small" style={{ width: 200 }} value={newVal} onChange={(e) => setNewVal(e.target.value)} placeholder="value" />
            <Button size="small" type="dashed" icon={<PlusOutlined />} onClick={addPair}>
              Thêm param
            </Button>
          </Space>
        )}
      </div>
    </div>
  );
}
