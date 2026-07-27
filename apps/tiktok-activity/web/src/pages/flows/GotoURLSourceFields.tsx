import { Input, Select, Typography } from "antd";
import { inferGotoUrlMode, type GotoUrlMode } from "./valueSourceTypes";

export function GotoURLSourceFields({
  params,
  onPatch,
}: {
  params: Record<string, string>;
  onPatch: (next: Record<string, string>) => void;
}) {
  const p = params ?? {};
  const mode = inferGotoUrlMode(p);

  const setMode = (m: GotoUrlMode) => {
    const next: Record<string, string> = { ...p };
    if (m === "fixed") {
      delete next.url_source;
      delete next.url_param_key;
      delete next.url_from;
      if (next.url === undefined) next.url = "";
    } else {
      delete next.url;
      next.url_source = "action_param";
      if (next.url_param_key === undefined) next.url_param_key = "";
      delete next.url_from;
    }
    onPatch(next);
  };

  const paramKey =
    p.url_param_key?.trim() ||
    (p.url_from?.trim().toLowerCase().startsWith("param:")
      ? p.url_from.trim().slice("param:".length).trim()
      : "");

  return (
    <div style={{ display: "grid", gap: 8, marginBottom: 4 }}>
      <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
        Nguồn URL
        <Select<GotoUrlMode>
          size="small"
          style={{ width: "100%", marginTop: 4 }}
          value={mode}
          onChange={(v) => setMode(v)}
          options={[
            { value: "fixed", label: "URL cố định" },
            { value: "action_param", label: "Từ params của step (tên key)" },
          ]}
        />
      </label>
      {mode === "fixed" ? (
        <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
          url
          <Input
            size="small"
            value={p.url ?? ""}
            onChange={(e) => onPatch({ ...p, url: e.target.value })}
            placeholder="https://..."
            style={{ marginTop: 4 }}
          />
        </label>
      ) : (
        <>
          <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
            Tên key trong params của step
            <Input
              size="small"
              value={paramKey}
              onChange={(e) =>
                onPatch({
                  ...p,
                  url_source: "action_param",
                  url_param_key: e.target.value,
                })
              }
              placeholder="vd: login_url"
              style={{ marginTop: 4 }}
            />
          </label>
          <Typography.Text type="secondary" style={{ fontSize: 11 }}>
            Đặt giá trị URL trong khối Params của step (cùng step playwright_atomics).
          </Typography.Text>
        </>
      )}
    </div>
  );
}
