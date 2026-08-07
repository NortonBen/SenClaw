//! Đọc `variables.tf` (mọi file `*.tf` cấp gốc) để render form Apply, và
//! đọc/ghi `*.tfvars` (file giá trị người dùng chọn, vd `prod.tfvars`).
//!
//! Parse bằng `hcl-rs` (HCL2 thật, không regex). Khi GHI tfvars, app tự
//! serialize expression từ JSON — output ổn định `key = value` mỗi dòng,
//! Terraform đọc được nguyên vẹn.

use anyhow::{anyhow, bail, Result};
use hcl::{Expression, ObjectKey};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Một biến khai trong *.tf → một ô trong form Apply.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VarDef {
    pub name: String,
    /// Text gốc của `type = …` (vd `string`, `list(string)`, `object({…})`).
    pub var_type: String,
    pub description: String,
    /// Giá trị `default` quy về JSON (None = biến bắt buộc).
    pub default: Option<Value>,
    pub sensitive: bool,
    /// File .tf khai biến này (để UI trỏ nguồn).
    pub file: String,
}

fn raw_expr(e: &Expression) -> String {
    hcl::format::to_string(e).unwrap_or_default()
}

/// Expression HCL literal → JSON. Expression không phải literal (ref, func…)
/// thì giữ nguyên text để form hiển thị được thay vì mất dữ liệu.
fn expr_to_json(e: &Expression) -> Value {
    match e {
        Expression::Null => Value::Null,
        Expression::Bool(b) => json!(b),
        Expression::Number(n) => {
            if let Some(i) = n.as_i64() {
                json!(i)
            } else {
                json!(n.as_f64())
            }
        }
        Expression::String(s) => json!(s),
        Expression::Array(items) => Value::Array(items.iter().map(expr_to_json).collect()),
        Expression::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let key = match k {
                    ObjectKey::Identifier(id) => id.as_str().to_string(),
                    ObjectKey::Expression(Expression::String(s)) => s.clone(),
                    ObjectKey::Expression(other) => raw_expr(other),
                    other => other.to_string(),
                };
                out.insert(key, expr_to_json(v));
            }
            Value::Object(out)
        }
        other => Value::String(raw_expr(other)),
    }
}

/// Các file *.tf cấp gốc, `variables.tf` xếp đầu để thứ tự biến ổn định.
fn tf_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file() && p.extension().is_some_and(|x| x == "tf")
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|p| {
        let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        (name != "variables.tf", name)
    });
    files
}

/// Quét mọi block `variable "x" {}` trong *.tf cấp gốc của workspace.
/// File hỏng cú pháp không chặn file khác — lỗi trả kèm để UI báo.
pub fn parse_variables(dir: &Path) -> (Vec<VarDef>, Vec<String>) {
    let mut defs = Vec::new();
    let mut errors = Vec::new();
    for path in tf_files(dir) {
        let fname = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("{fname}: {e}"));
                continue;
            }
        };
        let body = match hcl::parse(&src) {
            Ok(b) => b,
            Err(e) => {
                errors.push(format!("{fname}: {e}"));
                continue;
            }
        };
        for block in body.blocks() {
            if block.identifier() != "variable" {
                continue;
            }
            let Some(label) = block.labels().first() else {
                continue;
            };
            let mut def = VarDef {
                name: label.as_str().to_string(),
                var_type: "string".to_string(),
                description: String::new(),
                default: None,
                sensitive: false,
                file: fname.clone(),
            };
            for attr in block.body().attributes() {
                match attr.key() {
                    "type" => def.var_type = raw_expr(attr.expr()),
                    "description" => {
                        if let Expression::String(s) = attr.expr() {
                            def.description = s.clone();
                        } else {
                            def.description = raw_expr(attr.expr());
                        }
                    }
                    "default" => def.default = Some(expr_to_json(attr.expr())),
                    "sensitive" => {
                        if let Expression::Bool(b) = attr.expr() {
                            def.sensitive = *b;
                        }
                    }
                    _ => {}
                }
            }
            defs.push(def);
        }
    }
    (defs, errors)
}

/// Danh sách file `*.tfvars` / `*.tfvars.json` cấp gốc (để user chọn điền form).
pub fn list_tfvars(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                .filter(|n| n.ends_with(".tfvars") || n.ends_with(".tfvars.json"))
                .collect()
        })
        .unwrap_or_default();
    // terraform.tfvars được Terraform tự nạp — xếp đầu cho dễ thấy.
    out.sort_by_key(|n| (n != "terraform.tfvars", n.clone()));
    out
}

/// Tên file tfvars hợp lệ: không path, không traversal, đúng đuôi.
pub fn validate_tfvars_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !(name.ends_with(".tfvars") || name.ends_with(".tfvars.json"))
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        || name.starts_with('.')
        || name.contains("..")
    {
        bail!("tên file tfvars không hợp lệ: {name:?} (chỉ chữ/số/._- và đuôi .tfvars)");
    }
    Ok(())
}

/// Đọc giá trị từ một file tfvars → map JSON (form điền theo map này).
pub fn read_tfvars(dir: &Path, name: &str) -> Result<Map<String, Value>> {
    validate_tfvars_name(name)?;
    read_tfvars_at(&dir.join(name))
}

/// Bản nhận path đã được caller kiểm soát (var-file tương đối trong workspace).
pub fn read_tfvars_at(path: &Path) -> Result<Map<String, Value>> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let src = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("không đọc được {name}: {e}"))?;
    if name.ends_with(".json") {
        let v: Value = serde_json::from_str(&src)?;
        return v
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow!("{name}: JSON gốc phải là object"));
    }
    let body = hcl::parse(&src).map_err(|e| anyhow!("{name}: {e}"))?;
    let mut out = Map::new();
    for attr in body.attributes() {
        out.insert(attr.key().to_string(), expr_to_json(attr.expr()));
    }
    Ok(out)
}

fn hcl_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // ${ và %{ là template interpolation — escape để giữ nguyên chuỗi.
            '$' | '%' if chars.peek() == Some(&'{') => {
                out.push(c);
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// JSON → text expression HCL (dùng khi ghi tfvars).
pub fn json_to_hcl(v: &Value, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let pad_in = "  ".repeat(indent + 1);
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => hcl_quote(s),
        Value::Array(items) => {
            if items.is_empty() {
                return "[]".into();
            }
            let inner = items
                .iter()
                .map(|i| format!("{pad_in}{}", json_to_hcl(i, indent + 1)))
                .collect::<Vec<_>>()
                .join(",\n");
            format!("[\n{inner}\n{pad}]")
        }
        Value::Object(map) => {
            if map.is_empty() {
                return "{}".into();
            }
            let inner = map
                .iter()
                .map(|(k, val)| {
                    let key = if ident_ok(k) { k.clone() } else { hcl_quote(k) };
                    format!("{pad_in}{key} = {}", json_to_hcl(val, indent + 1))
                })
                .collect::<Vec<_>>()
                .join("\n");
            format!("{{\n{inner}\n{pad}}}")
        }
    }
}

/// Ghi map giá trị thành file tfvars (`key = value` mỗi biến).
pub fn write_tfvars(dir: &Path, name: &str, values: &Map<String, Value>) -> Result<()> {
    validate_tfvars_name(name)?;
    write_tfvars_at(&dir.join(name), values)
}

/// Bản nhận path đã được caller kiểm soát (var-file tương đối trong workspace).
pub fn write_tfvars_at(path: &Path, values: &Map<String, Value>) -> Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.ends_with(".json") {
        let text = serde_json::to_string_pretty(&Value::Object(values.clone()))?;
        std::fs::write(path, text + "\n")?;
        return Ok(());
    }
    let mut out = String::new();
    for (k, v) in values {
        let key = if ident_ok(k) { k.clone() } else { hcl_quote(k) };
        out.push_str(&format!("{key} = {}\n", json_to_hcl(v, 0)));
    }
    std::fs::write(path, out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, content) in files {
            std::fs::write(dir.path().join(name), content).unwrap();
        }
        dir
    }

    const VARS_TF: &str = r#"
variable "region" {
  type        = string
  description = "AWS region"
  default     = "ap-southeast-1"
}

variable "instance_count" {
  type    = number
  default = 2
}

variable "enable_cdn" {
  type    = bool
  default = false
}

variable "tags" {
  type = map(string)
  default = {
    env  = "prod"
    team = "infra"
  }
}

variable "subnets" {
  type    = list(string)
  default = ["10.0.1.0/24", "10.0.2.0/24"]
}

variable "db_password" {
  type      = string
  sensitive = true
}

variable "sizing" {
  type = object({ cpu = number, ram = number })
  default = { cpu = 2, ram = 4 }
}
"#;

    #[test]
    fn parse_variable_defs() {
        let dir = dir_with(&[("variables.tf", VARS_TF)]);
        let (defs, errors) = parse_variables(dir.path());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(defs.len(), 7);

        let region = defs.iter().find(|d| d.name == "region").unwrap();
        assert_eq!(region.var_type, "string");
        assert_eq!(region.description, "AWS region");
        assert_eq!(region.default, Some(json!("ap-southeast-1")));
        assert!(!region.sensitive);

        let count = defs.iter().find(|d| d.name == "instance_count").unwrap();
        assert_eq!(count.default, Some(json!(2)));

        let cdn = defs.iter().find(|d| d.name == "enable_cdn").unwrap();
        assert_eq!(cdn.default, Some(json!(false)));

        let tags = defs.iter().find(|d| d.name == "tags").unwrap();
        assert_eq!(tags.var_type, "map(string)");
        assert_eq!(tags.default, Some(json!({ "env": "prod", "team": "infra" })));

        let subnets = defs.iter().find(|d| d.name == "subnets").unwrap();
        assert_eq!(subnets.default, Some(json!(["10.0.1.0/24", "10.0.2.0/24"])));

        let pw = defs.iter().find(|d| d.name == "db_password").unwrap();
        assert!(pw.sensitive);
        assert!(pw.default.is_none());

        let sizing = defs.iter().find(|d| d.name == "sizing").unwrap();
        assert_eq!(sizing.default, Some(json!({ "cpu": 2, "ram": 4 })));
    }

    #[test]
    fn variables_spread_over_files_and_bad_file_reported() {
        let dir = dir_with(&[
            ("variables.tf", "variable \"a\" { default = 1 }\n"),
            ("main.tf", "variable \"b\" { default = \"x\" }\nresource \"null_resource\" \"r\" {}\n"),
            ("broken.tf", "variable \"c\" {"),
        ]);
        let (defs, errors) = parse_variables(dir.path());
        let names: Vec<_> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("broken.tf"));
    }

    #[test]
    fn non_literal_default_kept_as_text() {
        let dir = dir_with(&[(
            "variables.tf",
            "variable \"az\" { default = var.region }\n",
        )]);
        let (defs, _) = parse_variables(dir.path());
        assert_eq!(defs[0].default, Some(json!("var.region")));
    }

    #[test]
    fn tfvars_list_and_ordering() {
        let dir = dir_with(&[
            ("prod.tfvars", "a = 1\n"),
            ("dev.tfvars", "a = 2\n"),
            ("terraform.tfvars", "a = 3\n"),
            ("notes.txt", "x"),
        ]);
        assert_eq!(
            list_tfvars(dir.path()),
            vec!["terraform.tfvars", "dev.tfvars", "prod.tfvars"]
        );
    }

    #[test]
    fn tfvars_roundtrip_hcl() {
        let dir = dir_with(&[]);
        let mut values = Map::new();
        values.insert("region".into(), json!("ap-southeast-1"));
        values.insert("count".into(), json!(3));
        values.insert("ratio".into(), json!(1.5));
        values.insert("enable".into(), json!(true));
        values.insert("tags".into(), json!({ "env": "prod", "chủ đề": "hạ tầng" }));
        values.insert("subnets".into(), json!(["10.0.1.0/24"]));
        values.insert("tmpl".into(), json!("giá ${price} và %{if}"));

        write_tfvars(dir.path(), "prod.tfvars", &values).unwrap();
        let back = read_tfvars(dir.path(), "prod.tfvars").unwrap();
        assert_eq!(back.get("region"), Some(&json!("ap-southeast-1")));
        assert_eq!(back.get("count"), Some(&json!(3)));
        assert_eq!(back.get("ratio"), Some(&json!(1.5)));
        assert_eq!(back.get("enable"), Some(&json!(true)));
        assert_eq!(back.get("tags"), Some(&json!({ "env": "prod", "chủ đề": "hạ tầng" })));
        assert_eq!(back.get("subnets"), Some(&json!(["10.0.1.0/24"])));
        assert_eq!(back.get("tmpl"), Some(&json!("giá ${price} và %{if}")));
    }

    #[test]
    fn tfvars_json_roundtrip() {
        let dir = dir_with(&[]);
        let mut values = Map::new();
        values.insert("a".into(), json!([1, 2]));
        write_tfvars(dir.path(), "prod.tfvars.json", &values).unwrap();
        let back = read_tfvars(dir.path(), "prod.tfvars.json").unwrap();
        assert_eq!(back.get("a"), Some(&json!([1, 2])));
    }

    #[test]
    fn tfvars_name_validation_blocks_traversal() {
        assert!(validate_tfvars_name("prod.tfvars").is_ok());
        assert!(validate_tfvars_name("prod.auto.tfvars").is_ok());
        assert!(validate_tfvars_name("../evil.tfvars").is_err());
        assert!(validate_tfvars_name("a/b.tfvars").is_err());
        assert!(validate_tfvars_name(".hidden.tfvars").is_err());
        assert!(validate_tfvars_name("x.txt").is_err());
        assert!(validate_tfvars_name("").is_err());
    }
}
