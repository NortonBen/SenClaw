// api.ts — the single data layer for the UI.
//
// Every heavy transform runs in the Rust backend (`/api/*`), so a person and an
// agent always see identical bytes. The only work done in the browser is the
// TOON/TSON bridge (their encoders are npm-only) and the js-beautify code
// formatters — both pure, offline, no network.

import { encode as toonEncode, decode as toonDecode } from "@toon-format/toon";
import { dumps as tsonDumps, loads as tsonLoads } from "@zenoaihq/tson";

export type ApiResult = Record<string, unknown> & {
  ok: boolean;
  error?: string;
  line?: number;
  column?: number;
};

async function post(path: string, body: unknown): Promise<ApiResult> {
  const res = await fetch(`/api/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    return { ok: false, error: `HTTP ${res.status} ${res.statusText}` };
  }
  return (await res.json()) as ApiResult;
}

/** Backend container formats convertible directly (via JSON pivot) in Rust. */
export const BACKEND_FORMATS = ["json", "yaml", "csv", "tsv", "xml"] as const;
/** Extra formats bridged client-side. */
export const CLIENT_FORMATS = ["toon", "tson"] as const;
export const ALL_FORMATS = [...BACKEND_FORMATS, ...CLIENT_FORMATS] as const;
export type Format = (typeof ALL_FORMATS)[number];

export const CODECS = [
  "base64",
  "base64url",
  "hex",
  "url",
  "escape",
  "msgpack",
] as const;

// ── formatting / validation ────────────────────────────────────────────────

export function format(
  input: string,
  mode: "pretty" | "minify" | "sort",
  indent = 2,
): Promise<ApiResult> {
  return post("format", { input, mode, indent });
}

export function validate(input: string): Promise<ApiResult> {
  return post("validate", { input });
}

export function stats(input: string): Promise<ApiResult> {
  return post("stats", { input });
}

export function schema(input: string): Promise<ApiResult> {
  return post("schema", { input });
}

export function query(input: string, path: string): Promise<ApiResult> {
  return post("query", { input, path });
}

export function diff(left: string, right: string): Promise<ApiResult> {
  return post("diff", { left, right });
}

export function encode(input: string, fmt: string): Promise<ApiResult> {
  return post("encode", { input, format: fmt });
}

export function decode(input: string, fmt: string): Promise<ApiResult> {
  return post("decode", { input, format: fmt });
}

function backendConvert(
  from: string,
  to: string,
  input: string,
  root: string,
  columns: string[],
  indent: number,
): Promise<ApiResult> {
  return post("convert", { from, to, input, root, columns, indent });
}

// ── format bridge (any ↔ any, incl. TOON/TSON) ─────────────────────────────

/** Normalize `input` of format `from` into a pretty JSON string. */
async function toJsonString(
  from: Format,
  input: string,
  indent: number,
): Promise<string> {
  if (from === "json") return input;
  if (from === "toon") return JSON.stringify(toonDecode(input), null, indent);
  if (from === "tson") return JSON.stringify(tsonLoads(input), null, indent);
  const r = await backendConvert(from, "json", input, "root", [], indent);
  if (!r.ok) throw new Error(String(r.error ?? "convert failed"));
  return String(r.output ?? "");
}

/** Render a JSON string as format `to`. */
async function fromJsonString(
  to: Format,
  jsonStr: string,
  root: string,
  columns: string[],
  indent: number,
): Promise<string> {
  if (to === "json") {
    const r = await format(jsonStr, "pretty", indent);
    if (!r.ok) throw new Error(String(r.error ?? "format failed"));
    return String(r.output ?? "");
  }
  if (to === "toon") return toonEncode(JSON.parse(jsonStr));
  if (to === "tson") return tsonDumps(JSON.parse(jsonStr));
  const r = await backendConvert("json", to, jsonStr, root, columns, indent);
  if (!r.ok) throw new Error(String(r.error ?? "convert failed"));
  return String(r.output ?? "");
}

/**
 * Convert between any two supported formats. Pure backend when both sides are
 * container formats (keeps CSV column / XML root handling); otherwise bridges
 * through JSON for TOON/TSON.
 */
export async function convertAny(
  from: Format,
  to: Format,
  input: string,
  opts: { root?: string; columns?: string[]; indent?: number } = {},
): Promise<ApiResult> {
  const indent = opts.indent ?? 2;
  const root = opts.root ?? "root";
  const columns = opts.columns ?? [];
  try {
    if (from === to) return { ok: true, output: input };
    const backendOnly =
      (BACKEND_FORMATS as readonly string[]).includes(from) &&
      (BACKEND_FORMATS as readonly string[]).includes(to);
    if (backendOnly) return backendConvert(from, to, input, root, columns, indent);
    const jsonStr = await toJsonString(from, input, indent);
    const output = await fromJsonString(to, jsonStr, root, columns, indent);
    return { ok: true, output };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}
