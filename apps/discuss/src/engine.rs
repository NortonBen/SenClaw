//! Vòng thảo luận — trái tim của AI Discuss Team.
//!
//! Mỗi phiên `running` chạy theo vòng: các member lần lượt (hoặc song song) ra
//! lượt → Thư ký cập nhật biên bản → Manager (độc lập, không bàn nội dung)
//! chấm tham gia + tiến độ so với yêu cầu BOSS, nhắc member im lặng, và đề
//! nghị chốt khi đã đủ. BOSS chen lời bất kỳ lúc nào — tin BOSS là ngắt ưu
//! tiên: member kế tiếp phải trả lời trước khi làm việc khác.

use crate::api::AppState;
use crate::db::{self, Member, Message, NewMessage};
use crate::parse;
use serde_json::json;
use std::time::Duration;

pub const HISTORY_WINDOW: i64 = 30;
pub const MEMORY_RECALL: i64 = 5;
pub const THINKING_RECALL: i64 = 2;
pub const OPEN_OPINIONS_LIMIT: i64 = 6;
pub const DOCS_LISTED: i64 = 25;
pub const AGENT_TIMEOUT_SECS: u64 = 240;
pub const MEMBER_MAX_TOKENS: u32 = 2600;
pub const SECRETARY_MAX_TOKENS: u32 = 3000;
// Model reasoning tiêu budget vào trace ẩn trước khi in JSON — trần thấp là
// JSON đứt giữa chừng (đã gặp với gemini agent profile trong e2e đầu tiên).
pub const MANAGER_MAX_TOKENS: u32 = 2200;
pub const RESULT_MAX_TOKENS: u32 = 8000;
/// Im lặng từ N vòng là bị "bắt phát biểu".
pub const SILENT_ROUNDS_NUDGE: i64 = 2;
pub const MIN_ROUNDS_BEFORE_CONCLUDE: i64 = 2;
pub const MANAGER_SCORE_THRESHOLD: i64 = 80;
pub const MAX_MEMORY_NOTES_PER_TURN: usize = 5;

// ---------------- Scheduler ----------------

/// Quét phiên `running`, mỗi phiên một vòng đang chạy (busy flag trong runtime).
pub fn spawn_scheduler(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let running = match state.db.discussions_with_status("running") {
                Ok(v) => v,
                Err(_) => continue,
            };
            for d in running {
                if state.try_mark_busy(d.id) {
                    let st = state.clone();
                    tokio::spawn(async move {
                        let disc_id = d.id;
                        if let Err(e) = run_round(&st, disc_id).await {
                            let _ = st.db.message_insert(&NewMessage {
                                discussion_id: disc_id,
                                round: 0,
                                author_kind: "system".into(),
                                kind: "system".into(),
                                content: format!("Lỗi vòng thảo luận: {e}"),
                                citations: json!([]),
                                flags: json!({"error": true}),
                                ..Default::default()
                            });
                        }
                        st.clear_busy(disc_id);
                    });
                }
            }
        }
    });
}

fn still_running(state: &AppState, disc_id: i64) -> bool {
    state
        .db
        .discussion_get(disc_id)
        .ok()
        .flatten()
        .map(|d| d.status == "running")
        .unwrap_or(false)
}

// ---------------- Một vòng ----------------

async fn run_round(state: &AppState, disc_id: i64) -> anyhow::Result<()> {
    let Some(disc) = state.db.discussion_get(disc_id)? else {
        return Ok(());
    };
    if disc.status != "running" {
        return Ok(());
    }
    let round = disc.round + 1;
    state.db.discussion_set_round(disc_id, round)?;

    let members = state.db.discussion_members(disc_id)?;
    if members.is_empty() {
        state.db.message_insert(&NewMessage {
            discussion_id: disc_id,
            round,
            author_kind: "system".into(),
            kind: "system".into(),
            content: "Phiên chưa có thành viên nào — đã tạm dừng. Thêm thành viên rồi Resume.".into(),
            citations: json!([]),
            flags: json!({}),
            ..Default::default()
        })?;
        state.db.discussion_set_status(disc_id, "paused")?;
        return Ok(());
    }

    // Ai đang bị "bắt phát biểu": im lặng >= SILENT_ROUNDS_NUDGE vòng.
    let participation = state.db.participation(disc_id, round - 1)?;
    let forced: Vec<(i64, String)> = participation
        .iter()
        .filter(|p| round > 2 && p.silent_rounds >= SILENT_ROUNDS_NUDGE)
        .map(|p| {
            (
                p.member_id,
                format!(
                    "LỆNH CỦA MANAGER: bạn đã im lặng {} vòng liên tiếp. Lượt này BẮT BUỘC phát biểu — tối thiểu 1 phản hồi cho luận điểm mở hoặc 1 luận điểm mới có giá trị.",
                    p.silent_rounds
                ),
            )
        })
        .collect();
    let force_of = |mid: i64| forced.iter().find(|(id, _)| *id == mid).map(|(_, r)| r.clone());

    if disc.mode == "parallel" {
        // Song song: mỗi member một agent.run cô lập; Semaphore(3) trong lượt
        // giữ dưới trần 4 run/app của daemon.
        let mut handles = Vec::new();
        for m in members {
            let st = state.clone();
            let force = force_of(m.id);
            handles.push(tokio::spawn(async move {
                run_member_turn(&st, disc_id, &m, round, force).await
            }));
        }
        for h in handles {
            let _ = h.await;
        }
        let pace = state
            .db
            .discussion_get(disc_id)?
            .map(|d| d.pace_secs)
            .unwrap_or(0);
        if pace > 0 && still_running(state, disc_id) {
            tokio::time::sleep(Duration::from_secs(pace as u64)).await;
        }
    } else {
        for m in &members {
            if !still_running(state, disc_id) {
                return Ok(());
            }
            let force = force_of(m.id);
            let _ = run_member_turn(state, disc_id, m, round, force).await;
            // Pace đọc lại mỗi lượt — BOSS đổi tốc độ giữa chừng có hiệu lực ngay.
            let pace = state
                .db
                .discussion_get(disc_id)?
                .map(|d| d.pace_secs)
                .unwrap_or(0);
            if pace > 0 && still_running(state, disc_id) {
                tokio::time::sleep(Duration::from_secs(pace as u64)).await;
            }
        }
    }

    if !still_running(state, disc_id) {
        return Ok(());
    }
    secretary_update(state, disc_id, round).await;

    if !still_running(state, disc_id) {
        return Ok(());
    }
    let decision = manager_review(state, disc_id, round).await;

    let disc = state.db.discussion_get(disc_id)?.unwrap();
    let should_conclude = match decision {
        Some(ref m) => m.met && m.score >= MANAGER_SCORE_THRESHOLD && round >= MIN_ROUNDS_BEFORE_CONCLUDE,
        None => false,
    };
    let out_of_rounds = round >= disc.max_rounds;
    if (should_conclude || out_of_rounds) && disc.status == "running" {
        let reason = if should_conclude {
            "Manager đánh giá thảo luận đã ĐỦ so với yêu cầu của BOSS.".to_string()
        } else {
            format!(
                "Đã chạm trần {} vòng mà chưa đạt đủ tiêu chí — tổng hợp với phần thiếu được ghi rõ.",
                disc.max_rounds
            )
        };
        synthesize_result(state, disc_id, &reason).await;
    }
    Ok(())
}

// ---------------- Lượt member ----------------

fn render_message_line(m: &Message, members: &[Member]) -> String {
    let who = match m.author_kind.as_str() {
        "boss" => "BOSS".to_string(),
        "system" => "Hệ thống".to_string(),
        _ => members
            .iter()
            .find(|x| Some(x.id) == m.member_id)
            .map(|x| x.name.clone())
            .unwrap_or_else(|| m.author_kind.clone()),
    };
    let mut tags = Vec::new();
    if let Some(ct) = &m.claim_type {
        tags.push(match ct.as_str() {
            "evidence" => "dẫn chứng",
            "inference" => "suy diễn",
            "creative" => "sáng tạo",
            other => other,
        });
    }
    if let Some(p) = &m.provability {
        tags.push(match p.as_str() {
            "practical" => "thực tiễn",
            "theoretical" => "lý thuyết",
            other => other,
        });
    }
    if let Some(s) = &m.stance {
        tags.push(match s.as_str() {
            "agree" => "ĐỒNG TÌNH",
            "disagree" => "PHẢN ĐỐI",
            other => other,
        });
    }
    let tag_s = if tags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", tags.join(", "))
    };
    let reply = m.reply_to.map(|r| format!(" (trả lời #{r})")).unwrap_or_default();
    let mut content = m.content.clone();
    if content.chars().count() > 700 {
        content = content.chars().take(700).collect::<String>() + "…";
    }
    format!("[#{}] {}{}{}: {}", m.id, who, tag_s, reply, content)
}

fn tool_manual(member: &Member) -> String {
    let restricted = member
        .tools
        .as_ref()
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty());
    let mut s = String::from(
        "## CẨM NANG DÙNG TOOL (chọn đúng việc, đừng lạm dụng)\n\
         - `mcp__search-mcp__search_query` — tra cứu NHANH liên hợp nhiều nguồn, trả kèm nguồn; dùng khi cần kiểm tra một dữ kiện.\n\
         - `mcp__search-mcp__search_ask` — hỏi đáp có đếm số nguồn độc lập, báo `disputed` khi các nguồn mâu thuẫn; dùng khi cần xác nhận một khẳng định.\n\
         - `mcp__zeach-mcp__zeach_research` — nghiên cứu SÂU (depth: quick|standard|deep) trả báo cáo có trích dẫn [n]; dùng cho câu hỏi lớn cần nhiều bằng chứng — đắt, tối đa 1 lần/lượt.\n\
         - `mcp__news-mcp__news_latest` / `news_trends` / `news_search` — tin tức, xu hướng, dòng sự kiện thời điểm.\n\
         - `mcp__thinking-mcp__think_*` — dàn khung 6 mũ/5W khi vấn đề rối.\n\
         - `mcp__senclaw-memory__memory_search`, `mcp__senclaw-wiki__wiki_search` — tri thức nội bộ đã tích luỹ.\n\
         - `Read`/`Grep` — ĐỌC KHO TÀI LIỆU CHUNG: các file doc-*.md ngay trong thư mục làm việc của bạn.\n\
         - TRÁNH tool browser (cả hệ thống chia 1 tab duy nhất — dễ giẫm chân member khác).\n\
         Kết quả tool phải biến thành trích dẫn: {\"kind\":\"tool\",\"ref\":\"tên_tool: tham_số\",\"quote\":\"số liệu/câu chốt\"} hoặc {\"kind\":\"url\",...} khi tool trả URL nguồn.\n",
    );
    if let Some(list) = restricted {
        s.push_str(&format!(
            "- BOSS giới hạn bạn CHỈ ĐƯỢC dùng các tool sau (kỷ luật, không gọi tool ngoài danh sách): {list}\n"
        ));
    }
    s
}

fn member_system(member: &Member) -> String {
    // Mũ thiên hướng có thể NHIỀU (comma-list) — mỗi phát biểu member chọn
    // MỘT mũ, ưu tiên trong thiên hướng của mình.
    let hats: Vec<&str> = member
        .hat
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let hat_line = match hats.len() {
        0 => String::new(),
        1 => format!("Thiên hướng mũ tư duy của bạn: {}.\n", hat_vn(hats[0])),
        _ => format!(
            "Thiên hướng mũ tư duy của bạn gồm: {} — MỖI phát biểu chọn đúng MỘT mũ phù hợp trong số này (được dùng mũ khác khi thật cần thiết).\n",
            hats.iter().map(|h| hat_vn(h)).collect::<Vec<_>>().join(", ")
        ),
    };
    let tools_part = if member.use_tools {
        tool_manual(member)
    } else {
        "Bạn KHÔNG dùng tool lượt này — chỉ suy luận từ thông tin có trong phòng (tin nhắn, biên bản, tài liệu, bộ nhớ).\n".to_string()
    };
    format!(
        "Bạn là {name} — thành viên hội đồng thảo luận AI Discuss Team.\n\
         Chuyên môn: {expertise}.\nPhong cách: {style}.\n{hat_line}\
         \n## LUẬT THẢO LUẬN (bắt buộc)\n\
         1. Mỗi luận điểm phải gắn `claim_type`: `evidence` (tìm kiếm CÓ dẫn chứng), `inference` (suy diễn từ thông tin đã có trong phòng — nêu suy từ #id/doc nào), `creative` (sáng tạo — ý mới chưa có bằng chứng, dán nhãn rõ).\n\
         2. Mỗi luận điểm gắn `provability`: `practical` (thực tiễn — kiểm chứng được bằng nguồn/tài liệu) hoặc `theoretical` (lý thuyết — hợp lý nhưng chưa kiểm chứng).\n\
         3. Với TỪNG luận điểm mở của member khác: PHẢI tỏ thái độ — `agree` (xét xem cần bổ sung gì, ghi vào `supplement`) hoặc `disagree` (BẮT BUỘC ≥1 dẫn chứng trong `citations`; phản đối không dẫn chứng là vi phạm luật).\n\
         4. Tin của BOSS là ưu tiên số 1 — trả lời trước mọi việc khác.\n\
         5. Vận dụng 6 mũ tư duy: trắng=dữ kiện, đỏ=trực giác (nêu ngắn), đen=rủi ro, vàng=lợi ích, xanh lá=sáng tạo, xanh dương=quy trình (dành cho Manager). Chọn mũ hợp từng phát biểu, ghi vào `hat`.\n\
         6. Dẫn chứng: `doc:<id>` cho tài liệu trong kho, URL cho nguồn ngoài, tên tool cho kết quả tool. KHÔNG bịa nguồn — bịa nguồn là lỗi nặng nhất.\n\
         7. Ngắn gọn, tiếng Việt, đi thẳng vào nội dung. Tối đa 2 luận điểm mới mỗi lượt.\n\
         \n{tools_part}\
         \n## ĐẦU RA\nCHỈ trả về đúng MỘT khối JSON (không lời dẫn, không markdown ngoài JSON):\n\
         {{\"reactions\":[{{\"reply_to\":<id số>,\"stance\":\"agree|disagree\",\"content\":\"lập luận của bạn\",\"supplement\":\"bổ sung nếu agree\",\"citations\":[{{\"kind\":\"doc|url|tool\",\"ref\":\"doc:12 | https://… | tên_tool: tham số\",\"quote\":\"trích ngắn\"}}],\"hat\":\"white|red|black|yellow|green|blue\"}}],\
         \"claims\":[{{\"content\":\"luận điểm mới\",\"claim_type\":\"evidence|inference|creative\",\"provability\":\"practical|theoretical\",\"hat\":\"…\",\"citations\":[…]}}],\
         \"memory_notes\":[\"điều đáng ghi vào bộ nhớ riêng lâu dài (tối đa {max_notes})\"],\
         \"thinking\":\"2-4 câu tóm mạch suy nghĩ bạn đã dùng lượt này\"}}\n\
         Không có gì để phản hồi thì để mảng rỗng, nhưng im lặng hoàn toàn (mọi mảng rỗng) chỉ chấp nhận khi thực sự không còn gì đáng nói.",
        name = member.name,
        expertise = member.expertise,
        style = member.style,
        hat_line = hat_line,
        tools_part = tools_part,
        max_notes = MAX_MEMORY_NOTES_PER_TURN,
    )
}

fn hat_vn(h: &str) -> &'static str {
    match h {
        "white" => "mũ TRẮNG (dữ kiện)",
        "red" => "mũ ĐỎ (trực giác)",
        "black" => "mũ ĐEN (rủi ro)",
        "yellow" => "mũ VÀNG (lợi ích)",
        "green" => "mũ XANH LÁ (sáng tạo)",
        "blue" => "mũ XANH DƯƠNG (quy trình)",
        _ => "tự chọn",
    }
}

#[allow(clippy::too_many_arguments)]
fn member_prompt(
    state: &AppState,
    disc: &db::Discussion,
    member: &Member,
    all_members: &[Member],
    round: i64,
    force: &Option<String>,
) -> anyhow::Result<String> {
    let minutes = state
        .db
        .minutes_latest(disc.id)?
        .map(|m| truncate_chars(&m.content, 2000))
        .unwrap_or_else(|| "(chưa có — đây là những vòng đầu)".into());
    let docs = state.db.doc_list(Some(disc.id), DOCS_LISTED)?;
    let docs_s = if docs.is_empty() {
        "(kho trống)".to_string()
    } else {
        docs.iter()
            .map(|d| format!("- doc:{} — {} (file {})", d.id, d.title, d.filename))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let query = format!("{} {}", disc.title, disc.requirement);
    let memory = state.db.memory_recall(member.id, &query, MEMORY_RECALL)?;
    let memory_s = if memory.is_empty() {
        "(trống)".to_string()
    } else {
        memory
            .iter()
            .map(|m| format!("- [{}] {}", m.kind, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let thinking = state.db.thinking_recent(member.id, disc.id, THINKING_RECALL)?;
    let thinking_s = if thinking.is_empty() {
        "(chưa có)".to_string()
    } else {
        thinking
            .iter()
            .map(|(r, c)| format!("- (vòng {r}) {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let recent = state.db.messages_recent(disc.id, HISTORY_WINDOW)?;
    let recent_s = if recent.is_empty() {
        "(phòng họp vừa mở — bạn có thể là người phát biểu đầu tiên)".to_string()
    } else {
        recent
            .iter()
            .filter(|m| m.kind != "minutes_note")
            .map(|m| render_message_line(m, all_members))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let boss_pending = state.db.boss_messages_since_member_last(disc.id, member.id)?;
    let boss_s = if boss_pending.is_empty() {
        "(không có)".to_string()
    } else {
        boss_pending
            .iter()
            .map(|m| format!("[#{}] BOSS: {}", m.id, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let opens = state.db.open_opinions(disc.id, OPEN_OPINIONS_LIMIT)?;
    let opens_others: Vec<&Message> = opens
        .iter()
        .filter(|m| m.member_id != Some(member.id))
        .collect();
    let opens_s = if opens_others.is_empty() {
        "(không còn luận điểm mở)".to_string()
    } else {
        opens_others
            .iter()
            .map(|m| render_message_line(m, all_members))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let force_s = force
        .as_ref()
        .map(|f| format!("\n## LỆNH ĐIỀU PHỐI\n{f}\n"))
        .unwrap_or_default();

    Ok(format!(
        "# PHIÊN THẢO LUẬN #{id}: {title}\n\
         ## YÊU CẦU KẾT QUẢ CỦA BOSS (tiêu chí để chốt phiên)\n{req}\n\
         ## BIÊN BẢN MỚI NHẤT (Thư ký)\n{minutes}\n\
         ## KHO TÀI LIỆU CHUNG (trích dẫn bằng doc:<id>; nội dung đầy đủ là file .md trong thư mục làm việc)\n{docs_s}\n\
         ## BỘ NHỚ RIÊNG CỦA BẠN (member khác không thấy)\n{memory_s}\n\
         ## MẠCH SUY NGHĨ CÁC LƯỢT TRƯỚC CỦA BẠN (giữ nhất quán, được phép đổi ý nếu có dẫn chứng mới)\n{thinking_s}\n\
         ## DIỄN BIẾN GẦN NHẤT\n{recent_s}\n\
         ## TIN BOSS BẠN CHƯA TRẢ LỜI — ƯU TIÊN SỐ 1\n{boss_s}\n\
         ## LUẬN ĐIỂM MỞ PHẢI XEM XÉT (agree kèm bổ sung / disagree kèm dẫn chứng)\n{opens_s}\n\
         {force_s}\
         ## NHIỆM VỤ LƯỢT NÀY (vòng {round}, bạn là {name})\n\
         1) Trả lời tin BOSS nếu có. 2) Phản hồi TỪNG luận điểm mở ở trên theo luật. 3) Nêu tối đa 2 luận điểm mới đẩy thảo luận tiến về tiêu chí của BOSS. 4) Ghi memory_notes + thinking.\n\
         CHỈ trả về JSON đúng schema đã nêu trong system prompt.",
        id = disc.id,
        title = disc.title,
        req = disc.requirement,
        round = round,
        name = member.name,
    ))
}

fn truncate_chars(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

async fn call_member_llm(
    state: &AppState,
    member: &Member,
    disc_id: i64,
    system: &str,
    prompt: &str,
) -> Result<String, String> {
    if member.use_tools {
        let tools: Option<Vec<String>> = member.tools.as_ref().and_then(|t| t.as_array()).map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        });
        let space = format!("discuss:{}", member.key);
        let workspace = crate::config::docs_dir(disc_id);
        std::fs::create_dir_all(&workspace).ok();
        // Semaphore(3) giữ dưới trần 4 agent.run đồng thời/app của daemon.
        let _permit = state
            .agent_sema
            .acquire()
            .await
            .map_err(|_| "semaphore closed".to_string())?;
        crate::llm::agent_run(
            system,
            prompt,
            &space,
            &workspace.to_string_lossy(),
            tools.as_deref(),
            member.model.as_deref(),
            AGENT_TIMEOUT_SECS,
        )
        .await
        .map(|(text, _usage)| text)
    } else {
        // Per-member model chạy THẬT ở đường này: llm.request ghim profile.
        let (text, _model, finish) =
            crate::llm::llm_request_on(system, prompt, MEMBER_MAX_TOKENS, member.model.as_deref())
                .await?;
        if finish == "length" {
            // JSON có thể đứt — parser sẽ báo; retry ngắn hơn ở tầng trên.
        }
        Ok(text)
    }
}

/// Chạy một lượt của member: build ngữ cảnh → gọi LLM/agent → validate luật →
/// ghi tin nhắn + bộ nhớ + thinking. Trả số tin đã đăng.
pub async fn run_member_turn(
    state: &AppState,
    disc_id: i64,
    member: &Member,
    round: i64,
    force: Option<String>,
) -> anyhow::Result<usize> {
    let Some(disc) = state.db.discussion_get(disc_id)? else {
        return Ok(0);
    };
    if disc.status != "running" {
        return Ok(0);
    }
    state.set_member_status(disc_id, member.id, if member.use_tools { "tools" } else { "thinking" });

    let all_members = state.db.member_list()?;
    let system = member_system(member);
    let prompt = member_prompt(state, &disc, member, &all_members, round, &force)?;

    let mut posted = 0usize;
    let mut violations: Vec<String> = Vec::new();
    let mut turn: Option<parse::TurnOut> = None;

    for attempt in 0..2 {
        let p = if attempt == 0 {
            prompt.clone()
        } else {
            format!(
                "{prompt}\n\n## LỖI LƯỢT TRƯỚC — SỬA NGAY\n{}\nTrả về lại TOÀN BỘ JSON đúng luật.",
                violations.join("\n")
            )
        };
        let raw = match call_member_llm(state, member, disc_id, &system, &p).await {
            Ok(t) => t,
            Err(e) => {
                state.set_member_status(disc_id, member.id, "idle");
                state.db.message_insert(&NewMessage {
                    discussion_id: disc_id,
                    round,
                    author_kind: "system".into(),
                    kind: "system".into(),
                    content: format!("{} gặp lỗi lượt này: {}", member.name, truncate_chars(&e, 300)),
                    citations: json!([]),
                    flags: json!({"error": true, "member_id": member.id}),
                    ..Default::default()
                })?;
                return Ok(0);
            }
        };
        match parse::parse_turn(&raw) {
            None => {
                violations = vec!["Đầu ra không phải JSON đúng schema (hoặc bị cắt). CHỈ trả về một khối JSON, ngắn gọn lại nội dung.".into()];
                continue;
            }
            Some(t) => {
                // Luật: disagree phải có dẫn chứng.
                let bad: Vec<String> = t
                    .reactions
                    .iter()
                    .filter(|r| r.stance == "disagree" && r.citations.iter().all(|c| c.r#ref.trim().is_empty()))
                    .map(|r| format!("- Phản đối #{} nhưng KHÔNG có dẫn chứng — bổ sung citations (doc:/url/tool) hoặc đổi thành agree kèm bổ sung.", r.reply_to))
                    .collect();
                if !bad.is_empty() && attempt == 0 {
                    violations = bad;
                    continue;
                }
                turn = Some(t);
                break;
            }
        }
    }

    let Some(t) = turn else {
        state.set_member_status(disc_id, member.id, "idle");
        state.db.message_insert(&NewMessage {
            discussion_id: disc_id,
            round,
            author_kind: "system".into(),
            kind: "system".into(),
            content: format!("{} không đưa ra được phát biểu hợp lệ ở vòng {round}.", member.name),
            citations: json!([]),
            flags: json!({"invalid": true, "member_id": member.id}),
            ..Default::default()
        })?;
        return Ok(0);
    };

    state.set_member_status(disc_id, member.id, "speaking");

    // Reactions
    for r in &t.reactions {
        let stance = match r.stance.trim().to_lowercase().as_str() {
            "agree" | "đồng tình" | "dong tinh" => "agree",
            "disagree" | "phản đối" | "phan doi" => "disagree",
            _ => continue,
        };
        // reply_to phải là tin có thật trong phiên này
        let Ok(Some(target)) = state.db.message_get(r.reply_to) else {
            continue;
        };
        if target.discussion_id != disc_id {
            continue;
        }
        let citations = normalize_citations(state, &r.citations);
        let missing_evidence = stance == "disagree"
            && citations
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true);
        let mut content = r.content.trim().to_string();
        if !r.supplement.trim().is_empty() {
            content.push_str(&format!("\n\n**Bổ sung:** {}", r.supplement.trim()));
        }
        if content.is_empty() {
            continue;
        }
        state.db.message_insert(&NewMessage {
            discussion_id: disc_id,
            round,
            author_kind: "member".into(),
            member_id: Some(member.id),
            kind: "reaction".into(),
            content,
            stance: Some(stance.into()),
            reply_to: Some(r.reply_to),
            hat: parse::valid_hat(&r.hat).map(str::to_string),
            citations,
            flags: if missing_evidence {
                json!({"missing_evidence": true})
            } else {
                json!({})
            },
            ..Default::default()
        })?;
        posted += 1;
    }

    // Claims — tối đa 2, đúng nhãn
    for c in t.claims.iter().take(2) {
        let content = c.content.trim();
        if content.is_empty() {
            continue;
        }
        let claim_type = parse::valid_claim_type(&c.claim_type).unwrap_or("inference");
        let provability = parse::valid_provability(&c.provability).unwrap_or(match claim_type {
            "evidence" => "practical",
            _ => "theoretical",
        });
        let citations = normalize_citations(state, &c.citations);
        // evidence mà không nguồn nào → hạ nhãn thành inference/lý thuyết.
        let (claim_type, provability, flags) = if claim_type == "evidence"
            && citations.as_array().map(|a| a.is_empty()).unwrap_or(true)
        {
            ("inference", "theoretical", json!({"downgraded": "evidence không kèm nguồn"}))
        } else {
            (claim_type, provability, json!({}))
        };
        state.db.message_insert(&NewMessage {
            discussion_id: disc_id,
            round,
            author_kind: "member".into(),
            member_id: Some(member.id),
            kind: "opinion".into(),
            content: content.to_string(),
            claim_type: Some(claim_type.into()),
            provability: Some(provability.into()),
            hat: parse::valid_hat(&c.hat).map(str::to_string),
            citations,
            flags,
            ..Default::default()
        })?;
        posted += 1;
    }

    // Bộ nhớ riêng + mạch suy nghĩ ("nhớ cả thinking đã dùng")
    for note in t.memory_notes.iter().take(MAX_MEMORY_NOTES_PER_TURN) {
        if !note.trim().is_empty() {
            let _ = state
                .db
                .memory_add(member.id, Some(disc_id), "fact", note.trim());
        }
    }
    if !t.thinking.trim().is_empty() {
        let _ = state
            .db
            .thinking_add(member.id, disc_id, round, t.thinking.trim());
    }

    // Im lặng hợp lệ (JSON rỗng) vẫn phải hiện diện trên feed — nếu không
    // BOSS tưởng lượt bị nuốt; Manager thì đã thấy qua participation.
    if posted == 0 {
        let _ = state.db.message_insert(&NewMessage {
            discussion_id: disc_id,
            round,
            author_kind: "system".into(),
            kind: "system".into(),
            content: format!("🤐 {} chọn không phát biểu lượt này.", member.name),
            citations: json!([]),
            flags: json!({"silent": true, "member_id": member.id}),
            ..Default::default()
        });
    }
    state.set_member_status(disc_id, member.id, "idle");
    Ok(posted)
}

/// Chuẩn hoá citations: doc:<id> phải tồn tại (không thì đánh dấu
/// verified=false), url phải http(s), tool giữ nguyên.
fn normalize_citations(state: &AppState, cits: &[parse::CitationOut]) -> serde_json::Value {
    let mut out = Vec::new();
    for c in cits.iter().take(6) {
        let r = c.r#ref.trim();
        if r.is_empty() {
            continue;
        }
        let kind = match c.kind.trim() {
            "doc" => "doc",
            "url" => "url",
            "tool" => "tool",
            _ => {
                if r.starts_with("doc:") {
                    "doc"
                } else if r.starts_with("http") {
                    "url"
                } else {
                    "tool"
                }
            }
        };
        let verified = match kind {
            "doc" => r
                .trim_start_matches("doc:")
                .parse::<i64>()
                .map(|id| state.db.doc_exists(id))
                .unwrap_or(false),
            "url" => r.starts_with("http://") || r.starts_with("https://"),
            _ => true,
        };
        out.push(json!({
            "kind": kind,
            "ref": r,
            "quote": truncate_chars(c.quote.trim(), 300),
            "verified": verified,
        }));
    }
    json!(out)
}

// ---------------- Thư ký ----------------

async fn secretary_update(state: &AppState, disc_id: i64, round: i64) {
    let Ok(Some(disc)) = state.db.discussion_get(disc_id) else {
        return;
    };
    let Ok(secretary) = state.db.member_with_role("secretary") else {
        return;
    };
    let Some(secretary) = secretary else { return };
    state.set_member_status(disc_id, secretary.id, "thinking");

    let prev = state
        .db
        .minutes_latest(disc_id)
        .ok()
        .flatten()
        .map(|m| m.content)
        .unwrap_or_else(|| "(chưa có)".into());
    let all_members = state.db.member_list().unwrap_or_default();
    let recent = state.db.messages_recent(disc_id, 60).unwrap_or_default();
    let this_round: Vec<String> = recent
        .iter()
        .filter(|m| m.round == round && m.kind != "minutes_note")
        .map(|m| render_message_line(m, &all_members))
        .collect();
    if this_round.is_empty() {
        state.set_member_status(disc_id, secretary.id, "idle");
        return;
    }

    let system = format!(
        "Bạn là {} — thư ký cuộc thảo luận. Nhiệm vụ: cập nhật BIÊN BẢN đầy đủ, trung lập, KHÔNG thêm ý kiến riêng.\n\
         Cấu trúc bắt buộc (markdown):\n\
         # Biên bản: <tên phiên>\n## Yêu cầu của BOSS\n## Diễn biến chính (theo vòng, mỗi vòng 2-4 gạch đầu dòng)\n\
         ## Bảng luận điểm (| # | Ai | Luận điểm | Loại | Mức chứng minh | Trạng thái đồng thuận |)\n\
         ## Quyết định & đồng thuận\n## Bất đồng còn mở\n## Việc cần làm / thiếu\n\
         Giữ bảng luận điểm TÍCH LUỸ qua các vòng (cập nhật trạng thái thay vì xoá). Trả về TOÀN BỘ biên bản mới.",
        secretary.name
    );
    let prompt = format!(
        "# Phiên #{}: {}\n## Yêu cầu BOSS\n{}\n\n## BIÊN BẢN HIỆN TẠI\n{}\n\n## TIN NHẮN VÒNG {} (mới)\n{}\n\nCập nhật biên bản.",
        disc.id,
        disc.title,
        disc.requirement,
        truncate_chars(&prev, 4000),
        round,
        this_round.join("\n"),
    );
    match crate::llm::llm_request(&system, &prompt, SECRETARY_MAX_TOKENS).await {
        Ok((text, _, _)) if !text.trim().is_empty() => {
            let _ = state.db.minutes_add(disc_id, round, text.trim());
            let _ = state.db.message_insert(&NewMessage {
                discussion_id: disc_id,
                round,
                author_kind: "secretary".into(),
                member_id: Some(secretary.id),
                kind: "minutes_note".into(),
                content: format!("📝 Biên bản vòng {round} đã cập nhật (xem panel Biên bản)."),
                citations: json!([]),
                flags: json!({}),
                ..Default::default()
            });
        }
        _ => {}
    }
    state.set_member_status(disc_id, secretary.id, "idle");
}

// ---------------- Manager ----------------

async fn manager_review(state: &AppState, disc_id: i64, round: i64) -> Option<parse::ManagerOut> {
    let disc = state.db.discussion_get(disc_id).ok().flatten()?;
    let manager = state.db.member_with_role("manager").ok().flatten()?;
    state.set_member_status(disc_id, manager.id, "thinking");

    let participation = state.db.participation(disc_id, round).ok()?;
    let part_s = participation
        .iter()
        .map(|p| {
            format!(
                "- {} — {} phát biểu, vòng cuối nói: {}, im lặng {} vòng",
                p.name, p.message_count, p.last_round, p.silent_rounds
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let opens = state.db.open_opinions(disc_id, 20).ok()?;
    let minutes = state
        .db
        .minutes_latest(disc_id)
        .ok()
        .flatten()
        .map(|m| truncate_chars(&m.content, 3000))
        .unwrap_or_else(|| "(chưa có)".into());

    let system = format!(
        "Bạn là {} — MANAGER cuộc thảo luận. Bạn ĐỘC LẬP, KHÔNG bàn nội dung, không thiên vị member nào.\n\
         Nhiệm vụ: (1) chấm mức độ ĐÁP ỨNG YÊU CẦU CỦA BOSS 0-100 dựa trên biên bản; (2) liệt kê phần còn thiếu; \
         (3) chỉ mặt member lười tham gia (im lặng ≥ {} vòng) để bắt phát biểu; (4) quyết định `met` = thảo luận đã đủ để chốt chưa.\n\
         Nguyên tắc chấm: tiêu chí nào của BOSS chưa có kết luận kèm mức chứng minh rõ (thực tiễn/lý thuyết) thì chưa đủ; \
         luận điểm mở chưa ai phản hồi thì chưa đủ; tin BOSS chưa được trả lời thì chưa đủ.\n\
         CHỈ trả về JSON: {{\"score\":<0-100>,\"met\":true|false,\"missing\":[\"...\"],\"nudges\":[{{\"member\":\"<key>\",\"reason\":\"...\"}}],\"note\":\"nhận xét điều phối 1-3 câu\"}}",
        manager.name, SILENT_ROUNDS_NUDGE
    );
    let prompt = format!(
        "# Phiên #{}: {} (vòng {}/{})\n## YÊU CẦU CỦA BOSS\n{}\n\n## BIÊN BẢN MỚI NHẤT\n{}\n\n## THAM GIA\n{}\n\n## LUẬN ĐIỂM CHƯA AI PHẢN HỒI: {}\n\nĐánh giá và trả JSON.",
        disc.id, disc.title, round, disc.max_rounds, disc.requirement, minutes, part_s,
        opens.len(),
    );

    let mut out = match crate::llm::llm_request(&system, &prompt, MANAGER_MAX_TOKENS).await {
        Ok((text, _, _)) => parse::parse_manager(&text),
        Err(_) => None,
    };
    if out.is_none() {
        // retry 1 lần: nhắc CHỈ JSON, ngắn — vẫn hỏng thì bỏ qua vòng này
        // (engine tự chốt bằng trần vòng, không kẹt).
        let p2 = format!("{prompt}\n\nLẦN TRƯỚC ĐẦU RA KHÔNG PHẢI JSON HỢP LỆ. Trả về CHỈ một khối JSON đúng schema, ngắn gọn.");
        out = match crate::llm::llm_request(&system, &p2, MANAGER_MAX_TOKENS).await {
            Ok((text, _, _)) => parse::parse_manager(&text),
            Err(_) => None,
        };
    }
    if let Some(ref m) = out {
        let score = m.score.clamp(0, 100);
        let _ = state
            .db
            .discussion_set_manager_eval(disc_id, score, &json!(m.missing));
        let mut content = format!("🔵 [Điều phối] Vòng {round}: đạt {score}/100 so với yêu cầu BOSS.");
        if !m.note.trim().is_empty() {
            content.push_str(&format!(" {}", m.note.trim()));
        }
        if !m.missing.is_empty() {
            content.push_str(&format!("\nCòn thiếu: {}", m.missing.join("; ")));
        }
        for n in &m.nudges {
            if let Ok(Some(nm)) = state.db.member_get_by_key(n.member.trim()) {
                content.push_str(&format!(
                    "\n⚠️ Yêu cầu {} phát biểu vòng tới — {}",
                    nm.name,
                    n.reason.trim()
                ));
            }
        }
        let _ = state.db.message_insert(&NewMessage {
            discussion_id: disc_id,
            round,
            author_kind: "manager".into(),
            member_id: Some(manager.id),
            kind: "manager_note".into(),
            content,
            hat: Some("blue".into()),
            citations: json!([]),
            flags: json!({"score": score, "met": m.met}),
            ..Default::default()
        });
    }
    state.set_member_status(disc_id, manager.id, "idle");
    out
}

// ---------------- Tổng hợp kết quả ----------------

/// Chốt phiên: chuyển `review`, Thư ký tổng hợp KẾT QUẢ có phân loại mức chứng
/// minh, chờ BOSS nghiệm thu (approve/reject).
pub async fn synthesize_result(state: &AppState, disc_id: i64, reason: &str) {
    let Ok(Some(disc)) = state.db.discussion_get(disc_id) else {
        return;
    };
    let _ = state.db.discussion_set_status(disc_id, "review");
    let _ = state.db.message_insert(&NewMessage {
        discussion_id: disc_id,
        round: disc.round,
        author_kind: "manager".into(),
        kind: "manager_note".into(),
        content: format!("🔵 [Điều phối] Đề nghị chốt phiên: {reason} Thư ký đang tổng hợp kết quả…"),
        hat: Some("blue".into()),
        citations: json!([]),
        flags: json!({"concluding": true}),
        ..Default::default()
    });

    let minutes = state
        .db
        .minutes_latest(disc_id)
        .ok()
        .flatten()
        .map(|m| m.content)
        .unwrap_or_default();
    let all_members = state.db.member_list().unwrap_or_default();
    // Toàn bộ luận điểm + phản hồi (giới hạn ký tự để không vỡ trần token)
    let msgs = state.db.messages_after(disc_id, 0, 500).unwrap_or_default();
    let mut claims_s = String::new();
    for m in msgs.iter().filter(|m| m.kind == "opinion") {
        claims_s.push_str(&render_message_line(m, &all_members));
        let cits = m.citations.as_array().cloned().unwrap_or_default();
        if !cits.is_empty() {
            let refs: Vec<String> = cits
                .iter()
                .map(|c| {
                    format!(
                        "{}{}",
                        c.get("ref").and_then(|x| x.as_str()).unwrap_or(""),
                        if c.get("verified").and_then(|x| x.as_bool()) == Some(false) {
                            " (chưa kiểm được)"
                        } else {
                            ""
                        }
                    )
                })
                .collect();
            claims_s.push_str(&format!("\n   Nguồn: {}", refs.join(" | ")));
        }
        claims_s.push('\n');
    }
    let disagreements: Vec<String> = msgs
        .iter()
        .filter(|m| m.kind == "reaction" && m.stance.as_deref() == Some("disagree"))
        .map(|m| render_message_line(m, &all_members))
        .collect();
    let claims_s = truncate_chars(&claims_s, 9000);
    let minutes_s = truncate_chars(&minutes, 5000);

    let system = "Bạn là Thư ký tổng hợp KẾT QUẢ CUỐI của phiên thảo luận. Trung lập, bám dẫn chứng, tiếng Việt.\n\
        Cấu trúc bắt buộc (markdown):\n\
        # KẾT QUẢ: <tên phiên>\n\
        ## Trả lời yêu cầu của BOSS (từng tiêu chí một)\n\
        ## Kết luận chính — MỖI kết luận một mục, gắn nhãn:\n\
        - **[Loại: dẫn chứng|suy diễn|sáng tạo] [Mức: THỰC TIỄN|LÝ THUYẾT]** nội dung — nguồn: …\n\
        (THỰC TIỄN = có nguồn kiểm chứng được; LÝ THUYẾT = suy diễn/giả thuyết hợp lý chưa kiểm chứng — phân loại trung thực, không nâng cấp)\n\
        ## Bất đồng còn bảo lưu (ai phản đối gì, dẫn chứng nào)\n\
        ## Đề xuất bước tiếp theo\n## Nguồn tham khảo\n\
        Nếu lý do chốt ghi 'chưa đạt đủ' thì thêm mục '## Phần chưa đạt' liệt kê tiêu chí thiếu.";
    let prompt = format!(
        "# Phiên #{}: {}\n## Yêu cầu BOSS\n{}\n## Lý do chốt\n{}\n\n## BIÊN BẢN\n{}\n\n## TOÀN BỘ LUẬN ĐIỂM\n{}\n\n## CÁC PHẢN ĐỐI\n{}\n\nViết kết quả cuối.",
        disc.id,
        disc.title,
        disc.requirement,
        reason,
        minutes_s,
        claims_s,
        disagreements.join("\n"),
    );

    let mut text = match crate::llm::llm_request(system, &prompt, RESULT_MAX_TOKENS).await {
        Ok((t, _, finish)) => {
            let mut t = t;
            if finish == "length" {
                // một lần auto-continue: nối tiếp phần đuôi
                let tail: String = t.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect();
                if let Ok((more, _, _)) = crate::llm::llm_request(
                    system,
                    &format!("{prompt}\n\n## PHẦN ĐÃ VIẾT (đuôi)\n…{tail}\n\nViết TIẾP phần còn thiếu, không lặp lại."),
                    RESULT_MAX_TOKENS,
                )
                .await
                {
                    t.push_str("\n");
                    t.push_str(more.trim());
                }
            }
            t
        }
        Err(e) => format!(
            "# KẾT QUẢ: {}\n\n_(Không tổng hợp được bằng LLM: {e})_\n\n## Biên bản cuối\n{}",
            disc.title, minutes_s
        ),
    };
    if text.trim().is_empty() {
        text = format!("# KẾT QUẢ: {}\n\n(trống)", disc.title);
    }

    let _ = state.db.result_add(disc_id, text.trim());
    let _ = state.db.message_insert(&NewMessage {
        discussion_id: disc_id,
        round: disc.round,
        author_kind: "secretary".into(),
        kind: "result_note".into(),
        content: "📋 Dự thảo KẾT QUẢ đã sẵn sàng — chờ BOSS nghiệm thu (Duyệt / Từ chối kèm góp ý).".into(),
        citations: json!([]),
        flags: json!({"review": true}),
        ..Default::default()
    });
}

/// BOSS duyệt kết quả: phiên `done`, kết quả lưu vào kho tài liệu chung, mỗi
/// member được ghi một dòng bài học vào bộ nhớ riêng (nhớ xuyên phiên).
pub fn approve_result(state: &AppState, disc_id: i64) -> anyhow::Result<()> {
    let Some(disc) = state.db.discussion_get(disc_id)? else {
        anyhow::bail!("phiên không tồn tại");
    };
    let Some(res) = state.db.result_latest(disc_id)? else {
        anyhow::bail!("chưa có kết quả để duyệt");
    };
    state.db.result_set_status(res.id, "approved", "")?;
    state.db.discussion_set_status(disc_id, "done")?;
    // Kết quả trở thành tài liệu kho chung (mọi phiên sau đọc được)
    let doc_id = state.db.doc_add(
        None,
        &format!("Kết quả phiên #{}: {}", disc.id, disc.title),
        "",
        &res.content,
        "result",
        "secretary",
    )?;
    crate::api::write_doc_file(disc_id, doc_id, &format!("Kết quả phiên #{}", disc.id), &res.content);
    // Bài học vào bộ nhớ riêng từng member
    let summary = truncate_chars(&res.content, 400);
    for m in state.db.discussion_members(disc_id)? {
        let _ = state.db.memory_add(
            m.id,
            Some(disc_id),
            "lesson",
            &format!("Phiên #{} '{}' đã chốt (BOSS duyệt). Tóm tắt: {}", disc.id, disc.title, summary),
        );
    }
    state.db.message_insert(&NewMessage {
        discussion_id: disc_id,
        round: disc.round,
        author_kind: "boss".into(),
        kind: "boss".into(),
        content: "✅ BOSS đã DUYỆT kết quả. Phiên kết thúc — cảm ơn cả đội!".into(),
        citations: json!([]),
        flags: json!({"approved": true}),
        ..Default::default()
    })?;
    Ok(())
}

/// BOSS từ chối kèm góp ý: mở lại phiên, góp ý thành tin BOSS ưu tiên.
pub fn reject_result(state: &AppState, disc_id: i64, feedback: &str) -> anyhow::Result<()> {
    let Some(disc) = state.db.discussion_get(disc_id)? else {
        anyhow::bail!("phiên không tồn tại");
    };
    if let Some(res) = state.db.result_latest(disc_id)? {
        state.db.result_set_status(res.id, "rejected", feedback)?;
    }
    // Nới trần vòng để đội còn chỗ làm tiếp
    state
        .db
        .discussion_set_pace(disc_id, None, None, Some(disc.max_rounds + 4))?;
    state.db.message_insert(&NewMessage {
        discussion_id: disc_id,
        round: disc.round,
        author_kind: "boss".into(),
        kind: "boss".into(),
        content: format!("❌ BOSS CHƯA duyệt kết quả. Góp ý phải xử lý: {feedback}"),
        citations: json!([]),
        flags: json!({"rejected": true}),
        ..Default::default()
    })?;
    state.db.discussion_set_status(disc_id, "running")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AppState;

    fn st() -> AppState {
        crate::api::make_test_state()
    }

    #[test]
    fn member_system_contains_rules_and_manual() {
        let s = st();
        let m = s.db.member_get_by_key("an-dan-chung").unwrap().unwrap();
        let sys = member_system(&m);
        assert!(sys.contains("claim_type"));
        assert!(sys.contains("mcp__zeach-mcp__zeach_research"));
        assert!(sys.contains("disagree"));
        let chi = s.db.member_get_by_key("chi-suy-luan").unwrap().unwrap();
        let sys2 = member_system(&chi);
        assert!(sys2.contains("KHÔNG dùng tool"));
    }

    #[test]
    fn member_prompt_includes_boss_priority_and_open_opinions() {
        let s = st();
        let ms = s.db.member_list().unwrap();
        let an = ms.iter().find(|m| m.key == "an-dan-chung").unwrap();
        let binh = ms.iter().find(|m| m.key == "binh-phan-bien").unwrap();
        let d = s
            .db
            .discussion_create("Chủ đề X", "Cần 2 kết luận thực tiễn", "sequential", 0, 8, &[an.id, binh.id])
            .unwrap();
        s.db.message_insert(&crate::db::NewMessage {
            discussion_id: d,
            round: 1,
            author_kind: "member".into(),
            member_id: Some(binh.id),
            kind: "opinion".into(),
            content: "Luận điểm của Bình".into(),
            claim_type: Some("inference".into()),
            citations: serde_json::json!([]),
            flags: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
        s.db.message_insert(&crate::db::NewMessage {
            discussion_id: d,
            round: 1,
            author_kind: "boss".into(),
            kind: "boss".into(),
            content: "Tập trung vào chi phí nhé".into(),
            citations: serde_json::json!([]),
            flags: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
        let disc = s.db.discussion_get(d).unwrap().unwrap();
        let p = member_prompt(&s, &disc, an, &ms, 2, &None).unwrap();
        assert!(p.contains("Tập trung vào chi phí"));
        assert!(p.contains("Luận điểm của Bình"));
        assert!(p.contains("ƯU TIÊN SỐ 1"));
    }

    #[test]
    fn normalize_citations_verifies_doc_and_url() {
        let s = st();
        let doc = s.db.doc_add(None, "Tài liệu A", "a.md", "nội dung", "paste", "boss").unwrap();
        let cits = vec![
            parse::CitationOut { kind: "doc".into(), r#ref: format!("doc:{doc}"), quote: "q".into() },
            parse::CitationOut { kind: "doc".into(), r#ref: "doc:99999".into(), quote: "".into() },
            parse::CitationOut { kind: "".into(), r#ref: "https://vnexpress.net/x".into(), quote: "".into() },
            parse::CitationOut { kind: "".into(), r#ref: "zeach_research: chủ đề".into(), quote: "".into() },
        ];
        let v = normalize_citations(&s, &cits);
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0]["verified"], true);
        assert_eq!(arr[1]["verified"], false);
        assert_eq!(arr[2]["kind"], "url");
        assert_eq!(arr[2]["verified"], true);
        assert_eq!(arr[3]["kind"], "tool");
    }

    #[test]
    fn forced_members_computed_from_silence() {
        let s = st();
        let ms = s.db.member_list().unwrap();
        let an = ms.iter().find(|m| m.key == "an-dan-chung").unwrap();
        let chi = ms.iter().find(|m| m.key == "chi-suy-luan").unwrap();
        let d = s
            .db
            .discussion_create("t", "r", "sequential", 0, 8, &[an.id, chi.id])
            .unwrap();
        // An nói ở vòng 3, Chi im lặng từ đầu
        s.db.message_insert(&crate::db::NewMessage {
            discussion_id: d,
            round: 3,
            author_kind: "member".into(),
            member_id: Some(an.id),
            kind: "opinion".into(),
            content: "x".into(),
            citations: serde_json::json!([]),
            flags: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
        let p = s.db.participation(d, 3).unwrap();
        let chi_p = p.iter().find(|x| x.member_id == chi.id).unwrap();
        assert!(chi_p.silent_rounds >= SILENT_ROUNDS_NUDGE);
        let an_p = p.iter().find(|x| x.member_id == an.id).unwrap();
        assert_eq!(an_p.silent_rounds, 0);
    }

    #[test]
    fn approve_writes_shared_doc_and_member_lessons() {
        let s = st();
        let ms = s.db.member_list().unwrap();
        let an = ms.iter().find(|m| m.key == "an-dan-chung").unwrap();
        let d = s
            .db
            .discussion_create("Phiên duyệt", "r", "sequential", 0, 8, &[an.id])
            .unwrap();
        s.db.result_add(d, "# KẾT QUẢ\n- [Loại: dẫn chứng] [Mức: THỰC TIỄN] A đúng — nguồn doc:1").unwrap();
        s.db.discussion_set_status(d, "review").unwrap();
        approve_result(&s, d).unwrap();
        let disc = s.db.discussion_get(d).unwrap().unwrap();
        assert_eq!(disc.status, "done");
        let docs = s.db.doc_list(None, 10).unwrap();
        assert!(docs.iter().any(|x| x.source == "result"));
        let mem = s.db.memory_list(an.id, 10).unwrap();
        assert!(mem.iter().any(|m| m.kind == "lesson"));
    }

    #[test]
    fn reject_reopens_and_extends_rounds() {
        let s = st();
        let ms = s.db.member_list().unwrap();
        let an = ms.iter().find(|m| m.key == "an-dan-chung").unwrap();
        let d = s
            .db
            .discussion_create("Phiên từ chối", "r", "sequential", 0, 8, &[an.id])
            .unwrap();
        s.db.result_add(d, "kq").unwrap();
        s.db.discussion_set_status(d, "review").unwrap();
        reject_result(&s, d, "thiếu số liệu 2026").unwrap();
        let disc = s.db.discussion_get(d).unwrap().unwrap();
        assert_eq!(disc.status, "running");
        assert_eq!(disc.max_rounds, 12);
        let msgs = s.db.messages_after(d, 0, 10).unwrap();
        assert!(msgs.iter().any(|m| m.kind == "boss" && m.content.contains("thiếu số liệu")));
    }
}
