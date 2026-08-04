//! AI qua bridge SenClaw (không bao giờ gọi thẳng provider):
//! * [`generate`] — sinh test case từ mô tả tự nhiên / OpenAPI / curl mẫu.
//!   Model bị ép trả về MẢNG JSON đúng schema case của app; parse có bước
//!   cứu vãn (bóc code fence, cắt từ `[` đầu đến `]` cuối). `finish=="length"`
//!   bị coi là LỖI — JSON bị cắt giữa chừng không cứu được (xem memory
//!   bridge llm.request output ceiling).
//! * [`diagnose`] — nhận run fail + log, trả chẩn đoán markdown. Số liệu/log
//!   cung cấp là chân lý — prompt cấm bịa.

use app_space_sdk::SpaceClient;
use serde_json::{json, Value};

const GENERATE_SYSTEM: &str = r#"Bạn là kỹ sư QA tự động hoá. Nhiệm vụ: từ mô tả của người dùng (mô tả tính năng, đoạn OpenAPI, lệnh curl, hoặc yêu cầu kiểm thử), sinh ra danh sách test case dưới dạng MẢNG JSON THUẦN — không markdown, không giải thích, không code fence.

Schema mỗi phần tử:
{
  "name": "tên ngắn gọn tiếng Việt",
  "kind": "http" | "script" | "web",
  "timeout_ms": 30000,
  "config": … tuỳ kind …,
  "assertions": [ … ],
  "extract": [ … ]  // tuỳ chọn
}

kind "http": config = {"method":"GET|POST|…","url":"…","headers":{…},"body":"chuỗi hoặc object JSON"}.
kind "script": config = {"command":"lệnh shell","cwd":"tuỳ chọn","env":{…}}.
kind "web": config = {"steps":[{"action":"navigate","url":"…"}|{"action":"act","instruction":"mô tả hành động tiếng tự nhiên"}|{"action":"wait","ms":1000}]}.

assertions — các loại:
  http:   {"type":"status","op":"eq|ne|lt|gte…","value":200} · {"type":"json","path":"data.x","op":"eq|exists|contains|gt…","value":…} · {"type":"body_contains","value":"…"} · {"type":"header","name":"content-type","value":"json"} · {"type":"duration_max_ms","value":2000}
  script: {"type":"exit_code","value":0} · {"type":"stdout_contains","value":"…"} · {"type":"stdout_matches","value":"regex"} · {"type":"stderr_contains","value":"…"}
  web:    {"type":"text_contains","value":"…"} · {"type":"text_not_contains","value":"…"} · {"type":"url_contains","value":"…"}

extract (trích biến cho case sau): {"var":"token","from":"json","path":"data.token"} · {"var":"x","from":"header","name":"x-id"} · {"var":"y","from":"regex","pattern":"id=(\\d+)"}.

Dùng biến {{tên_biến}} trong mọi chuỗi (vd "{{base_url}}/login"). Nếu người dùng cho danh sách biến môi trường sẵn có, hãy dùng chúng thay vì hard-code. Sinh test THIẾT THỰC: happy path + lỗi chính (401/404/validation), mỗi case ít assertion nhưng đúng trọng tâm. TRẢ VỀ DUY NHẤT mảng JSON."#;

/// Bóc mảng JSON từ text model trả về (bỏ code fence, cắt `[`…`]`).
pub fn parse_cases(text: &str) -> Result<Vec<Value>, String> {
    let mut t = text.trim();
    // Bỏ ```json … ``` nếu có.
    if t.starts_with("```") {
        t = t.trim_start_matches("```json").trim_start_matches("```");
        if let Some(end) = t.rfind("```") {
            t = &t[..end];
        }
    }
    let t = t.trim();
    let start = t
        .find('[')
        .ok_or("không tìm thấy '[' trong output của model")?;
    let end = t
        .rfind(']')
        .ok_or("không tìm thấy ']' trong output của model")?;
    if end < start {
        return Err("output của model không phải mảng JSON".into());
    }
    let arr: Value = serde_json::from_str(&t[start..=end])
        .map_err(|e| format!("mảng JSON không parse được: {e}"))?;
    match arr {
        Value::Array(items) => {
            let cases: Vec<Value> = items.into_iter().filter(|c| c.is_object()).collect();
            if cases.is_empty() {
                Err("model trả về mảng rỗng".into())
            } else {
                Ok(cases)
            }
        }
        _ => Err("output không phải mảng JSON".into()),
    }
}

/// Sinh test case từ mô tả. `env_vars_hint` — tên các biến environment sẵn có.
/// Trả về `(cases, model)`.
pub async fn generate(
    sc: &SpaceClient,
    description: &str,
    env_vars_hint: &[String],
) -> Result<(Vec<Value>, String), String> {
    let hint = if env_vars_hint.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nBiến environment sẵn có (dùng dạng {{{{tên}}}}): {}",
            env_vars_hint.join(", ")
        )
    };
    let prompt = format!("Yêu cầu kiểm thử:\n{}{hint}", description.trim());
    let (text, model, finish) = sc
        .llm_request_full(GENERATE_SYSTEM, &prompt, 32000, None)
        .await
        .map_err(|e| format!("bridge LLM lỗi: {e}"))?;
    if finish == "length" {
        return Err(
            "model bị cắt giữa chừng (finish=length) — mô tả ngắn hơn hoặc chia nhỏ yêu cầu".into(),
        );
    }
    let cases = parse_cases(&text)?;
    Ok((cases, model))
}

/// Chẩn đoán một run fail. `run` là output của [`crate::db::Db::get_run`].
pub async fn diagnose(sc: &SpaceClient, run: &Value, question: &str) -> (String, String) {
    let system = "Bạn là kỹ sư QA cứng tay chẩn đoán kết quả kiểm thử tự động. \
        Bạn nhận JSON một lần chạy: trạng thái từng case, từng assertion (desc/pass/actual/expected), \
        log request/response/stdout/stderr. NGUYÊN TẮC: log và số liệu là chân lý — không bịa, \
        không đoán ngoài dữ liệu; kết luận trước (1 câu: lỗi ở đâu), chi tiết sau; phân biệt rõ \
        LỖI SẢN PHẨM (API trả sai) với LỖI TEST (assertion/URL/biến sai, môi trường chưa bật); \
        chỉ ra chính xác assertion nào lệch và giá trị thực tế; đề xuất tối đa 3 bước sửa cụ thể. \
        Trả lời tiếng Việt, markdown gọn.";
    // Rút gọn: chỉ đưa case không-pass + đếm tổng, log đã được runner cắt sẵn.
    let mut slim = run.clone();
    if let Some(results) = slim.get_mut("results").and_then(|r| r.as_array_mut()) {
        results.retain(|c| c["status"] != "pass" && c["status"] != "skipped");
    }
    let question = if question.trim().is_empty() {
        "Vì sao lần chạy này fail? Lỗi sản phẩm hay lỗi test? Sửa thế nào?"
    } else {
        question.trim()
    };
    let prompt = format!(
        "Kết quả lần chạy (JSON, chỉ gồm case không pass):\n{}\n\nCâu hỏi: {question}",
        serde_json::to_string_pretty(&slim).unwrap_or_default()
    );
    match sc.llm_request(system, &prompt, 4000).await {
        Ok((text, model)) => (text.trim().to_string(), model),
        Err(e) => (
            format!("Không gọi được AI qua bridge SenClaw: {e}"),
            String::new(),
        ),
    }
}

/// Kiểm tra một case JSON do AI/agent sinh có đủ trường hợp lệ không;
/// chuẩn hoá về bộ trường app dùng. Trả về (name, kind, timeout_ms, config, assertions, extract).
pub fn normalize_case(c: &Value) -> Result<(String, String, i64, String, String, String), String> {
    let name = c
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return Err("case thiếu name".into());
    }
    let kind = c
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("http")
        .to_string();
    if !matches!(kind.as_str(), "http" | "script" | "web") {
        return Err(format!("case \"{name}\": kind \"{kind}\" không hợp lệ"));
    }
    let timeout_ms = c
        .get("timeout_ms")
        .and_then(|v| v.as_i64())
        .unwrap_or(30000);
    let config = c.get("config").cloned().unwrap_or(json!({}));
    if !config.is_object() {
        return Err(format!("case \"{name}\": config phải là object"));
    }
    let assertions = c.get("assertions").cloned().unwrap_or(json!([]));
    if !assertions.is_array() {
        return Err(format!("case \"{name}\": assertions phải là mảng"));
    }
    let extract = c.get("extract").cloned().unwrap_or(json!([]));
    if !extract.is_array() {
        return Err(format!("case \"{name}\": extract phải là mảng"));
    }
    Ok((
        name,
        kind,
        timeout_ms,
        config.to_string(),
        assertions.to_string(),
        extract.to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_array() {
        let cases = parse_cases(r#"[{"name":"a","kind":"http"}]"#).unwrap();
        assert_eq!(cases.len(), 1);
    }

    #[test]
    fn parse_with_fence_and_chatter() {
        let text = "Đây là test:\n```json\n[{\"name\":\"a\"},{\"name\":\"b\"}]\n```\nXong.";
        let cases = parse_cases(text).unwrap();
        assert_eq!(cases.len(), 2);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_cases("không có json").is_err());
        assert!(parse_cases("[]").is_err());
        assert!(parse_cases("[{\"name\":\"a\"").is_err());
    }

    #[test]
    fn normalize_case_validates() {
        let ok = json!({"name":"t","kind":"http","config":{"url":"x"},"assertions":[{"type":"status","value":200}]});
        let (name, kind, timeout, config, asserts, extract) = normalize_case(&ok).unwrap();
        assert_eq!(
            (name.as_str(), kind.as_str(), timeout),
            ("t", "http", 30000)
        );
        assert!(config.contains("url"));
        assert!(asserts.contains("status"));
        assert_eq!(extract, "[]");

        assert!(normalize_case(&json!({"kind":"http"})).is_err());
        assert!(normalize_case(&json!({"name":"x","kind":"ftp"})).is_err());
        assert!(normalize_case(&json!({"name":"x","config":[]})).is_err());
    }
}
