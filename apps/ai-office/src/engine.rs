use crate::db::Db;
use crate::llm;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Orchestrates one task through the office pipeline, mirroring the reel:
///
/// SẾP giao việc → TRƯỞNG PHÒNG lập kế hoạch & phân công → từng agent chuyên môn
/// làm phần việc của mình rồi "đi bàn giao" → KIỂM ĐỊNH soát chất lượng →
/// TRƯỞNG PHÒNG tổng hợp và nộp BÁO CÁO TỔNG HỢP.
///
/// DEMO mode simulates every step with canned text and short delays (no API).
/// LIVE mode runs each step as an LLM completion through the daemon bridge,
/// feeding each agent the task plus everything handed over so far.
pub fn spawn(db: Arc<Db>, task_id: i64) {
    tokio::spawn(async move {
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
    });
}

const MANAGER: &str = "truong-phong";
const QA: &str = "kiem-dinh";
/// Worker order used by the default plan (and DEMO mode).
const WORKERS: &[(&str, &str)] = &[
    ("nghien-cuu", "phân tích đề bài & chuẩn bị đầu vào"),
    ("noi-dung", "triển khai phần việc chính"),
    ("phan-tich", "rà soát & hoàn thiện kết quả"),
];

fn demo_pause() -> Duration {
    Duration::from_millis(1800)
}

async fn run(db: Arc<Db>, task_id: i64) -> anyhow::Result<()> {
    let task = db
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("task {} không tồn tại", task_id))?;
    let live = task.mode == "live";
    let agents = db.list_agents()?;
    let name_of = |key: &str| -> String {
        agents
            .iter()
            .find(|a| a.key == key)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| key.to_uppercase())
    };

    db.reset_agent_statuses()?;
    db.set_task_status(task_id, "planning")?;

    // Sếp giao việc xuất hiện trong feed.
    db.add_event(Some(task_id), "chat", "sep", "", &task.title)?;
    db.set_agent_status(MANAGER, "working", "đang lập kế hoạch & phân công")?;

    // ---- 1. Trưởng phòng lập kế hoạch ----
    let plan: Vec<(String, String)> = if live {
        plan_live(&db, task_id, &task.title).await
    } else {
        tokio::time::sleep(demo_pause()).await;
        WORKERS.iter().map(|(k, t)| (k.to_string(), t.to_string())).collect()
    };

    let plan_lines: Vec<String> = plan
        .iter()
        .enumerate()
        .map(|(i, (key, title))| format!("• {}. {}: {}", i + 1, name_of(key), title))
        .collect();
    db.add_event(
        Some(task_id),
        "chat",
        MANAGER,
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
    // Kiểm định luôn là chốt chặn cuối trước khi tổng hợp.
    let qa_step = db.add_step(task_id, QA, "soát chất lượng & rủi ro", plan.len() as i64)?;
    db.set_agent_status(MANAGER, "done", "đã phân công — chờ kết quả")?;
    db.set_task_status(task_id, "running")?;

    // ---- 2. Từng agent làm phần việc của mình ----
    let mut handovers: Vec<(String, String, String)> = Vec::new(); // (key, title, result)
    for (i, (key, title)) in plan.iter().enumerate() {
        let step_id = step_ids[i];
        db.add_event(Some(task_id), "assign", MANAGER, key, title)?;
        db.set_step_status(step_id, "working")?;
        db.set_agent_status(key, "working", title)?;

        let result = if live {
            let agent = db.get_agent(key)?;
            let (role, duty) = agent
                .map(|a| (a.role, a.duty))
                .unwrap_or_default();
            let mut context = format!(
                "Nhiệm vụ chung của phòng: {}\n\nPhần việc của bạn: {}",
                task.title, title
            );
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
                "Bạn là {} — {} trong một văn phòng AI \"công ty một người\". Nhiệm vụ cố định của bạn: {}",
                name_of(key),
                role,
                duty
            );
            call_llm(&db, task_id, &system, &context).await
        } else {
            tokio::time::sleep(demo_pause()).await;
            format!(
                "Phần việc của tôi ({}) đã xong:\n• Lưu ý: đây là kết quả mô phỏng — chế độ DEMO, chưa gọi API.\n• Ở chế độ LIVE, tôi sẽ xử lý thật nhiệm vụ \"{}\" theo đúng vai trò của mình.\n• Kết quả đã sẵn sàng bàn giao cho bước tiếp theo.",
                title, task.title
            )
        };

        db.set_step_result(step_id, &result)?;
        db.set_step_status(step_id, "done")?;
        db.add_event(Some(task_id), "chat", key, "", &result)?;

        // Bàn giao: agent đi sang bàn kế tiếp (hoặc về Trưởng phòng nếu là bước cuối).
        let next = plan.get(i + 1).map(|(k, _)| k.as_str()).unwrap_or(QA);
        db.set_agent_status(key, "handoff", "đi bàn giao")?;
        db.add_event(
            Some(task_id),
            "bubble",
            key,
            next,
            "Em xong phần của mình rồi, bàn giao anh!",
        )?;
        tokio::time::sleep(Duration::from_millis(1200)).await;
        db.set_agent_status(key, "done", "hoàn thành")?;
        handovers.push((key.clone(), title.clone(), result));
    }

    // ---- 3. Kiểm định soát chất lượng ----
    db.set_task_status(task_id, "review")?;
    db.add_event(Some(task_id), "assign", MANAGER, QA, "soát chất lượng & rủi ro toàn bộ kết quả")?;
    db.set_step_status(qa_step, "working")?;
    db.set_agent_status(QA, "working", "đang soát chất lượng & rủi ro")?;
    let qa_result = if live {
        let mut context = format!(
            "Nhiệm vụ chung: {}\n\nToàn bộ kết quả của phòng:\n",
            task.title
        );
        for (k, t, r) in &handovers {
            context.push_str(&format!("\n--- {} ({}) ---\n{}\n", name_of(k), t, r));
        }
        context.push_str("\n\nHãy kiểm định: chỉ ra tối đa 3 rủi ro/lỗ hổng quan trọng nhất và xác nhận những phần đạt chất lượng. Trả lời tiếng Việt, ngắn gọn.");
        call_llm(
            &db,
            task_id,
            "Bạn là KIỂM ĐỊNH — giám sát chất lượng & rủi ro của văn phòng AI. Bạn khó tính nhưng công bằng.",
            &context,
        )
        .await
    } else {
        tokio::time::sleep(demo_pause()).await;
        "Đã soát toàn bộ kết quả: không phát hiện rủi ro chặn. Chất lượng đạt, đủ điều kiện bàn giao Trưởng phòng tổng hợp. (Kết quả mô phỏng — chế độ DEMO.)".to_string()
    };
    db.set_step_result(qa_step, &qa_result)?;
    db.set_step_status(qa_step, "done")?;
    db.add_event(Some(task_id), "chat", QA, "", &qa_result)?;
    db.set_agent_status(QA, "handoff", "đi bàn giao")?;
    db.add_event(
        Some(task_id),
        "handoff",
        QA,
        MANAGER,
        "bàn giao kết quả kiểm định để tổng hợp",
    )?;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    db.set_agent_status(QA, "done", "hoàn thành")?;

    // ---- 4. Trưởng phòng tổng hợp & nộp báo cáo ----
    db.add_event(
        Some(task_id),
        "handoff",
        "nghien-cuu + noi-dung + phan-tich",
        MANAGER,
        "bàn giao kết quả để tổng hợp",
    )?;
    db.set_agent_status(MANAGER, "working", "đang tổng hợp báo cáo")?;
    let report = if live {
        let mut context = format!(
            "Nhiệm vụ Sếp giao: {}\n\nKết quả từng bộ phận:\n",
            task.title
        );
        for (k, t, r) in &handovers {
            context.push_str(&format!("\n--- {} ({}) ---\n{}\n", name_of(k), t, r));
        }
        context.push_str(&format!("\n--- KIỂM ĐỊNH ---\n{}\n", qa_result));
        context.push_str("\n\nHãy viết BÁO CÁO TỔNG HỢP cuối cùng nộp cho Sếp: mở đầu 1 câu tóm tắt, sau đó các phần chính có tiêu đề, cuối cùng là đề xuất bước tiếp theo. Tiếng Việt, rõ ràng, không lời chào thừa.");
        call_llm(
            &db,
            task_id,
            "Bạn là TRƯỞNG PHÒNG của văn phòng AI \"công ty một người\" — điều phối & tổng hợp. Bạn viết báo cáo gọn, đúng trọng tâm cho Sếp.",
            &context,
        )
        .await
    } else {
        tokio::time::sleep(demo_pause()).await;
        format!(
            "BÁO CÁO TỔNG HỢP — nhiệm vụ \"{}\"\n\n• Cả phòng đã hoàn thành đủ {} phần việc theo phân công, Kiểm định xác nhận chất lượng đạt.\n• Đây là báo cáo mô phỏng (chế độ DEMO, chưa gọi API). Chuyển sang chế độ LIVE để phòng xử lý thật nhiệm vụ.\n• Sếp cần bổ sung yêu cầu, cứ giao tiếp nhiệm vụ mới cho tôi.",
            task.title,
            WORKERS.len()
        )
    };
    db.set_task_report(task_id, &report)?;
    db.add_event(Some(task_id), "report", MANAGER, "sep", &report)?;
    db.add_event(
        Some(task_id),
        "bubble",
        MANAGER,
        "sep",
        "Gửi Sếp tổng hợp đây ạ, cả phòng đã hoàn thành nhiệm vụ!",
    )?;
    db.set_agent_status(MANAGER, "done", "đã nộp báo cáo")?;
    db.set_task_status(task_id, "done")?;
    Ok(())
}

/// LIVE planning: ask the manager LLM to split the task across the three
/// specialist desks; falls back to the default plan when parsing fails.
async fn plan_live(db: &Arc<Db>, task_id: i64, title: &str) -> Vec<(String, String)> {
    let system = "Bạn là TRƯỞNG PHÒNG của một văn phòng AI. Phòng có đúng 3 nhân sự chuyên môn: nghien-cuu (thu thập & phân tích thông tin), noi-dung (viết & biên tập), phan-tich (số liệu, logic, đánh giá). Bạn chia nhiệm vụ thành đúng 3 phần việc nối tiếp nhau, mỗi phần giao cho một nhân sự theo thế mạnh.";
    let user = format!(
        "Nhiệm vụ Sếp giao: \"{}\"\n\nTrả về DUY NHẤT một mảng JSON, mỗi phần tử {{\"agent\": \"nghien-cuu|noi-dung|phan-tich\", \"title\": \"mô tả phần việc ngắn gọn bằng tiếng Việt\"}}. Đúng 3 phần tử, theo thứ tự thực hiện.",
        title
    );
    let raw = call_llm(db, task_id, system, &user).await;
    parse_plan(&raw).unwrap_or_else(|| {
        WORKERS.iter().map(|(k, t)| (k.to_string(), t.to_string())).collect()
    })
}

/// Tolerant JSON-array extraction (the model may wrap the array in prose/fences).
fn parse_plan(raw: &str) -> Option<Vec<(String, String)>> {
    let start = raw.find('[')?;
    let end = raw.rfind(']')?;
    let arr: Value = serde_json::from_str(raw.get(start..=end)?).ok()?;
    let valid = ["nghien-cuu", "noi-dung", "phan-tich"];
    let steps: Vec<(String, String)> = arr
        .as_array()?
        .iter()
        .filter_map(|s| {
            let agent = s["agent"].as_str()?.trim().to_lowercase();
            let title = s["title"].as_str()?.trim().to_string();
            if valid.contains(&agent.as_str()) && !title.is_empty() {
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

/// One bridge completion; on failure the pipeline degrades to a visible notice
/// instead of aborting (the office keeps moving, like a real crew would).
async fn call_llm(db: &Arc<Db>, task_id: i64, system: &str, user: &str) -> String {
    match llm::bridge_llm(system, user, 1600).await {
        Ok((text, model)) => {
            let _ = db.bump_llm(task_id, &model);
            text.trim().to_string()
        }
        Err(e) => format!(
            "(Không gọi được LLM qua daemon: {} — kiểm tra SenClaw daemon & cấu hình model. Phần việc này tạm ghi nhận là chưa xử lý.)",
            e
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_plan;

    #[test]
    fn parse_plan_extracts_array_from_prose() {
        let raw = "Kế hoạch đây:\n```json\n[{\"agent\":\"nghien-cuu\",\"title\":\"tìm hiểu\"},{\"agent\":\"noi-dung\",\"title\":\"viết bài\"}]\n```";
        let plan = parse_plan(raw).unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].0, "nghien-cuu");
    }

    #[test]
    fn parse_plan_rejects_unknown_agents() {
        assert!(parse_plan("[{\"agent\":\"ceo\",\"title\":\"x\"}]").is_none());
        assert!(parse_plan("no json here").is_none());
    }
}
