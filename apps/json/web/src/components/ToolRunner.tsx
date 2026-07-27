// ToolRunner.tsx — the shared workbench: renders a tool's controls, one or two
// input editors, a Run action, and either a text output editor or the tool's
// custom view. All tool-specific behaviour lives in tools.tsx; this only wires
// state, buttons, and layout.

import { useEffect, useMemo, useRef, useState } from "react";
import {
  App as AntApp,
  Alert,
  Button,
  Input,
  InputNumber,
  Segmented,
  Select,
  Space,
  Tag,
  Typography,
} from "antd";
import {
  ClearOutlined,
  CopyOutlined,
  DownloadOutlined,
  ExperimentOutlined,
  PlayCircleOutlined,
  UploadOutlined,
} from "@ant-design/icons";

import type { ControlSpec, RunResult, Tool } from "../tools";

const { Title, Paragraph, Text } = Typography;
const { TextArea } = Input;

const EDITOR_STYLE: React.CSSProperties = {
  fontFamily: "'JetBrains Mono', ui-monospace, SFMono-Regular, Menlo, monospace",
  fontSize: 13,
  minHeight: 320,
};

function defaultOpts(tool: Tool): Record<string, string | number> {
  const o: Record<string, string | number> = {};
  for (const c of tool.controls ?? []) {
    o[c.key] = c.kind === "number" ? c.default : (c.default ?? "");
  }
  return o;
}

export default function ToolRunner({ tool }: { tool: Tool }) {
  const { message } = AntApp.useApp();
  const [input, setInput] = useState("");
  const [second, setSecond] = useState("");
  const [opts, setOpts] = useState<Record<string, string | number>>(() => defaultOpts(tool));
  const [result, setResult] = useState<RunResult | null>(null);
  const [busy, setBusy] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  // Reset the whole workbench whenever the selected tool changes.
  useEffect(() => {
    setInput("");
    setSecond("");
    setOpts(defaultOpts(tool));
    setResult(null);
  }, [tool]);

  const run = async () => {
    setBusy(true);
    try {
      setResult(await tool.run(input, opts, second));
    } catch (e) {
      setResult({ ok: false, error: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  const copy = async (text: string) => {
    if (!text) return;
    await navigator.clipboard.writeText(text);
    message.success("Đã copy");
  };

  const download = (text: string) => {
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${tool.key}-output.txt`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const onFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    file.text().then(setInput);
    e.target.value = "";
  };

  const outputText = result?.output ?? "";

  return (
    <div style={{ maxWidth: 1100, margin: "0 auto" }}>
      <Title level={4} style={{ marginBottom: 0 }}>
        {tool.label}
      </Title>
      <Paragraph type="secondary" style={{ marginTop: 4 }}>
        {tool.desc}
      </Paragraph>

      {tool.controls && tool.controls.length > 0 && (
        <Space wrap style={{ marginBottom: 12 }}>
          {tool.controls.map((c) => (
            <Control
              key={c.key}
              spec={c}
              value={opts[c.key]}
              onChange={(v) => setOpts((prev) => ({ ...prev, [c.key]: v }))}
            />
          ))}
        </Space>
      )}

      <div style={{ display: "grid", gridTemplateColumns: tool.dual ? "1fr 1fr" : "1fr", gap: 12 }}>
        <TextArea
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder={tool.dual ? "Tài liệu bên trái…" : "Dán dữ liệu vào đây…"}
          style={EDITOR_STYLE}
          autoSize={{ minRows: 14, maxRows: 28 }}
          spellCheck={false}
        />
        {tool.dual && (
          <TextArea
            value={second}
            onChange={(e) => setSecond(e.target.value)}
            placeholder="Tài liệu bên phải…"
            style={EDITOR_STYLE}
            autoSize={{ minRows: 14, maxRows: 28 }}
            spellCheck={false}
          />
        )}
      </div>

      <Space wrap style={{ marginTop: 12 }}>
        <Button type="primary" icon={<PlayCircleOutlined />} loading={busy} onClick={run}>
          Chạy
        </Button>
        {tool.sample && (
          <Button icon={<ExperimentOutlined />} onClick={() => setInput(tool.sample!)}>
            Mẫu
          </Button>
        )}
        <Button icon={<UploadOutlined />} onClick={() => fileRef.current?.click()}>
          Mở file
        </Button>
        <Button
          icon={<ClearOutlined />}
          onClick={() => {
            setInput("");
            setSecond("");
            setResult(null);
          }}
        >
          Xoá
        </Button>
        <input ref={fileRef} type="file" hidden onChange={onFile} />
      </Space>

      {result?.error && (
        <Alert type="error" showIcon style={{ marginTop: 16 }} message="Lỗi" description={result.error} />
      )}

      {result?.meta && (
        <div style={{ marginTop: 16 }}>
          <Tag color="blue">{result.meta}</Tag>
        </div>
      )}

      {result?.node && (
        <div style={{ marginTop: 16, border: "1px solid var(--jt-border)", borderRadius: 8 }}>
          {result.node}
        </div>
      )}

      {result?.ok && result.output != null && !result.node && (
        <div style={{ marginTop: 16 }}>
          <Space style={{ marginBottom: 8 }}>
            <Text strong>Kết quả</Text>
            <Button size="small" icon={<CopyOutlined />} onClick={() => copy(outputText)}>
              Copy
            </Button>
            <Button size="small" icon={<DownloadOutlined />} onClick={() => download(outputText)}>
              Tải về
            </Button>
          </Space>
          <TextArea
            value={outputText}
            readOnly
            style={EDITOR_STYLE}
            autoSize={{ minRows: 10, maxRows: 28 }}
          />
        </div>
      )}
    </div>
  );
}

function Control({
  spec,
  value,
  onChange,
}: {
  spec: ControlSpec;
  value: string | number;
  onChange: (v: string | number) => void;
}) {
  const label = <Text type="secondary">{spec.label}</Text>;
  if (spec.kind === "select") {
    // A short option list reads better as a segmented switch.
    if (spec.options.length <= 3 && !spec.width) {
      return (
        <Space>
          {label}
          <Segmented
            value={String(value)}
            onChange={(v) => onChange(String(v))}
            options={spec.options}
          />
        </Space>
      );
    }
    return (
      <Space>
        {label}
        <Select
          value={String(value)}
          onChange={onChange}
          options={spec.options}
          style={{ width: spec.width ?? 160 }}
        />
      </Space>
    );
  }
  if (spec.kind === "number") {
    return (
      <Space>
        {label}
        <InputNumber
          value={Number(value)}
          min={spec.min}
          max={spec.max}
          onChange={(v) => onChange(Number(v ?? spec.default))}
          style={{ width: 90 }}
        />
      </Space>
    );
  }
  return (
    <Space>
      {label}
      <Input
        value={String(value)}
        placeholder={spec.placeholder}
        onChange={(e) => onChange(e.target.value)}
        style={{ width: 260 }}
      />
    </Space>
  );
}
