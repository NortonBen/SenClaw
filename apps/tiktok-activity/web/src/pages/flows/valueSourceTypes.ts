export type FillSource = "literal" | "account_username" | "account_password" | "action_param";

export function inferFillSource(pr: Record<string, string>): FillSource {
  const vs = pr.value_source?.trim();
  if (vs === "literal" || vs === "account_username" || vs === "account_password" || vs === "action_param") {
    return vs;
  }
  if (pr.text?.trim()) return "literal";
  const tf = (pr.text_from ?? "").trim().toLowerCase();
  if (tf === "account_username" || tf === "username") return "account_username";
  if (tf === "account_password" || tf === "password") return "account_password";
  if (tf === "action_param" || tf === "step_param" || tf.startsWith("param:")) return "action_param";
  return "literal";
}

export type GotoUrlMode = "fixed" | "action_param";

export function inferGotoUrlMode(p: Record<string, string>): GotoUrlMode {
  if (p.url?.trim()) return "fixed";
  const us = (p.url_source ?? "").trim().toLowerCase();
  if (us === "action_param" || us === "step_param" || p.url_param_key?.trim()) return "action_param";
  const uf = (p.url_from ?? "").trim().toLowerCase();
  if (uf.startsWith("param:")) return "action_param";
  return "fixed";
}
