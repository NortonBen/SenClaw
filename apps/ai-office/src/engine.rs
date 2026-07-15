use crate::db::{Agent, Db};
use crate::llm;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Orchestrates one task through the office pipeline, mirroring the reel:
///
/// SẾP giao việc → TRƯỞNG PHÒNG (manager) lập kế hoạch & phân công → từng
/// nhân sự chuyên môn (worker) làm phần việc của mình rồi "đi bàn giao" →
/// KIỂM ĐỊNH (qa, nếu có) soát chất lượng → manager tổng hợp và nộp
/// BÁO CÁO TỔNG HỢP.
///
/// The roster is dynamic: staff can be added/removed, disabled (excluded
/// entirely), or set to "tự nhận nhiệm vụ" (auto_assign — always in the
/// plan) vs. on-demand (the manager assigns them only when their specialty
/// is needed). Each step runs as an LLM completion through the daemon
/// bridge, with the agent's private knowledge space and held skills folded
/// into its context.
/// Single background worker that drains the task queue FIFO — one task at a
/// time so the office never runs two jobs at once. Spawned once at startup.
pub fn spawn_drainer(db: Arc<Db>) {
    tokio::spawn(async move {
        loop {
            match db.next_pending() {
                Ok(Some(task_id)) => {
                    if let Err(e) = run(db.clone(), task_id).await {
                        let _ = db.add_event(
                            Some(task_id),
                            "system",
                            "he-thong",
                            "",
                            &format!("Nhiệm vụ dừng vì lỗi: {}", e),
                        );
                        let _ = db.set_task_status(task_id, "error");
                        let _ = db.reset_agent_statuses();
                    }
                }
                _ => tokio::time::sleep(Duration::from_millis(600)).await,
            }
        }
    });
}

struct Roster {
    manager: Agent,
    qa: Option<Agent>,
    workers: Vec<Agent>,
}

fn roster(agents: &[Agent]) -> anyhow::Result<Roster> {
    let active: Vec<&Agent> = agents.iter().filter(|a| a.enabled).collect();
    let manager = active
        .iter()
        .find(|a| a.kind == "manager")
        .or_else(|| active.first())
        .map(|a| (*a).clone())
        .ok_or_else(|| anyhow::anyhow!("văn phòng chưa có nhân sự nào đang hoạt động"))?;
    let qa = active.iter().find(|a| a.kind == "qa").map(|a| (*a).clone());
    let workers: Vec<Agent> = active
        .iter()
        .filter(|a| a.kind == "worker")
        .map(|a| (*a).clone())
        .collect();
    if workers.is_empty() {
        anyhow::bail!("văn phòng chưa có nhân sự chuyên môn (worker) nào đang hoạt động");
    }
    Ok(Roster { manager, qa, workers })
}

/// "kỹ năng/sub-agent nắm giữ" context line for one agent, resolved against
/// the daemon inventory (falls back to bare names when the daemon is away).
fn skills_line(agent: &Agent, inventory: &HashMap<String, String>) -> String {
    if agent.skills.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = agent
        .skills
        .iter()
        .map(|s| match inventory.get(s) {
            Some(desc) if !desc.is_empty() => format!("- {}: {}", s, desc),
            _ => format!("- {}", s),
        })
        .collect();
    format!(
        "\nBạn nắm giữ các kỹ năng / sub-agent sau — vận dụng đúng chuyên môn của chúng khi làm việc:\n{}",
        parts.join("\n")
    )
}

async fn run(db: Arc<Db>, task_id: i64) -> anyhow::Result<()> {
    let task = db
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("task {} không tồn tại", task_id))?;
    let agents = db.list_agents()?;
    let Roster { manager, qa, workers } = roster(&agents)?;
    let mgr = manager.key.as_str();
    let name_of = |key: &str| -> String {
        agents
            .iter()
            .find(|a| a.key == key)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| key.to_uppercase())
    };

    // Feature toggles (Cài đặt): let Sếp turn parts of the pipeline off.
    let feat_memory = db.feature("memory");
    let feat_wiki = db.feature("wiki");
    let feat_workspace = db.feature("workspace");
    let feat_tools = db.feature("tools");
    let feat_autocontinue = db.feature("autocontinue");

    db.reset_agent_statuses()?;
    db.set_task_status(task_id, "planning")?;

    // Sếp giao việc xuất hiện trong feed.
    db.add_event(Some(task_id), "chat", "sep", "", &task.title)?;
    db.set_agent_status(mgr, "working", "đang lập kế hoạch & phân công")?;

    // Kho tài liệu chung + inventory kỹ năng (best-effort, một lần cho cả task).
    let wiki_ctx = if feat_wiki { wiki_context(&db, task_id, &task.title, mgr).await } else { String::new() };
    let ws_dir = db.workspace_dir();
    let (ws_ctx, ws_count) = if feat_workspace {
        let _ = crate::workspace::ensure_dir(&ws_dir);
        crate::workspace::read_context(&ws_dir, &task.title)
    } else {
        (String::new(), 0)
    };
    if !ws_ctx.is_empty() {
        db.add_event(
            Some(task_id),
            "file",
            mgr,
            "",
            &format!("đọc workspace: {} tệp trong {}", ws_count, ws_dir.to_string_lossy()),
        )?;
    }
    let inventory = crate::senclaw::skills_inventory().await.unwrap_or_default();

    // ---- 1. Trưởng phòng lập kế hoạch ----
    let plan = plan_live(&db, task_id, &task.title, &workers).await;

    let plan_lines: Vec<String> = plan
        .iter()
        .enumerate()
        .map(|(i, (key, title))| format!("• {}. {}: {}", i + 1, name_of(key), title))
        .collect();
    db.add_event(
        Some(task_id),
        "chat",
        mgr,
        "",
        &format!(
            "Đã nhận nhiệm vụ: \"{}\". Tôi sẽ phân công cho anh em theo quy trình dưới đây rồi tổng hợp báo cáo cho Sếp.\n\nPhân công:\n{}",
            task.title,
            plan_lines.join("\n")
        ),
    )?;

    let mut step_ids = Vec::new();
    for (i, (key, title)) in plan.iter().enumerate() {
        step_ids.push(db.add_step(task_id, key, title, i as i64)?);
    }
    // Kiểm định (nếu biên chế có) luôn là chốt chặn cuối trước khi tổng hợp.
    let qa_step = match &qa {
        Some(q) => Some(db.add_step(task_id, &q.key, "soát chất lượng & rủi ro", plan.len() as i64)?),
        None => None,
    };
    db.set_agent_status(mgr, "done", "đã phân công — chờ kết quả")?;
    db.set_task_status(task_id, "running")?;

    // ---- 2. Từng agent làm phần việc của mình ----
    let mut handovers: Vec<(String, String, String)> = Vec::new(); // (key, title, result)
    for (i, (key, title)) in plan.iter().enumerate() {
        let step_id = step_ids[i];
        db.add_event(Some(task_id), "assign", mgr, key, title)?;
        db.set_step_status(step_id, "working")?;
        db.set_agent_status(key, "working", title)?;

        let agent = db.get_agent(key)?;
        let agent_skills: Vec<String> = agent.as_ref().map(|a| a.skills.clone()).unwrap_or_default();
        let (role, duty, skills) = agent
            .map(|a| (a.role.clone(), a.duty.clone(), a.clone()))
            .map(|(r, d, a)| (r, d, skills_line(&a, &inventory)))
            .unwrap_or_default();
        let mut context = format!(
            "Nhiệm vụ chung của phòng: {}\n\nPhần việc của bạn: {}",
            task.title, title
        );
        // Trí nhớ riêng: mỗi nhân sự chỉ thấy knowledge space của mình.
        let space = crate::senclaw::agent_space(key);
        if feat_memory {
            if let Ok(memory) = crate::senclaw::knowledge_recall(&space, &task.title).await {
                if !memory.is_empty() {
                    context.push_str(&format!(
                        "\n\nTrí nhớ riêng của bạn (từ các nhiệm vụ trước — tham khảo nếu liên quan):\n{}",
                        clip(&memory, 1200)
                    ));
                }
            }
        }
        if !wiki_ctx.is_empty() {
            context.push_str(&format!("\n\nTài liệu nội bộ (wiki của văn phòng):\n{}", wiki_ctx));
        }
        if !ws_ctx.is_empty() {
            context.push_str(&format!("\n\nTài liệu trong workspace của văn phòng:\n{}", ws_ctx));
        }
        if !handovers.is_empty() {
            context.push_str("\n\nKết quả các đồng nghiệp đã bàn giao:\n");
            for (k, t, r) in &handovers {
                context.push_str(&format!("\n--- {} ({}) ---\n{}\n", name_of(k), t, r));
            }
        }
        context.push_str(
            "\n\nHãy hoàn thành đúng phần việc của bạn, trả lời bằng tiếng Việt, súc tích và có cấu trúc (gạch đầu dòng khi phù hợp). Chỉ trả về nội dung phần việc, không lời chào.",
        );
        let system = format!(
            "Bạn là {} — {} trong một văn phòng AI \"công ty một người\". Nhiệm vụ cố định của bạn: {}{}",
            name_of(key),
            role,
            duty,
            skills
        );
        // Nhân sự nắm skill/sub-agent + bật "worker dùng tool" → chạy như một
        // agent thật (gọi MCP / search / browser). Ngược lại: một-shot LLM.
        let result = if feat_tools && !agent_skills.is_empty() {
            db.set_agent_status(key, "working", &format!("đang dùng công cụ: {}", agent_skills.join(", ")))?;
            match crate::senclaw::agent_run(&space, &system, &context, &ws_dir.to_string_lossy(), 300).await {
                Ok(t) if !t.is_empty() => {
                    db.add_event(Some(task_id), "tool", key, "", &format!("đã dùng công cụ ({}) để xử lý", agent_skills.join(", ")))?;
                    t
                }
                Ok(_) => call_llm(&db, task_id, &system, &context, feat_autocontinue).await,
                Err(e) => {
                    db.add_event(Some(task_id), "system", "he-thong", "", &format!("{} không dùng được công cụ ({}) — xử lý bằng LLM thường", name_of(key), e))?;
                    call_llm(&db, task_id, &system, &context, feat_autocontinue).await
                }
            }
        } else {
            call_llm(&db, task_id, &system, &context, feat_autocontinue).await
        };
        // Ghi vào trí nhớ riêng để lần sau nhớ đã làm gì (best-effort).
        if feat_memory {
            let memo = format!(
                "Nhiệm vụ: {}\nPhần việc của tôi: {}\nKết quả tóm tắt: {}",
                task.title,
                title,
                clip(&result, 800)
            );
            let _ = crate::senclaw::knowledge_save(&space, &memo, &format!("ai-office:task-{}", task_id)).await;
        }

        db.set_step_result(step_id, &result)?;
        db.set_step_status(step_id, "done")?;
        db.add_event(Some(task_id), "chat", key, "", &result)?;

        // Nhân sự ghi phần việc của mình vào workspace làm tài liệu.
        if feat_workspace {
            let doc_rel = format!("task-{}/{:02}-{}.md", task_id, i + 1, key);
            let doc = format!("# {} — {}\n\nNhiệm vụ: {}\n\n{}\n", name_of(key), title, task.title, result);
            if crate::workspace::write_doc(&ws_dir, &doc_rel, &doc).is_ok() {
                db.add_event(Some(task_id), "file", key, "", &format!("đã lưu tài liệu: {}", doc_rel))?;
            }
        }

        // Bàn giao: agent đi sang bàn kế tiếp (QA hoặc Trưởng phòng nếu là bước cuối).
        let next = plan
            .get(i + 1)
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| qa.as_ref().map(|q| q.key.clone()).unwrap_or_else(|| mgr.to_string()));
        db.set_agent_status(key, "handoff", "đi bàn giao")?;
        db.add_event(
            Some(task_id),
            "bubble",
            key,
            &next,
            "Em xong phần của mình rồi, bàn giao anh!",
        )?;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        db.set_agent_status(key, "done", "hoàn thành")?;
        handovers.push((key.clone(), title.clone(), result));
    }

    // ---- 3. Kiểm định soát chất lượng (nếu biên chế có) ----
    let mut qa_result = String::new();
    if let (Some(q), Some(qa_step)) = (&qa, qa_step) {
        db.set_task_status(task_id, "review")?;
        db.add_event(Some(task_id), "assign", mgr, &q.key, "soát chất lượng & rủi ro toàn bộ kết quả")?;
        db.set_step_status(qa_step, "working")?;
        db.set_agent_status(&q.key, "working", "đang soát chất lượng & rủi ro")?;
        let mut context = format!(
            "Nhiệm vụ chung: {}\n\nToàn bộ kết quả của phòng:\n",
            task.title
        );
        for (k, t, r) in &handovers {
            context.push_str(&format!("\n--- {} ({}) ---\n{}\n", name_of(k), t, r));
        }
        context.push_str("\n\nHãy kiểm định: chỉ ra tối đa 3 rủi ro/lỗ hổng quan trọng nhất và xác nhận những phần đạt chất lượng. Trả lời tiếng Việt, ngắn gọn.");
        let system = format!(
            "Bạn là {} — {} của văn phòng AI. Bạn khó tính nhưng công bằng. Nhiệm vụ cố định: {}{}",
            q.name,
            q.role,
            q.duty,
            skills_line(q, &inventory)
        );
        qa_result = call_llm(&db, task_id, &system, &context, feat_autocontinue).await;
        db.set_step_result(qa_step, &qa_result)?;
        db.set_step_status(qa_step, "done")?;
        db.add_event(Some(task_id), "chat", &q.key, "", &qa_result)?;
        db.set_agent_status(&q.key, "handoff", "đi bàn giao")?;
        db.add_event(
            Some(task_id),
            "handoff",
            &q.key,
            mgr,
            "bàn giao kết quả kiểm định để tổng hợp",
        )?;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        db.set_agent_status(&q.key, "done", "hoàn thành")?;
    }

    // ---- 4. Trưởng phòng tổng hợp & nộp báo cáo ----
    let assigned = plan.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>().join(" + ");
    db.add_event(Some(task_id), "handoff", &assigned, mgr, "bàn giao kết quả để tổng hợp")?;
    db.set_agent_status(mgr, "working", "đang tổng hợp báo cáo")?;
    let mut context = format!(
        "Nhiệm vụ Sếp giao: {}\n\nKết quả từng bộ phận:\n",
        task.title
    );
    for (k, t, r) in &handovers {
        context.push_str(&format!("\n--- {} ({}) ---\n{}\n", name_of(k), t, r));
    }
    if !qa_result.is_empty() {
        context.push_str(&format!("\n--- KIỂM ĐỊNH ---\n{}\n", qa_result));
    }
    context.push_str("\n\nHãy viết BÁO CÁO TỔNG HỢP cuối cùng nộp cho Sếp: mở đầu 1 câu tóm tắt, sau đó các phần chính có tiêu đề, cuối cùng là đề xuất bước tiếp theo. Tiếng Việt, rõ ràng, không lời chào thừa.");
    let system = format!(
        "Bạn là {} của văn phòng AI \"công ty một người\" — {}. Bạn viết báo cáo gọn, đúng trọng tâm cho Sếp.",
        manager.name, manager.role
    );
    let report = call_llm(&db, task_id, &system, &context, feat_autocontinue).await;
    db.set_task_report(task_id, &report)?;
    db.add_event(Some(task_id), "report", mgr, "sep", &report)?;

    // Trưởng phòng ghi nhớ nhiệm vụ đã hoàn thành vào trí nhớ riêng.
    if feat_memory {
        let memo = format!(
            "Nhiệm vụ đã hoàn thành: {}\nPhân công: {}\nBáo cáo tóm tắt: {}",
            task.title,
            plan.iter().map(|(k, t)| format!("{}={}", k, t)).collect::<Vec<_>>().join("; "),
            clip(&report, 800)
        );
        let _ = crate::senclaw::knowledge_save(
            &crate::senclaw::agent_space(mgr),
            &memo,
            &format!("ai-office:task-{}", task_id),
        )
        .await;
    }
    let doc = format!("# Báo cáo: {}\n\n{}\n", task.title, report);
    // Nộp báo cáo vào wiki — kho tài liệu của văn phòng.
    if feat_wiki {
        let wiki_path = format!(
            "ai-office/{}-{}.md",
            task_id,
            crate::db::slugify(&clip(&task.title, 60))
        );
        match crate::senclaw::wiki_write(&wiki_path, &doc, &format!("ai-office: báo cáo nhiệm vụ #{}", task_id)).await {
            Ok(()) => {
                db.add_event(Some(task_id), "wiki", mgr, "", &format!("đã lưu báo cáo vào wiki: {}", wiki_path))?;
            }
            Err(e) => {
                db.add_event(Some(task_id), "system", "he-thong", "", &format!("không lưu được báo cáo vào wiki: {}", e))?;
            }
        }
    }
    // ...và bản sao trong workspace để Sếp mở trực tiếp bằng Finder/editor.
    if feat_workspace {
        let report_rel = format!("task-{}/bao-cao.md", task_id);
        if crate::workspace::write_doc(&ws_dir, &report_rel, &doc).is_ok() {
            db.add_event(Some(task_id), "file", mgr, "", &format!("đã lưu tài liệu: {}", report_rel))?;
        }
    }
    db.add_event(
        Some(task_id),
        "bubble",
        mgr,
        "sep",
        "Gửi Sếp tổng hợp đây ạ, cả phòng đã hoàn thành nhiệm vụ!",
    )?;
    db.set_agent_status(mgr, "done", "đã nộp báo cáo")?;
    db.set_task_status(task_id, "done")?;
    Ok(())
}

/// Fallback plan: one step per auto-assign worker (all workers when nobody
/// is auto-assign), in roster order, titled by role.
fn default_plan(workers: &[Agent]) -> Vec<(String, String)> {
    let auto: Vec<&Agent> = workers.iter().filter(|w| w.auto_assign).collect();
    let picked: Vec<&Agent> = if auto.is_empty() { workers.iter().collect() } else { auto };
    picked
        .iter()
        .map(|w| {
            let title = if w.role.is_empty() {
                "xử lý phần việc theo vai trò".to_string()
            } else {
                w.role.to_lowercase()
            };
            (w.key.clone(), title)
        })
        .collect()
}

/// Ask the manager LLM to split the task across the roster. Workers with
/// auto_assign are mandatory (đúng một phần việc mỗi người); the rest are
/// optional — assigned only when their specialty is needed. Falls back to
/// the default plan when parsing fails.
async fn plan_live(
    db: &Arc<Db>,
    task_id: i64,
    title: &str,
    workers: &[Agent],
) -> Vec<(String, String)> {
    let mandatory: Vec<&Agent> = workers.iter().filter(|w| w.auto_assign).collect();
    let optional: Vec<&Agent> = workers.iter().filter(|w| !w.auto_assign).collect();
    let describe = |list: &[&Agent]| {
        list.iter()
            .map(|w| format!("- {} ({}): {}", w.key, w.role, w.duty))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut staff_desc = String::new();
    if !mandatory.is_empty() {
        staff_desc.push_str(&format!(
            "Nhân sự trực ca (LUÔN giao cho mỗi người đúng MỘT phần việc):\n{}\n",
            describe(&mandatory)
        ));
    }
    if !optional.is_empty() {
        staff_desc.push_str(&format!(
            "Nhân sự tăng cường (CHỈ giao khi nhiệm vụ thật sự cần chuyên môn của họ):\n{}\n",
            describe(&optional)
        ));
    }
    let keys = workers.iter().map(|w| w.key.clone()).collect::<Vec<_>>();
    let system = format!(
        "Bạn là TRƯỞNG PHÒNG của một văn phòng AI. Biên chế hiện tại:\n{}\nBạn chia nhiệm vụ thành các phần việc nối tiếp nhau theo thứ tự thực hiện hợp lý.",
        staff_desc
    );
    let user = format!(
        "Nhiệm vụ Sếp giao: \"{}\"\n\nTrả về DUY NHẤT một mảng JSON, mỗi phần tử {{\"agent\": \"<key nhân sự>\", \"title\": \"mô tả phần việc ngắn gọn bằng tiếng Việt\"}}. Key hợp lệ: {}. Mỗi nhân sự xuất hiện tối đa một lần.",
        title,
        keys.join(", ")
    );
    let raw = call_llm(db, task_id, &system, &user, false).await;
    match parse_plan(&raw, &keys) {
        Some(mut plan) => {
            // Bảo đảm nhân sự trực ca nào bị LLM bỏ sót vẫn có phần việc.
            for w in &mandatory {
                if !plan.iter().any(|(k, _)| k == &w.key) {
                    let title = if w.role.is_empty() {
                        "xử lý phần việc theo vai trò".to_string()
                    } else {
                        w.role.to_lowercase()
                    };
                    plan.push((w.key.clone(), title));
                }
            }
            plan
        }
        None => default_plan(workers),
    }
}

/// Tolerant JSON-array extraction (the model may wrap the array in prose/fences).
fn parse_plan(raw: &str, valid: &[String]) -> Option<Vec<(String, String)>> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    let arr: Value = serde_json::from_str(raw.get(start..=end)?).ok()?;
    let mut seen = std::collections::HashSet::new();
    let steps: Vec<(String, String)> = arr
        .as_array()?
        .iter()
        .filter_map(|s| {
            let agent = s["agent"].as_str()?.trim().to_lowercase();
            let title = s["title"].as_str()?.trim().to_string();
            if valid.iter().any(|v| v == &agent) && !title.is_empty() && seen.insert(agent.clone()) {
                Some((agent, title))
            } else {
                None
            }
        })
        .collect();
    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

/// Truncate on a char boundary (never byte-slice — Vietnamese is multibyte).
fn clip(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{}…", cut)
    }
}

/// One wiki lookup for the whole task: top hits + the best document's body,
/// logged into the feed so Sếp sees the office consulting its library.
async fn wiki_context(db: &Arc<Db>, task_id: i64, title: &str, mgr: &str) -> String {
    let hits = match crate::senclaw::wiki_search(title, 3).await {
        Ok(h) => h,
        Err(_) => return String::new(), // wiki chưa bật — im lặng bỏ qua
    };
    if hits.is_empty() {
        return String::new();
    }
    let _ = db.add_event(
        Some(task_id),
        "wiki",
        mgr,
        "",
        &format!(
            "tra cứu wiki: {} tài liệu liên quan ({})",
            hits.len(),
            hits.iter().map(|(p, _, _)| p.as_str()).collect::<Vec<_>>().join(", ")
        ),
    );
    let mut ctx = String::new();
    for (path, hit_title, snippet) in &hits {
        ctx.push_str(&format!("- [{}] {}: {}\n", path, hit_title, snippet));
    }
    if let Some((path, _, _)) = hits.first() {
        if let Ok(body) = crate::senclaw::wiki_read(path).await {
            ctx.push_str(&format!("\nTrích tài liệu {}:\n{}\n", path, clip(&body, 1500)));
        }
    }
    clip(&ctx, 2400)
}

/// The daemon bridge doesn't report real usage, so the ledger tracks an
/// estimate: ~4 chars per token (the usual rough heuristic).
fn est_tokens(s: &str) -> i64 {
    (s.chars().count() as i64 + 3) / 4
}

/// Token cap per completion. Reports need room — 1600 was cutting Vietnamese
/// reports mid-sentence (the very defect Kiểm định kept flagging).
const MAX_TOKENS: u32 = 8000;
/// Continuation rounds when the provider still cuts at the cap.
const MAX_CONTINUES: usize = 2;

/// Heuristic truncation check for daemons that don't report finish_reason:
/// a finished Vietnamese report ends with terminal punctuation or a closing
/// markdown token, not a bare letter/digit/comma.
fn looks_truncated(text: &str) -> bool {
    match text.trim_end().chars().last() {
        Some(c) => c.is_alphanumeric() || matches!(c, ',' | ';' | '-' | '–' | '(' | '['),
        None => false,
    }
}

/// One bridge completion with auto-continue: when the provider cuts the
/// output at the token cap (finish == "length", or a truncation heuristic on
/// older daemons), ask the model to keep writing from where it stopped and
/// stitch the parts together. On failure the pipeline degrades to a visible
/// notice instead of aborting (the office keeps moving, like a real crew
/// would).
async fn call_llm(db: &Arc<Db>, task_id: i64, system: &str, user: &str, autocontinue: bool) -> String {
    let max_continues = if autocontinue { MAX_CONTINUES } else { 0 };
    let mut out = String::new();
    for round in 0..=max_continues {
        let prompt = if round == 0 {
            user.to_string()
        } else {
            format!(
                "{}\n\n--- PHẦN BẠN ĐÃ VIẾT (bị cắt giữa chừng) ---\n{}\n--- HẾT PHẦN ĐÃ VIẾT ---\n\nViết TIẾP NGAY từ đúng chỗ bị cắt cho đến khi hoàn chỉnh. Không lặp lại phần đã viết, không mở đầu lại, không lời dẫn.",
                user, out
            )
        };
        match llm::bridge_llm(system, &prompt, MAX_TOKENS).await {
            Ok((text, model, finish)) => {
                let tokens_in = est_tokens(system) + est_tokens(&prompt);
                let _ = db.bump_llm(task_id, &model, tokens_in, est_tokens(&text));
                if round == 0 {
                    out = text.trim().to_string();
                } else {
                    out.push_str(text.trim_start_matches(['\n', ' ']).trim_end());
                }
                let cut = finish == "length" || (finish.is_empty() && looks_truncated(&out));
                if !cut {
                    break;
                }
            }
            Err(e) => {
                if out.is_empty() {
                    return format!(
                        "(Không gọi được LLM qua daemon: {} — kiểm tra SenClaw daemon & cấu hình model. Phần việc này tạm ghi nhận là chưa xử lý.)",
                        e
                    );
                }
                break; // giữ phần đã có thay vì vứt đi
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_plan;

    fn keys() -> Vec<String> {
        vec!["nghien-cuu".into(), "noi-dung".into(), "phan-tich".into()]
    }

    #[test]
    fn parse_plan_extracts_array_from_prose() {
        let raw = "Kế hoạch đây:\n```json\n[{\"agent\":\"nghien-cuu\",\"title\":\"tìm hiểu\"},{\"agent\":\"noi-dung\",\"title\":\"viết bài\"}]\n```";
        let plan = parse_plan(raw, &keys()).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].0, "nghien-cuu");
    }

    #[test]
    fn truncation_heuristic() {
        use super::looks_truncated;
        // cut mid-word / mid-clause → truncated
        assert!(looks_truncated("cạnh tranh trên Sho"));
        assert!(looks_truncated("Yêu cầu các bộ"));
        assert!(looks_truncated("giá dỡ laptop,"));
        // clean endings → complete
        assert!(!looks_truncated("hoàn thành nhiệm vụ."));
        assert!(!looks_truncated("ưu tiên số 1!"));
        assert!(!looks_truncated("- xong.\n"));
        assert!(!looks_truncated("kết thúc)"));
        assert!(!looks_truncated(""));
    }

    #[test]
    fn parse_plan_rejects_unknown_agents_and_dupes() {
        assert!(parse_plan("[{\"agent\":\"ceo\",\"title\":\"x\"}]", &keys()).is_none());
        assert!(parse_plan("no json here", &keys()).is_none());
        // duplicate assignments collapse to the first one
        let plan = parse_plan(
            "[{\"agent\":\"noi-dung\",\"title\":\"a\"},{\"agent\":\"noi-dung\",\"title\":\"b\"}]",
            &keys(),
        )
        .unwrap();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].1, "a");
    }
}
