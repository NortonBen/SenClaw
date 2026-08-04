//! Thực thi test. Ba loại case:
//!
//! * `http`   — gọi API bằng reqwest: `{method,url,headers{},body}`.
//! * `script` — chạy lệnh shell (`sh -c`): `{command,cwd,env{}}`. Chỉ chạy
//!              lệnh NGƯỜI DÙNG tự định nghĩa trong test case của họ.
//! * `web`    — điều khiển app Mini Browser (port 4360) qua MCP HTTP:
//!              `{steps:[{action:navigate|act|wait,...}]}`; cuối chuỗi bước tự
//!              lấy text trang + URL cho assertion.
//!
//! Biến `{{var}}` được thay trong TOÀN BỘ chuỗi của config/assertions/extract
//! (thay sau khi parse JSON, trên từng string — không bao giờ phá vỡ JSON dù
//! giá trị biến chứa dấu nháy). Rule `extract` kéo giá trị từ response ra
//! biến, chảy sang các case sau trong cùng lần chạy suite (login lấy token →
//! case sau dùng).

use crate::assert::{self, Outcome};
use crate::db::{CaseRow, Db};
use crate::tmpl::{self, Vars};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Runner {
    pub db: Arc<Db>,
    http: reqwest::Client,
    cancels: Mutex<HashSet<i64>>,
}

/// Kết quả thực thi một case (chưa ghi DB).
pub struct CaseResult {
    pub status: &'static str,
    pub duration_ms: i64,
    pub log: String,
    pub assertions: Vec<Value>,
    pub error: String,
}

/// Thay `{{var}}` trong mọi string của cây JSON; trả về danh sách biến thiếu.
pub fn substitute_value(v: &mut Value, vars: &Vars) -> Vec<String> {
    let mut missing = vec![];
    fn walk(v: &mut Value, vars: &Vars, missing: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                let (out, miss) = tmpl::substitute(s, vars);
                *s = out;
                for m in miss {
                    if !missing.contains(&m) {
                        missing.push(m);
                    }
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(x, vars, missing)),
            Value::Object(o) => o.values_mut().for_each(|x| walk(x, vars, missing)),
            _ => {}
        }
    }
    walk(v, vars, &mut missing);
    missing
}

fn clip(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let cut: String = s.chars().take(max_chars).collect();
    format!("{cut}… (cắt bớt, tổng {} ký tự)", s.chars().count())
}

impl Runner {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            http: reqwest::Client::new(),
            cancels: Mutex::new(HashSet::new()),
        }
    }

    // ---- cancel ----

    pub fn request_cancel(&self, run_id: i64) {
        self.cancels.lock().unwrap().insert(run_id);
    }
    fn is_cancelled(&self, run_id: i64) -> bool {
        self.cancels.lock().unwrap().contains(&run_id)
    }
    fn clear_cancel(&self, run_id: i64) {
        self.cancels.lock().unwrap().remove(&run_id);
    }

    // ---- vars ----

    fn env_vars(&self, env_id: Option<i64>) -> Vars {
        let mut vars = Vars::new();
        if let Some(id) = env_id {
            if let Some((_, vars_json)) = self.db.env_get(id) {
                if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&vars_json) {
                    for (k, v) in map {
                        vars.insert(k, tmpl::value_to_string(&v));
                    }
                }
            }
        }
        vars
    }

    // ---- chạy suite / case đơn ----

    /// Chạy cả suite (blocking đến khi xong — caller muốn nền thì tokio::spawn).
    /// Trả về run_id.
    pub async fn run_suite(
        &self,
        suite_id: i64,
        env_override: Option<i64>,
        trigger: &str,
    ) -> Result<i64> {
        let suite = self
            .db
            .get_suite(suite_id)
            .ok_or_else(|| anyhow!("không có suite #{suite_id}"))?;
        let cases = self.db.list_cases(suite_id);
        if cases.is_empty() {
            return Err(anyhow!("suite #{suite_id} chưa có test case nào"));
        }
        let env_id = env_override.or_else(|| suite["env_id"].as_i64());
        let run_id = self.db.create_run(Some(suite_id), None, env_id, trigger)?;
        self.db.log(
            "run",
            &format!(
                "chạy suite \"{}\" ({} case, trigger {trigger})",
                suite["name"].as_str().unwrap_or(""),
                cases.len()
            ),
            &run_id.to_string(),
        );
        let mut vars = self.env_vars(env_id);
        let (mut n_fail, mut n_err) = (0, 0);
        let mut cancelled = false;
        for case in &cases {
            if self.is_cancelled(run_id) {
                cancelled = true;
                break;
            }
            if !case.enabled {
                self.db.add_result(
                    run_id, case.id, &case.name, &case.kind, "skipped", 0, "", "[]", "",
                );
                continue;
            }
            let r = self.exec_case(case, &mut vars).await;
            match r.status {
                "fail" => n_fail += 1,
                "error" => n_err += 1,
                _ => {}
            }
            self.db.add_result(
                run_id,
                case.id,
                &case.name,
                &case.kind,
                r.status,
                r.duration_ms,
                &r.log,
                &Value::Array(r.assertions).to_string(),
                &r.error,
            );
        }
        let status = if cancelled {
            "cancelled"
        } else if n_fail > 0 {
            "fail"
        } else if n_err > 0 {
            "error"
        } else {
            "pass"
        };
        self.db.finish_run(run_id, status);
        self.clear_cancel(run_id);
        self.db.log(
            "run",
            &format!("run #{run_id} kết thúc: {status}"),
            &run_id.to_string(),
        );
        Ok(run_id)
    }

    /// Chạy một case đơn lẻ (vẫn tạo run để có lịch sử đồng nhất).
    pub async fn run_case_solo(
        &self,
        case_id: i64,
        env_override: Option<i64>,
        trigger: &str,
    ) -> Result<i64> {
        let case = self
            .db
            .get_case(case_id)
            .ok_or_else(|| anyhow!("không có case #{case_id}"))?;
        let suite = self.db.get_suite(case.suite_id);
        let env_id = env_override.or_else(|| suite.as_ref().and_then(|s| s["env_id"].as_i64()));
        let run_id = self
            .db
            .create_run(Some(case.suite_id), Some(case_id), env_id, trigger)?;
        let mut vars = self.env_vars(env_id);
        let r = self.exec_case(&case, &mut vars).await;
        let status = match r.status {
            "pass" => "pass",
            "error" => "error",
            _ => "fail",
        };
        self.db.add_result(
            run_id,
            case.id,
            &case.name,
            &case.kind,
            r.status,
            r.duration_ms,
            &r.log,
            &Value::Array(r.assertions).to_string(),
            &r.error,
        );
        self.db.finish_run(run_id, status);
        self.db.log(
            "run",
            &format!("chạy case \"{}\": {status}", case.name),
            &run_id.to_string(),
        );
        Ok(run_id)
    }

    // ---- thực thi một case ----

    pub async fn exec_case(&self, case: &CaseRow, vars: &mut Vars) -> CaseResult {
        let started = Instant::now();
        let mut log = String::new();

        let mut config: Value = serde_json::from_str(&case.config).unwrap_or(json!({}));
        let mut asserts: Value = serde_json::from_str(&case.assertions).unwrap_or(json!([]));
        let mut extract: Value = serde_json::from_str(&case.extract).unwrap_or(json!([]));
        let mut missing = substitute_value(&mut config, vars);
        missing.extend(substitute_value(&mut asserts, vars));
        missing.extend(substitute_value(&mut extract, vars));
        if !missing.is_empty() {
            missing.dedup();
            let _ = writeln!(log, "⚠ biến chưa có giá trị: {}", missing.join(", "));
        }

        let timeout = Duration::from_millis(case.timeout_ms.clamp(100, 600_000) as u64);
        let exec = async {
            match case.kind.as_str() {
                "http" => self.exec_http(&config, &mut log).await,
                "script" => self.exec_script(&config, timeout, &mut log).await,
                "web" => self.exec_web(&config, &mut log).await,
                other => Err(anyhow!("kind không hỗ trợ: {other}")),
            }
        };
        let outcome = match tokio::time::timeout(timeout, exec).await {
            Err(_) => {
                return CaseResult {
                    status: "error",
                    duration_ms: started.elapsed().as_millis() as i64,
                    log,
                    assertions: vec![],
                    error: format!("timeout sau {}ms", timeout.as_millis()),
                }
            }
            Ok(Err(e)) => {
                return CaseResult {
                    status: "error",
                    duration_ms: started.elapsed().as_millis() as i64,
                    log,
                    assertions: vec![],
                    error: e.to_string(),
                }
            }
            Ok(Ok(mut o)) => {
                o.duration_ms = started.elapsed().as_millis() as u64;
                o
            }
        };

        // Assertions.
        let specs: Vec<Value> = asserts.as_array().cloned().unwrap_or_default();
        let (results, all_pass) = assert::evaluate_all(&specs, &outcome);
        for r in &results {
            let ok = r["pass"].as_bool().unwrap_or(false);
            let _ = writeln!(
                log,
                "{} {} (thực tế: {})",
                if ok { "✓" } else { "✗" },
                r["desc"].as_str().unwrap_or(""),
                clip(r["actual"].as_str().unwrap_or(""), 120)
            );
        }

        // Extract → vars cho case sau.
        for rule in extract.as_array().cloned().unwrap_or_default() {
            let var = rule
                .get("var")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if var.is_empty() {
                continue;
            }
            match extract_var(&rule, &outcome) {
                Some(val) => {
                    let _ = writeln!(log, "→ biến {var} = {}", clip(&val, 120));
                    vars.insert(var, val);
                }
                None => {
                    let _ = writeln!(log, "⚠ không trích được biến {var}");
                }
            }
        }

        CaseResult {
            status: if all_pass { "pass" } else { "fail" },
            duration_ms: outcome.duration_ms as i64,
            log,
            assertions: results,
            error: String::new(),
        }
    }

    async fn exec_http(&self, config: &Value, log: &mut String) -> Result<Outcome> {
        let method = config
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET")
            .to_uppercase();
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if url.is_empty() {
            return Err(anyhow!("case http thiếu config.url"));
        }
        let m = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| anyhow!("method không hợp lệ: {method}"))?;
        let mut req = self.http.request(m, &url);
        if let Some(Value::Object(headers)) = config.get("headers") {
            for (k, v) in headers {
                req = req.header(k.as_str(), tmpl::value_to_string(v));
            }
        }
        match config.get("body") {
            None | Some(Value::Null) => {}
            Some(Value::String(s)) if s.is_empty() => {}
            // Body object/array → JSON; string → gửi nguyên văn.
            Some(Value::String(s)) => req = req.body(s.clone()),
            Some(other) => {
                req = req
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(other.to_string());
            }
        }
        let _ = writeln!(log, "{method} {url}");
        let resp = req.send().await.map_err(|e| anyhow!("request lỗi: {e}"))?;
        let status = resp.status().as_u16();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_lowercase(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        let body = resp.text().await.unwrap_or_default();
        let _ = writeln!(log, "← {status}, {} byte", body.len());
        let _ = writeln!(log, "{}", clip(&body, 1500));
        Ok(Outcome {
            status: Some(status),
            headers,
            body,
            ..Default::default()
        })
    }

    async fn exec_script(
        &self,
        config: &Value,
        timeout: Duration,
        log: &mut String,
    ) -> Result<Outcome> {
        let command = config
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if command.trim().is_empty() {
            return Err(anyhow!("case script thiếu config.command"));
        }
        let _ = writeln!(log, "$ {command}");
        let mut cmd = tokio::process::Command::new("sh");
        cmd.arg("-c").arg(&command);
        if let Some(cwd) = config
            .get("cwd")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            cmd.current_dir(cwd);
        }
        if let Some(Value::Object(env)) = config.get("env") {
            for (k, v) in env {
                cmd.env(k, tmpl::value_to_string(v));
            }
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        cmd.kill_on_drop(true);
        let child = cmd
            .spawn()
            .map_err(|e| anyhow!("không spawn được lệnh: {e}"))?;
        // Timeout tổng của case đã bao ngoài (tokio::time::timeout trong exec_case);
        // kill_on_drop đảm bảo process con bị dọn khi future bị hủy vì timeout.
        let out = child
            .wait_with_output()
            .await
            .map_err(|e| anyhow!("chờ lệnh lỗi: {e}"))?;
        let _ = timeout; // đã enforce ở exec_case
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let exit_code = out.status.code().unwrap_or(-1);
        let _ = writeln!(log, "exit {exit_code}");
        if !stdout.is_empty() {
            let _ = writeln!(log, "stdout:\n{}", clip(&stdout, 1500));
        }
        if !stderr.is_empty() {
            let _ = writeln!(log, "stderr:\n{}", clip(&stderr, 1000));
        }
        Ok(Outcome {
            exit_code: Some(exit_code),
            stdout,
            stderr,
            ..Default::default()
        })
    }

    // ---- web qua Mini Browser MCP ----

    fn browser_base(&self) -> String {
        self.db
            .get_setting("browser_url")
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("AUTOTEST_BROWSER_URL").ok())
            .unwrap_or_else(|| "http://127.0.0.1:4360".to_string())
            .trim_end_matches('/')
            .to_string()
    }

    /// Gọi một tool MCP của Mini Browser. Trả về text content đầu tiên.
    async fn browser_call(&self, base: &str, name: &str, args: Value) -> Result<String> {
        let body = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": { "name": name, "arguments": args }
        });
        let v: Value = self
            .http
            .post(format!("{base}/api/mcp/message"))
            .json(&body)
            .timeout(Duration::from_secs(90))
            .send()
            .await
            .map_err(|e| anyhow!("không gọi được Mini Browser ({base}) — app Mini Browser có đang chạy không? {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Mini Browser trả về không phải JSON: {e}"))?;
        let result = &v["result"];
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if result["isError"].as_bool().unwrap_or(false) {
            return Err(anyhow!("{name} lỗi: {}", clip(&text, 300)));
        }
        Ok(text)
    }

    async fn exec_web(&self, config: &Value, log: &mut String) -> Result<Outcome> {
        let base = self.browser_base();
        let steps = config
            .get("steps")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if steps.is_empty() {
            return Err(anyhow!("case web thiếu config.steps"));
        }
        for (i, step) in steps.iter().enumerate() {
            let action = step.get("action").and_then(|v| v.as_str()).unwrap_or("");
            match action {
                "navigate" => {
                    let url = step.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    if url.is_empty() {
                        return Err(anyhow!("bước {} navigate thiếu url", i + 1));
                    }
                    let _ = writeln!(log, "[{}] navigate {url}", i + 1);
                    self.browser_call(&base, "browser_navigate", json!({ "url": url }))
                        .await?;
                }
                "act" => {
                    let instruction = step
                        .get("instruction")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if instruction.is_empty() {
                        return Err(anyhow!("bước {} act thiếu instruction", i + 1));
                    }
                    let _ = writeln!(log, "[{}] act: {instruction}", i + 1);
                    let out = self
                        .browser_call(&base, "browser_act", json!({ "instruction": instruction }))
                        .await?;
                    let _ = writeln!(log, "    {}", clip(&out, 300));
                }
                "wait" => {
                    let ms = step
                        .get("ms")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1000)
                        .min(30_000);
                    let _ = writeln!(log, "[{}] wait {ms}ms", i + 1);
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                }
                other => {
                    return Err(anyhow!(
                        "bước {}: action không hỗ trợ \"{other}\" (navigate|act|wait)",
                        i + 1
                    ))
                }
            }
        }
        // Chốt trạng thái trang cho assertion.
        let page_text = self
            .browser_call(&base, "browser_extract_text", json!({}))
            .await
            .unwrap_or_default();
        let final_url = match self
            .browser_call(&base, "browser_get_info", json!({}))
            .await
        {
            Ok(info_text) => serde_json::from_str::<Value>(&info_text)
                .ok()
                .and_then(|v| v.get("url").and_then(|u| u.as_str()).map(String::from))
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        let _ = writeln!(log, "URL cuối: {final_url}");
        let _ = writeln!(log, "text trang: {}", clip(&page_text, 800));
        Ok(Outcome {
            page_text,
            final_url,
            ..Default::default()
        })
    }
}

/// Trích một biến từ outcome theo rule:
/// `{var, from: "json", path}` (body JSON) · `{var, from:"header", name}` ·
/// `{var, from:"regex", pattern[, group]}` — regex chạy trên body (http),
/// stdout (script) hoặc text trang (web), lấy group 1 mặc định.
fn extract_var(rule: &Value, outcome: &Outcome) -> Option<String> {
    let from = rule.get("from").and_then(|v| v.as_str()).unwrap_or("json");
    match from {
        "json" => {
            let path = rule.get("path").and_then(|v| v.as_str())?;
            let root = outcome.body_json()?;
            tmpl::json_path(&root, path).map(tmpl::value_to_string)
        }
        "header" => {
            let name = rule.get("name").and_then(|v| v.as_str())?.to_lowercase();
            outcome.headers.get(&name).cloned()
        }
        "regex" => {
            let pattern = rule.get("pattern").and_then(|v| v.as_str())?;
            let group = rule.get("group").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
            let re = regex::Regex::new(pattern).ok()?;
            let hay = if !outcome.body.is_empty() {
                &outcome.body
            } else if !outcome.stdout.is_empty() {
                &outcome.stdout
            } else {
                &outcome.page_text
            };
            re.captures(hay)
                .and_then(|c| {
                    c.get(group.min(c.len().saturating_sub(1)))
                        .or_else(|| c.get(0))
                })
                .map(|m| m.as_str().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::{get, post};
    use axum::{Json, Router};

    fn mem_runner() -> Runner {
        Runner::new(Arc::new(Db::open_memory().unwrap()))
    }

    /// Server API thật (loopback) để test executor http đầu-cuối.
    async fn spawn_test_api() -> String {
        let app = Router::new()
            .route(
                "/health",
                get(|| async { Json(json!({"ok": true, "data": {"token": "tok-1"}})) }),
            )
            .route("/echo", post(|body: String| async move { body }))
            .route(
                "/slow",
                get(|| async {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    "slow"
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }

    fn case(kind: &str, config: Value, asserts: Value, extract: Value) -> CaseRow {
        CaseRow {
            id: 1,
            suite_id: 1,
            name: "t".into(),
            kind: kind.into(),
            position: 1,
            enabled: true,
            timeout_ms: 5000,
            config: config.to_string(),
            assertions: asserts.to_string(),
            extract: extract.to_string(),
        }
    }

    #[tokio::test]
    async fn http_case_pass_and_extract() {
        let base = spawn_test_api().await;
        let r = mem_runner();
        let mut vars = Vars::new();
        vars.insert("base_url".into(), base);
        let c = case(
            "http",
            json!({"method":"GET","url":"{{base_url}}/health"}),
            json!([{"type":"status","value":200},{"type":"json","path":"data.token","op":"exists"}]),
            json!([{"var":"token","from":"json","path":"data.token"}]),
        );
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "pass", "log: {}", out.log);
        assert_eq!(vars.get("token").map(String::as_str), Some("tok-1"));
        assert_eq!(out.assertions.len(), 2);
    }

    #[tokio::test]
    async fn http_case_assertion_fail() {
        let base = spawn_test_api().await;
        let r = mem_runner();
        let mut vars = Vars::new();
        let c = case(
            "http",
            json!({"method":"GET","url": format!("{base}/health")}),
            json!([{"type":"status","value":404}]),
            json!([]),
        );
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "fail");
        assert!(!out.assertions[0]["pass"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn http_case_timeout_is_error() {
        let base = spawn_test_api().await;
        let r = mem_runner();
        let mut vars = Vars::new();
        let mut c = case(
            "http",
            json!({"method":"GET","url": format!("{base}/slow")}),
            json!([]),
            json!([]),
        );
        c.timeout_ms = 100;
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "error");
        assert!(out.error.contains("timeout"));
    }

    #[tokio::test]
    async fn http_body_object_sent_as_json() {
        let base = spawn_test_api().await;
        let r = mem_runner();
        let mut vars = Vars::new();
        let c = case(
            "http",
            json!({"method":"POST","url": format!("{base}/echo"), "body": {"name":"vn"}}),
            json!([{"type":"body_contains","value":"\"name\":\"vn\""}]),
            json!([]),
        );
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "pass", "log: {}", out.log);
    }

    #[tokio::test]
    async fn script_case_pass_and_fail() {
        let r = mem_runner();
        let mut vars = Vars::new();
        vars.insert("msg".into(), "xin chào".into());
        let c = case(
            "script",
            json!({"command":"echo {{msg}}"}),
            json!([{"type":"exit_code","value":0},{"type":"stdout_contains","value":"xin chào"}]),
            json!([{"var":"first","from":"regex","pattern":"(\\S+)"}]),
        );
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "pass", "log: {}", out.log);
        assert_eq!(vars.get("first").map(String::as_str), Some("xin"));

        let c2 = case(
            "script",
            json!({"command":"exit 3"}),
            json!([{"type":"exit_code","value":0}]),
            json!([]),
        );
        let out2 = r.exec_case(&c2, &mut vars).await;
        assert_eq!(out2.status, "fail");
    }

    #[tokio::test]
    async fn script_env_passed() {
        let r = mem_runner();
        let mut vars = Vars::new();
        let c = case(
            "script",
            json!({"command":"echo $MY_FLAG", "env": {"MY_FLAG": "bật"}}),
            json!([{"type":"stdout_contains","value":"bật"}]),
            json!([]),
        );
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "pass", "log: {}", out.log);
    }

    #[tokio::test]
    async fn script_timeout_kills() {
        let r = mem_runner();
        let mut vars = Vars::new();
        let mut c = case(
            "script",
            json!({"command":"sleep 30"}),
            json!([]),
            json!([]),
        );
        c.timeout_ms = 200;
        let started = Instant::now();
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "error");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn web_case_without_browser_is_error() {
        let r = mem_runner();
        r.db.set_setting("browser_url", "http://127.0.0.1:1"); // chắc chắn không có gì lắng nghe
        let mut vars = Vars::new();
        let c = case(
            "web",
            json!({"steps":[{"action":"navigate","url":"http://example.com"}]}),
            json!([]),
            json!([]),
        );
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "error");
        assert!(out.error.contains("Mini Browser"), "err: {}", out.error);
    }

    #[tokio::test]
    async fn unknown_kind_is_error() {
        let r = mem_runner();
        let mut vars = Vars::new();
        let c = case("ftp", json!({}), json!([]), json!([]));
        let out = r.exec_case(&c, &mut vars).await;
        assert_eq!(out.status, "error");
    }

    #[tokio::test]
    async fn run_suite_end_to_end() {
        let base = spawn_test_api().await;
        let db = Arc::new(Db::open_memory().unwrap());
        let r = Runner::new(db.clone());
        let env_id = db
            .env_set("test", &json!({"base_url": base}).to_string())
            .unwrap();
        let sid = db.add_suite("smoke", "", Some(env_id)).unwrap();
        db.add_case(
            sid,
            "health",
            "http",
            None,
            true,
            5000,
            &json!({"method":"GET","url":"{{base_url}}/health"}).to_string(),
            &json!([{"type":"status","value":200}]).to_string(),
            &json!([{"var":"token","from":"json","path":"data.token"}]).to_string(),
        )
        .unwrap();
        // Case sau dùng biến token do case trước trích ra.
        db.add_case(
            sid,
            "echo token",
            "http",
            None,
            true,
            5000,
            &json!({"method":"POST","url":"{{base_url}}/echo","body":"tk={{token}}"}).to_string(),
            &json!([{"type":"body_contains","value":"tk=tok-1"}]).to_string(),
            "[]",
        )
        .unwrap();
        // Case disabled → skipped.
        db.add_case(sid, "tắt", "http", None, false, 5000, "{}", "[]", "[]")
            .unwrap();

        let run_id = r.run_suite(sid, None, "manual").await.unwrap();
        let run = db.get_run(run_id).unwrap();
        assert_eq!(run["status"], "pass", "run: {run}");
        assert_eq!(run["passed"], 2);
        assert_eq!(run["skipped"], 1);
    }

    #[tokio::test]
    async fn run_suite_fail_status() {
        let base = spawn_test_api().await;
        let db = Arc::new(Db::open_memory().unwrap());
        let r = Runner::new(db.clone());
        let sid = db.add_suite("s", "", None).unwrap();
        db.add_case(
            sid,
            "bad",
            "http",
            None,
            true,
            5000,
            &json!({"method":"GET","url": format!("{base}/health")}).to_string(),
            &json!([{"type":"status","value":500}]).to_string(),
            "[]",
        )
        .unwrap();
        let run_id = r.run_suite(sid, None, "manual").await.unwrap();
        assert_eq!(db.get_run(run_id).unwrap()["status"], "fail");
    }

    #[tokio::test]
    async fn run_case_solo_records_history() {
        let base = spawn_test_api().await;
        let db = Arc::new(Db::open_memory().unwrap());
        let r = Runner::new(db.clone());
        let sid = db.add_suite("s", "", None).unwrap();
        let cid = db
            .add_case(
                sid,
                "health",
                "http",
                None,
                true,
                5000,
                &json!({"method":"GET","url": format!("{base}/health")}).to_string(),
                &json!([{"type":"status","value":200}]).to_string(),
                "[]",
            )
            .unwrap();
        let run_id = r.run_case_solo(cid, None, "mcp").await.unwrap();
        let run = db.get_run(run_id).unwrap();
        assert_eq!(run["status"], "pass");
        assert_eq!(run["case_id"], cid);
    }

    #[test]
    fn substitute_value_never_breaks_json() {
        let mut vars = Vars::new();
        vars.insert("q".into(), "he said \"hi\"".into());
        let mut v = json!({"body": "{{q}}", "n": 5});
        let missing = substitute_value(&mut v, &vars);
        assert!(missing.is_empty());
        assert_eq!(v["body"], "he said \"hi\"");
        // Vẫn là JSON hợp lệ có thể serialize/parse lại.
        let s = v.to_string();
        assert!(serde_json::from_str::<Value>(&s).is_ok());
    }
}
