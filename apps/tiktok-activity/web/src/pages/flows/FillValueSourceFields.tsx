import { Input, Select, Typography } from "antd";
import { inferFillSource, type FillSource } from "./valueSourceTypes";

export type { FillSource } from "./valueSourceTypes";
export { inferFillSource } from "./valueSourceTypes";

export function FillValueSourceFields({
  params,
  onPatch,
}: {
  params: Record<string, string>;
  onPatch: (next: Record<string, string>) => void;
}) {
  const p = params ?? {};
  const source = (p.value_source?.trim() || inferFillSource(p)) as FillSource;

  const setSource = (s: FillSource) => {
    const next: Record<string, string> = { ...p, value_source: s };
    delete next.text_from;
    if (s === "literal") {
      if (next.text === undefined) next.text = "";
    } else {
      delete next.text;
    }
    if (s !== "action_param") {
      delete next.param_key;
    } else if (next.param_key === undefined) {
      next.param_key = "";
    }
    onPatch(next);
  };

  const paramKey =
    p.param_key?.trim() ||
    (p.text_from?.trim().toLowerCase().startsWith("param:")
      ? p.text_from.trim().slice("param:".length).trim()
      : "");

  return (
    <div style={{ display: "grid", gap: 8, marginBottom: 4 }}>
      <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
        Nguồn giá trị → ô input
        <Select<FillSource>
          size="small"
          style={{ width: "100%", marginTop: 4 }}
          value={source}
          onChange={(v) => setSource(v)}
          options={[
            { value: "literal", label: "Text cố định" },
            { value: "account_username", label: "Username tài khoản đang chạy" },
            { value: "account_password", label: "Password tài khoản đang chạy" },
            { value: "action_param", label: "Từ params của step (nhập tên key)" },
          ]}
        />
      </label>
      {source === "literal" ? (
        <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
          Text
          <Input
            size="small"
            value={p.text ?? ""}
            onChange={(e) => onPatch({ ...p, value_source: "literal", text: e.target.value })}
            style={{ marginTop: 4 }}
          />
        </label>
      ) : null}
      {source === "action_param" ? (
        <>
          <label style={{ fontSize: 11, color: "var(--muted-text)" }}>
            Tên key trong &quot;Params của step&quot;
            <Input
              size="small"
              value={paramKey}
              onChange={(e) =>
                onPatch({
                  ...p,
                  value_source: "action_param",
                  param_key: e.target.value,
                })
              }
              placeholder="vd: search_query, landing_url"
              style={{ marginTop: 4 }}
            />
          </label>
          <Typography.Text type="secondary" style={{ fontSize: 11 }}>
            Khai báo key/value ở khối Params phía trên chuỗi atomic trong cùng step.
          </Typography.Text>
        </>
      ) : null}
    </div>
  );
}
