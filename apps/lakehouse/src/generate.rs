//! `lake_flow_generate` (§9): mô tả tự nhiên → **draft** flow DSL qua bridge
//! `llm.request`. Draft KHÔNG auto-enable, KHÔNG lưu — chỉ trả về để agent/UI kiểm rồi
//! gọi `lake_flow_create`. Phần thuần (build prompt + parse draft + validate) tách khỏi
//! lời gọi mạng để test được (mock response JSON), theo bài học rewrite-story/ontology.

use anyhow::{anyhow, Result};
use serde_json::Value;

use crate::flow::{self, FlowDef};

/// Đặc tả DSL nhúng thẳng vào prompt — đủ để model sinh flow hợp lệ mà không cần fetch
/// tài liệu ngoài. Giữ ngắn gọn: chỉ các field load-bearing + 4 mode + kind transform.
const DSL_SPEC: &str = r#"DSL flow (JSON) — CHỈ các field sau:
{
  "flow": "<id [a-z0-9_-]{1,64}>",
  "sources": [{
    "id": "<step id>",
    "connection": "<connection_id>",
    "table": "<schema.table>" | "query": "<SQL SELECT>",
    "mode": "full_refresh" | "incremental_append" | "incremental_merge" | "snapshot",
    "cursor": {"column": "<col>", "initial": <giá trị>}   // BẮT BUỘC khi incremental_*
    "primary_key": ["<col>"],                              // BẮT BUỘC khi merge/snapshot
    "merge_key": ["<col>"],                                // merge: ⊆ target.partition_by
    "target": {"namespace": "raw", "dataset": "<name>", "partition_by": ["<col>"]}
  }],
  "transforms": [{
    "id": "<step id>",
    "kind": "full" | "incremental_by_time",
    "sql": "SELECT ... FROM <source_step_id> ...",         // tham chiếu step bằng ID
    "time_column": "<col>", "interval": "hour|day|week|month", "lookback": 0,  // khi incremental_by_time
    "target": {"namespace": "marts", "dataset": "<name>"}
  }],
  "schedule": {"every_minutes": N} | {"daily_at": "HH:MM"}   // tùy chọn
}
QUY TẮC:
- Một trong table|query cho mỗi source, KHÔNG cả hai.
- incremental_merge: cần cursor + primary_key + merge_key + target.partition_by.
- snapshot (SCD2): cần primary_key; strategy timestamp cần cursor.column.
- Transform SQL tham chiếu source/transform khác bằng step ID trần trong FROM/JOIN.
- incremental_by_time SQL dùng @start / @end cho khoảng thời gian."#;

/// System prompt cố định: buộc model trả DUY NHẤT một object JSON hợp lệ.
pub fn system_prompt() -> String {
    format!(
        "Bạn là kỹ sư dữ liệu. Sinh MỘT định nghĩa flow ETL/ELT theo đúng DSL dưới đây từ \
         yêu cầu người dùng. TRẢ VỀ DUY NHẤT một object JSON hợp lệ (không giải thích, \
         không markdown). Dùng đúng tên bảng/cột trong schema nguồn (nếu có).\n\n{DSL_SPEC}"
    )
}

/// Prompt người dùng: mô tả + (tùy chọn) schema nguồn đã introspect (mảng `tables`).
/// Schema đưa vào để model dùng đúng tên bảng/cột thay vì bịa.
pub fn build_prompt(description: &str, introspection: Option<&Value>) -> String {
    let mut p = format!("YÊU CẦU:\n{}\n", description.trim());
    if let Some(tables) = introspection
        .and_then(|v| v.get("tables"))
        .and_then(|t| t.as_array())
    {
        p.push_str("\nSCHEMA NGUỒN (dùng đúng tên bảng/cột):\n");
        for t in tables {
            let schema = t.get("schema").and_then(|s| s.as_str());
            let name = t.get("name").and_then(|s| s.as_str()).unwrap_or("?");
            let full = match schema {
                Some(s) if !s.is_empty() => format!("{s}.{name}"),
                _ => name.to_string(),
            };
            let cols: Vec<String> = t
                .get("columns")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            let cn = c.get("name").and_then(|x| x.as_str())?;
                            let dt = c.get("data_type").and_then(|x| x.as_str()).unwrap_or("");
                            Some(format!("{cn} {dt}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
            p.push_str(&format!("- {full}({})\n", cols.join(", ")));
        }
    }
    p.push_str("\nTRẢ VỀ object JSON flow:");
    p
}

/// Rút object JSON từ text model (có thể kèm fence ```json hoặc lời dẫn) rồi parse +
/// validate về `FlowDef`. Không auto-enable/lưu — chỉ trả draft để agent kiểm.
pub fn parse_draft(text: &str) -> Result<FlowDef> {
    let json = extract_json_object(text)
        .ok_or_else(|| anyhow!("không tìm thấy object JSON trong phản hồi model"))?;
    let def = flow::parse(&json)?;
    flow::validate(&def).map_err(|errs| {
        let joined = errs
            .iter()
            .map(|e| format!("[{}] {}: {}", e.step_id, e.field, e.message))
            .collect::<Vec<_>>()
            .join("; ");
        anyhow!("draft flow không hợp lệ: {joined}")
    })?;
    Ok(def)
}

/// Cắt object JSON đầu tiên: từ `{` đầu tới `}` cân bằng ngoặc cuối cùng, bỏ qua ngoặc
/// trong chuỗi (có thoát). Bỏ fence markdown quanh nó tự nhiên vì chỉ quét trong khoảng.
fn extract_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        let c = b as char;
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompt_includes_description_and_schema() {
        let intro = json!({
            "tables": [{
                "schema": "public", "name": "orders",
                "columns": [{"name": "id", "data_type": "int"}, {"name": "amount", "data_type": "numeric"}]
            }]
        });
        let p = build_prompt("Đồng bộ orders mỗi ngày", Some(&intro));
        assert!(p.contains("Đồng bộ orders mỗi ngày"));
        assert!(p.contains("public.orders"));
        assert!(p.contains("amount numeric"));
    }

    #[test]
    fn prompt_without_schema_is_fine() {
        let p = build_prompt("mô tả", None);
        assert!(p.contains("mô tả"));
        assert!(!p.contains("SCHEMA NGUỒN"));
    }

    #[test]
    fn parse_draft_from_fenced_response() {
        // Model trả kèm lời dẫn + fence — vẫn rút được object.
        let text = "Đây là flow của bạn:\n```json\n{\
            \"flow\": \"ev\", \
            \"sources\": [{\"id\": \"e\", \"connection\": \"c\", \"table\": \"t\", \"mode\": \"full_refresh\"}]\
        }\n```\nHy vọng giúp ích.";
        let def = parse_draft(text).unwrap();
        assert_eq!(def.flow, "ev");
        assert_eq!(def.sources.len(), 1);
    }

    #[test]
    fn parse_draft_rejects_invalid_flow() {
        // mode sai → validate fail → lỗi (không trả draft rác).
        let text = "{\"flow\": \"ev\", \"sources\": [{\"id\": \"e\", \"connection\": \"c\", \
                    \"table\": \"t\", \"mode\": \"bogus\"}]}";
        let err = parse_draft(text).unwrap_err().to_string();
        assert!(err.contains("không hợp lệ"), "{err}");
    }

    #[test]
    fn parse_draft_no_json_errors() {
        assert!(parse_draft("xin lỗi tôi không thể").is_err());
    }

    #[test]
    fn extract_handles_braces_in_strings() {
        let text = r#"{"flow":"x","sources":[{"id":"a","connection":"c","query":"SELECT '{' AS b","mode":"full_refresh"}]}"#;
        let extracted = extract_json_object(text).unwrap();
        assert!(extracted.starts_with('{') && extracted.ends_with('}'));
    }
}
