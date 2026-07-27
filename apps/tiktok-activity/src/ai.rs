//! AI features routed through the SenClaw bridge LLM.
//! Ports internal/agent/flow_generate.go + agent.go + profile.go, but the LLM
//! call goes to `bridge.llm(...)` instead of a per-app OpenAI client.

use crate::bridge::Bridge;
use crate::domain::{BrowserProfile, Flow, FlowAtomic, StrMap};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowGenCatalogItem {
    pub palette_id: String,
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub implementation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowGenAIStep {
    pub palette_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<StrMap>,
    #[serde(skip_serializing_if = "is_zero")]
    pub timeout_seconds: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<StrMap>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub atomics: Vec<FlowAtomic>,
}

fn is_zero(v: &i64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowGenAIResult {
    pub name: String,
    pub params: StrMap,
    pub actions: Vec<FlowGenAIStep>,
}

const FLOWGEN_SYSTEM: &str = r#"Bạn là kỹ sư automation TikTok (flow nhiều bước).
Nhiệm vụ: từ "Catalog" (JSON) và yêu cầu người dùng, trả về DUY NHẤT một object JSON hợp lệ — không markdown, không giải thích ngoài JSON.

Schema bắt buộc:
{"name":"string","params":{},"actions":[{"paletteId":"...","name":"optional","config":{},"timeoutSeconds":15,"params":{},"atomics":[]}]}

Quy tắc:
- Mỗi phần tử actions phải có "paletteId" trùng CHÍNH XÁC một giá trị paletteId trong Catalog (không bịa id).
- Ưu tiên các bước thực tế: mở trang, đăng nhập nếu cần, chờ trang, thao tác nội dung, delay ngẫu nhiên giữa các bước nhạy cảm.
- "config": chỉ các key kiểu string (giá trị string), ví dụ open_url cần "url", search cần "keyword".
- "timeoutSeconds": số nguyên dương hợp lý (mặc định 15–60 tùy bước).
- Thứ tự phần tử trong "actions" là thứ tự chạy tuyến tính; KHÔNG điền _next_on_success — client tự nối nhánh ok.
- "atomics": chỉ điền khi paletteId là playwright_atomics dạng chuỗi tùy chỉnh.
- Nếu có "Context": dùng để hiểu trạng thái UI/account thật; vẫn CHỈ chọn bước từ Catalog."#;

/// Generate a flow from the palette catalog + a natural-language goal.
/// `account_context` carries the account summary / probe transcript / DOM.
pub async fn generate_flow_from_catalog(
    bridge: &Bridge,
    user_prompt: &str,
    catalog: &[FlowGenCatalogItem],
    account_context: &str,
) -> Result<FlowGenAIResult> {
    let prompt = user_prompt.trim();
    if prompt.is_empty() {
        return Err(anyhow!("thiếu prompt"));
    }
    if catalog.is_empty() {
        return Err(anyhow!("actionsCatalog rỗng"));
    }
    let allowed: std::collections::HashSet<String> =
        catalog.iter().map(|c| c.palette_id.trim().to_string()).filter(|s| !s.is_empty()).collect();

    let cat_json = serde_json::to_string(&catalog.iter().map(|c| {
        serde_json::json!({"paletteId": c.palette_id, "type": c.r#type, "name": c.name, "implementation": c.implementation})
    }).collect::<Vec<_>>())?;

    let mut user = format!("Catalog (JSON):\n{cat_json}\n\nYêu cầu người dùng:\n{prompt}");
    if !account_context.trim().is_empty() {
        user.push_str("\n\nContext (account + probe transcript + post-probe DOM; no passwords):\n");
        user.push_str(account_context.trim());
    }

    let reply = bridge.llm(FLOWGEN_SYSTEM, &user, 8000, Duration::from_secs(180)).await?;
    let obj = extract_json_object(&reply.text).ok_or_else(|| anyhow!("LLM không trả JSON hợp lệ"))?;

    let name = obj.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let mut res = FlowGenAIResult {
        name: if name.is_empty() { "Flow (AI)".into() } else { name },
        params: StrMap::new(),
        actions: vec![],
    };
    if let Some(p) = obj.get("params").and_then(Value::as_object) {
        for (k, v) in p {
            let kk = k.trim();
            if kk.is_empty() {
                continue;
            }
            res.params.insert(kk.to_string(), value_to_string(v));
        }
    }

    let actions = obj.get("actions").and_then(Value::as_array).cloned().unwrap_or_default();
    for (i, a) in actions.iter().enumerate() {
        let pid = a.get("paletteId").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if pid.is_empty() {
            return Err(anyhow!("actions[{i}]: thiếu paletteId"));
        }
        if !allowed.contains(&pid) {
            return Err(anyhow!("actions[{i}]: paletteId không có trong catalog: {pid:?}"));
        }
        res.actions.push(FlowGenAIStep {
            palette_id: pid,
            name: a.get("name").and_then(Value::as_str).unwrap_or("").trim().to_string(),
            config: obj_str_map(a.get("config")),
            timeout_seconds: timeout_from_value(a.get("timeoutSeconds")),
            params: obj_str_map(a.get("params")),
            atomics: a
                .get("atomics")
                .and_then(|v| serde_json::from_value::<Vec<FlowAtomic>>(v.clone()).ok())
                .unwrap_or_default(),
        });
    }
    if res.actions.is_empty() {
        return Err(anyhow!("LLM không trả bước actions nào"));
    }
    Ok(res)
}

/// Advisory next-step suggestion for a flow (Planner.SuggestNext).
pub async fn suggest_next(bridge: &Bridge, flow: &Flow) -> Result<String> {
    let names: Vec<String> = flow.actions.iter().map(|a| format!("{} ({})", a.name, a.type_)).collect();
    let user = format!(
        "Flow tên: {}\nCác bước hiện có (thứ tự):\n{}\n\nGợi ý NGẮN GỌN (1-3 câu) bước tiếp theo nên thêm để flow tự nhiên và an toàn hơn với TikTok.",
        flow.name,
        names.join("\n")
    );
    let reply = bridge
        .llm("Bạn là trợ lý thiết kế flow automation TikTok. Trả lời ngắn gọn, thực dụng, tiếng Việt.", &user, 400, Duration::from_secs(60))
        .await?;
    Ok(reply.text.trim().to_string())
}

/// Draft a BrowserProfile. Uses the LLM when reachable; otherwise a heuristic
/// (matching the Go fallback so the endpoint always returns something usable).
pub async fn generate_profile_draft(
    bridge: &Bridge,
    account: Option<&crate::domain::TikTokAccount>,
    note: &str,
) -> BrowserProfile {
    let heuristic = || BrowserProfile {
        name: account.map(|a| format!("Profile {}", a.username)).unwrap_or_else(|| "Profile mới".into()),
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36".into(),
        viewport_width: 1280,
        viewport_height: 800,
        locale: "vi-VN".into(),
        timezone_id: "Asia/Ho_Chi_Minh".into(),
        account_id: account.map(|a| a.id.clone()).unwrap_or_default(),
        notes: note.trim().to_string(),
        ..Default::default()
    };

    let acct = account.map(|a| format!("account username={} id={}", a.username, a.id)).unwrap_or_default();
    let user = format!(
        "Tạo cấu hình BrowserProfile hợp lý (fingerprint) cho automation TikTok.\n{acct}\nGhi chú: {note}\nTrả JSON: {{\"name\":\"\",\"userAgent\":\"\",\"viewportWidth\":1280,\"viewportHeight\":800,\"locale\":\"vi-VN\",\"timezoneId\":\"Asia/Ho_Chi_Minh\"}}"
    );
    match bridge
        .llm("Bạn tạo fingerprint trình duyệt hợp lệ. Trả DUY NHẤT một JSON object.", &user, 500, Duration::from_secs(60))
        .await
    {
        Ok(reply) => match extract_json_object(&reply.text) {
            Some(obj) => BrowserProfile {
                name: obj.get("name").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(|s| s.to_string()).unwrap_or_else(|| heuristic().name),
                user_agent: obj.get("userAgent").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(|s| s.to_string()).unwrap_or_else(|| heuristic().user_agent),
                viewport_width: obj.get("viewportWidth").and_then(Value::as_i64).unwrap_or(1280) as i32,
                viewport_height: obj.get("viewportHeight").and_then(Value::as_i64).unwrap_or(800) as i32,
                locale: obj.get("locale").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("vi-VN").to_string(),
                timezone_id: obj.get("timezoneId").and_then(Value::as_str).filter(|s| !s.trim().is_empty()).unwrap_or("Asia/Ho_Chi_Minh").to_string(),
                account_id: account.map(|a| a.id.clone()).unwrap_or_default(),
                notes: note.trim().to_string(),
                ..Default::default()
            },
            None => heuristic(),
        },
        Err(_) => heuristic(),
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.trim().to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn obj_str_map(v: Option<&Value>) -> Option<StrMap> {
    let obj = v?.as_object()?;
    let mut m = StrMap::new();
    for (k, val) in obj {
        let kk = k.trim();
        if kk.is_empty() {
            continue;
        }
        m.insert(kk.to_string(), value_to_string(val));
    }
    if m.is_empty() {
        None
    } else {
        Some(m)
    }
}

fn timeout_from_value(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().filter(|f| *f > 0.0).map(|f| (f + 0.5) as i64).unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().filter(|f| *f > 0.0).map(|f| (f + 0.5) as i64).unwrap_or(0),
        _ => 0,
    }
}

/// Pull the first balanced top-level JSON object out of a raw LLM reply
/// (tolerates ```json fences and surrounding prose). Ported from decodeJSONObject.
pub fn extract_json_object(raw: &str) -> Option<Value> {
    let s = raw.trim().trim_start_matches("```json").trim_start_matches("```").trim_end_matches("```").trim();
    if let Ok(v) = serde_json::from_str::<Value>(s) {
        if v.is_object() {
            return Some(v);
        }
    }
    let bytes = s.as_bytes();
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
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
                    return serde_json::from_str::<Value>(&s[start..=i]).ok().filter(Value::is_object);
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

    #[test]
    fn extract_plain_object() {
        let v = extract_json_object(r#"{"name":"x","actions":[]}"#).unwrap();
        assert_eq!(v["name"], "x");
    }

    #[test]
    fn extract_fenced_object() {
        let raw = "trước\n```json\n{\"name\":\"y\",\"a\":1}\n```\nsau";
        let v = extract_json_object(raw).unwrap();
        assert_eq!(v["name"], "y");
    }

    #[test]
    fn extract_object_with_nested_braces() {
        let raw = r#"blah {"name":"z","config":{"k":"{v}"}} tail"#;
        let v = extract_json_object(raw).unwrap();
        assert_eq!(v["config"]["k"], "{v}");
    }
}
