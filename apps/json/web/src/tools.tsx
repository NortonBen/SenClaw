// tools.tsx — the declarative catalogue of tools. Each entry knows how to run
// itself (mostly a thin call into api.ts) and, when its output is richer than
// text, which custom view to render. App.tsx maps this to a sidebar menu and
// ToolRunner drives the shared input → run → output loop.

import type { ReactNode } from "react";
import { js as beautifyJs, css as beautifyCss, html as beautifyHtml } from "js-beautify";

import * as api from "./api";
import { DiffView, JsonTree, StatsView } from "./components/views";

export type ControlSpec =
  | {
      kind: "select";
      key: string;
      label: string;
      options: { label: string; value: string }[];
      default: string;
      width?: number;
    }
  | { kind: "number"; key: string; label: string; default: number; min?: number; max?: number }
  | { kind: "text"; key: string; label: string; default?: string; placeholder?: string };

export type RunResult = {
  ok: boolean;
  output?: string;
  error?: string;
  node?: ReactNode;
  meta?: string;
};

export type Tool = {
  key: string;
  label: string;
  group: string;
  desc: string;
  dual?: boolean;
  controls?: ControlSpec[];
  sample?: string;
  run: (input: string, opts: Record<string, string | number>, second: string) => Promise<RunResult>;
};

const SAMPLE = `{
  "name": "SenClaw",
  "version": 3,
  "features": ["memory", "schedule", "wiki"],
  "active": true,
  "owner": { "id": 1, "handle": "norton" }
}`;

const num = (v: unknown, d = 2) => (typeof v === "number" ? v : Number(v) || d);
const str = (v: unknown, d = "") => (v == null ? d : String(v));

const fmtOptions = api.ALL_FORMATS.map((f) => ({ label: f.toUpperCase(), value: f }));
const codecOptions = api.CODECS.map((c) => ({ label: c, value: c }));

export const TOOLS: Tool[] = [
  // ── Định dạng ─────────────────────────────────────────────────────────────
  {
    key: "format",
    label: "Format / Minify",
    group: "Định dạng",
    desc: "Làm đẹp, nén hoặc sắp xếp khoá — giữ nguyên thứ tự khoá gốc.",
    sample: SAMPLE,
    controls: [
      {
        kind: "select",
        key: "mode",
        label: "Chế độ",
        default: "pretty",
        options: [
          { label: "Pretty", value: "pretty" },
          { label: "Minify", value: "minify" },
          { label: "Sort keys", value: "sort" },
        ],
      },
      { kind: "number", key: "indent", label: "Thụt lề", default: 2, min: 0, max: 8 },
    ],
    run: async (input, o) => {
      const r = await api.format(input, str(o.mode, "pretty") as never, num(o.indent));
      if (!r.ok) return { ok: false, error: withPos(r) };
      return { ok: true, output: str(r.output) };
    },
  },
  {
    key: "validate",
    label: "Validate",
    group: "Định dạng",
    desc: "Kiểm tra JSON hợp lệ, báo dòng/cột lỗi.",
    sample: SAMPLE,
    run: async (input) => {
      const r = await api.validate(input);
      if (r.valid === false) return { ok: false, error: withPos(r) };
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, output: "", meta: `hợp lệ · ${str(r.type)} · ${str(r.bytes)} bytes` };
    },
  },
  {
    key: "tree",
    label: "Tree Viewer",
    group: "Định dạng",
    desc: "Duyệt JSON dạng cây có thể mở/gập.",
    sample: SAMPLE,
    run: async (input) => ({ ok: true, node: <JsonTree source={input} /> }),
  },
  {
    key: "stats",
    label: "Thống kê",
    group: "Định dạng",
    desc: "Đếm node, độ sâu, số khoá và loại giá trị.",
    sample: SAMPLE,
    run: async (input) => {
      const r = await api.stats(input);
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, node: <StatsView data={r.stats as Record<string, unknown>} /> };
    },
  },
  {
    key: "schema",
    label: "Suy ra Schema",
    group: "Định dạng",
    desc: "Sinh JSON Schema từ dữ liệu mẫu.",
    sample: SAMPLE,
    run: async (input) => {
      const r = await api.schema(input);
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, output: str(r.output) };
    },
  },

  // ── Chuyển đổi ────────────────────────────────────────────────────────────
  {
    key: "convert",
    label: "Chuyển đổi",
    group: "Chuyển đổi",
    desc: "JSON ↔ YAML · CSV · TSV · XML · TOON · TSON (bất kỳ chiều nào).",
    sample: SAMPLE,
    controls: [
      { kind: "select", key: "from", label: "Từ", default: "json", options: fmtOptions, width: 110 },
      { kind: "select", key: "to", label: "Sang", default: "yaml", options: fmtOptions, width: 110 },
      { kind: "number", key: "indent", label: "Thụt lề", default: 2, min: 0, max: 8 },
    ],
    run: async (input, o) => {
      const r = await api.convertAny(str(o.from, "json") as never, str(o.to, "yaml") as never, input, {
        indent: num(o.indent),
      });
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, output: str(r.output) };
    },
  },
  {
    key: "query",
    label: "Truy vấn (Pointer)",
    group: "Chuyển đổi",
    desc: "Lấy giá trị theo JSON Pointer, ví dụ /owner/handle.",
    sample: SAMPLE,
    controls: [{ kind: "text", key: "path", label: "Pointer", placeholder: "/owner/handle" }],
    run: async (input, o) => {
      const r = await api.query(input, str(o.path));
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, output: str(r.output), meta: `${str(r.pointer)} · ${str(r.type)}` };
    },
  },
  {
    key: "diff",
    label: "So sánh (Diff)",
    group: "Chuyển đổi",
    desc: "Khác biệt cấu trúc giữa hai tài liệu JSON.",
    dual: true,
    sample: SAMPLE,
    run: async (input, _o, second) => {
      const r = await api.diff(input, second);
      if (!r.ok) return { ok: false, error: str(r.error) };
      const meta = r.equal ? "giống nhau" : `${str(r.count)} khác biệt`;
      return { ok: true, meta, node: <DiffView data={r as Record<string, unknown>} /> };
    },
  },

  // ── Mã hoá ────────────────────────────────────────────────────────────────
  {
    key: "encode",
    label: "Encode",
    group: "Mã hoá",
    desc: "base64 · base64url · hex · url · escape · msgpack.",
    controls: [{ kind: "select", key: "format", label: "Kiểu", default: "base64", options: codecOptions }],
    run: async (input, o) => {
      const r = await api.encode(input, str(o.format, "base64"));
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, output: str(r.output) };
    },
  },
  {
    key: "decode",
    label: "Decode",
    group: "Mã hoá",
    desc: "Giải mã base64 · url · hex · escape · msgpack · jwt.",
    controls: [
      {
        kind: "select",
        key: "format",
        label: "Kiểu",
        default: "base64",
        options: [...codecOptions, { label: "jwt", value: "jwt" }],
      },
    ],
    run: async (input, o) => {
      const r = await api.decode(input, str(o.format, "base64"));
      if (!r.ok) return { ok: false, error: str(r.error) };
      return { ok: true, output: str(r.output) };
    },
  },

  // ── Formatter mã nguồn ─────────────────────────────────────────────────────
  {
    key: "beautify",
    label: "Beautify code",
    group: "Formatter",
    desc: "Làm đẹp JS/JSON, CSS, HTML/XML (offline, js-beautify).",
    controls: [
      {
        kind: "select",
        key: "lang",
        label: "Ngôn ngữ",
        default: "js",
        options: [
          { label: "JavaScript / JSON", value: "js" },
          { label: "CSS", value: "css" },
          { label: "HTML / XML", value: "html" },
        ],
      },
      { kind: "number", key: "indent", label: "Thụt lề", default: 2, min: 0, max: 8 },
    ],
    run: async (input, o) => {
      const opts = { indent_size: num(o.indent) };
      try {
        const lang = str(o.lang, "js");
        const output =
          lang === "css"
            ? beautifyCss(input, opts)
            : lang === "html"
              ? beautifyHtml(input, opts)
              : beautifyJs(input, opts);
        return { ok: true, output };
      } catch (e) {
        return { ok: false, error: e instanceof Error ? e.message : String(e) };
      }
    },
  },
];

/** Append line/column to a parse error when the backend reports them. */
function withPos(r: api.ApiResult): string {
  const base = str(r.error, "không hợp lệ");
  if (typeof r.line === "number" && typeof r.column === "number") {
    return `Dòng ${r.line}, cột ${r.column}: ${base}`;
  }
  return base;
}

export const GROUPS = Array.from(new Set(TOOLS.map((t) => t.group)));
