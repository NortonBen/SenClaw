//! Built-in pipeline agents — port of `internal/agent/agents/*.go`. The real
//! system prompts live in `souls/*.md`; each agent carries a concise in-code
//! fallback used only when the soul file is missing (souls::or_default).

use crate::agents::{Agent, Pool, Task, TaskResult};
use crate::context::{AgentContext, WorkingContext};
use crate::db::{self, str_of, Db, Row};
use crate::state::Core;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

/// Register every built-in agent (Go: pool.New wiring).
pub fn register_builtins(pool: &Arc<Pool>) {
    let core = pool.core.clone();
    pool.register(Arc::new(OrchestratorAgent { core: core.clone(), pool: Arc::downgrade(pool) }));
    pool.register(Arc::new(DirectorAgent));
    pool.register(Arc::new(ScreenwriterAgent));
    pool.register(Arc::new(ScenePlanAgent));
    pool.register(Arc::new(ShotDesignAgent));
    pool.register(Arc::new(VisualAssetAgent));
    pool.register(Arc::new(SceneBuilderAgent));
    pool.register(Arc::new(ScriptParserAgent));
    pool.register(Arc::new(GenRefAgent));
    pool.register(Arc::new(DirectorFrameAgent));
    pool.register(Arc::new(CharacterAgent { core: core.clone() }));
    pool.register(Arc::new(ImageAgent { core: core.clone() }));
    pool.register(Arc::new(VideoAgent { core: core.clone() }));
    pool.register(Arc::new(AudioAgent { core: pool.core.clone() }));
    pool.register(Arc::new(MediaDownloadAgent { core: core.clone() }));
    pool.register(Arc::new(ConcatAgent { core }));
    pool.register(Arc::new(CriticAgent));
}

// ---------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------

/// One JSON completion with a single "JSON only" retry (Go CompleteJSON +
/// extractJSONObject robustness).
///
/// A reply cut at the token cap is the dangerous case: `llm::parse_value`
/// repairs it into *valid* JSON with fewer elements, so a `shot_design` that
/// should hold 27 shots silently becomes 4 and the stage reports success.
/// `finish == "length"` is therefore treated as failure and retried, not
/// salvaged.
async fn complete_json(system: &str, user: &str, max_tokens: u32) -> Result<Map<String, Value>, String> {
    let (raw, _model, finish) = crate::llm::bridge_llm(system, user, max_tokens).await?;
    if finish != "length" {
        if let Some(m) = crate::llm::parse_value(&raw).ok().and_then(|v| v.as_object().cloned()) {
            return Ok(m);
        }
    } else {
        eprintln!("[llm] reply hit the {max_tokens}-token cap — asking for a tighter one");
    }
    let nudged = format!(
        "{user}\n\nReturn ONLY a single valid JSON object. No markdown fences, no prose. \
Keep it COMPACT — no line breaks or spaces beyond what JSON requires, and keep every \
free-text field short — the previous reply did not fit in the response budget."
    );
    let (raw2, _model2, finish2) = crate::llm::bridge_llm(system, &nudged, max_tokens).await?;
    let parsed = crate::llm::parse_value(&raw2)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| format!("invalid JSON from LLM: {}", crate::llm::truncate(&raw2, 500)))?;
    if finish2 == "length" {
        // Salvaged, but the caller is getting a partial answer — say so rather
        // than passing off a truncated plan as a complete one.
        return Err(format!(
            "LLM trả lời bị cắt vì hết {max_tokens} token (kết quả không đầy đủ) — \
tăng giới hạn token hoặc chia nhỏ đầu vào"
        ));
    }
    Ok(parsed)
}

fn sysprompt(ctx: &AgentContext, fallback: &str) -> String {
    crate::souls::or_default(&ctx.soul, fallback)
}

/// Prefix the project summary when present (Go: Memory.GetProjectSummary).
fn with_summary(ctx: &AgentContext, prompt: String) -> String {
    let summary = ctx.memory.project_summary();
    if summary.is_empty() {
        prompt
    } else {
        format!("{summary}\n\n{prompt}")
    }
}

fn mstr(m: &Map<String, Value>, k: &str) -> String {
    m.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn string_slice(v: Option<&Value>) -> Vec<String> {
    v.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a raw upstream result (stored as a JSON string) into an object map.
fn parse_upstream_json(raw: &str) -> Option<Map<String, Value>> {
    if raw.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(raw).ok().and_then(|v| v.as_object().cloned())
}

/// Find an upstream result containing `required_field`: canonical label first,
/// then a scan over every result. Returns the raw JSON string.
fn resolve_by_field(w: &WorkingContext, canonical_label: &str, required_field: &str) -> Option<String> {
    if let Some(raw) = w.get_result(canonical_label) {
        if let Some(obj) = parse_upstream_json(raw) {
            if obj.contains_key(required_field) {
                return Some(raw.clone());
            }
        }
    }
    for raw in w.all_results().values() {
        if let Some(obj) = parse_upstream_json(raw) {
            if obj.contains_key(required_field) {
                return Some(raw.clone());
            }
        }
    }
    None
}

/// Screenplay text from upstream results ("screenplay" label / field), else fallback.
fn resolve_screenplay(w: &WorkingContext, fallback: &str) -> String {
    if let Some(raw) = w.get_result("screenplay") {
        if let Some(obj) = parse_upstream_json(raw) {
            let s = mstr(&obj, "screenplay");
            if !s.is_empty() {
                return s;
            }
        }
        if !raw.is_empty() && parse_upstream_json(raw).is_none() {
            return raw.clone();
        }
    }
    for raw in w.all_results().values() {
        if let Some(obj) = parse_upstream_json(raw) {
            let s = mstr(&obj, "screenplay");
            if !s.is_empty() {
                return s;
            }
        }
    }
    fallback.to_string()
}

/// The screenwriter's per-scene text blocks (`scenes[]` with a "content" field).
fn resolve_scene_blocks(w: &WorkingContext) -> Vec<Map<String, Value>> {
    for label in ["screenplay", "screenwriter"] {
        if let Some(raw) = w.get_result(label) {
            let blocks = extract_scene_blocks(raw);
            if !blocks.is_empty() {
                return blocks;
            }
        }
    }
    for raw in w.all_results().values() {
        let blocks = extract_scene_blocks(raw);
        if !blocks.is_empty() {
            return blocks;
        }
    }
    Vec::new()
}

fn extract_scene_blocks(raw: &str) -> Vec<Map<String, Value>> {
    let Some(obj) = parse_upstream_json(raw) else { return Vec::new() };
    let Some(arr) = obj.get("scenes").and_then(|v| v.as_array()) else { return Vec::new() };
    // Verify it's a text-block array (has "content"), not production data.
    match arr.first().and_then(|v| v.as_object()) {
        Some(first) if first.contains_key("content") => {}
        _ => return Vec::new(),
    }
    arr.iter().filter_map(|v| v.as_object().cloned()).collect()
}

/// video_id from upstream results ("parse_script" label first, then scan).
fn resolve_video_id(w: &WorkingContext) -> String {
    if let Some(raw) = w.get_result("parse_script") {
        if let Some(obj) = parse_upstream_json(raw) {
            let vid = mstr(&obj, "video_id");
            if !vid.is_empty() {
                return vid;
            }
        }
    }
    for raw in w.all_results().values() {
        if let Some(obj) = parse_upstream_json(raw) {
            let vid = mstr(&obj, "video_id");
            if !vid.is_empty() {
                return vid;
            }
        }
    }
    String::new()
}

/// Split a Fountain screenplay at each scene heading into
/// `{scene_id, heading, content}` blocks.
fn split_screenplay_into_blocks(screenplay: &str) -> Vec<Map<String, Value>> {
    let mut blocks: Vec<Map<String, Value>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut current_heading = String::new();

    let flush = |blocks: &mut Vec<Map<String, Value>>, heading: &str, current: &[&str]| {
        if heading.is_empty() {
            return;
        }
        let mut m = Map::new();
        m.insert("scene_id".into(), json!(format!("{}", blocks.len() + 1)));
        m.insert("heading".into(), json!(heading));
        m.insert("content".into(), json!(current.join("\n").trim()));
        blocks.push(m);
    };

    for line in screenplay.lines() {
        let trimmed = line.trim();
        if is_scene_heading(trimmed) {
            flush(&mut blocks, &current_heading, &current);
            current_heading = trimmed.to_string();
            current = vec![line];
        } else {
            current.push(line);
        }
    }
    flush(&mut blocks, &current_heading, &current);
    blocks
}

/// Scene heading detection: standard Fountain (INT./EXT.) + Vietnamese formats.
fn is_scene_heading(line: &str) -> bool {
    let up = line.trim().to_uppercase();
    up.starts_with("INT.")
        || up.starts_with("EXT.")
        || up.starts_with("INT ")
        || up.starts_with("EXT ")
        || up.starts_with("PHÂN CẢNH")
        || up.starts_with("CẢNH ")
}

// ---- Vietnamese diacritic folding (gen_ref.go, ported exactly) ----

fn fold_vn_char(r: char) -> char {
    match r {
        'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ' | 'ấ'
        | 'ẩ' | 'ẫ' | 'ậ' => 'a',
        'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => 'e',
        'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
        'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ' | 'ớ'
        | 'ở' | 'ỡ' | 'ợ' => 'o',
        'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => 'u',
        'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
        'đ' => 'd',
        other => other,
    }
}

/// Canonical comparison key: lowercase, diacritics folded, alphanumerics only.
pub fn canonical_name_key(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let mut b = String::with_capacity(s.len());
    for r in s.chars() {
        let r = fold_vn_char(r);
        if r.is_alphabetic() || r.is_numeric() {
            b.push(r);
        }
    }
    b
}

/// Like `canonical_name_key` but KEEPS word boundaries: non-alphanumeric runs
/// become single spaces, padded with a leading+trailing space. Lets callers do a
/// bounded whole-word test `.contains(" name ")` instead of a raw substring.
pub fn canonical_name_phrase(s: &str) -> String {
    let s = s.trim().to_lowercase();
    let mut b = String::from(" ");
    let mut last_space = true;
    for r in s.chars() {
        let f = fold_vn_char(r);
        if f.is_alphabetic() || f.is_numeric() {
            b.push(f);
            last_space = false;
        } else if !last_space {
            b.push(' ');
            last_space = true;
        }
    }
    if !b.ends_with(' ') {
        b.push(' ');
    }
    b
}

/// "Cụ Già" → "CU GIA": uppercase Vietnamese-no-diacritic display form, other
/// characters collapsed to single spaces.
pub fn to_vietnamese_no_accent_name(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let mut b = String::with_capacity(s.len());
    let mut space = false;
    for r in s.to_lowercase().chars() {
        let r = fold_vn_char(r);
        if r.is_alphabetic() || r.is_numeric() {
            for u in r.to_uppercase() {
                b.push(u);
            }
            space = false;
            continue;
        }
        if !space {
            b.push(' ');
            space = true;
        }
    }
    b.trim().to_string()
}

// ---- DB helpers (repo.go equivalents) ----

fn list_videos(db: &Db, project_id: &str) -> Vec<Row> {
    db.query(
        "SELECT * FROM video WHERE project_id = ?1 ORDER BY display_order ASC, created_at ASC",
        &[&project_id],
    )
    .unwrap_or_default()
}

fn list_scenes(db: &Db, video_id: &str) -> Vec<Row> {
    db.query(
        "SELECT * FROM scene WHERE video_id = ?1 ORDER BY display_order ASC, created_at ASC",
        &[&video_id],
    )
    .unwrap_or_default()
}

fn ensure_video(db: &Db, project_id: &str) -> Result<String, String> {
    let rows = list_videos(db, project_id);
    if let Some(first) = rows.first() {
        return Ok(str_of(first, "id"));
    }
    let mut v = Map::new();
    v.insert("project_id".into(), json!(project_id));
    v.insert("title".into(), json!("Pipeline Video"));
    v.insert("display_order".into(), json!(0));
    v.insert("status".into(), json!("DRAFT"));
    db.insert("video", &v).map_err(|e| e.to_string())
}

fn delete_scenes_for_video(db: &Db, video_id: &str) -> Result<(), String> {
    db.execute("DELETE FROM scene WHERE video_id = ?1", &[&video_id])
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn link_character(db: &Db, project_id: &str, character_id: &str) -> Result<(), String> {
    db.execute(
        "INSERT OR REPLACE INTO project_character (project_id, character_id) VALUES (?1, ?2)",
        &[&project_id, &character_id],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// video_id from upstream, else the project's first video row.
fn resolve_video_id_or_first(w: &WorkingContext, db: &Db, project_id: &str) -> String {
    let vid = resolve_video_id(w);
    if !vid.is_empty() {
        return vid;
    }
    list_videos(db, project_id).first().map(|v| str_of(v, "id")).unwrap_or_default()
}

// ---- prompt assembly (script.go helpers) ----

fn contains_ci(haystack_lower: &str, needle: &str) -> bool {
    !needle.is_empty() && haystack_lower.contains(&needle.to_lowercase())
}

fn char_prefix(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// First clause of an action_sequence — the initial state before actions unfold.
fn first_action_clause(seq: &str) -> String {
    for sep in [". ", ".\n", " — ", " - "] {
        if let Some(idx) = seq.find(sep) {
            if idx > 8 {
                return seq[..idx].trim().to_string();
            }
        }
    }
    if seq.chars().count() > 100 {
        char_prefix(seq, 100)
    } else {
        seq.to_string()
    }
}

/// "NAME: text" lines → `NAME speaks: "text"` joined with "; then".
fn format_narrator_dialogue(narrator: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in narrator.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(':') {
            if idx > 0 {
                let name = line[..idx].trim();
                let text = line[idx + 1..].trim();
                if !name.is_empty() && !text.is_empty() {
                    out.push(format!("{name} speaks: \"{text}\""));
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    out.join("; then ")
}

fn chain_type_for_index(i: usize) -> &'static str {
    if i == 0 {
        "ROOT"
    } else {
        "CONTINUATION"
    }
}

/// Structured Veo3 prompt: [camera] — [scene/action base] — [action_sequence]
/// — [dialogue] — [atmosphere] — [continuity cue].
fn assemble_video_prompt(fields: &Map<String, Value>, prev_scene_cue: &str) -> String {
    let mut base = mstr(fields, "video_prompt");
    if base.is_empty() {
        base = mstr(fields, "prompt");
    }
    let base_lower = base.to_lowercase();
    let mut sections: Vec<String> = Vec::new();

    let mut cam_parts: Vec<String> = Vec::new();
    let shot = mstr(fields, "shot_type");
    if !shot.is_empty() && !contains_ci(&base_lower, &shot) {
        cam_parts.push(shot);
    }
    let cam = mstr(fields, "camera_movement");
    if !cam.is_empty() && !contains_ci(&base_lower, &cam) {
        cam_parts.push(cam);
    }
    if !cam_parts.is_empty() {
        sections.push(cam_parts.join(" "));
    }

    if !base.is_empty() {
        sections.push(base.trim().trim_end_matches('.').to_string());
    }

    let seq = mstr(fields, "action_sequence");
    if !seq.is_empty() {
        sections.push(seq.trim().trim_end_matches('.').to_string());
    }

    let narrator = mstr(fields, "narrator_text");
    if !narrator.is_empty() {
        let d = format_narrator_dialogue(&narrator);
        if !d.is_empty() {
            sections.push(d);
        }
    }

    let env_json = mstr(fields, "scene_environment");
    if !env_json.is_empty() {
        if let Some(env) = parse_upstream_json(&env_json) {
            let has_light = base_lower.contains("light")
                || base_lower.contains("k ")
                || base_lower.contains("sun");
            let has_color = base_lower.contains("color")
                || base_lower.contains("saturat")
                || base_lower.contains("tone");
            let v = mstr(&env, "lighting_setup");
            if !v.is_empty() && !has_light {
                sections.push(v);
            }
            let v = mstr(&env, "color_grading");
            if !v.is_empty() && !has_color {
                sections.push(v);
            }
        }
    }

    if !prev_scene_cue.is_empty() {
        sections.push(format!("Continuity from previous scene: {prev_scene_cue}"));
    }
    sections.join(". ")
}

/// Static image prompt for the scene's opening frame.
fn assemble_image_prompt(fields: &Map<String, Value>, prev_scene_cue: &str) -> String {
    let base = mstr(fields, "prompt");
    let base_lower = base.to_lowercase();
    let mut parts: Vec<String> = Vec::new();

    let shot = mstr(fields, "shot_type");
    if !shot.is_empty() && !contains_ci(&base_lower, &shot) {
        parts.push(shot);
    }
    if !base.is_empty() {
        parts.push(base.trim().trim_end_matches('.').to_string());
    }

    let seq = mstr(fields, "action_sequence");
    if !seq.is_empty() {
        let opening = first_action_clause(&seq);
        let prefix = char_prefix(&opening, 20);
        if !opening.is_empty() && !contains_ci(&base_lower, &prefix) {
            parts.push(opening);
        }
    }

    let env_json = mstr(fields, "scene_environment");
    if !env_json.is_empty() {
        if let Some(env) = parse_upstream_json(&env_json) {
            let v = mstr(&env, "spatial_layout");
            if !v.is_empty() {
                let prefix = char_prefix(&v, 20);
                if !contains_ci(&base_lower, &prefix) {
                    parts.push(v);
                }
            }
            let v = mstr(&env, "lighting_setup");
            if !v.is_empty() && !base_lower.contains("light") {
                parts.push(v);
            }
        }
    }

    let raw_names = mstr(fields, "character_names");
    if !raw_names.trim().is_empty() {
        if let Ok(names) = serde_json::from_str::<Vec<String>>(&raw_names) {
            if !names.is_empty() {
                parts.push(format!("Reference entities: {}", names.join(", ")));
            }
        }
    }
    if !prev_scene_cue.is_empty() {
        parts.push(format!("Continuity anchor from previous scene: {prev_scene_cue}"));
    }
    parts.join(". ")
}

fn build_scene_continuity_cue(fields: &Map<String, Value>) -> String {
    let mut parts: Vec<String> = Vec::new();
    for k in ["shot_type", "camera_movement"] {
        let v = mstr(fields, k);
        if !v.is_empty() {
            parts.push(v);
        }
    }
    let base = mstr(fields, "prompt");
    if !base.is_empty() {
        parts.push(base.trim().trim_end_matches('.').to_string());
    }
    parts.join(" | ")
}

// ---- upstream index builders (script.go) ----

/// scene index (0-based) → first shot for that scene. Ordered scene_ids sorted
/// as strings (faithful to the Go implementation).
fn build_shot_by_idx(w: &WorkingContext) -> HashMap<usize, Map<String, Value>> {
    let mut out = HashMap::new();
    let Some(raw) = resolve_by_field(w, "shot_list", "shots") else { return out };
    let Some(obj) = parse_upstream_json(&raw) else { return out };
    let Some(shots) = obj.get("shots").and_then(|v| v.as_array()) else { return out };

    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered_ids: Vec<String> = Vec::new();
    for s in shots {
        if let Some(shot) = s.as_object() {
            let sid = mstr(shot, "scene_id");
            if !sid.is_empty() && seen.insert(sid.clone()) {
                ordered_ids.push(sid);
            }
        }
    }
    ordered_ids.sort();
    let id_to_idx: HashMap<&String, usize> =
        ordered_ids.iter().enumerate().map(|(i, s)| (s, i)).collect();

    for s in shots {
        if let Some(shot) = s.as_object() {
            let sid = mstr(shot, "scene_id");
            if let Some(&idx) = id_to_idx.get(&sid) {
                out.entry(idx).or_insert_with(|| shot.clone());
            }
        }
    }
    out
}

/// scene index (0-based) → environment JSON string.
fn build_env_by_idx(w: &WorkingContext) -> HashMap<usize, String> {
    let mut out = HashMap::new();
    let Some(raw) = resolve_by_field(w, "environments", "scene_environments") else { return out };
    let Some(obj) = parse_upstream_json(&raw) else { return out };
    if let Some(envs) = obj.get("scene_environments").and_then(|v| v.as_array()) {
        for (i, e) in envs.iter().enumerate() {
            if e.is_object() {
                out.insert(i, e.to_string());
            }
        }
    }
    out
}

/// scene index (0-based) → director scene_block.
fn build_director_by_idx(w: &WorkingContext) -> HashMap<usize, Map<String, Value>> {
    let mut out = HashMap::new();
    let Some(raw) = resolve_by_field(w, "director", "scene_blocks") else { return out };
    let Some(obj) = parse_upstream_json(&raw) else { return out };
    if let Some(blocks) = obj.get("scene_blocks").and_then(|v| v.as_array()) {
        for (i, b) in blocks.iter().enumerate() {
            if let Some(m) = b.as_object() {
                out.insert(i, m.clone());
            }
        }
    }
    out
}

fn director_narrative_context(block: &Map<String, Value>) -> String {
    let mut nctx = Map::new();
    for k in ["narrative_beat", "conflict_type", "scene_objective", "value_charge_shift"] {
        nctx.insert(k.into(), block.get(k).cloned().unwrap_or(Value::Null));
    }
    Value::Object(nctx).to_string()
}

// ---------------------------------------------------------------------------
// orchestrator
// ---------------------------------------------------------------------------

const DEFAULT_ORCHESTRATOR: &str = "You are the OrchestratorAgent for a video production pipeline. \
Decompose the given script and project context into a DAG of tasks. Each task has: label (unique), \
agent_type (from the available types), prompt, depends_on (prerequisite labels), optional input_from, \
timeout_seconds. Return JSON only: {\"tasks\": [...]}.";

struct OrchestratorAgent {
    core: Arc<Core>,
    pool: Weak<Pool>,
}

#[async_trait::async_trait]
impl Agent for OrchestratorAgent {
    fn agent_type(&self) -> &str {
        "orchestrator"
    }
    fn description(&self) -> String {
        "Decompose a goal/script into a DAG of pipeline tasks using the available agent types.".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_ORCHESTRATOR.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let pool = self.pool.upgrade().ok_or("orchestrator: agent pool unavailable")?;
        let goal = ctx.working.inject_into_prompt(&task.prompt);
        let summary = ctx.memory.project_summary();
        let tasks = crate::pipeline::plan_with_llm(&self.core, &pool, &goal, &summary)
            .await
            .map_err(|e| format!("orchestrator llm: {e}"))?;
        let n = tasks.len();
        let mut data = Map::new();
        data.insert(
            "dag_plan".into(),
            json!({ "tasks": serde_json::to_value(&tasks).map_err(|e| e.to_string())? }),
        );
        Ok(TaskResult::new(data, format!("Planned {n} tasks")))
    }
}

// ---------------------------------------------------------------------------
// director
// ---------------------------------------------------------------------------

const DEFAULT_DIRECTOR: &str = "You are an Executive Director. Transform a raw story concept into a \
Hierarchical Narrative Blueprint with strict causality between scenes. Output JSON only: \
{\"scene_blocks\": [{\"scene_id\", \"narrative_beat\", \"conflict_type\" (Internal|Interpersonal|Environmental), \
\"scene_objective\", \"value_charge_shift\"}]}. At least 3 blocks; same language as the input; no camera/visual details.";

struct DirectorAgent;

#[async_trait::async_trait]
impl Agent for DirectorAgent {
    fn agent_type(&self) -> &str {
        "director"
    }
    fn description(&self) -> String {
        "Decompose a logline/synopsis into a hierarchical narrative blueprint (scene blocks with beats, conflict types, objectives). INPUT: task prompt with logline/story concept.".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_DIRECTOR.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let prompt = with_summary(ctx, ctx.working.inject_into_prompt(&task.prompt));
        let sys = sysprompt(ctx, DEFAULT_DIRECTOR);
        let result = complete_json(&sys, &prompt, 4000)
            .await
            .map_err(|e| format!("director llm: {e}"))?;
        let blocks = result.get("scene_blocks").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        Ok(TaskResult::new(result, format!("Narrative blueprint: {blocks} scene blocks")))
    }
}

// ---------------------------------------------------------------------------
// screenwriter
// ---------------------------------------------------------------------------

const DEFAULT_SCREENWRITER_SCENE: &str = "You are a professional Screenwriter (Fountain format). \
Write ONE scene in Fountain from the given scene block: ALL-CAPS INT./EXT. heading, present-tense \
action lines, every present character speaks 2-3+ lines with natural back-and-forth, optional \
NARRATOR block, same language as the input. Output plain Fountain text only for this one scene — \
no JSON, no fences, no commentary.";

const DEFAULT_SCREENWRITER_FULL: &str = "You are a professional Screenwriter (Fountain format). \
Expand the narrative scene blocks into a full cinematic screenplay: ALL-CAPS INT./EXT. headings, \
present-tense action, complete natural dialogue, same language as the input. Output JSON only: \
{\"screenplay\": \"<full Fountain screenplay, \\n line breaks>\", \"scene_count\": <int>}.";

struct ScreenwriterAgent;

#[async_trait::async_trait]
impl Agent for ScreenwriterAgent {
    fn agent_type(&self) -> &str {
        "screenwriter"
    }
    fn description(&self) -> String {
        "Expand director scene blocks into a full Fountain-format screenplay. INPUT: requires 'scene_blocks' from director output (depends_on: director task).".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_SCREENWRITER_FULL.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let entity_ctx = build_entity_context(ctx);

        // Scene-by-scene mode: one plain-text LLM call per director block.
        let dir_blocks = resolve_director_blocks(&ctx.working);
        if !dir_blocks.is_empty() {
            let sys = sysprompt(ctx, DEFAULT_SCREENWRITER_SCENE);
            // Each call sees only its own director block, so the scenes are
            // already independent — write them concurrently and restore
            // screenplay order afterwards.
            // Own every input before the fan-out: borrowing `dir_blocks` into
            // the async bodies makes the closure non-`for<'a>`.
            let jobs: Vec<(usize, String)> = dir_blocks
                .iter()
                .enumerate()
                .map(|(i, block)| (i, serde_json::to_string_pretty(block).unwrap_or_default()))
                .collect();
            let mut written: Vec<(usize, String)> = {
                use futures_util::stream::{self, StreamExt};
                stream::iter(jobs.into_iter().map(|(i, block_json)| {
                    let sys = sys.clone();
                    let entity_ctx = entity_ctx.clone();
                    async move {
                        let prompt = format!("{entity_ctx}\nScene block to write:\n{block_json}");
                        crate::llm::complete(&sys, &prompt, 4000)
                            .await
                            .map(|(raw, _)| (i, strip_accidental_fences(raw.trim())))
                            .map_err(|e| {
                                format!("screenwriter scene-by-scene: scene {}: {e}", i + 1)
                            })
                    }
                }))
                .buffer_unordered(crate::config::llm_concurrency())
                .collect::<Vec<_>>()
                .await
                .into_iter()
                .collect::<Result<Vec<_>, String>>()?
            };
            written.sort_by_key(|(i, _)| *i);
            let full = written.into_iter().map(|(_, s)| s).collect::<Vec<_>>().join("\n\n");
            let blocks = split_screenplay_into_blocks(&full);
            let n = blocks.len();
            let mut result = Map::new();
            result.insert("screenplay".into(), json!(full));
            result.insert("scene_count".into(), json!(n));
            result.insert("scenes".into(), Value::Array(blocks.into_iter().map(Value::Object).collect()));
            return Ok(TaskResult::new(result, format!("Screenplay written: {n} scenes (scene-by-scene mode)")));
        }

        // Fallback: full screenplay in one shot.
        let prompt = format!("{entity_ctx}{}", ctx.working.inject_into_prompt(&task.prompt));
        let sys = sysprompt(ctx, DEFAULT_SCREENWRITER_FULL);
        let (raw, _) = crate::llm::complete(&sys, &prompt, 8000)
            .await
            .map_err(|e| format!("screenwriter llm: {e}"))?;

        let mut result = match crate::llm::parse_value(&raw).ok().and_then(|v| v.as_object().cloned()) {
            Some(m) => m,
            None => parse_screenwriter_fallback(&crate::llm::strip_fences(&raw))
                .map_err(|e| format!("screenwriter unmarshal: {e}\nraw: {}", crate::llm::truncate(&raw, 500)))?,
        };

        let sp = mstr(&result, "screenplay");
        if !sp.is_empty() {
            let blocks = split_screenplay_into_blocks(&sp);
            result.insert("scenes".into(), Value::Array(blocks.into_iter().map(Value::Object).collect()));
        }
        let count = result.get("scene_count").and_then(|v| v.as_i64()).unwrap_or(0);
        Ok(TaskResult::new(result, format!("Screenplay written: {count} scenes")))
    }
}

fn strip_accidental_fences(scene: &str) -> String {
    if scene.starts_with("```") {
        if let Some(end) = scene.rfind("```") {
            if end > 3 {
                let start = scene.find('\n').map(|i| i + 1).unwrap_or(3);
                if start < end {
                    return scene[start..end].trim().to_string();
                }
            }
        }
    }
    scene.to_string()
}

/// Entity profiles from project memory grouped by type + project summary.
fn build_entity_context(ctx: &AgentContext) -> String {
    let mut sb = String::new();
    let chars = ctx.memory.list_characters();
    if !chars.is_empty() {
        let order = ["character", "location", "creature", "visual_asset", "generic_troop", "faction"];
        let labels: HashMap<&str, &str> = [
            ("character", "CHARACTER PROFILES"),
            ("location", "LOCATIONS"),
            ("creature", "CREATURES"),
            ("visual_asset", "VISUAL ASSETS / PROPS"),
            ("generic_troop", "TROOPS / CROWDS"),
            ("faction", "FACTIONS / ORGANIZATIONS"),
        ]
        .into_iter()
        .collect();
        let mut groups: HashMap<String, Vec<&Row>> = HashMap::new();
        for c in &chars {
            let mut et = str_of(c, "entity_type");
            if et.is_empty() {
                et = "character".into();
            }
            groups.entry(et).or_default().push(c);
        }
        for et in order {
            if let Some(list) = groups.get(et) {
                if list.is_empty() {
                    continue;
                }
                sb.push_str(labels[et]);
                sb.push_str(":\n");
                for c in list {
                    let name = str_of(c, "name");
                    if !name.is_empty() {
                        sb.push_str(&format!("- {name}: {}\n", str_of(c, "description")));
                    }
                }
                sb.push('\n');
            }
        }
    }
    let summary = ctx.memory.project_summary();
    if !summary.is_empty() {
        sb.push_str(&summary);
        sb.push_str("\n\n");
    }
    sb
}

fn resolve_director_blocks(w: &WorkingContext) -> Vec<Map<String, Value>> {
    let Some(raw) = resolve_by_field(w, "director", "scene_blocks") else { return Vec::new() };
    let Some(obj) = parse_upstream_json(&raw) else { return Vec::new() };
    obj.get("scene_blocks")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|b| b.as_object().cloned()).collect())
        .unwrap_or_default()
}

/// Salvage `{"screenplay": ..., "scene_count": N}` from broken JSON.
fn parse_screenwriter_fallback(s: &str) -> Result<Map<String, Value>, String> {
    let sc_key = "\"scene_count\"";
    let sc_idx = s.rfind(sc_key).ok_or("fallback parse failed: scene_count not found")?;
    let after = &s[sc_idx + sc_key.len()..];
    let colon = after.find(':').ok_or("fallback parse failed: scene_count has no value")?;
    let digits: String = after[colon + 1..]
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let scene_count: i64 = digits
        .parse()
        .map_err(|e| format!("fallback parse failed: invalid scene_count: {e}"))?;

    let key = "\"screenplay\"";
    let k = s.find(key).ok_or("fallback parse failed: screenplay not found")?;
    let colon2 = s[k + key.len()..]
        .find(':')
        .ok_or("fallback parse failed: screenplay key has no value")?;
    let value_start = k + key.len() + colon2 + 1;
    if sc_idx <= value_start {
        return Err("fallback parse failed: screenplay boundaries invalid".to_string());
    }
    let chunk = s[value_start..sc_idx]
        .trim()
        .trim_end_matches(',')
        .trim()
        .trim_matches('"')
        .replace("\\r\\n", "\n")
        .replace("\\n", "\n")
        .replace("\\\"", "\"");
    let chunk = chunk.trim();
    if chunk.is_empty() {
        return Err("fallback parse failed: screenplay is empty".to_string());
    }
    let mut m = Map::new();
    m.insert("screenplay".into(), json!(chunk));
    m.insert("scene_count".into(), json!(scene_count));
    Ok(m)
}

// ---------------------------------------------------------------------------
// scene_plan
// ---------------------------------------------------------------------------

const DEFAULT_SCENE_PLAN: &str = "You are a Production Designer. Convert a Fountain screenplay into \
per-scene Environmental Blueprints (architecture, lighting setup, color grading, spatial layout) \
used to condition every downstream video prompt. Output JSON only: {\"scene_environments\": \
[{\"scene_id\", \"scene_architecture\", \"lighting_setup\", \"color_grading\", \"spatial_layout\"}]}.";

struct ScenePlanAgent;

#[async_trait::async_trait]
impl Agent for ScenePlanAgent {
    fn agent_type(&self) -> &str {
        "scene_plan"
    }
    fn description(&self) -> String {
        "Generate per-scene environmental blueprints (architecture, lighting, spatial layout). INPUT: requires 'screenplay' field from screenwriter output (depends_on: screenwriter task).".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_SCENE_PLAN.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let screenplay = resolve_screenplay(&ctx.working, "");
        let prompt = if !screenplay.is_empty() {
            format!("SCREENPLAY:\n{screenplay}\n\n{}", task.prompt)
        } else {
            ctx.working.inject_into_prompt(&task.prompt)
        };
        let prompt = with_summary(ctx, prompt);
        let sys = sysprompt(ctx, DEFAULT_SCENE_PLAN);
        let result = complete_json(&sys, &prompt, 8000)
            .await
            .map_err(|e| format!("scene_plan llm: {e}"))?;
        let envs = result.get("scene_environments").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        Ok(TaskResult::new(result, format!("Environment blueprints: {envs} scenes planned")))
    }
}

// ---------------------------------------------------------------------------
// shot_design
// ---------------------------------------------------------------------------

const DEFAULT_SHOT_DESIGN: &str = "You are a Director of Photography. Break the screenplay and its \
environmental blueprints into a formal Shot List using exact cinematic vocabulary (shot sizes EWS/WS/\
FS/MS/CU/ECU/OTS; movements Static, Trucking, Dolly In/Out, Pan, Tilt, Arc; angles Eye-Level/High/Low/\
Dutch). Each shot carries a single-sentence English synthesis_prompt (environment + lighting + shot \
size + angle + movement + subject action). Output JSON only: {\"shots\": [{\"shot_id\", \"scene_id\", \
\"shot_size\", \"camera_angle\", \"camera_movement\", \"action_description\", \"synthesis_prompt\"}]}.";

struct ShotDesignAgent;

#[async_trait::async_trait]
impl Agent for ShotDesignAgent {
    fn agent_type(&self) -> &str {
        "shot_design"
    }
    fn description(&self) -> String {
        "Generate a formal shot list (shot sizes, camera moves, synthesis prompts). INPUT: requires 'screenplay' from screenwriter AND 'scene_environments' from scene_plan (depends_on: screenwriter + scene_plan tasks).".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_SHOT_DESIGN.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let screenplay = resolve_screenplay(&ctx.working, "");
        let env_raw = resolve_by_field(&ctx.working, "environments", "scene_environments");
        let dna_raw = resolve_by_field(&ctx.working, "visual_asset", "characters");

        let mut prompt;
        if !screenplay.is_empty() || env_raw.is_some() {
            prompt = task.prompt.clone();
            if !screenplay.is_empty() {
                prompt = format!("SCREENPLAY:\n{screenplay}\n\n{prompt}");
            }
            if let Some(raw) = &env_raw {
                if let Some(obj) = parse_upstream_json(raw) {
                    if let Some(envs) = obj.get("scene_environments") {
                        let b = serde_json::to_string_pretty(envs).unwrap_or_default();
                        prompt = format!("SCENE ENVIRONMENTS:\n{b}\n\n{prompt}");
                    }
                }
            }
            if let Some(raw) = &dna_raw {
                if let Some(obj) = parse_upstream_json(raw) {
                    if let Some(chars) = obj.get("characters") {
                        let b = serde_json::to_string_pretty(chars).unwrap_or_default();
                        prompt = format!("CHARACTER DNA:\n{b}\n\n{prompt}");
                    }
                }
            }
        } else {
            prompt = ctx.working.inject_into_prompt(&task.prompt);
        }
        let prompt = with_summary(ctx, prompt);
        let sys = sysprompt(ctx, DEFAULT_SHOT_DESIGN);
        let result = complete_json(&sys, &prompt, 8000)
            .await
            .map_err(|e| format!("shot_design llm: {e}"))?;
        let shots = result.get("shots").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
        Ok(TaskResult::new(result, format!("Shot list designed: {shots} shots")))
    }
}

// ---------------------------------------------------------------------------
// visual_asset
// ---------------------------------------------------------------------------

const DEFAULT_VISUAL_ASSET: &str = "You are a Visual Asset Director. For every listed entity, output \
one DNA Blueprint row: copy character_id exactly, produce a golden_image_prompt suited to its \
entity_type, plus compact base_appearance_tags. \
For a CHARACTER (or creature), the golden_image_prompt MUST describe a CHARACTER MODEL SHEET / \
turnaround, NOT a single portrait — the SAME person shown in a horizontal row of full-body views \
(front view, 3/4 view, and side profile) PLUS one head-and-shoulders face close-up, arranged side by \
side on a plain neutral light-grey studio background, flat even lighting, neutral standing pose, \
neutral expression, no props, no scene. Bake in EVERY invariant identity trait so the character is \
unmistakable from any angle: exact age, ethnicity, gender, skin tone, face shape, eye color, hair \
color/length/style, build/height, distinctive marks (scars, glasses, facial hair), and one default \
outfit. This multi-view sheet is what keeps the character consistent across scenes. \
For a LOCATION use an establishing shot; for visual_asset/generic_troop/faction use a \
type-appropriate reference. No emotion, action, or scene context in golden prompts. Exactly one \
output row per input row. Output JSON only: {\"characters\": [{\"character_id\", \"name\", \
\"entity_type\", \"golden_image_prompt\", \"base_appearance_tags\", \"ref_scenes\": []}]}.";

struct VisualAssetAgent;

#[async_trait::async_trait]
impl Agent for VisualAssetAgent {
    fn agent_type(&self) -> &str {
        "visual_asset"
    }
    fn description(&self) -> String {
        "Generate Character DNA Blueprints (golden image prompts + base appearance tags + ref_scenes) for all project characters. INPUT: grounded by screenplay context (depends_on: screenwriter task) and reads persisted entities from DB when available.".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_VISUAL_ASSET.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let mut chars = ctx.memory.list_characters();
        let db_ids: HashSet<String> =
            chars.iter().map(|c| str_of(c, "id")).filter(|s| !s.is_empty()).collect();
        if chars.is_empty() {
            chars = build_visual_asset_fallback_entities(&ctx.working);
            if chars.is_empty() {
                let mut data = Map::new();
                data.insert("characters".into(), json!([]));
                return Ok(TaskResult::new(data, "No characters found"));
            }
        }

        let mut sb = String::new();
        let summary = ctx.memory.project_summary();
        if !summary.is_empty() {
            sb.push_str(&summary);
            sb.push_str("\n\n");
        }
        sb.push_str("Generate DNA Blueprints for the following entities:\n\n");
        for c in &chars {
            let mut et = str_of(c, "entity_type");
            if et.is_empty() {
                et = "character".into();
            }
            sb.push_str(&format!(
                "character_id: {}\nname: {}\nentity_type: {et}\ndescription: {}\nexisting_prompt: {}\n\n",
                str_of(c, "id"),
                str_of(c, "name"),
                str_of(c, "description"),
                str_of(c, "image_prompt"),
            ));
        }
        if !task.prompt.is_empty() {
            sb.push_str("\nAdditional instructions: ");
            sb.push_str(&task.prompt);
        }

        let sys = sysprompt(ctx, DEFAULT_VISUAL_ASSET);
        let mut result = complete_json(&sys, &sb, 8000)
            .await
            .map_err(|e| format!("visual_asset llm: {e}"))?;

        // Write golden_image_prompt back to each character's image_prompt.
        let ref_scenes_by_name = build_visual_asset_ref_scenes_index(&db, &ctx.project_id);
        let name_by_id: HashMap<String, String> = chars
            .iter()
            .filter_map(|c| {
                let id = str_of(c, "id");
                let name = str_of(c, "name");
                (!id.is_empty() && !name.is_empty()).then(|| (id, name.to_lowercase()))
            })
            .collect();

        let mut count = 0usize;
        if let Some(Value::Array(dna_list)) = result.get_mut("characters") {
            count = dna_list.len();
            for item in dna_list.iter_mut() {
                let Some(dna) = item.as_object_mut() else { continue };
                let cid = mstr(dna, "character_id").trim().to_string();
                let prompt = mstr(dna, "golden_image_prompt");
                let tags = mstr(dna, "base_appearance_tags");
                let mut lookup = mstr(dna, "name").trim().to_lowercase();
                if lookup.is_empty() {
                    lookup = name_by_id.get(&cid).cloned().unwrap_or_default();
                }
                match ref_scenes_by_name.get(&lookup) {
                    Some(refs) if !refs.is_empty() => {
                        dna.insert("ref_scenes".into(), json!(refs));
                    }
                    _ => {
                        dna.insert("ref_scenes".into(), json!([]));
                    }
                }
                if cid.is_empty() || (prompt.is_empty() && tags.is_empty()) || prompt.is_empty() {
                    continue;
                }
                if !db_ids.contains(&cid) {
                    continue;
                }
                let mut patch = Map::new();
                patch.insert("image_prompt".into(), json!(prompt));
                // Persist the compact appearance so each scene prompt can reinforce
                // the character's invariant look (see `scene_ref_appearance`).
                if !tags.trim().is_empty() {
                    patch.insert("appearance_tags".into(), json!(tags.trim()));
                }
                if let Err(e) = db.update("character", &cid, &patch) {
                    eprintln!("[visual_asset] update character {cid}: {e}");
                }
            }
        }

        Ok(TaskResult::new(result, format!("Character DNA generated: {count} characters")))
    }
}

/// name(lowercase) → scene ids referencing it (across all project videos).
fn build_visual_asset_ref_scenes_index(db: &Db, project_id: &str) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    for v in list_videos(db, project_id) {
        let video_id = str_of(&v, "id");
        if video_id.is_empty() {
            continue;
        }
        for sc in list_scenes(db, &video_id) {
            let scene_id = str_of(&sc, "id");
            if scene_id.is_empty() {
                continue;
            }
            for name in parse_names(&str_of(&sc, "character_names")) {
                let key = name.trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                if seen.entry(key.clone()).or_default().insert(scene_id.clone()) {
                    out.entry(key).or_default().push(scene_id.clone());
                }
            }
        }
    }
    out
}

fn build_visual_asset_fallback_entities(w: &WorkingContext) -> Vec<Row> {
    if let Some(raw) = resolve_by_field(w, "script_parser", "characters") {
        let chars = extract_characters_from_result(&raw);
        if !chars.is_empty() {
            return chars;
        }
    }
    for raw in w.all_results().values() {
        let chars = extract_characters_from_result(raw);
        if !chars.is_empty() {
            return chars;
        }
    }
    if let Some(raw) = resolve_by_field(w, "screenwriter", "scenes") {
        let chars = infer_characters_from_screenwriter_scenes(&raw);
        if !chars.is_empty() {
            return chars;
        }
    }
    Vec::new()
}

fn extract_characters_from_result(raw: &str) -> Vec<Row> {
    let Some(obj) = parse_upstream_json(raw) else { return Vec::new() };
    let Some(arr) = obj.get("characters").and_then(|v| v.as_array()) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let Some(m) = item.as_object() else { continue };
        let name = mstr(m, "name").trim().to_string();
        if name.is_empty() {
            continue;
        }
        let mut cid = mstr(m, "character_id");
        if cid.is_empty() {
            cid = mstr(m, "id");
        }
        if cid.is_empty() {
            cid = format!("virtual:{}:{}", i + 1, name.to_lowercase().replace(' ', "_"));
        }
        let mut et = mstr(m, "entity_type");
        if et.is_empty() {
            et = "character".into();
        }
        let mut row = Map::new();
        row.insert("id".into(), json!(cid));
        row.insert("name".into(), json!(name));
        row.insert("entity_type".into(), json!(et));
        row.insert("description".into(), json!(mstr(m, "description").trim()));
        row.insert("image_prompt".into(), json!(mstr(m, "image_prompt").trim()));
        out.push(row);
    }
    out
}

fn infer_characters_from_screenwriter_scenes(raw: &str) -> Vec<Row> {
    let Some(obj) = parse_upstream_json(raw) else { return Vec::new() };
    let Some(scenes) = obj.get("scenes").and_then(|v| v.as_array()) else { return Vec::new() };
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for sc in scenes {
        let content = sc.get("content").and_then(|v| v.as_str()).unwrap_or("");
        if content.is_empty() {
            continue;
        }
        for line in content.lines() {
            let cand = line.trim();
            if !is_ascii_dialogue_cue(cand) || is_ignored_cue(cand) {
                continue;
            }
            let name = title_case(&cand.to_lowercase());
            if !seen.insert(name.clone()) {
                continue;
            }
            let mut row = Map::new();
            row.insert(
                "id".into(),
                json!(format!("virtual:scene:{}", name.to_lowercase().replace(' ', "_"))),
            );
            row.insert("name".into(), json!(name));
            row.insert("entity_type".into(), json!("character"));
            row.insert("description".into(), json!("Inferred from screenplay dialogue cue"));
            row.insert("image_prompt".into(), json!(""));
            out.push(row);
        }
    }
    out
}

/// `^[A-Z][A-Z0-9 \-']{1,40}$` (ASCII, screenwriter fallback inference).
fn is_ascii_dialogue_cue(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() < 2 || bytes.len() > 41 || !s.is_ascii() {
        return false;
    }
    if !bytes[0].is_ascii_uppercase() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b' ' || b == b'-' || b == b'\'')
}

fn is_ignored_cue(s: &str) -> bool {
    matches!(s, "INT" | "EXT" | "INT/EXT" | "CUT TO" | "FADE IN" | "FADE OUT" | "NARRATOR")
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// script_parser
// ---------------------------------------------------------------------------

struct ScriptParserAgent;

#[async_trait::async_trait]
impl Agent for ScriptParserAgent {
    fn agent_type(&self) -> &str {
        "script_parser"
    }
    fn description(&self) -> String {
        "Fallback scene parser: parses screenplay markdown → scenes and characters via LLM. Outputs 'video_id', 'scene_ids', 'scene_count'. INPUT: requires 'screenplay' from screenwriter and visual grounding from visual_asset (depends_on: screenwriter + visual_asset tasks). Use scene_builder instead when shot_design output is available.".into()
    }
    fn default_system(&self) -> String {
        // The full Vietnamese parser prompt is the script module's built-in.
        crate::script::PARSER_SYSTEM_PROMPT.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let soul = ctx.soul.clone(); // parser system override (SetSystemPrompt)

        // 1. Prefer per-scene blocks; else split programmatically; else full parse.
        let mut blocks = resolve_scene_blocks(&ctx.working);
        if blocks.is_empty() {
            let screenplay = resolve_screenplay(&ctx.working, &task.prompt);
            blocks = split_screenplay_into_blocks(&screenplay);
        }
        let parsed = if !blocks.is_empty() {
            crate::script::parse_blocks(&soul, &blocks)
                .await
                .map_err(|e| format!("script_parser (blocks): {e}"))?
        } else {
            let screenplay = resolve_screenplay(&ctx.working, &task.prompt);
            crate::script::parse(&soul, &screenplay)
                .await
                .map_err(|e| format!("script_parser: {e}"))?
        };

        // 2. Upstream overlays by scene index.
        let shot_by_idx = build_shot_by_idx(&ctx.working);
        let env_by_idx = build_env_by_idx(&ctx.working);
        let director_by_idx = build_director_by_idx(&ctx.working);

        // 4. Ensure video + purge old scenes (re-runs reuse the video row).
        let video_id = ensure_video(&db, &ctx.project_id)
            .map_err(|e| format!("script_parser ensure video: {e}"))?;
        delete_scenes_for_video(&db, &video_id)
            .map_err(|e| format!("script_parser purge scenes: {e}"))?;

        // 5. Upsert referenced characters that carry an image prompt.
        let referenced: HashSet<String> = parsed
            .scenes
            .iter()
            .flat_map(|sc| sc.character_names.iter())
            .map(|n| n.trim().to_lowercase())
            .filter(|n| !n.is_empty())
            .collect();
        let mut character_ids = Map::new();
        for ch in &parsed.characters {
            if ch.image_prompt.trim().is_empty() {
                continue;
            }
            if !referenced.contains(&ch.name.trim().to_lowercase()) {
                continue;
            }
            let cid = upsert_character(&db, &ctx.project_id, &ch.name, &ch.entity_type, &ch.description, &ch.image_prompt)
                .map_err(|e| format!("script_parser upsert character {:?}: {e}", ch.name))?;
            character_ids.insert(ch.name.clone(), json!(cid));
        }

        // 6. Write scenes with all overlays merged.
        let entity_canonical_by_key = load_entity_canonical_name_map(ctx);
        let mut scene_ids: Vec<String> = Vec::new();
        let mut prev_scene_cue = String::new();
        for (i, sc) in parsed.scenes.iter().enumerate() {
            let names = normalize_names_to_entity_catalog(&sc.character_names, &entity_canonical_by_key);
            let sid = db::new_id();
            let mut fields = Map::new();
            fields.insert("id".into(), json!(sid));
            fields.insert("video_id".into(), json!(video_id));
            fields.insert("display_order".into(), json!(sc.display_order));
            fields.insert("prompt".into(), json!(sc.prompt));
            fields.insert("video_prompt".into(), json!(sc.video_prompt));
            fields.insert("action_sequence".into(), json!(sc.action_sequence));
            fields.insert("camera_movement".into(), json!(sc.camera_movement));
            fields.insert("shot_type".into(), json!(sc.shot_type));
            fields.insert("character_names".into(), json!(serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())));
            fields.insert("narrator_text".into(), json!(sc.narrator_text));
            fields.insert("duration".into(), json!(sc.duration));
            fields.insert("chain_type".into(), json!(chain_type_for_index(i)));
            fields.insert("source".into(), json!("system"));

            // shot_design overlay
            if let Some(shot) = shot_by_idx.get(&i) {
                let v = mstr(shot, "synthesis_prompt");
                if !v.is_empty() {
                    fields.insert("video_prompt".into(), json!(v));
                }
                let v = mstr(shot, "camera_movement");
                if !v.is_empty() {
                    fields.insert("camera_movement".into(), json!(v));
                }
                let v = mstr(shot, "shot_size");
                if !v.is_empty() {
                    fields.insert("shot_type".into(), json!(v));
                }
            }
            // scene_plan overlay
            if let Some(env_json) = env_by_idx.get(&i) {
                fields.insert("scene_environment".into(), json!(env_json));
            }
            // director overlay
            if let Some(block) = director_by_idx.get(&i) {
                fields.insert("narrative_context".into(), json!(director_narrative_context(block)));
            }

            let vp = assemble_video_prompt(&fields, &prev_scene_cue);
            fields.insert("video_prompt".into(), json!(vp));
            let ip = assemble_image_prompt(&fields, &prev_scene_cue);
            fields.insert("image_prompt".into(), json!(ip));

            db.insert("scene", &fields)
                .map_err(|e| format!("script_parser create scene: {e}"))?;
            prev_scene_cue = build_scene_continuity_cue(&fields);
            scene_ids.push(sid);
        }

        let mut data = Map::new();
        data.insert("video_id".into(), json!(video_id));
        data.insert("scene_ids".into(), json!(scene_ids));
        data.insert("character_ids".into(), Value::Object(character_ids));
        data.insert("scene_count".into(), json!(parsed.scenes.len()));
        data.insert("character_count".into(), json!(parsed.characters.len()));
        Ok(TaskResult::new(
            data,
            format!("Parsed {} scenes and {} characters", parsed.scenes.len(), parsed.characters.len()),
        ))
    }
}

fn upsert_character(
    db: &Db,
    project_id: &str,
    name: &str,
    entity_type: &str,
    description: &str,
    image_prompt: &str,
) -> Result<String, String> {
    let target = name.trim().to_lowercase();
    if target.is_empty() {
        return Err("empty character/entity name".to_string());
    }
    let rows = db
        .query(
            "SELECT c.* FROM character c JOIN project_character pc ON pc.character_id = c.id \
             WHERE pc.project_id = ?1 ORDER BY c.name",
            &[&project_id],
        )
        .map_err(|e| e.to_string())?;
    for r in &rows {
        if str_of(r, "name").to_lowercase() == target {
            return Ok(str_of(r, "id"));
        }
    }
    let mut row = Map::new();
    row.insert("name".into(), json!(name));
    row.insert("entity_type".into(), json!(entity_type));
    row.insert("description".into(), json!(description));
    row.insert("image_prompt".into(), json!(image_prompt));
    let cid = db.insert("character", &row).map_err(|e| e.to_string())?;
    link_character(db, project_id, &cid)?;
    Ok(cid)
}

/// canonical key → canonical (first-seen) entity name for the project.
fn load_entity_canonical_name_map(ctx: &AgentContext) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for row in ctx.memory.list_characters() {
        let name = str_of(&row, "name");
        if name.is_empty() {
            continue;
        }
        let key = canonical_name_key(&name);
        if key.is_empty() {
            continue;
        }
        out.entry(key).or_insert(name);
    }
    out
}

/// Keep only names that map into the entity catalog (skip unknown aliases).
fn normalize_names_to_entity_catalog(
    names: &[String],
    entity_canonical_by_key: &HashMap<String, String>,
) -> Vec<String> {
    if names.is_empty() {
        return Vec::new();
    }
    // An empty catalog means the entities aren't in the DB *yet* (fresh
    // project), not that the scene has no characters. Dropping the parsed
    // names here silently removed the `Reference entities:` clause from every
    // image prompt — i.e. it quietly disabled character consistency.
    if entity_canonical_by_key.is_empty() {
        let mut seen: HashSet<String> = HashSet::new();
        return names
            .iter()
            .filter(|n| !n.trim().is_empty())
            .filter(|n| seen.insert(canonical_name_key(n)))
            .map(|n| n.trim().to_string())
            .collect();
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for raw in names {
        let key = canonical_name_key(raw);
        if key.is_empty() {
            continue;
        }
        let Some(canonical) = entity_canonical_by_key.get(&key) else { continue };
        if seen.insert(key) {
            out.push(canonical.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// scene_builder
// ---------------------------------------------------------------------------

struct SceneBuilderAgent;

#[async_trait::async_trait]
impl Agent for SceneBuilderAgent {
    fn agent_type(&self) -> &str {
        "scene_builder"
    }
    fn description(&self) -> String {
        "PREFERRED synthesis agent: builds scenes and entities from structured shot_design shots. Produces correct image_prompt (synthesis_prompt per shot), video_prompt, action_sequence, narrator_text without LLM scene parsing. Outputs 'video_id', 'scene_ids', 'scene_count'. INPUT: requires 'shots' from shot_design (depends_on: shot_design + scene_plan + director + screenwriter). Use this instead of script_parser when shot_design has run.".into()
    }
    fn default_system(&self) -> String {
        crate::script::PARSER_SYSTEM_PROMPT.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let soul = ctx.soul.clone();

        // 1. Structured upstream data.
        let shots_by_scene = sb_group_shots_by_scene_id(&ctx.working);
        let env_by_scene = sb_group_env_by_scene_id(&ctx.working);
        let dir_by_scene = sb_group_dir_by_scene_id(&ctx.working);
        if shots_by_scene.is_empty() {
            return Err("scene_builder: no shots found — shot_design must run first".to_string());
        }

        // 2. Entity extraction via LLM only (scenes come from shot_design).
        let mut entities: Vec<crate::script::ParsedCharacter> = Vec::new();
        let mut blocks = resolve_scene_blocks(&ctx.working);
        if blocks.is_empty() {
            let screenplay = resolve_screenplay(&ctx.working, &task.prompt);
            blocks = split_screenplay_into_blocks(&screenplay);
        }
        if !blocks.is_empty() {
            match crate::script::parse_blocks(&soul, &blocks).await {
                Ok(parsed) => entities = parsed.characters,
                Err(e) => eprintln!("[SceneBuilder] entity extraction failed (non-fatal): {e}"),
            }
        }

        let narrator_by_scene = sb_extract_narrator_by_scene(&ctx.working);
        let scene_entity_refs = sb_build_scene_entity_refs(&shots_by_scene, &narrator_by_scene, &entities);
        let entities = sb_filter_entities_by_usage(&entities, &scene_entity_refs);

        // 3. Ensure video + purge old scenes.
        let video_id = ensure_video(&db, &ctx.project_id)
            .map_err(|e| format!("scene_builder ensure video: {e}"))?;
        delete_scenes_for_video(&db, &video_id)
            .map_err(|e| format!("scene_builder purge scenes: {e}"))?;

        // 4. Upsert entities (exact-name match like the Go side).
        let mut character_ids = Map::new();
        for ch in &entities {
            match sb_upsert_entity(&db, &ctx.project_id, ch) {
                Ok(cid) => {
                    character_ids.insert(ch.name.clone(), json!(cid));
                }
                Err(e) => eprintln!("[SceneBuilder] upsert entity {:?}: {e}", ch.name),
            }
        }

        // 5. Build scenes from shot_design in canonical scene order.
        let scene_order = sb_ordered_scene_ids(&shots_by_scene);
        let mut scene_ids_out: Vec<String> = Vec::new();
        let mut prev_scene_cue = String::new();
        for (i, scene_id) in scene_order.iter().enumerate() {
            let shots = &shots_by_scene[scene_id];
            if shots.is_empty() {
                continue;
            }
            let sid = db::new_id();
            let fields = sb_build_scene_fields(
                &sid,
                &video_id,
                i + 1,
                shots,
                env_by_scene.get(scene_id).map(|s| s.as_str()).unwrap_or(""),
                dir_by_scene.get(scene_id),
                narrator_by_scene.get(scene_id).map(|s| s.as_str()).unwrap_or(""),
                &prev_scene_cue,
                scene_entity_refs.get(scene_id).cloned().unwrap_or_default(),
            );
            db.insert("scene", &fields)
                .map_err(|e| format!("scene_builder create scene {scene_id}: {e}"))?;
            prev_scene_cue = build_scene_continuity_cue(&fields);
            scene_ids_out.push(sid);
        }

        let mut data = Map::new();
        data.insert("video_id".into(), json!(video_id));
        data.insert("scene_ids".into(), json!(scene_ids_out));
        data.insert("character_ids".into(), Value::Object(character_ids));
        data.insert("scene_count".into(), json!(scene_ids_out.len()));
        data.insert("character_count".into(), json!(entities.len()));
        Ok(TaskResult::new(
            data,
            format!("Built {} scenes and {} entities from shot_design", scene_ids_out.len(), entities.len()),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn sb_build_scene_fields(
    sid: &str,
    video_id: &str,
    display_order: usize,
    shots: &[Map<String, Value>],
    env_json: &str,
    dir_block: Option<&Map<String, Value>>,
    narrator_text: &str,
    prev_scene_cue: &str,
    scene_entities: Vec<String>,
) -> Map<String, Value> {
    let primary = &shots[0]; // first shot = establishing shot
    let shot_size = mstr(primary, "shot_size");
    let camera_angle = mstr(primary, "camera_angle");
    let camera_move = mstr(primary, "camera_movement");

    let action_seq = shots
        .iter()
        .filter_map(|s| {
            let d = mstr(s, "action_description");
            (!d.is_empty()).then(|| d.trim().trim_end_matches('.').to_string())
        })
        .collect::<Vec<_>>()
        .join(". ");

    let synthesis = mstr(primary, "synthesis_prompt");
    let prompt = mstr(primary, "action_description");

    let video_prompt = sb_assemble_video_prompt(
        &synthesis, &camera_angle, &camera_move, &action_seq, narrator_text, env_json, prev_scene_cue,
    );
    let image_prompt = sb_assemble_image_prompt(&synthesis, &action_seq, env_json, prev_scene_cue, &scene_entities);

    let mut fields = Map::new();
    fields.insert("id".into(), json!(sid));
    fields.insert("video_id".into(), json!(video_id));
    fields.insert("display_order".into(), json!(display_order));
    fields.insert("prompt".into(), json!(prompt));
    fields.insert("image_prompt".into(), json!(image_prompt));
    fields.insert("video_prompt".into(), json!(video_prompt));
    fields.insert("action_sequence".into(), json!(action_seq));
    fields.insert("shot_type".into(), json!(shot_size));
    fields.insert("camera_movement".into(), json!(camera_move));
    fields.insert("narrator_text".into(), json!(narrator_text));
    fields.insert("duration".into(), json!(sb_default_duration(shots.len())));
    fields.insert("chain_type".into(), json!(chain_type_for_index(display_order - 1)));
    fields.insert("source".into(), json!("system"));
    if !scene_entities.is_empty() {
        fields.insert(
            "character_names".into(),
            json!(serde_json::to_string(&scene_entities).unwrap_or_else(|_| "[]".into())),
        );
    }
    if !env_json.is_empty() {
        fields.insert("scene_environment".into(), json!(env_json));
    }
    if let Some(block) = dir_block {
        fields.insert("narrative_context".into(), json!(director_narrative_context(block)));
    }
    fields
}

fn sb_assemble_video_prompt(
    synthesis: &str,
    camera_angle: &str,
    camera_move: &str,
    action_seq: &str,
    narrator_text: &str,
    env_json: &str,
    prev_scene_cue: &str,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let base_lower = synthesis.to_lowercase();

    let mut cam_parts: Vec<String> = Vec::new();
    if !camera_angle.is_empty() && !contains_ci(&base_lower, camera_angle) {
        cam_parts.push(camera_angle.to_string());
    }
    if !camera_move.is_empty() && !contains_ci(&base_lower, camera_move) {
        cam_parts.push(camera_move.to_string());
    }
    if !cam_parts.is_empty() {
        parts.push(cam_parts.join(" "));
    }
    if !synthesis.is_empty() {
        parts.push(synthesis.trim().trim_end_matches('.').to_string());
    }
    if !action_seq.is_empty() {
        parts.push(action_seq.to_string());
    }
    if !narrator_text.is_empty() {
        let d = format_narrator_dialogue(narrator_text);
        if !d.is_empty() {
            parts.push(d);
        }
    }
    if !env_json.is_empty() {
        if let Some(env) = parse_upstream_json(env_json) {
            let has_light = base_lower.contains("light")
                || base_lower.contains("k ")
                || base_lower.contains("sun");
            let v = mstr(&env, "lighting_setup");
            if !v.is_empty() && !has_light {
                parts.push(v);
            }
        }
    }
    if !prev_scene_cue.is_empty() {
        parts.push(format!("Continuity from previous scene: {prev_scene_cue}"));
    }
    parts.join(". ")
}

fn sb_assemble_image_prompt(
    base: &str,
    action_seq: &str,
    env_json: &str,
    prev_scene_cue: &str,
    scene_entities: &[String],
) -> String {
    let mut parts: Vec<String> = Vec::new();
    let base = base.trim();
    if !base.is_empty() {
        parts.push(base.trim_end_matches('.').to_string());
    }
    let opening = first_action_clause(action_seq);
    if !opening.is_empty() {
        parts.push(opening.trim_end_matches('.').to_string());
    }
    if !scene_entities.is_empty() {
        parts.push(format!("Reference entities: {}", scene_entities.join(", ")));
    }
    if !env_json.is_empty() {
        if let Some(env) = parse_upstream_json(env_json) {
            let v = mstr(&env, "spatial_layout");
            if !v.is_empty() {
                parts.push(v.trim().trim_end_matches('.').to_string());
            }
        }
    }
    if !prev_scene_cue.is_empty() {
        parts.push(format!("Continuity anchor from previous scene: {prev_scene_cue}"));
    }
    parts.join(". ")
}

fn sb_build_scene_entity_refs(
    shots_by_scene: &HashMap<String, Vec<Map<String, Value>>>,
    narrator_by_scene: &HashMap<String, String>,
    entities: &[crate::script::ParsedCharacter],
) -> HashMap<String, Vec<String>> {
    let mut scene_refs: HashMap<String, Vec<String>> = HashMap::new();
    let mut entity_by_lower: Vec<(String, String)> = Vec::new();
    let mut lower_seen: HashSet<String> = HashSet::new();
    for e in entities {
        let name = e.name.trim();
        if name.is_empty() {
            continue;
        }
        let lower = name.to_lowercase();
        if lower_seen.insert(lower.clone()) {
            entity_by_lower.push((lower, name.to_string()));
        }
    }
    for (scene_id, shots) in shots_by_scene {
        let mut scene_text_parts: Vec<String> = Vec::new();
        for shot in shots {
            for key in ["synthesis_prompt", "action_description"] {
                let txt = mstr(shot, key).trim().to_string();
                if !txt.is_empty() {
                    scene_text_parts.push(txt);
                }
            }
        }
        if let Some(narrator) = narrator_by_scene.get(scene_id) {
            if !narrator.trim().is_empty() {
                scene_text_parts.push(narrator.trim().to_string());
            }
        }
        let scene_text = scene_text_parts.join(" ").to_lowercase();
        let mut seen: HashSet<&String> = HashSet::new();
        for (lower, canonical) in &entity_by_lower {
            if scene_text.contains(lower.as_str()) && seen.insert(canonical) {
                scene_refs.entry(scene_id.clone()).or_default().push(canonical.clone());
            }
        }
    }
    scene_refs
}

fn sb_filter_entities_by_usage(
    entities: &[crate::script::ParsedCharacter],
    scene_refs: &HashMap<String, Vec<String>>,
) -> Vec<crate::script::ParsedCharacter> {
    let used: HashSet<String> = scene_refs
        .values()
        .flatten()
        .map(|n| n.trim().to_lowercase())
        .collect();
    entities
        .iter()
        .filter(|e| {
            let name = e.name.trim().to_lowercase();
            !name.is_empty() && !e.image_prompt.trim().is_empty() && used.contains(&name)
        })
        .cloned()
        .collect()
}

fn sb_group_shots_by_scene_id(w: &WorkingContext) -> HashMap<String, Vec<Map<String, Value>>> {
    let mut out: HashMap<String, Vec<Map<String, Value>>> = HashMap::new();
    let Some(raw) = resolve_by_field(w, "shot_list", "shots") else { return out };
    let Some(obj) = parse_upstream_json(&raw) else { return out };
    if let Some(shots) = obj.get("shots").and_then(|v| v.as_array()) {
        for s in shots {
            let Some(shot) = s.as_object() else { continue };
            let mut sid = mstr(shot, "scene_id");
            if sid.is_empty() {
                sid = sb_scene_id_from_shot_id(&mstr(shot, "shot_id"));
            }
            if sid.is_empty() {
                continue;
            }
            out.entry(sid).or_default().push(shot.clone());
        }
    }
    out
}

fn sb_group_env_by_scene_id(w: &WorkingContext) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(raw) = resolve_by_field(w, "environments", "scene_environments") else { return out };
    let Some(obj) = parse_upstream_json(&raw) else { return out };
    if let Some(envs) = obj.get("scene_environments").and_then(|v| v.as_array()) {
        for (i, e) in envs.iter().enumerate() {
            let Some(env) = e.as_object() else { continue };
            let mut sid = mstr(env, "scene_id");
            if sid.is_empty() {
                sid = (i + 1).to_string();
            }
            out.insert(sid, e.to_string());
        }
    }
    out
}

fn sb_group_dir_by_scene_id(w: &WorkingContext) -> HashMap<String, Map<String, Value>> {
    let mut out = HashMap::new();
    let Some(raw) = resolve_by_field(w, "director", "scene_blocks") else { return out };
    let Some(obj) = parse_upstream_json(&raw) else { return out };
    if let Some(blocks) = obj.get("scene_blocks").and_then(|v| v.as_array()) {
        for (i, b) in blocks.iter().enumerate() {
            let Some(block) = b.as_object() else { continue };
            let mut sid = mstr(block, "scene_id");
            if sid.is_empty() {
                sid = (i + 1).to_string();
            }
            out.insert(sid, block.clone());
        }
    }
    out
}

/// Dialogue lines per scene from raw screenwriter blocks — a plain scanner, no LLM.
fn sb_extract_narrator_by_scene(w: &WorkingContext) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (i, block) in resolve_scene_blocks(w).iter().enumerate() {
        let content = mstr(block, "content");
        if content.is_empty() {
            continue;
        }
        let mut scene_id = mstr(block, "scene_id");
        if scene_id.is_empty() {
            scene_id = (i + 1).to_string();
        }
        let narrator = sb_extract_dialogue(&content);
        if !narrator.is_empty() {
            out.insert(scene_id, narrator);
        }
    }
    out
}

/// Scan a screenplay block for dialogue: inline "NAME: text" and Fountain
/// (ALL-CAPS cue line followed by a dialogue line).
fn sb_extract_dialogue(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();

        if let Some(idx) = line.find(':') {
            if idx > 0 {
                let name = line[..idx].trim();
                let text = line[idx + 1..].trim();
                if is_speaker_name(name) && !text.is_empty() {
                    out.push(format!("{name}: {text}"));
                    i += 1;
                    continue;
                }
            }
        }

        if is_fountain_character_cue(line) && i + 1 < lines.len() {
            let next_line = lines[i + 1].trim();
            if !next_line.is_empty() && !is_scene_heading(next_line) {
                out.push(format!("{line}: {next_line}"));
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    out.join("\n")
}

fn is_letter_or_vietnamese(r: char) -> bool {
    r.is_ascii_alphabetic() || (r as u32) > 127
}

/// 1-4 title-case or all-caps words, letters/spaces/dots only.
fn is_speaker_name(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.len() > 40 {
        return false;
    }
    s.chars().all(|r| r == ' ' || r == '.' || is_letter_or_vietnamese(r))
}

fn is_fountain_character_cue(s: &str) -> bool {
    if s.is_empty() || s.len() > 40 {
        return false;
    }
    if s.to_uppercase() != s {
        return false;
    }
    if is_scene_heading(s) {
        return false;
    }
    s.chars().all(|r| r == ' ' || is_letter_or_vietnamese(r))
}

/// scene_ids sorted numerically when possible, else lexicographically.
fn sb_ordered_scene_ids(shots_by_scene: &HashMap<String, Vec<Map<String, Value>>>) -> Vec<String> {
    let mut ids: Vec<String> = shots_by_scene.keys().cloned().collect();
    ids.sort_by(|a, b| match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(na), Ok(nb)) => na.cmp(&nb),
        _ => a.cmp(b),
    });
    ids
}

/// "1_001" → "1".
fn sb_scene_id_from_shot_id(shot_id: &str) -> String {
    match shot_id.find('_') {
        Some(idx) if idx > 0 => shot_id[..idx].to_string(),
        _ => String::new(),
    }
}

/// Scene duration from shot count (each shot ≈ 4-6s).
fn sb_default_duration(num_shots: usize) -> f64 {
    if num_shots <= 1 {
        6.0
    } else {
        num_shots as f64 * 5.0
    }
}

fn sb_upsert_entity(db: &Db, project_id: &str, ch: &crate::script::ParsedCharacter) -> Result<String, String> {
    let rows = db
        .query(
            "SELECT c.* FROM character c JOIN project_character pc ON pc.character_id = c.id \
             WHERE pc.project_id = ?1 ORDER BY c.name",
            &[&project_id],
        )
        .map_err(|e| e.to_string())?;
    for r in &rows {
        if str_of(r, "name") == ch.name {
            return Ok(str_of(r, "id"));
        }
    }
    let mut row = Map::new();
    row.insert("name".into(), json!(ch.name));
    row.insert("entity_type".into(), json!(ch.entity_type));
    row.insert("description".into(), json!(ch.description));
    row.insert("image_prompt".into(), json!(ch.image_prompt));
    let cid = db.insert("character", &row).map_err(|e| e.to_string())?;
    link_character(db, project_id, &cid)?;
    Ok(cid)
}

// ---------------------------------------------------------------------------
// gen_ref
// ---------------------------------------------------------------------------

const DEFAULT_GEN_REF: &str = r#"You are GenRef, a scene reference alignment agent.
Goal: normalize scene entity references into Vietnamese names WITHOUT diacritics (khong dau), uppercase words, consistent across all scenes.

Rules:
- Use ONLY entity names provided in the entity catalog.
- Output character_names as Vietnamese/ASCII no-diacritic normalized names (e.g. "CỤ GIÀ" -> "CU GIA").
- Never invent unknown entities.
- Prefer consistency over recall: if uncertain, skip.
- Keep output JSON only.

Schema:
{
  "scene_refs": [
    {
      "scene_id": "<scene id>",
      "character_names": ["<NAME_KHONG_DAU>", "..."]
    }
  ]
}"#;

struct GenRefAgent;

#[async_trait::async_trait]
impl Agent for GenRefAgent {
    fn agent_type(&self) -> &str {
        "gen_ref"
    }
    fn description(&self) -> String {
        "Post-process persisted scenes and synchronize scene reference entities/locations for downstream consistency. INPUT: requires script_parser outputs ('video_id','scene_ids') (depends_on: script_parser task).".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_GEN_REF.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, _task: &Task) -> Result<TaskResult, String> {
        // Precondition is "scenes exist", not "script_parser handed me its
        // result in memory": this agent reads scenes from the DB anyway, and
        // under the workflow engine each step is its own process with no
        // in-memory upstream to inherit.
        let db = ctx.memory.db.clone();
        let video_id = resolve_video_id_or_first(&ctx.working, &db, &ctx.project_id);
        if video_id.is_empty() {
            return Err("gen_ref: no video_id found".to_string());
        }
        let scenes = list_scenes(&db, &video_id);
        if scenes.is_empty() {
            return Err(format!(
                "gen_ref: video {video_id} chưa có scene nào (script_parser phải chạy trước)"
            ));
        }
        let entities = ctx.memory.list_characters();
        if scenes.is_empty() || entities.is_empty() {
            let mut data = Map::new();
            data.insert("video_id".into(), json!(video_id));
            data.insert("updated_scenes".into(), json!([]));
            return Ok(TaskResult::new(data, "No scenes/entities to synchronize"));
        }

        // Entity name indexes.
        let mut entity_names: Vec<(String, String)> = Vec::new(); // (name, key)
        let mut entity_by_key: HashMap<String, String> = HashMap::new();
        let mut entity_no_accent_by_key: HashMap<String, String> = HashMap::new();
        for e in &entities {
            let name = str_of(e, "name");
            if name.is_empty() {
                continue;
            }
            let key = canonical_name_key(&name);
            if key.is_empty() {
                continue;
            }
            entity_names.push((name.clone(), key.clone()));
            entity_by_key.entry(key.clone()).or_insert_with(|| name.clone());
            entity_no_accent_by_key
                .entry(key)
                .or_insert_with(|| to_vietnamese_no_accent_name(&name));
        }

        // LLM-inferred refs (constrained to the catalog; failures non-fatal).
        let llm_by_scene_id = infer_scene_refs_with_llm(ctx, &scenes, &entities).await;

        let mut updated: Vec<String> = Vec::new();
        for sc in &scenes {
            let scene_id = str_of(sc, "id");
            if scene_id.is_empty() {
                continue;
            }
            let existing = parse_names(&str_of(sc, "character_names"));
            let scene_text = [
                str_of(sc, "prompt"),
                str_of(sc, "video_prompt"),
                str_of(sc, "action_sequence"),
                str_of(sc, "narrator_text"),
            ]
            .join(" ")
            .to_lowercase();
            // Word-boundary form (padded with spaces) for whole-word auto-include
            // matching, so a short name like "Na"/"Ao" doesn't match inside "nao".
            let scene_text_phrase = canonical_name_phrase(&scene_text);

            let mut merged: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for n in &existing {
                let key = canonical_name_key(n);
                if key.is_empty() {
                    continue;
                }
                if let Some(canonical) = entity_by_key.get(&key) {
                    let ck = canonical_name_key(canonical);
                    if seen.insert(ck) {
                        merged.push(to_vietnamese_no_accent_name(canonical));
                    }
                    continue;
                }
                if seen.insert(key) {
                    merged.push(n.trim().to_string());
                }
            }
            for (name, key) in &entity_names {
                if key.is_empty() {
                    continue;
                }
                // Whole-word match: the entity's name must appear as complete
                // token(s), not as a substring inside a longer word.
                let name_phrase = canonical_name_phrase(name);
                let needle = name_phrase.trim();
                if needle.is_empty() || !scene_text_phrase.contains(&format!(" {needle} ")) {
                    continue;
                }
                if seen.insert(key.clone()) {
                    merged.push(to_vietnamese_no_accent_name(name));
                }
            }
            // Dialogue cues → only accepted when they map to a catalog entity.
            let cue_source = format!("{}\n{}", str_of(sc, "prompt"), str_of(sc, "narrator_text"));
            for cue in infer_dialogue_cues(&cue_source) {
                let key = canonical_name_key(&cue);
                if key.is_empty() || seen.contains(&key) {
                    continue;
                }
                let Some(canonical) = entity_by_key.get(&key) else { continue };
                seen.insert(key);
                merged.push(to_vietnamese_no_accent_name(canonical));
            }
            // LLM refs, still catalog-constrained.
            if let Some(names) = llm_by_scene_id.get(&scene_id) {
                for n in names {
                    let key = canonical_name_key(n);
                    if key.is_empty() || seen.contains(&key) {
                        continue;
                    }
                    let Some(no_accent) = entity_no_accent_by_key.get(&key) else { continue };
                    seen.insert(key);
                    merged.push(no_accent.clone());
                }
            }

            if same_canonical_names(&existing, &merged) {
                continue;
            }
            let raw = serde_json::to_string(&merged).unwrap_or_else(|_| "[]".into());
            let mut patch = Map::new();
            patch.insert("character_names".into(), json!(raw));
            db.update("scene", &scene_id, &patch)
                .map_err(|e| format!("gen_ref update scene {scene_id}: {e}"))?;
            updated.push(scene_id);
        }

        let n = updated.len();
        let mut data = Map::new();
        data.insert("video_id".into(), json!(video_id));
        data.insert("updated_scenes".into(), json!(updated));
        Ok(TaskResult::new(data, format!("Synchronized references for {n} scene(s)")))
    }
}

async fn infer_scene_refs_with_llm(
    ctx: &AgentContext,
    scenes: &[Row],
    entities: &[Row],
) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    if scenes.is_empty() || entities.is_empty() {
        return out;
    }
    let mut sb = String::from("ENTITY_CATALOG:\n");
    for e in entities {
        let name = str_of(e, "name");
        if !name.is_empty() {
            sb.push_str(&format!("- {name}\n"));
        }
    }
    sb.push_str("\nSCENES:\n");
    for sc in scenes {
        let scene_id = str_of(sc, "id");
        if scene_id.is_empty() {
            continue;
        }
        let text = [
            str_of(sc, "prompt"),
            str_of(sc, "video_prompt"),
            str_of(sc, "action_sequence"),
            str_of(sc, "narrator_text"),
        ]
        .join("\n");
        sb.push_str(&format!("scene_id: {scene_id}\ntext:\n{}\n\n", text.trim()));
    }
    let sys = sysprompt(ctx, DEFAULT_GEN_REF);
    let Ok(obj) = complete_json(&sys, &sb, 4000).await else { return out };
    if let Some(arr) = obj.get("scene_refs").and_then(|v| v.as_array()) {
        for row in arr {
            let Some(m) = row.as_object() else { continue };
            let scene_id = mstr(m, "scene_id").trim().to_string();
            if scene_id.is_empty() {
                continue;
            }
            let names = string_slice(m.get("character_names"));
            if !names.is_empty() {
                out.insert(scene_id, names);
            }
        }
    }
    out
}

/// JSON array or comma-separated character_names column.
fn parse_names(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Vec::new();
    }
    if let Ok(out) = serde_json::from_str::<Vec<String>>(raw) {
        return out;
    }
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// ALL-CAPS dialogue cue lines (`^[A-ZÀ-Ỹ][A-ZÀ-Ỹ0-9 \-']{1,40}$`, minus
/// screenplay keywords) — implemented without a regex dependency.
fn infer_dialogue_cues(content: &str) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for line in content.lines() {
        let cand = line.trim();
        if !is_upper_dialogue_cue(cand) || is_ignored_cue(cand) {
            continue;
        }
        if seen.insert(cand.to_string()) {
            out.push(cand.to_string());
        }
    }
    out
}

fn is_upper_dialogue_cue(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false };
    if !(first.is_alphabetic() && first.is_uppercase()) {
        return false;
    }
    let rest: Vec<char> = chars.collect();
    if rest.is_empty() || rest.len() > 40 {
        return false;
    }
    rest.iter().all(|&r| {
        (r.is_alphabetic() && r.is_uppercase()) || r.is_ascii_digit() || r == ' ' || r == '-' || r == '\''
    })
}

fn same_canonical_names(a: &[String], b: &[String]) -> bool {
    let ka: Vec<String> = a.iter().map(|x| canonical_name_key(x)).filter(|k| !k.is_empty()).collect();
    let kb: Vec<String> = b.iter().map(|x| canonical_name_key(x)).filter(|k| !k.is_empty()).collect();
    ka == kb
}

// ---------------------------------------------------------------------------
// director_frame
// ---------------------------------------------------------------------------

const DIRECTOR_FRAME_BRIDGE_START: &str = "[DIRECTOR_FRAME_BRIDGE]";
const DIRECTOR_FRAME_BRIDGE_END: &str = "[/DIRECTOR_FRAME_BRIDGE]";

const DEFAULT_DIRECTOR_FRAME: &str = "You are a Continuity Supervisor bridging two adjacent video \
shots into a seamless visual relay (frame-anchoring: the last frame of Shot A becomes frame 0 of \
Shot B). Respect Shot A camera momentum. Output JSON only: {\"visual_anchor_directive\", \
\"motion_continuation_prompt\", \"negative_constraints\"}.";

struct DirectorFrameAgent;

#[async_trait::async_trait]
impl Agent for DirectorFrameAgent {
    fn agent_type(&self) -> &str {
        "director_frame"
    }
    fn description(&self) -> String {
        "Generate and apply frame-anchored continuity directives between adjacent scenes so the next scene inherits visual state/momentum. INPUT: requires persisted scenes/video_id (depends_on: scene_builder or script_parser). Must run BEFORE image/video generation.".into()
    }
    fn default_system(&self) -> String {
        DEFAULT_DIRECTOR_FRAME.into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let video_id = resolve_video_id_or_first(&ctx.working, &db, &ctx.project_id);
        if video_id.is_empty() {
            return Err("director_frame: no video_id found".to_string());
        }
        let scenes = list_scenes(&db, &video_id);
        if scenes.len() < 2 {
            let mut data = Map::new();
            data.insert("bridges".into(), json!([]));
            data.insert("video_id".into(), json!(video_id));
            return Ok(TaskResult::new(data, "No adjacent scenes to bridge"));
        }

        let sys = sysprompt(ctx, DEFAULT_DIRECTOR_FRAME);

        // Pair (i, i+1) only ever writes scene i+1, and every pair strips any
        // existing bridge before reading — so the pairs are independent and the
        // LLM calls run concurrently. The DB writes stay sequential below,
        // keeping bridge order deterministic.
        let pair_inputs: Vec<(usize, String, String, String)> = (0..scenes.len() - 1)
            .filter_map(|i| {
                let cur_prompt = strip_director_frame_bridge(&str_of(&scenes[i], "video_prompt"));
                let next_prompt = strip_director_frame_bridge(&str_of(&scenes[i + 1], "video_prompt"));
                if cur_prompt.is_empty() || next_prompt.is_empty() {
                    return None;
                }
                Some((i, cur_prompt, next_prompt, str_of(&scenes[i], "camera_movement")))
            })
            .collect();

        let mut answers: Vec<(usize, Map<String, Value>)> = {
            use futures_util::stream::{self, StreamExt};
            stream::iter(pair_inputs.into_iter().map(|(i, cur_p, next_p, cam)| {
                let sys = sys.clone();
                let extra = task.prompt.clone();
                async move {
                    let prompt = build_director_frame_prompt(&cur_p, &next_p, &cam, &extra);
                    complete_json(&sys, &prompt, 2000)
                        .await
                        .map(|r| (i, r))
                        .map_err(|e| format!("director_frame llm (cặp {}): {e}", i + 1))
                }
            }))
            .buffer_unordered(crate::config::llm_concurrency())
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, String>>()?
        };
        answers.sort_by_key(|(i, _)| *i);

        let mut bridges: Vec<Value> = Vec::new();
        for (i, result) in answers {
            let cur = &scenes[i];
            let next = &scenes[i + 1];
            let next_prompt = strip_director_frame_bridge(&str_of(next, "video_prompt"));
            let directive = mstr(&result, "visual_anchor_directive");
            let motion = mstr(&result, "motion_continuation_prompt");
            let negative = mstr(&result, "negative_constraints");
            if motion.is_empty() && directive.is_empty() {
                continue;
            }

            let mut next_video = next_prompt.trim().trim_end_matches('.').to_string();
            next_video.push_str(". ");
            next_video.push_str(&build_director_frame_bridge_block(&directive, &motion, &negative));
            let mut updates = Map::new();
            updates.insert("video_prompt".into(), json!(next_video));
            let next_image = strip_director_frame_bridge(&str_of(next, "image_prompt"));
            if !next_image.is_empty() && !directive.is_empty() {
                let img = format!(
                    "{}. {}",
                    next_image.trim().trim_end_matches('.'),
                    build_director_frame_bridge_block(&directive, "", "")
                );
                updates.insert("image_prompt".into(), json!(img));
            }
            db.update("scene", &str_of(next, "id"), &updates)
                .map_err(|e| format!("director_frame update scene: {e}"))?;
            bridges.push(json!({
                "from_scene_id": str_of(cur, "id"),
                "to_scene_id": str_of(next, "id"),
                "visual_anchor_directive": directive,
                "motion_continuation_prompt": motion,
                "negative_constraints": negative,
                "applied_to_next_video_prompt": true,
            }));
        }

        let n = bridges.len();
        let mut data = Map::new();
        data.insert("video_id".into(), json!(video_id));
        data.insert("bridges".into(), Value::Array(bridges));
        Ok(TaskResult::new(data, format!("Applied {n} scene-to-scene continuity bridges")))
    }
}

fn build_director_frame_prompt(cur: &str, next: &str, camera_move_a: &str, extra: &str) -> String {
    let mut b = String::new();
    b.push_str("Create a bridge from Shot A to Shot B.\n");
    b.push_str(&format!("camera_movement_a: {camera_move_a}\n\n"));
    b.push_str(&format!("shot_a_prompt:\n{cur}\n\nshot_b_target_prompt:\n{next}"));
    if !extra.trim().is_empty() {
        b.push_str(&format!("\n\nExtra continuity requirements:\n{}", extra.trim()));
    }
    b
}

fn build_director_frame_bridge_block(directive: &str, motion: &str, negative: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !directive.trim().is_empty() {
        parts.push(directive.trim().to_string());
    }
    if !motion.trim().is_empty() {
        parts.push(motion.trim().to_string());
    }
    if !negative.trim().is_empty() {
        parts.push(format!("Constraints: {}", negative.trim()));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("{DIRECTOR_FRAME_BRIDGE_START} {} {DIRECTOR_FRAME_BRIDGE_END}", parts.join(". "))
}

/// Remove any (possibly repeated / malformed) bridge blocks from a prompt.
fn strip_director_frame_bridge(prompt: &str) -> String {
    let mut p = prompt.trim().to_string();
    if p.is_empty() {
        return String::new();
    }
    loop {
        let Some(start) = p.find(DIRECTOR_FRAME_BRIDGE_START) else { break };
        match p[start..].find(DIRECTOR_FRAME_BRIDGE_END) {
            None => {
                // malformed old content: cut from marker to end
                p = p[..start].trim().to_string();
                break;
            }
            Some(rel_end) => {
                let end = start + rel_end + DIRECTOR_FRAME_BRIDGE_END.len();
                let left = p[..start].trim().to_string();
                let right = p[end..].trim().to_string();
                p = if left.is_empty() {
                    right
                } else if right.is_empty() {
                    left
                } else {
                    format!("{left}. {right}")
                };
            }
        }
    }
    p.trim().trim_matches(|c| c == '.' || c == ' ').to_string()
}

// ---------------------------------------------------------------------------
// character / image / video (process delegates)
// ---------------------------------------------------------------------------

#[derive(Default, serde::Deserialize)]
struct MediaParams {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    character_id: String,
    #[serde(default)]
    scene_id: String,
    #[serde(default)]
    video_id: String,
    /// The `request` row this task fulfils. The API marks it PROCESSING when it
    /// schedules the task; this agent is what closes it out.
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    orientation: String,
}

/// Close out the `request` row an image task was scheduled for, mirroring what
/// the worker does for video/upscale: COMPLETED with the produced media_id +
/// url, or FAILED with the error. Emits the same dashboard events so the UI
/// updates without a refresh.
fn finish_image_request(
    core: &Arc<Core>,
    request_id: &str,
    project_id: &str,
    params: &MediaParams,
    out: &Result<TaskResult, String>,
) {
    let db = &core.db;
    let mut up = Map::new();
    match out {
        Ok(_) => {
            up.insert("status".into(), json!("COMPLETED"));
            up.insert("error_message".into(), Value::Null);
            // Report the asset this request produced, read back from the row it
            // wrote (process.rs owns those columns).
            let (media_id, url) = if !params.character_id.is_empty() {
                db.get("character", &params.character_id)
                    .ok()
                    .flatten()
                    .map(|c| (str_of(&c, "media_id"), str_of(&c, "reference_image_url")))
                    .unwrap_or_default()
            } else if !params.scene_id.is_empty() {
                let cols = db::scene_cols(&params.orientation);
                db.get("scene", &params.scene_id)
                    .ok()
                    .flatten()
                    .map(|s| (str_of(&s, &cols.image_media_id), str_of(&s, &cols.image_url)))
                    .unwrap_or_default()
            } else {
                (String::new(), String::new())
            };
            if !media_id.is_empty() {
                up.insert("media_id".into(), json!(media_id));
            }
            if !url.is_empty() {
                up.insert("output_url".into(), json!(url));
            }
        }
        Err(e) => {
            up.insert("status".into(), json!("FAILED"));
            up.insert("error_message".into(), json!(e));
        }
    }
    if let Err(e) = db.update("request", request_id, &up) {
        eprintln!("[AgentImage] close request {request_id}: {e}");
        return;
    }
    match out {
        Ok(_) => core.dash.emit(
            "request_completed",
            json!({ "request_id": request_id, "project_id": project_id }),
        ),
        Err(e) => core.dash.emit(
            "request_failed",
            json!({ "request_id": request_id, "error_message": e }),
        ),
    }
}

fn parse_media_params(prompt: &str) -> MediaParams {
    if prompt.is_empty() {
        return MediaParams::default();
    }
    serde_json::from_str(prompt).unwrap_or_default()
}

struct CharacterAgent {
    core: Arc<Core>,
}

#[async_trait::async_trait]
impl Agent for CharacterAgent {
    fn agent_type(&self) -> &str {
        "character"
    }
    fn description(&self) -> String {
        "Generate reference images for all named characters/locations in the project. INPUT: reads from DB only; does not consume director_frame task output JSON. Pipeline waits for director_frame before character when that step exists (depends_on: script_parser task).".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, _task: &Task) -> Result<TaskResult, String> {
        let n = crate::process::process_all_entities(&self.core, &ctx.project_id)
            .await
            .map_err(|e| format!("character agent: {e}"))?;
        let mut data = Map::new();
        data.insert("processed_count".into(), json!(n));
        Ok(TaskResult::new(data, format!("Generated {n} entity images")))
    }
}

struct ImageAgent {
    core: Arc<Core>,
}

#[async_trait::async_trait]
impl Agent for ImageAgent {
    fn agent_type(&self) -> &str {
        "image"
    }
    fn description(&self) -> String {
        "Generate scene still images using character references. INPUT: requires 'video_id' from script_parser output (depends_on: script_parser + character tasks).".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let params = parse_media_params(&task.prompt);
        // The API sets the request row to PROCESSING when it schedules this
        // task and nothing else ever closes it — without this the UI shows
        // "PROCESSING" forever even though the images are already generated.
        let request_id = params.request_id.clone();
        let out = self.run(ctx, &params).await;
        if !request_id.is_empty() {
            finish_image_request(&self.core, &request_id, &ctx.project_id, &params, &out);
        }
        out
    }
}

impl ImageAgent {
    async fn run(&self, ctx: &mut AgentContext, params: &MediaParams) -> Result<TaskResult, String> {
        let orientation = if params.orientation.is_empty() {
            crate::config::default_orientation()
        } else {
            params.orientation.clone()
        };
        println!(
            "[AgentImage] Execute project={} mode={:?} char={} scene={} ext_connected={}",
            ctx.project_id, params.mode, params.character_id, params.scene_id,
            self.core.ext.is_connected()
        );

        match params.mode.as_str() {
            "single_entity" => {
                crate::process::entity_image(&self.core, &params.character_id, &ctx.project_id, false).await?;
                Ok(TaskResult::new(Map::new(), "Generated entity image"))
            }
            "all_entities" => {
                let count = crate::process::process_all_entities(&self.core, &ctx.project_id).await?;
                let mut data = Map::new();
                data.insert("processed_count".into(), json!(count));
                Ok(TaskResult::new(data, format!("Generated {count} entity images")))
            }
            "single_scene" => {
                crate::process::scene_image(
                    &self.core, &params.scene_id, &ctx.project_id, &orientation, false, None,
                )
                .await?;
                Ok(TaskResult::new(Map::new(), "Generated scene image"))
            }
            _ => {
                // "all_scenes" / pipeline default
                let db = ctx.memory.db.clone();
                let mut vid = params.video_id.clone();
                if vid.is_empty() {
                    vid = resolve_video_id_or_first(&ctx.working, &db, &ctx.project_id);
                }
                if vid.is_empty() {
                    return Err("image agent: no video found for project".to_string());
                }
                if !self.core.ext.is_connected() {
                    return Err("image agent: extension bridge not connected".to_string());
                }
                let status_col = db::scene_cols(&orientation).image_status;
                let mut count = 0usize;
                let mut attempts = 0usize;
                let mut last_err = String::new();
                for sc in list_scenes(&db, &vid) {
                    if str_of(&sc, &status_col) == "COMPLETED" {
                        continue;
                    }
                    attempts += 1;
                    let sid = str_of(&sc, "id");
                    match crate::process::scene_image(
                        &self.core, &sid, &ctx.project_id, &orientation, false, None,
                    )
                    .await
                    {
                        Ok(_) => count += 1,
                        Err(e) => {
                            eprintln!("[AgentImage] scene {sid}: {e}");
                            last_err = e;
                        }
                    }
                }
                if count == 0 && attempts > 0 {
                    return Err(format!("all {attempts} scene image generations failed: {last_err}"));
                }
                let mut data = Map::new();
                data.insert("processed_count".into(), json!(count));
                data.insert("video_id".into(), json!(vid));
                Ok(TaskResult::new(data, format!("Generated {count} scene images")))
            }
        }
    }
}

struct VideoAgent {
    core: Arc<Core>,
}

#[async_trait::async_trait]
impl Agent for VideoAgent {
    fn agent_type(&self) -> &str {
        "video"
    }
    fn description(&self) -> String {
        "Generate Veo3 video clips from scene images. INPUT: requires scene images to be completed — must run after image agent (depends_on: image task).".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let params = parse_media_params(&task.prompt);
        let orientation = if params.orientation.is_empty() {
            crate::config::default_orientation()
        } else {
            params.orientation.clone()
        };
        println!(
            "[VideoAgent] Execute project={} mode={:?} scene={} ext_connected={}",
            ctx.project_id, params.mode, params.scene_id,
            self.core.ext.is_connected()
        );

        if params.mode == "single_scene" {
            if params.scene_id.is_empty() {
                return Err("video agent: single_scene requires scene_id".to_string());
            }
            crate::process::scene_video(&self.core, &params.scene_id, &ctx.project_id, &orientation, false)
                .await?;
            return Ok(TaskResult::new(Map::new(), "Generated scene video"));
        }

        // "all_scenes" / pipeline default.
        let db = ctx.memory.db.clone();
        let mut vid = params.video_id.clone();
        if vid.is_empty() {
            vid = resolve_video_id_or_first(&ctx.working, &db, &ctx.project_id);
        }
        if vid.is_empty() {
            return Err("video agent: no video found for project".to_string());
        }
        if !self.core.ext.is_connected() {
            return Err("video agent: extension bridge not connected".to_string());
        }
        let cols = db::scene_cols(&orientation);
        let scenes = list_scenes(&db, &vid);

        // Pre-pass: anchor each scene's end frame to the NEXT scene's still
        // image (Veo3 StartAndEndImage) without overwriting user-set anchors.
        for i in 0..scenes.len().saturating_sub(1) {
            let cur = &scenes[i];
            let next = &scenes[i + 1];
            let next_media_id = str_of(next, &cols.image_media_id);
            let next_status = str_of(next, &cols.image_status);
            if next_media_id.is_empty() || next_status != "COMPLETED" {
                continue;
            }
            if !str_of(cur, &cols.end_scene_media_id).is_empty() {
                continue;
            }
            let mut patch = Map::new();
            patch.insert(cols.end_scene_media_id.clone(), json!(next_media_id));
            let _ = db.update("scene", &str_of(cur, "id"), &patch);
        }

        let mut count = 0usize;
        let mut attempts = 0usize;
        let mut last_err = String::new();
        for sc in &scenes {
            let image_status = str_of(sc, &cols.image_status);
            let video_status = str_of(sc, &cols.video_status);
            if image_status != "COMPLETED" || video_status == "COMPLETED" {
                continue;
            }
            attempts += 1;
            let sid = str_of(sc, "id");

            // Request record so the generation is visible in the UI.
            let mut req = Map::new();
            req.insert("type".into(), json!("GENERATE_VIDEO"));
            req.insert("status".into(), json!("PROCESSING"));
            req.insert("project_id".into(), json!(ctx.project_id));
            req.insert("video_id".into(), json!(vid));
            req.insert("scene_id".into(), json!(sid));
            req.insert("orientation".into(), json!(orientation));
            let _ = db.insert("request", &req);

            match crate::process::scene_video(&self.core, &sid, &ctx.project_id, &orientation, false).await {
                Ok(_) => count += 1,
                Err(e) => {
                    eprintln!("[VideoAgent] scene {sid}: {e}");
                    last_err = e;
                }
            }
        }
        if count == 0 && attempts > 0 {
            return Err(format!("all {attempts} scene video generations failed: {last_err}"));
        }
        let mut data = Map::new();
        data.insert("processed_count".into(), json!(count));
        data.insert("video_id".into(), json!(vid));
        Ok(TaskResult::new(data, format!("Generated {count} scene videos")))
    }
}

// ---------------------------------------------------------------------------
// audio
// ---------------------------------------------------------------------------

struct AudioAgent {
    core: Arc<Core>,
}

/// Optional per-run overrides, same JSON-in-prompt convention the image/video
/// agents use: `{"video_id":"…","voice":"…","language":"vi","speed":1.0,
/// "model_id":"…","regenerate":true}`.
#[derive(serde::Deserialize, Default)]
struct AudioParams {
    #[serde(default)]
    video_id: String,
    #[serde(default)]
    voice: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    speed: Option<f32>,
    #[serde(default)]
    model_id: String,
    #[serde(default)]
    regenerate: bool,
}

#[async_trait::async_trait]
impl Agent for AudioAgent {
    fn agent_type(&self) -> &str {
        "audio"
    }
    fn description(&self) -> String {
        "Synthesize TTS narration audio for every scene that has narrator_text, using the SenClaw daemon's TTS subsystem (voice/model from SenClaw Settings, overridable per project). Saves a WAV per scene into local media and records it on the scene. INPUT: reads scenes from DB (depends_on: video task).".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let params: AudioParams = if task.prompt.trim().is_empty() {
            AudioParams::default()
        } else {
            serde_json::from_str(&task.prompt).unwrap_or_default()
        };

        let video_id = if !params.video_id.is_empty() {
            params.video_id.clone()
        } else {
            match list_videos(&db, &ctx.project_id).first() {
                Some(v) => str_of(v, "id"),
                None => {
                    let mut data = Map::new();
                    data.insert("narrator_count".into(), json!(0));
                    return Ok(TaskResult::new(data, "No video found for audio generation"));
                }
            }
        };

        // Project-level narration settings; each falls back to the daemon's own
        // TTS settings when unset (tts::synthesize omits empty fields).
        let project = db.get("project", &ctx.project_id).ok().flatten().unwrap_or_default();
        let voice = if params.voice.is_empty() { str_of(&project, "narrator_voice") } else { params.voice.clone() };
        let language = if params.language.is_empty() { str_of(&project, "language") } else { params.language.clone() };

        let scenes = list_scenes(&db, &video_id);
        let mut narrations: Vec<Value> = Vec::new();
        let (mut generated, mut skipped, mut failed) = (0usize, 0usize, 0usize);
        let mut first_error = String::new();

        for sc in &scenes {
            let text = str_of(sc, "narrator_text");
            if text.is_empty() {
                continue;
            }
            let scene_id = str_of(sc, "id");
            let existing = str_of(sc, "narrator_audio_url");
            let done = str_of(sc, "narrator_audio_status") == "COMPLETED" && !existing.is_empty();
            if done && !params.regenerate {
                skipped += 1;
                narrations.push(json!({
                    "scene_id": scene_id,
                    "display_order": db::i64_of(sc, "display_order"),
                    "narrator_text": text,
                    "audio_url": existing,
                    "status": "COMPLETED",
                }));
                continue;
            }

            let mut mark = Map::new();
            mark.insert("narrator_audio_status".into(), json!("PROCESSING"));
            let _ = db.update("scene", &scene_id, &mark);

            match crate::tts::synthesize(&text, &language, &voice, params.speed, &params.model_id).await
            {
                Ok(wav) => {
                    match self.store_wav(&db, &scene_id, wav) {
                        Ok(url) => {
                            generated += 1;
                            narrations.push(json!({
                                "scene_id": scene_id,
                                "display_order": db::i64_of(sc, "display_order"),
                                "narrator_text": text,
                                "audio_url": url,
                                "status": "COMPLETED",
                            }));
                            self.core.dash.emit(
                                "scene_updated",
                                json!({ "project_id": ctx.project_id, "scene_id": scene_id }),
                            );
                        }
                        Err(e) => {
                            failed += 1;
                            if first_error.is_empty() {
                                first_error = e.clone();
                            }
                            let mut m = Map::new();
                            m.insert("narrator_audio_status".into(), json!("FAILED"));
                            let _ = db.update("scene", &scene_id, &m);
                            narrations.push(json!({
                                "scene_id": scene_id, "narrator_text": text,
                                "status": "FAILED", "error": e,
                            }));
                        }
                    }
                }
                Err(e) => {
                    failed += 1;
                    if first_error.is_empty() {
                        first_error = e.clone();
                    }
                    let mut m = Map::new();
                    m.insert("narrator_audio_status".into(), json!("FAILED"));
                    let _ = db.update("scene", &scene_id, &m);
                    narrations.push(json!({
                        "scene_id": scene_id, "narrator_text": text,
                        "status": "FAILED", "error": e,
                    }));
                }
            }
        }

        let n = narrations.len();
        let mut data = Map::new();
        data.insert("video_id".into(), json!(video_id));
        data.insert("narrator_count".into(), json!(n));
        data.insert("generated".into(), json!(generated));
        data.insert("skipped".into(), json!(skipped));
        data.insert("failed".into(), json!(failed));
        data.insert("voice".into(), json!(voice));
        data.insert("narrations".into(), Value::Array(narrations));

        // No narrator text at all is a legitimate no-op, not a failure.
        if n == 0 {
            return Ok(TaskResult::new(data, "No narrator_text on any scene — nothing to narrate"));
        }
        // Every synthesis failing means TTS is unusable (no model installed,
        // daemon down). Surface it as a task error so the pipeline shows why,
        // rather than reporting success with zero audio.
        if generated == 0 && skipped == 0 && failed > 0 {
            data.insert("error".into(), json!(first_error.clone()));
            return Err(format!("audio agent: TTS failed for all {failed} narration(s): {first_error}"));
        }
        let summary = if failed > 0 {
            format!("Narrated {generated} scene(s), {skipped} already done, {failed} failed ({first_error})")
        } else {
            format!("Narrated {generated} scene(s) with SenClaw TTS, {skipped} already done")
        };
        Ok(TaskResult::new(data, summary))
    }
}

impl AudioAgent {
    /// Persist WAV bytes as a media row and attach it to the scene.
    fn store_wav(&self, db: &Db, scene_id: &str, wav: Vec<u8>) -> Result<String, String> {
        let id = db::new_id();
        std::fs::create_dir_all(&self.core.media_dir).map_err(|e| format!("mkdir: {e}"))?;
        let file_name = format!("{id}.wav");
        let dest_path = self.core.media_dir.join(&file_name);
        std::fs::write(&dest_path, &wav).map_err(|e| format!("write: {e}"))?;

        let mut cm = Map::new();
        cm.insert("id".into(), json!(id));
        cm.insert("file_name".into(), json!(file_name));
        cm.insert("file_path".into(), json!(dest_path.to_string_lossy()));
        cm.insert("mime_type".into(), json!("audio/wav"));
        cm.insert("size_bytes".into(), json!(wav.len()));
        cm.insert("media_type".into(), json!("audio"));
        db.insert("media", &cm).map_err(|e| {
            let _ = std::fs::remove_file(&dest_path);
            format!("create media record: {e}")
        })?;

        let url = format!("/api/media/{id}/file");
        let mut sm = Map::new();
        sm.insert("narrator_audio_url".into(), json!(url));
        sm.insert("narrator_audio_media_id".into(), json!(id));
        sm.insert("narrator_audio_status".into(), json!("COMPLETED"));
        db.update("scene", scene_id, &sm).map_err(|e| format!("update scene: {e}"))?;
        Ok(url)
    }
}

// ---------------------------------------------------------------------------
// media_download
// ---------------------------------------------------------------------------

struct MediaDownloadAgent {
    core: Arc<Core>,
}

const SCENE_URL_FIELDS: &[(&str, &str)] = &[
    ("vertical_image_url", "image"),
    ("horizontal_image_url", "image"),
    ("vertical_video_url", "video"),
    ("horizontal_video_url", "video"),
    ("vertical_upscale_url", "video"),
    ("horizontal_upscale_url", "video"),
];

#[async_trait::async_trait]
impl Agent for MediaDownloadAgent {
    fn agent_type(&self) -> &str {
        "media_download"
    }
    fn description(&self) -> String {
        "Download all remote image/video URLs (scenes and characters) to local media storage and update DB with local paths. INPUT: requires 'video_id' from script_parser; must run after video generation (depends_on: video task). Run BEFORE concat.".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let params = parse_media_params(&task.prompt);
        let mut vid = params.video_id.clone();
        if vid.is_empty() {
            vid = resolve_video_id_or_first(&ctx.working, &db, &ctx.project_id);
        }
        if vid.is_empty() {
            return Err("media_download: no video found for project".to_string());
        }

        let (mut downloaded, mut skipped, mut failed) = (0usize, 0usize, 0usize);

        // Scenes — pure I/O over independent URLs, so fetch many at once
        // instead of one 6-column × N-scene chain. DB writes are grouped per
        // scene after its own downloads land.
        let scene_rows = list_scenes(&db, &vid);
        let mut jobs: Vec<(String, String, String, String)> = Vec::new();
        for sc in &scene_rows {
            let sid = str_of(sc, "id");
            for (col, media_type) in SCENE_URL_FIELDS {
                let raw = str_of(sc, col);
                if !is_remote_url(&raw) {
                    skipped += 1;
                    continue;
                }
                jobs.push((sid.clone(), (*col).to_string(), (*media_type).to_string(), raw));
            }
        }

        let outcomes: Vec<(String, String, Result<String, String>)> = {
            use futures_util::stream::{self, StreamExt};
            stream::iter(jobs.into_iter().map(|(sid, col, media_type, raw)| {
                let db = db.clone();
                async move {
                    let r = self.download_url(&db, &raw, &media_type).await;
                    (sid, col, r)
                }
            }))
            .buffer_unordered(crate::config::io_concurrency())
            .collect()
            .await
        };

        let mut per_scene: HashMap<String, Map<String, Value>> = HashMap::new();
        for (sid, col, res) in outcomes {
            match res {
                Ok(local) => {
                    per_scene.entry(sid).or_default().insert(col, json!(local));
                    downloaded += 1;
                }
                Err(e) => {
                    eprintln!("[MediaDownloadAgent] scene {sid} {col}: {e}");
                    failed += 1;
                }
            }
        }
        for (sid, updates) in per_scene {
            if !updates.is_empty() {
                let _ = db.update("scene", &sid, &updates);
            }
        }

        // Characters.
        for ch in ctx.memory.list_characters() {
            let cid = str_of(&ch, "id");
            let raw = str_of(&ch, "reference_image_url");
            if !is_remote_url(&raw) {
                skipped += 1;
                continue;
            }
            match self.download_url(&db, &raw, "image").await {
                Ok(local) => {
                    let mut patch = Map::new();
                    patch.insert("reference_image_url".into(), json!(local));
                    let _ = db.update("character", &cid, &patch);
                    downloaded += 1;
                }
                Err(e) => {
                    eprintln!("[MediaDownloadAgent] character {cid}: {e}");
                    failed += 1;
                }
            }
        }

        println!(
            "[MediaDownloadAgent] project={} video={vid} downloaded={downloaded} skipped={skipped} failed={failed}",
            ctx.project_id
        );
        let mut data = Map::new();
        data.insert("downloaded".into(), json!(downloaded));
        data.insert("skipped".into(), json!(skipped));
        data.insert("failed".into(), json!(failed));
        data.insert("video_id".into(), json!(vid));
        Ok(TaskResult::new(
            data,
            format!("Downloaded {downloaded} media files (skipped {skipped} already local, {failed} failed)"),
        ))
    }
}

impl MediaDownloadAgent {
    /// Download a remote URL into the media dir, create a media row (with
    /// original_url + probed dims) and return `/api/media/{id}/file`. An
    /// already-downloaded URL reuses the existing record.
    async fn download_url(&self, db: &Db, raw_url: &str, media_type: &str) -> Result<String, String> {
        if let Ok(Some(existing)) =
            db.query_one("SELECT id FROM media WHERE original_url = ?1", &[&raw_url])
        {
            return Ok(format!("/api/media/{}/file", str_of(&existing, "id")));
        }

        let resp = crate::llm::http()
            .get(raw_url)
            .send()
            .await
            .map_err(|e| format!("download: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.bytes().await.map_err(|e| format!("download body: {e}"))?;

        let mut ext = ext_from_content_type(&content_type).to_string();
        if ext.is_empty() {
            let path_part = raw_url.splitn(2, '?').next().unwrap_or("");
            if let Some(idx) = path_part.rfind('.') {
                let cand = &path_part[idx..];
                if cand.len() <= 5 && !cand.contains('/') {
                    ext = cand.to_string();
                }
            }
        }
        if ext.is_empty() {
            ext = default_media_ext(media_type).to_string();
        }

        let id = db::new_id();
        std::fs::create_dir_all(&self.core.media_dir).map_err(|e| format!("mkdir: {e}"))?;
        let file_name = format!("{id}{ext}");
        let dest_path = self.core.media_dir.join(&file_name);
        std::fs::write(&dest_path, &bytes).map_err(|e| format!("write: {e}"))?;

        let mime_type = content_type.split(';').next().unwrap_or("").trim().to_string();
        let (w_px, h_px) = probe_dimensions(media_type, &dest_path, &bytes);

        let mut cm = Map::new();
        cm.insert("id".into(), json!(id));
        cm.insert("file_name".into(), json!(file_name));
        cm.insert("file_path".into(), json!(dest_path.to_string_lossy()));
        cm.insert("mime_type".into(), json!(mime_type));
        cm.insert("size_bytes".into(), json!(bytes.len()));
        cm.insert("media_type".into(), json!(media_type));
        cm.insert("original_url".into(), json!(raw_url));
        if w_px > 0 && h_px > 0 {
            cm.insert("width_px".into(), json!(w_px));
            cm.insert("height_px".into(), json!(h_px));
        }
        db.insert("media", &cm).map_err(|e| {
            let _ = std::fs::remove_file(&dest_path);
            format!("create media record: {e}")
        })?;
        Ok(format!("/api/media/{id}/file"))
    }
}

fn is_remote_url(u: &str) -> bool {
    u.starts_with("http://") || u.starts_with("https://")
}

fn ext_from_content_type(ct: &str) -> &'static str {
    match ct.split(';').next().unwrap_or("").trim() {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        "video/quicktime" => ".mov",
        _ => "",
    }
}

fn default_media_ext(media_type: &str) -> &'static str {
    if media_type == "video" {
        ".mp4"
    } else {
        ".jpg"
    }
}

/// Best-effort dimension probe: byte sniff for images, ffprobe for video.
fn probe_dimensions(media_type: &str, path: &std::path::Path, bytes: &[u8]) -> (i64, i64) {
    if media_type == "image" {
        if let Some(dims) = sniff_image_dimensions(bytes) {
            return dims;
        }
    }
    // ffprobe fallback (works for video and any image format it knows).
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height",
            "-of", "csv=s=x:p=0",
        ])
        .arg(path)
        .output();
    if let Ok(out) = out {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut it = s.trim().split('x');
        if let (Some(w), Some(h)) = (it.next(), it.next()) {
            if let (Ok(w), Ok(h)) = (w.trim().parse::<i64>(), h.trim().parse::<i64>()) {
                return (w, h);
            }
        }
    }
    (0, 0)
}

fn sniff_image_dimensions(b: &[u8]) -> Option<(i64, i64)> {
    // PNG: IHDR width/height big-endian at offsets 16/20.
    if b.len() > 24 && b.starts_with(&[0x89, b'P', b'N', b'G']) {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]) as i64;
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]) as i64;
        return Some((w, h));
    }
    // GIF: little-endian u16 at 6/8.
    if b.len() > 10 && (b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        let w = u16::from_le_bytes([b[6], b[7]]) as i64;
        let h = u16::from_le_bytes([b[8], b[9]]) as i64;
        return Some((w, h));
    }
    // JPEG: scan for an SOF marker.
    if b.len() > 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2usize;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as i64;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as i64;
                return Some((w, h));
            }
            if i + 3 >= b.len() {
                break;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if len < 2 {
                break;
            }
            i += 2 + len;
        }
    }
    // WebP VP8X: 24-bit little-endian minus-one dims at offset 24.
    if b.len() > 30 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" && &b[12..16] == b"VP8X" {
        let w = 1 + (b[24] as i64 | (b[25] as i64) << 8 | (b[26] as i64) << 16);
        let h = 1 + (b[27] as i64 | (b[28] as i64) << 8 | (b[29] as i64) << 16);
        return Some((w, h));
    }
    None
}

// ---------------------------------------------------------------------------
// concat
// ---------------------------------------------------------------------------

struct ConcatAgent {
    core: Arc<Core>,
}

#[async_trait::async_trait]
impl Agent for ConcatAgent {
    fn agent_type(&self) -> &str {
        "concat"
    }
    fn description(&self) -> String {
        "Concatenate all local video clips into final output MP4 using ffmpeg. INPUT: requires local video files — must run AFTER media_download has converted remote URLs to local paths (depends_on: media_download task).".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, _task: &Task) -> Result<TaskResult, String> {
        let db = ctx.memory.db.clone();
        let videos = list_videos(&db, &ctx.project_id);
        let Some(video_row) = videos.first() else {
            return Err("concat agent: no video found".to_string());
        };
        let video_id = str_of(video_row, "id");

        let orientation = crate::config::default_orientation();
        let cols = db::scene_cols(&orientation);
        let video_urls: Vec<String> = list_scenes(&db, &video_id)
            .iter()
            .map(|sc| str_of(sc, &cols.video_url))
            .filter(|u| !u.is_empty())
            .collect();

        if video_urls.is_empty() {
            let mut data = Map::new();
            data.insert("status".into(), json!("no_videos_ready"));
            return Ok(TaskResult::new(data, "No completed video clips to concat yet"));
        }

        // Graceful when ffmpeg is absent.
        if !ffmpeg_available() {
            let mut data = Map::new();
            data.insert("status".into(), json!("ffmpeg_unavailable"));
            data.insert("video_urls".into(), json!(video_urls));
            data.insert("count".into(), json!(video_urls.len()));
            return Ok(TaskResult::new(
                data,
                format!("ffmpeg not found — {} clips ready for manual concat", video_urls.len()),
            ));
        }

        // Local-path resolution: /api/media/{id}/file → media.file_path;
        // remote http(s) URLs stay as-is (whitelisted in the ffmpeg args).
        let inputs: Vec<String> = video_urls.iter().map(|u| resolve_media_input(&db, u)).collect();

        let out_dir = crate::config::data_dir().join("output");
        std::fs::create_dir_all(&out_dir).map_err(|e| format!("concat mkdir: {e}"))?;
        let out_path = out_dir.join(format!("{}_final.mp4", ctx.project_id));

        let list_content = inputs
            .iter()
            .map(|u| format!("file '{}'", u.replace('\'', r"'\''")))
            .collect::<Vec<_>>()
            .join("\n");
        let list_path = std::env::temp_dir().join(format!("vf-concat-{}.txt", db::new_id()));
        std::fs::write(&list_path, &list_content).map_err(|e| format!("concat list: {e}"))?;

        let out_path_s = out_path.to_string_lossy().to_string();
        let list_path_c = list_path.clone();
        let run = tokio::task::spawn_blocking(move || {
            let out = std::process::Command::new("ffmpeg")
                .args(["-y", "-f", "concat", "-safe", "0"])
                .args(["-protocol_whitelist", "file,http,https,tcp,tls"])
                .arg("-i")
                .arg(&list_path_c)
                .args(["-c", "copy"])
                .arg(&out_path_s)
                .output();
            match out {
                Ok(o) if o.status.success() => Ok(()),
                Ok(o) => Err(format!(
                    "exit {}\n{}",
                    o.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&o.stderr)
                )),
                Err(e) => Err(e.to_string()),
            }
        })
        .await
        .map_err(|e| format!("concat join: {e}"))?;
        let _ = std::fs::remove_file(&list_path);
        run.map_err(|e| format!("concat ffmpeg: {e}"))?;

        // Update the video row with the final path.
        let url_col = if db::ori_prefix(&orientation) == "horizontal" {
            "horizontal_url"
        } else {
            "vertical_url"
        };
        let mut patch = Map::new();
        patch.insert(url_col.into(), json!(out_path.to_string_lossy()));
        patch.insert("status".into(), json!("COMPLETED"));
        let _ = db.update("video", &video_id, &patch);

        let mut data = Map::new();
        data.insert("output_path".into(), json!(out_path.to_string_lossy()));
        data.insert("clip_count".into(), json!(inputs.len()));
        data.insert("video_id".into(), json!(video_id));
        Ok(TaskResult::new(
            data,
            format!("Concatenated {} clips → {}", inputs.len(), out_path.to_string_lossy()),
        ))
    }
}

fn ffmpeg_available() -> bool {
    match std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(_) => true,
        Err(e) => e.kind() != std::io::ErrorKind::NotFound,
    }
}

/// `/api/media/{id}/file` → the media row's file_path when known; anything else
/// (remote URL or plain local path) passes through unchanged.
fn resolve_media_input(db: &Db, url: &str) -> String {
    if let Some(rest) = url.strip_prefix("/api/media/") {
        if let Some(id) = rest.strip_suffix("/file") {
            if let Ok(Some(m)) = db.get("media", id) {
                let p = str_of(&m, "file_path");
                if !p.is_empty() {
                    return p;
                }
            }
        }
    }
    url.to_string()
}

// ---------------------------------------------------------------------------
// critic
// ---------------------------------------------------------------------------

const DEFAULT_CRITIC: &str = "You are a GenAI Video Evaluator with negative bias. Audit the clip \
across 4 axes: object permanence, physics & geometry, temporal consistency, script faithfulness. \
Output JSON only: {\"status\": \"PASS\"} or {\"status\": \"FAIL\", \"error_timestamp\", \
\"error_category\", \"error_description\", \"recalculated_correction_prompt\"}.";

struct CriticAgent;

#[async_trait::async_trait]
impl Agent for CriticAgent {
    fn agent_type(&self) -> &str {
        "critic"
    }
    fn description(&self) -> String {
        "Pre-flight check before rendering: verifies every scene has an image prompt, timed video prompt, resolved reference entities with reference images, and continuity bridges. Blocks the render stages on errors so a broken setup fails in seconds instead of after N clips. INPUT: reads scenes + entities from the DB.".into()
    }

    async fn execute(&self, ctx: &mut AgentContext, _task: &Task) -> Result<TaskResult, String> {
        // This used to ask a TEXT model to audit a video it was never given —
        // hunting "melting faces" in a `{"video_id":…}` string — and nothing
        // read the verdict. Rendering is the expensive step, so the useful job
        // here is checking, deterministically and for free, that the inputs to
        // that step are sound BEFORE the clips are paid for.
        let db = ctx.memory.db.clone();
        let video_id = resolve_video_id_or_first(&ctx.working, &db, &ctx.project_id);
        if video_id.is_empty() {
            return Err("critic: no video found for project".to_string());
        }
        let scenes = list_scenes(&db, &video_id);
        if scenes.is_empty() {
            return Err("critic: video chưa có scene nào".to_string());
        }

        // Entities that actually have a usable reference image.
        let entities = ctx.memory.list_characters();
        let mut ref_ready: HashSet<String> = HashSet::new();
        let mut known: HashSet<String> = HashSet::new();
        for e in &entities {
            let key = canonical_name_key(&str_of(e, "name"));
            if key.is_empty() {
                continue;
            }
            known.insert(key.clone());
            if !str_of(e, "media_id").is_empty() || !str_of(e, "reference_image_url").is_empty() {
                ref_ready.insert(key);
            }
        }

        let mut errors: Vec<Value> = Vec::new();
        let mut warnings: Vec<Value> = Vec::new();
        let mut issue = |list: &mut Vec<Value>, order: i64, code: &str, detail: String| {
            list.push(json!({ "scene": order, "code": code, "detail": detail }));
        };

        for (i, sc) in scenes.iter().enumerate() {
            let order = db::i64_of(sc, "display_order");
            let image_prompt = {
                let p = str_of(sc, "image_prompt");
                if p.is_empty() { str_of(sc, "prompt") } else { p }
            };
            if image_prompt.trim().is_empty() {
                issue(&mut errors, order, "no_image_prompt",
                      "không có image_prompt — ảnh khung hình sẽ vô nghĩa".into());
            }

            let video_prompt = str_of(sc, "video_prompt");
            if video_prompt.trim().is_empty() {
                issue(&mut errors, order, "no_video_prompt",
                      "không có video_prompt — clip sẽ thiếu chỉ dẫn máy quay".into());
            } else if !has_subclip_timing(&video_prompt) {
                issue(&mut warnings, order, "no_timing",
                      "video_prompt thiếu mốc thời gian kiểu \"0-3s: …\"".into());
            }

            let names = parse_names(&str_of(sc, "character_names"));
            if names.is_empty() {
                issue(&mut warnings, order, "no_entities",
                      "không tham chiếu entity nào — nhân vật dễ lệch giữa các cảnh".into());
            }
            for n in &names {
                let key = canonical_name_key(n);
                if key.is_empty() {
                    continue;
                }
                if !known.contains(&key) {
                    issue(&mut warnings, order, "unknown_entity",
                          format!("`{n}` không có trong danh sách entity"));
                } else if !ref_ready.contains(&key) {
                    issue(&mut errors, order, "entity_without_reference",
                          format!("`{n}` chưa có ảnh tham chiếu — sinh ảnh ref trước"));
                }
            }

            // Continuity: every scene after the first should carry a bridge.
            if i > 0 && !video_prompt.contains(DIRECTOR_FRAME_BRIDGE_START) {
                issue(&mut warnings, order, "no_continuity_bridge",
                      "chưa có cầu nối liên tục từ cảnh trước".into());
            }
        }

        let mut data = Map::new();
        data.insert("video_id".into(), json!(video_id));
        data.insert("scene_count".into(), json!(scenes.len()));
        data.insert("errors".into(), Value::Array(errors.clone()));
        data.insert("warnings".into(), Value::Array(warnings.clone()));
        data.insert("status".into(), json!(if errors.is_empty() { "PASS" } else { "FAIL" }));

        if !errors.is_empty() {
            // Fail the node: the render stages depend on it, so nothing is
            // spent on clips that were going to come out wrong.
            let detail = errors
                .iter()
                .take(5)
                .map(|e| {
                    format!(
                        "cảnh {}: {}",
                        e.get("scene").and_then(|v| v.as_i64()).unwrap_or(0),
                        e.get("detail").and_then(|v| v.as_str()).unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(format!(
                "kiểm tra trước khi render: {} lỗi — {detail}",
                errors.len()
            ));
        }

        let summary = if warnings.is_empty() {
            format!("Kiểm tra trước render: {} cảnh, không có vấn đề", scenes.len())
        } else {
            format!(
                "Kiểm tra trước render: {} cảnh, {} cảnh báo (vẫn render được)",
                scenes.len(),
                warnings.len()
            )
        };
        Ok(TaskResult::new(data, summary))
    }
}

/// Does a video prompt carry sub-clip timing like `0-3s:` / `3-5s:`?
fn has_subclip_timing(prompt: &str) -> bool {
    let b = prompt.as_bytes();
    for i in 0..b.len() {
        if b[i] != b's' {
            continue;
        }
        // Walk back over digits and an optional `-`digits range.
        let mut j = i;
        let mut digits = 0;
        while j > 0 && (b[j - 1].is_ascii_digit() || b[j - 1] == b'-' || b[j - 1] == b'.') {
            if b[j - 1].is_ascii_digit() {
                digits += 1;
            }
            j -= 1;
        }
        if digits >= 1 && i + 1 < b.len() && (b[i + 1] == b':' || b[i + 1] == b' ') {
            // Require a range (`0-3s`) rather than a bare duration.
            if prompt[j..i].contains('-') {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diacritic_folding_canonical_key() {
        assert_eq!(canonical_name_key("CỤ GIÀ"), "cugia");
        assert_eq!(canonical_name_key("Đường Phố"), "duongpho");
        assert_eq!(canonical_name_key("  Trương Ba  "), "truongba");
        assert_eq!(canonical_name_key("NAM-2"), "nam2");
        assert_eq!(canonical_name_key(""), "");
        assert_eq!(canonical_name_key("!!!"), "");
        // Same entity, different diacritics/case → same key.
        assert_eq!(canonical_name_key("cụ già"), canonical_name_key("CU GIA"));
    }

    #[test]
    fn diacritic_folding_no_accent_display_name() {
        assert_eq!(to_vietnamese_no_accent_name("Cụ Già"), "CU GIA");
        assert_eq!(to_vietnamese_no_accent_name("Đường Phố"), "DUONG PHO");
        assert_eq!(to_vietnamese_no_accent_name("ông Tư - xe ôm"), "ONG TU XE OM");
        assert_eq!(to_vietnamese_no_accent_name("  "), "");
        assert_eq!(to_vietnamese_no_accent_name("NAM"), "NAM");
    }

    #[test]
    fn scene_heading_detection() {
        assert!(is_scene_heading("INT. COFFEE SHOP - DAY"));
        assert!(is_scene_heading("ext. street - night"));
        assert!(is_scene_heading("PHÂN CẢNH 1"));
        assert!(is_scene_heading("Cảnh 2: bờ sông"));
        assert!(!is_scene_heading("NAM"));
        assert!(!is_scene_heading("He walks into the room."));
    }

    #[test]
    fn split_blocks_and_ids() {
        let sp = "INT. HOUSE - DAY\nNAM sits.\n\nEXT. STREET - NIGHT\nNAM runs.";
        let blocks = split_screenplay_into_blocks(sp);
        assert_eq!(blocks.len(), 2);
        assert_eq!(mstr(&blocks[0], "scene_id"), "1");
        assert_eq!(mstr(&blocks[1], "heading"), "EXT. STREET - NIGHT");
        assert!(mstr(&blocks[0], "content").contains("NAM sits."));
    }

    #[test]
    fn narrator_dialogue_formatting() {
        let n = "NAM: Chào bác\nBÀ TƯ: Chào cháu\nplain narration line";
        assert_eq!(
            format_narrator_dialogue(n),
            "NAM speaks: \"Chào bác\"; then BÀ TƯ speaks: \"Chào cháu\"; then plain narration line"
        );
    }

    #[test]
    fn chain_types() {
        assert_eq!(chain_type_for_index(0), "ROOT");
        assert_eq!(chain_type_for_index(1), "CONTINUATION");
        assert_eq!(chain_type_for_index(5), "CONTINUATION");
    }

    #[test]
    fn dialogue_cue_matching() {
        assert!(is_upper_dialogue_cue("NAM"));
        assert!(is_upper_dialogue_cue("BÀ TƯ"));
        assert!(is_upper_dialogue_cue("CỤ GIÀ-2"));
        assert!(!is_upper_dialogue_cue("Nam"));
        assert!(!is_upper_dialogue_cue("N"));
        assert!(!is_upper_dialogue_cue("NAM walks away"));
        assert!(is_ignored_cue("NARRATOR"));
        assert!(is_ignored_cue("FADE OUT"));
    }

    #[test]
    fn screenwriter_fallback_salvage() {
        let broken = r#"{"screenplay": "INT. HOUSE - DAY\nNAM sits.", "scene_count": 3}"#;
        let m = parse_screenwriter_fallback(broken).unwrap();
        assert_eq!(m.get("scene_count").unwrap().as_i64().unwrap(), 3);
        let sp = m.get("screenplay").unwrap().as_str().unwrap();
        assert!(sp.contains("INT. HOUSE - DAY\nNAM sits."));
        assert!(parse_screenwriter_fallback("no json here").is_err());
    }

    #[test]
    fn director_frame_bridge_strip() {
        let p = format!(
            "A walks. {DIRECTOR_FRAME_BRIDGE_START} anchor stuff {DIRECTOR_FRAME_BRIDGE_END} B follows."
        );
        // Go joins `left + ". " + right` without de-duplicating left's own
        // terminator, so a doubled period here is upstream parity, not a bug.
        assert_eq!(strip_director_frame_bridge(&p), "A walks.. B follows");
        // malformed (no end marker) → cut to the marker
        let malformed = format!("A walks. {DIRECTOR_FRAME_BRIDGE_START} dangling");
        assert_eq!(strip_director_frame_bridge(&malformed), "A walks");
        assert_eq!(strip_director_frame_bridge(""), "");
    }

    /// A fresh project has no entity rows yet — keeping the parsed names is what
    /// preserves the `Reference entities:` clause in image prompts. Dropping
    /// them silently disabled character consistency.
    #[test]
    fn empty_catalog_keeps_parsed_names() {
        let names = vec!["HẬU".to_string(), "hậu".to_string(), " PHONG ".to_string(), "".to_string()];
        let out = normalize_names_to_entity_catalog(&names, &HashMap::new());
        assert_eq!(out, vec!["HẬU".to_string(), "PHONG".to_string()], "deduped, trimmed, order kept");
        // No names at all is still nothing.
        assert!(normalize_names_to_entity_catalog(&[], &HashMap::new()).is_empty());
    }

    #[test]
    fn names_normalized_to_catalog() {
        let mut catalog = HashMap::new();
        catalog.insert("cugia".to_string(), "Cụ Già".to_string());
        catalog.insert("nam".to_string(), "NAM".to_string());
        let names = vec![
            "CU GIA".to_string(),
            "cụ già".to_string(), // duplicate after folding
            "Nam".to_string(),
            "Unknown Guy".to_string(), // not in catalog → dropped
        ];
        let out = normalize_names_to_entity_catalog(&names, &catalog);
        assert_eq!(out, vec!["Cụ Già".to_string(), "NAM".to_string()]);
    }

    #[test]
    fn sb_helpers() {
        assert_eq!(sb_scene_id_from_shot_id("1_001"), "1");
        assert_eq!(sb_scene_id_from_shot_id("12_003"), "12");
        assert_eq!(sb_scene_id_from_shot_id("nounderscore"), "");
        assert_eq!(sb_default_duration(1), 6.0);
        assert_eq!(sb_default_duration(3), 15.0);
        let mut m: HashMap<String, Vec<Map<String, Value>>> = HashMap::new();
        for id in ["10", "2", "1"] {
            m.insert(id.to_string(), vec![Map::new()]);
        }
        assert_eq!(sb_ordered_scene_ids(&m), vec!["1", "2", "10"]);
    }

    #[test]
    fn media_ext_helpers() {
        assert_eq!(ext_from_content_type("image/jpeg"), ".jpg");
        assert_eq!(ext_from_content_type("video/mp4; charset=binary"), ".mp4");
        assert_eq!(ext_from_content_type("application/x-unknown"), "");
        assert_eq!(default_media_ext("video"), ".mp4");
        assert_eq!(default_media_ext("image"), ".jpg");
        assert!(is_remote_url("https://x.test/a.mp4"));
        assert!(!is_remote_url("/api/media/abc/file"));
    }

    #[test]
    fn png_and_gif_sniff() {
        // Minimal PNG header: signature + IHDR len/type + 64x32.
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0, 0, 0, 13]);
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&64u32.to_be_bytes());
        png.extend_from_slice(&32u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        assert_eq!(sniff_image_dimensions(&png), Some((64, 32)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&120u16.to_le_bytes());
        gif.extend_from_slice(&90u16.to_le_bytes());
        gif.extend_from_slice(&[0, 0, 0]);
        assert_eq!(sniff_image_dimensions(&gif), Some((120, 90)));
    }
}
