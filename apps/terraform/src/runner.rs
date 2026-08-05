//! Chạy tiến trình (git / terraform) cho một run: nhiều bước tuần tự, output
//! stream từng dòng vào `run_lines` để UI/agent poll như console thật, hỗ trợ
//! huỷ giữa chừng và timeout tổng. Không đi qua shell — argv trực tiếp.

use crate::db::Db;
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;

/// Một bước trong run (vd `git pull` rồi `terraform apply`).
#[derive(Debug, Clone)]
pub struct Step {
    /// Dòng `$ …` in ra console trước khi chạy.
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub envs: Vec<(String, String)>,
}

impl Step {
    pub fn new(program: &str, args: Vec<String>, cwd: Option<PathBuf>) -> Self {
        let label = format!("$ {} {}", program, args.join(" "));
        Self {
            label,
            program: program.to_string(),
            args,
            cwd,
            envs: Vec::new(),
        }
    }
}

/// Cắt dòng quá dài an toàn UTF-8 (không panic giữa multibyte).
fn clip(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (dòng bị cắt)", &s[..end])
}

const MAX_LINE_BYTES: usize = 4000;
const MAX_LINES_PER_RUN: i64 = 20_000;

pub struct Runner {
    db: Arc<Db>,
    cancels: Mutex<HashMap<i64, watch::Sender<bool>>>,
}

impl Runner {
    pub fn new(db: Arc<Db>) -> Self {
        Self {
            db,
            cancels: Mutex::new(HashMap::new()),
        }
    }

    /// Huỷ một run đang chạy. `true` nếu có run để huỷ.
    pub fn cancel(&self, run_id: i64) -> bool {
        let map = self.cancels.lock().unwrap();
        map.get(&run_id).map(|tx| tx.send(true).is_ok()).unwrap_or(false)
    }

    fn append(db: &Db, counter: &AtomicI64, run_id: i64, stream: &str, line: &str) {
        let n = counter.fetch_add(1, Ordering::Relaxed);
        if n >= MAX_LINES_PER_RUN {
            if n == MAX_LINES_PER_RUN {
                let _ = db.run_append(run_id, "sys", "… output vượt giới hạn, các dòng sau bị bỏ …");
            }
            return;
        }
        let _ = db.run_append(run_id, stream, &clip(line, MAX_LINE_BYTES));
    }

    /// Chạy các bước tuần tự trong background; dừng ở bước fail đầu tiên.
    pub fn spawn_steps(self: &Arc<Self>, run_id: i64, steps: Vec<Step>, timeout: Duration) {
        let (tx, rx) = watch::channel(false);
        self.cancels.lock().unwrap().insert(run_id, tx);
        let this = self.clone();
        tokio::spawn(async move {
            let status = this.drive(run_id, steps, timeout, rx).await;
            this.cancels.lock().unwrap().remove(&run_id);
            this.post_run_hook(run_id, &status);
        });
    }

    async fn drive(
        &self,
        run_id: i64,
        steps: Vec<Step>,
        timeout: Duration,
        mut cancel_rx: watch::Receiver<bool>,
    ) -> String {
        let counter = Arc::new(AtomicI64::new(
            self.db.run_line_count(run_id).unwrap_or(0),
        ));
        let deadline = tokio::time::Instant::now() + timeout;
        let mut last_code: Option<i64> = Some(0);

        for step in steps {
            Self::append(&self.db, &counter, run_id, "sys", &step.label);
            let mut cmd = Command::new(&step.program);
            cmd.args(&step.args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env("TF_IN_AUTOMATION", "1")
                .env("TF_INPUT", "0")
                .env("GIT_TERMINAL_PROMPT", "0")
                .kill_on_drop(true);
            if let Some(cwd) = &step.cwd {
                cmd.current_dir(cwd);
            }
            for (k, v) in &step.envs {
                cmd.env(k, v);
            }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    Self::append(
                        &self.db,
                        &counter,
                        run_id,
                        "sys",
                        &format!("✗ không chạy được {}: {e}", step.program),
                    );
                    let _ = self.db.run_finish(run_id, "failed", None);
                    return "failed".into();
                }
            };

            let out_task = child.stdout.take().map(|out| {
                let db = self.db.clone();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(out).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        Self::append(&db, &counter, run_id, "out", &line);
                    }
                })
            });
            let err_task = child.stderr.take().map(|err| {
                let db = self.db.clone();
                let counter = counter.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(err).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        Self::append(&db, &counter, run_id, "err", &line);
                    }
                })
            });

            let outcome = tokio::select! {
                res = child.wait() => match res {
                    Ok(st) => Ok(st),
                    Err(e) => {
                        Self::append(&self.db, &counter, run_id, "sys", &format!("✗ lỗi chờ tiến trình: {e}"));
                        let _ = self.db.run_finish(run_id, "failed", None);
                        return "failed".into();
                    }
                },
                _ = cancel_rx.changed() => Err("canceled"),
                _ = tokio::time::sleep_until(deadline) => Err("timeout"),
            };

            match outcome {
                Err(why) => {
                    let _ = child.kill().await;
                    if let Some(t) = out_task { let _ = t.await; }
                    if let Some(t) = err_task { let _ = t.await; }
                    let (mark, status) = if why == "canceled" {
                        ("⛔ Đã huỷ theo yêu cầu", "canceled")
                    } else {
                        ("✗ Quá thời gian cho phép — đã dừng tiến trình", "failed")
                    };
                    Self::append(&self.db, &counter, run_id, "sys", mark);
                    let _ = self.db.run_finish(run_id, status, None);
                    return status.into();
                }
                Ok(st) => {
                    if let Some(t) = out_task { let _ = t.await; }
                    if let Some(t) = err_task { let _ = t.await; }
                    last_code = st.code().map(|c| c as i64);
                    if !st.success() {
                        Self::append(
                            &self.db,
                            &counter,
                            run_id,
                            "sys",
                            &format!("✗ Thoát với mã {}", last_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())),
                        );
                        let _ = self.db.run_finish(run_id, "failed", last_code);
                        return "failed".into();
                    }
                }
            }
        }

        Self::append(&self.db, &counter, run_id, "sys", "✓ Hoàn tất");
        let _ = self.db.run_finish(run_id, "success", last_code);
        "success".into()
    }

    /// Sau run: clone xong thì cập nhật trạng thái workspace tương ứng.
    fn post_run_hook(&self, run_id: i64, status: &str) {
        let Ok(Some(run)) = self.db.run_get(run_id) else { return };
        if run["kind"] != "clone" {
            return;
        }
        let Some(ws_id) = run["workspace_id"].as_i64() else { return };
        if status == "success" {
            let _ = self.db.workspace_update(ws_id, None, None, None, None, Some("ready"), Some(""), None, None);
            self.db.log(&format!("workspace #{ws_id}: clone xong, sẵn sàng"));
        } else {
            let tail = self.db.run_tail(run_id, 5).unwrap_or_default();
            let _ = self.db.workspace_update(
                ws_id,
                None,
                None,
                None,
                None,
                Some("error"),
                Some(&format!("clone thất bại:\n{tail}")),
                None,
                None,
            );
        }
    }

    /// Đợi run kết thúc (cho MCP): poll DB tới khi hết `timeout`.
    pub async fn wait_run(&self, run_id: i64, timeout: Duration) -> Result<(serde_json::Value, bool)> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let run = self
                .db
                .run_get(run_id)?
                .ok_or_else(|| anyhow::anyhow!("run {run_id} không tồn tại"))?;
            if run["status"] != "running" {
                return Ok((run, true));
            }
            if tokio::time::Instant::now() >= deadline {
                return Ok((run, false));
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Arc<Db>, Arc<Runner>) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path().join("t.db")).unwrap());
        std::mem::forget(dir);
        let runner = Arc::new(Runner::new(db.clone()));
        (db, runner)
    }

    #[tokio::test]
    async fn steps_run_and_stream_output() {
        let (db, runner) = setup();
        let run = db.run_create(None, "test").unwrap();
        runner.spawn_steps(
            run,
            vec![
                Step::new("sh", vec!["-c".into(), "echo xin-chào".into()], None),
                Step::new("sh", vec!["-c".into(), "echo bước-hai 1>&2".into()], None),
            ],
            Duration::from_secs(30),
        );
        let (r, done) = runner.wait_run(run, Duration::from_secs(20)).await.unwrap();
        assert!(done);
        assert_eq!(r["status"], "success");
        assert_eq!(r["exit_code"], 0);
        let tail = db.run_tail(run, 100).unwrap();
        assert!(tail.contains("xin-chào"));
        assert!(tail.contains("bước-hai"));
        assert!(tail.contains("✓ Hoàn tất"));
    }

    #[tokio::test]
    async fn failing_step_stops_chain_with_exit_code() {
        let (db, runner) = setup();
        let run = db.run_create(None, "test").unwrap();
        runner.spawn_steps(
            run,
            vec![
                Step::new("sh", vec!["-c".into(), "echo a; exit 3".into()], None),
                Step::new("sh", vec!["-c".into(), "echo không-được-chạy".into()], None),
            ],
            Duration::from_secs(30),
        );
        let (r, _) = runner.wait_run(run, Duration::from_secs(20)).await.unwrap();
        assert_eq!(r["status"], "failed");
        assert_eq!(r["exit_code"], 3);
        let tail = db.run_tail(run, 100).unwrap();
        assert!(!tail.contains("không-được-chạy"));
    }

    #[tokio::test]
    async fn cancel_kills_running_process() {
        let (db, runner) = setup();
        let run = db.run_create(None, "test").unwrap();
        runner.spawn_steps(
            run,
            vec![Step::new("sleep", vec!["30".into()], None)],
            Duration::from_secs(60),
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(runner.cancel(run));
        let (r, done) = runner.wait_run(run, Duration::from_secs(10)).await.unwrap();
        assert!(done);
        assert_eq!(r["status"], "canceled");
    }

    #[tokio::test]
    async fn missing_program_fails_gracefully() {
        let (db, runner) = setup();
        let run = db.run_create(None, "test").unwrap();
        runner.spawn_steps(
            run,
            vec![Step::new("chương-trình-không-tồn-tại-9x", vec![], None)],
            Duration::from_secs(10),
        );
        let (r, _) = runner.wait_run(run, Duration::from_secs(10)).await.unwrap();
        assert_eq!(r["status"], "failed");
        assert!(db.run_tail(run, 10).unwrap().contains("không chạy được"));
    }

    #[test]
    fn clip_is_utf8_safe() {
        let s = "tiếng Việt có dấu — đường độ ộ".repeat(400);
        let clipped = clip(&s, 100);
        assert!(clipped.len() < 200);
        // Không panic là đạt; kèm marker cắt.
        assert!(clipped.contains("dòng bị cắt"));
        assert_eq!(clip("ngắn", 100), "ngắn");
    }
}
