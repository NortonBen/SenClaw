//! Pipeline manager — port of `internal/pipeline/manager.go` plus the
//! orchestrator planning/validation/normalization logic from
//! `internal/agent/agents/orchestrator.go` + `orchestrator_validate.go`.

use crate::agents::{AgentInfo, Pool};
use crate::dag;
use crate::db::{self, Db};
use crate::llm;
use crate::state::Core;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

/// One node of a planned DAG (Go: agents.PlannedTask).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlannedTask {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub agent_type: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_from: Vec<String>,
    #[serde(default)]
    pub timeout_seconds: i64,
}

#[derive(serde::Deserialize)]
struct PlanDoc {
    #[serde(default)]
    tasks: Vec<PlannedTask>,
}

/// Canonical builtin DAG agent types (Go: DAGAgentTypeOrder, reordered to the
/// pool's canonical `builtin_order` — media_download before concat).
pub const KNOWN_DAG_TYPES: &[&str] = &[
    "director", "screenwriter", "scene_plan", "shot_design", "visual_asset",
    "scene_builder", "script_parser", "gen_ref", "director_frame", "character",
    "image", "video", "audio", "media_download", "concat", "critic",
];

/// Canonical per-type description; encodes required upstream deps in a
/// "(depends_on: ...)" clause that `normalize_dependencies` parses.
fn dag_agent_line(agent_type: &str) -> Option<&'static str> {
    Some(match agent_type {
        "director" => "Decompose a logline/synopsis into a hierarchical narrative blueprint (scene blocks with beats, conflict types, objectives). INPUT: task prompt with logline/story concept.",
        "screenwriter" => "Expand director scene blocks into a full Fountain-format screenplay. INPUT: requires 'scene_blocks' from director output (depends_on: director task).",
        "scene_plan" => "Generate per-scene environmental blueprints (architecture, lighting, spatial layout). INPUT: requires 'screenplay' field from screenwriter output (depends_on: screenwriter task).",
        "shot_design" => "Generate a formal shot list (shot sizes, camera moves, synthesis prompts). INPUT: requires 'screenplay' from screenwriter AND 'scene_environments' from scene_plan (depends_on: screenwriter + scene_plan tasks).",
        "visual_asset" => "Generate Character DNA Blueprints (golden image prompts + base appearance tags + ref_scenes) for all project characters. INPUT: grounded by screenplay context (depends_on: screenwriter task) and reads persisted entities from DB when available.",
        "scene_builder" => "PREFERRED synthesis agent: builds scenes and entities from structured shot_design shots. Produces correct image_prompt (synthesis_prompt per shot), video_prompt, action_sequence, narrator_text without LLM scene parsing. Outputs 'video_id', 'scene_ids', 'scene_count'. INPUT: requires 'shots' from shot_design (depends_on: shot_design + scene_plan + director + screenwriter). Use this instead of script_parser when shot_design has run.",
        "script_parser" => "Fallback scene parser: parses screenplay markdown → scenes and characters via LLM. Outputs 'video_id', 'scene_ids', 'scene_count'. INPUT: requires 'screenplay' from screenwriter and visual grounding from visual_asset (depends_on: screenwriter + visual_asset tasks). Use scene_builder instead when shot_design output is available.",
        "gen_ref" => "Post-process persisted scenes and synchronize scene reference entities/locations for downstream consistency. INPUT: requires script_parser outputs ('video_id','scene_ids') (depends_on: script_parser task).",
        "character" => "Generate reference images for all named characters/locations in the project. INPUT: reads from DB only; does not consume director_frame task output JSON. Pipeline waits for director_frame before character when that step exists (depends_on: script_parser task).",
        "image" => "Generate scene still images using character references. INPUT: requires 'video_id' from script_parser output (depends_on: script_parser + character tasks).",
        "video" => "Generate Veo3 video clips from scene images. INPUT: requires scene images to be completed — must run after image agent (depends_on: image task).",
        "audio" => "Generate TTS narration audio for scenes. INPUT: requires completed video clips in DB (depends_on: video task).",
        "concat" => "Concatenate all local video clips into final output MP4 using ffmpeg. INPUT: requires local video files — must run AFTER media_download has converted remote URLs to local paths (depends_on: media_download task).",
        "critic" => "Evaluate a generated video clip across 4 axes (object permanence, physics, temporal consistency, script faithfulness); returns PASS or FAIL + correction prompt. INPUT: requires video output.",
        "director_frame" => "Generate and apply frame-anchored continuity directives between adjacent scenes so the next scene inherits visual state/momentum. INPUT: requires persisted scenes/video_id (depends_on: scene_builder or script_parser). Must run BEFORE image/video generation.",
        "media_download" => "Download all remote image/video URLs (scenes and characters) to local media storage and update DB with local paths. INPUT: requires 'video_id' from script_parser; must run after video generation (depends_on: video task). Run BEFORE concat.",
        _ => return None,
    })
}

/// Appended to the user goal on the 2nd planning attempt.
pub const PLAN_REFINEMENT_USER_SUFFIX: &str = "\n\nRETRY CONSTRAINTS: Return a single JSON object with key \"tasks\" only. Every \"depends_on\" value must be the exact \"label\" of another task. Use only the agent_type values from the system message. The task graph must be a DAG (no cycles). \"prompt\" is the instruction the worker will send to that agent; upstream work is merged by label in working context.";

/// Appended to the system prompt on the 2nd planning attempt.
pub const PLAN_REFINEMENT_SYSTEM_SUFFIX: &str = "\n\nPLANNING: (1) Classify the request: full pre-production and production, or production-only with an existing screenplay. (2) Use only the listed available agent_type values. (3) Set depends_on to encode input order. (4) Return parseable JSON only, no surrounding prose.";

/// Used when souls/orchestrator.md is missing or empty.
const DEFAULT_ORCHESTRATOR_INTRO: &str = r#"You are the OrchestratorAgent for a video production pipeline.
Given a script and project context, decompose the work into a DAG of tasks.

For each task you return:
- "label" — unique id used in depends_on and to attach outputs
- "agent_type" — must be one of the available types below
- "prompt" — the instruction the downstream agent will receive
- "depends_on" — labels of prerequisite tasks, defining input / execution order
- "input_from" — optional: whose JSON outputs are injected into this prompt. Omit or [] to inject only outputs of tasks listed in depends_on (direct predecessors). Set explicitly when this task needs extra upstream branches beyond depends_on.
- "timeout_seconds" — per-task cap"#;

const CATALOG_OUTPUT_EXAMPLE: &str = r#"
OUTPUT: JSON only (no markdown). Example (expand with real labels and agent_type from the catalog):
{
  "tasks": [
    {"label": "blueprint", "agent_type": "director", "prompt": "Build narrative blueprint from the concept.", "input_from": [], "depends_on": [], "timeout_seconds": 180},
    {"label": "draft", "agent_type": "screenwriter", "prompt": "Expand into Fountain screenplay.", "input_from": ["director"], "depends_on": ["blueprint"], "timeout_seconds": 900}
  ]
}
"#;

// ---- orchestrator system prompt (orchestrator.go) ----

/// Dynamic agent catalog appended to the orchestrator system prompt.
fn build_agent_catalog(infos: &[AgentInfo]) -> String {
    let mut desc_map: HashMap<String, String> = HashMap::new();
    let mut type_set: HashSet<String> = HashSet::new();
    for a in infos {
        let desc = if a.description.is_empty() {
            dag_agent_line(&a.agent_type).unwrap_or("").to_string()
        } else {
            a.description.clone()
        };
        desc_map.insert(a.agent_type.clone(), desc);
        type_set.insert(a.agent_type.clone());
    }
    if type_set.is_empty() {
        for t in KNOWN_DAG_TYPES {
            type_set.insert(t.to_string());
            desc_map.insert(t.to_string(), dag_agent_line(t).unwrap_or("").to_string());
        }
    }

    let mut b = String::from("\nAVAILABLE AGENT TYPES (only these may appear in \"agent_type\"):\n");
    let write_block = |b: &mut String, title: &str, types: &[&str]| {
        let present: Vec<&str> = types.iter().copied().filter(|t| type_set.contains(*t)).collect();
        if present.is_empty() {
            return;
        }
        b.push('\n');
        b.push_str(title);
        b.push('\n');
        for t in present {
            let desc = match desc_map.get(t) {
                Some(d) if !d.is_empty() => d.clone(),
                _ => t.to_string(),
            };
            b.push_str("- ");
            b.push_str(t);
            b.push_str(": ");
            b.push_str(&desc);
            b.push('\n');
        }
    };
    write_block(&mut b, "Pre-production (when starting from a raw concept, run these first):",
        &["director", "screenwriter", "scene_plan", "shot_design", "visual_asset"]);
    write_block(&mut b, "Production (parse script and generate media):",
        &["scene_builder", "script_parser", "gen_ref", "director_frame", "character", "image", "video", "audio", "concat"]);
    write_block(&mut b, "QA (optional, per-clip review):", &["critic"]);
    write_block(&mut b, "Post-production (ALWAYS run media_download BEFORE concat — download remote URLs to local first, then concat local files):",
        &["media_download"]);

    let known: HashSet<&str> = KNOWN_DAG_TYPES.iter().copied().collect();
    let custom: Vec<&AgentInfo> =
        infos.iter().filter(|a| !known.contains(a.agent_type.as_str())).collect();
    if !custom.is_empty() {
        b.push_str("\nCustom Agents (dynamically registered skill agents):\n");
        for a in custom {
            let desc = if a.description.is_empty() { a.agent_type.clone() } else { a.description.clone() };
            b.push_str("- ");
            b.push_str(&a.agent_type);
            b.push_str(": ");
            b.push_str(&desc);
            b.push('\n');
        }
    }
    b.push_str(CATALOG_OUTPUT_EXAMPLE);
    b
}

/// Full orchestrator system message: soul override (souls/orchestrator.md via
/// the pool) or the built-in intro, plus the dynamic agent catalog.
pub fn build_orchestrator_system(pool: &Pool) -> String {
    let base = pool.system_prompt("orchestrator");
    let base = base.trim();
    let base = if base.is_empty() { DEFAULT_ORCHESTRATOR_INTRO } else { base };
    format!("{}\n\n{}", base, build_agent_catalog(&pool.list_info()))
}

// ---- plan validation (orchestrator_validate.go) ----

fn allowed_set_for_validation(allowed: &[String]) -> HashSet<String> {
    let mut m: HashSet<String> = allowed
        .iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if m.is_empty() {
        m = KNOWN_DAG_TYPES.iter().map(|t| t.to_string()).collect();
    }
    m
}

/// Checks labels, allowed agent_type, dependency keys, and acyclicity.
pub fn validate_plan(tasks: &[PlannedTask], known_types: &[String]) -> Result<(), String> {
    if tasks.is_empty() {
        return Err("orchestrator: no tasks in plan".to_string());
    }
    let allow = allowed_set_for_validation(known_types);
    let mut labels: HashSet<&str> = HashSet::with_capacity(tasks.len());
    for (i, t) in tasks.iter().enumerate() {
        if t.label.trim().is_empty() {
            return Err(format!("orchestrator: task {i} has empty label"));
        }
        if !labels.insert(t.label.as_str()) {
            return Err(format!("orchestrator: duplicate label {:?}", t.label));
        }
        if t.agent_type.is_empty() {
            return Err(format!("orchestrator: task {:?} has empty agent_type", t.label));
        }
        if !allow.contains(&t.agent_type) {
            return Err(format!(
                "orchestrator: task {:?} uses disallowed agent_type {:?}",
                t.label, t.agent_type
            ));
        }
    }
    for t in tasks {
        for d in &t.depends_on {
            if !labels.contains(d.as_str()) {
                return Err(format!("orchestrator: task {:?} depends on unknown label {d:?}", t.label));
            }
        }
        for src in &t.input_from {
            let src = src.trim();
            if src.is_empty() {
                continue;
            }
            if !labels.contains(src) {
                return Err(format!(
                    "orchestrator: task {:?} input_from references unknown label {src:?}",
                    t.label
                ));
            }
        }
    }
    validate_no_cycle(tasks)
}

/// Kahn check on planned task labels.
fn validate_no_cycle(tasks: &[PlannedTask]) -> Result<(), String> {
    let mut in_degree: HashMap<&str, usize> = tasks.iter().map(|t| (t.label.as_str(), 0)).collect();
    for t in tasks {
        *in_degree.entry(t.label.as_str()).or_insert(0) += t.depends_on.len();
    }
    let mut queue: Vec<&str> =
        in_degree.iter().filter(|(_, d)| **d == 0).map(|(l, _)| *l).collect();
    let mut visited = 0usize;
    while let Some(u) = queue.pop() {
        visited += 1;
        for t in tasks {
            for dep in &t.depends_on {
                if dep == u {
                    let d = in_degree.entry(t.label.as_str()).or_insert(0);
                    if *d > 0 {
                        *d -= 1;
                        if *d == 0 {
                            queue.push(t.label.as_str());
                        }
                    }
                }
            }
        }
    }
    if visited != tasks.len() {
        return Err("orchestrator: cycle or inconsistent dependency graph".to_string());
    }
    Ok(())
}

// ---- dependency normalization (NormalizePipelineDependencies) ----

/// Tokens matching `[a-z][a-z0-9_]*` in the "(depends_on: ...)" clause of a
/// builtin agent description, filtered to known agent types.
fn dependency_types_from_description(agent_type: &str) -> Vec<String> {
    let desc = match dag_agent_line(agent_type) {
        Some(d) => d.trim().to_lowercase(),
        None => return Vec::new(),
    };
    let idx = match desc.find("depends_on:") {
        Some(i) => i,
        None => return Vec::new(),
    };
    let mut seg = &desc[idx + "depends_on:".len()..];
    if let Some(end) = seg.find(')') {
        seg = &seg[..end];
    }
    if let Some(end) = seg.find('.') {
        seg = &seg[..end];
    }
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in seg.chars().chain(std::iter::once(' ')) {
        if cur.is_empty() {
            if ch.is_ascii_lowercase() {
                cur.push(ch);
            }
        } else if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' {
            cur.push(ch);
        } else {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for tok in tokens {
        if tok == "task" || tok == "tasks" || tok == "and" || tok == "or" {
            continue;
        }
        if dag_agent_line(&tok).is_none() {
            continue;
        }
        if seen.insert(tok.clone()) {
            out.push(tok);
        }
    }
    out
}

fn contains(s: &[String], v: &str) -> bool {
    s.iter().any(|x| x == v)
}

/// Graph repair (Go: NormalizePipelineDependencies). Promotes input_from into
/// depends_on, applies per-type "(depends_on: ...)" hints, inserts gen_ref
/// after script_parser when missing, enforces hard ordering rules, and strips
/// reverse-order edges against the canonical builtin `order`.
pub fn normalize_dependencies(tasks: &mut Vec<PlannedTask>, order: &[&str]) {
    if tasks.is_empty() {
        return;
    }
    // Index labels by agent_type (supports multiple tasks per type).
    let mut labels_by_type: HashMap<String, Vec<String>> = HashMap::new();
    for t in tasks.iter() {
        labels_by_type.entry(t.agent_type.clone()).or_default().push(t.label.clone());
    }

    // Ensure gen_ref exists whenever script_parser exists.
    let has_script = labels_by_type.get("script_parser").is_some_and(|v| !v.is_empty());
    let has_gen_ref = labels_by_type.get("gen_ref").is_some_and(|v| !v.is_empty());
    if has_script && !has_gen_ref {
        let script_label = labels_by_type["script_parser"][0].clone();
        let gen_ref_label = "gen_ref_sync".to_string();
        tasks.push(PlannedTask {
            label: gen_ref_label.clone(),
            agent_type: "gen_ref".to_string(),
            prompt: "Synchronize scene entity/location references from parsed scenes and persisted entities.".to_string(),
            depends_on: vec![script_label.clone()],
            input_from: vec![script_label],
            timeout_seconds: 300,
        });
        labels_by_type.entry("gen_ref".to_string()).or_default().push(gen_ref_label);
    }

    let script_labels = labels_by_type.get("script_parser").cloned().unwrap_or_default();
    let visual_labels = labels_by_type.get("visual_asset").cloned().unwrap_or_default();
    let gen_ref_labels = labels_by_type.get("gen_ref").cloned().unwrap_or_default();

    for i in 0..tasks.len() {
        // Explicit input_from labels must be predecessors.
        let input_from = tasks[i].input_from.clone();
        for src in input_from {
            let src = src.trim().to_string();
            if !src.is_empty() && !contains(&tasks[i].depends_on, &src) {
                tasks[i].depends_on.push(src);
            }
        }

        // Dependency labels required by the agent description hints.
        for dep_type in dependency_types_from_description(&tasks[i].agent_type) {
            for dep_label in labels_by_type.get(&dep_type).cloned().unwrap_or_default() {
                if !dep_label.is_empty()
                    && dep_label != tasks[i].label
                    && !contains(&tasks[i].depends_on, &dep_label)
                {
                    tasks[i].depends_on.push(dep_label);
                }
            }
        }

        // Hard rules: script_parser MUST run after visual_asset;
        // visual_asset MUST NOT depend on script_parser.
        if tasks[i].agent_type == "script_parser" {
            for va in &visual_labels {
                if !va.is_empty() && *va != tasks[i].label && !contains(&tasks[i].depends_on, va) {
                    tasks[i].depends_on.push(va.clone());
                }
            }
        }
        if tasks[i].agent_type == "visual_asset" {
            for sp in &script_labels {
                tasks[i].depends_on.retain(|d| d != sp);
            }
        }
        // gen_ref must run after script_parser (and inject its output).
        if tasks[i].agent_type == "gen_ref" {
            for sp in &script_labels {
                if !sp.is_empty() && *sp != tasks[i].label && !contains(&tasks[i].depends_on, sp) {
                    tasks[i].depends_on.push(sp.clone());
                }
                if !contains(&tasks[i].input_from, sp) {
                    tasks[i].input_from.push(sp.clone());
                }
            }
        }
        if tasks[i].agent_type == "script_parser" {
            for gr in &gen_ref_labels {
                tasks[i].depends_on.retain(|d| d != gr);
            }
        }

        // Keep explicit input_from aligned with dependencies.
        if !tasks[i].input_from.is_empty() {
            let deps = tasks[i].depends_on.clone();
            for d in deps {
                if !d.is_empty() && !contains(&tasks[i].input_from, &d) {
                    tasks[i].input_from.push(d);
                }
            }
        }
    }

    // Character waits for director_frame when that step exists, but character
    // reads DB only — exclude director_frame from upstream JSON injection.
    let type_by_label: HashMap<String, String> =
        tasks.iter().map(|t| (t.label.clone(), t.agent_type.clone())).collect();
    let df_labels = labels_by_type.get("director_frame").cloned().unwrap_or_default();
    if !df_labels.is_empty() {
        for i in 0..tasks.len() {
            if tasks[i].agent_type != "character" {
                continue;
            }
            for df in &df_labels {
                if !df.is_empty() && *df != tasks[i].label && !contains(&tasks[i].depends_on, df) {
                    tasks[i].depends_on.push(df.clone());
                }
            }
            let inject: Vec<String> = tasks[i]
                .depends_on
                .iter()
                .filter(|d| type_by_label.get(*d).map(|s| s.as_str()) != Some("director_frame"))
                .cloned()
                .collect();
            tasks[i].input_from = inject;
        }
    }

    // Final safety pass: remove reverse-order edges for known builtin types.
    enforce_known_type_order(tasks, order);
}

fn enforce_known_type_order(tasks: &mut [PlannedTask], order: &[&str]) {
    if tasks.is_empty() {
        return;
    }
    let type_order: HashMap<&str, usize> = order.iter().enumerate().map(|(i, t)| (*t, i)).collect();
    let type_by_label: HashMap<String, String> =
        tasks.iter().map(|t| (t.label.clone(), t.agent_type.clone())).collect();
    for t in tasks.iter_mut() {
        let Some(&cur) = type_order.get(t.agent_type.as_str()) else { continue };
        t.depends_on.retain(|dep| {
            match type_by_label.get(dep).and_then(|dt| type_order.get(dt.as_str())) {
                Some(&d) if d > cur => false, // reverse builtin order
                _ => true,
            }
        });
    }
}

// ---- deterministic pipeline templates (manager.go M4) ----

/// Canonical agent-type chain per mode (Go: templateChains). Each task is
/// seeded with a dependency on the previous enabled task, then
/// `normalize_dependencies` enriches it — same as the LLM-planned path.
fn template_chain(mode: &str) -> &'static [&'static str] {
    match mode {
        "full" => &[
            "director", "screenwriter", "scene_plan", "shot_design", "visual_asset",
            "script_parser", "gen_ref", "director_frame",
            "character", "image", "video", "audio", "concat", "media_download",
        ],
        _ => &[
            "script_parser", "gen_ref", "director_frame",
            "character", "image", "video", "audio", "concat", "media_download",
        ],
    }
}

fn template_prompt(agent_type: &str) -> &'static str {
    match agent_type {
        "director" => "Decompose the concept into a hierarchical narrative blueprint (scene blocks: beats, conflict, objective, value-charge shift).",
        "screenwriter" => "Expand the narrative blueprint into a full Fountain-format screenplay.",
        "scene_plan" => "Produce per-scene environmental blueprints (architecture, lighting, spatial layout) from the screenplay.",
        "shot_design" => "Produce a formal shot list (shot sizes, camera moves, synthesis prompts) from the screenplay and scene environments.",
        "visual_asset" => "Generate Character DNA Blueprints (golden image prompts + base appearance tags) for all project characters.",
        "script_parser" => "Parse the screenplay into scenes and characters and persist them.",
        "scene_builder" => "Build scenes and entities from the structured shot list.",
        "gen_ref" => "Synchronize scene entity/location references from the persisted scenes and entities.",
        "director_frame" => "Generate frame-anchored continuity directives between adjacent scenes.",
        "character" => "Generate reference images for all named characters/locations in the project.",
        "image" => "Generate scene still images using the character references.",
        "video" => "Generate Veo3 video clips from the completed scene images.",
        "audio" => "Generate TTS narration audio for the scenes.",
        "concat" => "Concatenate all local video clips into the final output MP4.",
        "media_download" => "Download all remote image/video URLs to local media storage.",
        _ => "",
    }
}

/// Deterministic DAG for a template mode, restricted to enabled agent types.
/// Disabled/unavailable agents are skipped and the chain re-links across the gap.
fn build_template_plan(mode: &str, allowed: &[String], order: &[&str]) -> Result<Vec<PlannedTask>, String> {
    let allowed_set: HashSet<&str> = allowed.iter().map(|s| s.as_str()).collect();
    let mut tasks: Vec<PlannedTask> = Vec::new();
    let mut prev_label = String::new();
    for typ in template_chain(mode) {
        if !allowed_set.contains(typ) {
            continue;
        }
        let deps = if prev_label.is_empty() { Vec::new() } else { vec![prev_label.clone()] };
        tasks.push(PlannedTask {
            label: typ.to_string(),
            agent_type: typ.to_string(),
            prompt: template_prompt(typ).to_string(),
            depends_on: deps,
            input_from: Vec::new(),
            timeout_seconds: 900,
        });
        prev_label = typ.to_string();
    }
    if tasks.is_empty() {
        return Err(format!("no enabled agents available for template mode {mode:?}"));
    }
    normalize_dependencies(&mut tasks, order);
    Ok(tasks)
}

/// Drop audio tasks when the built-in audio agent is disabled in Settings and
/// strip references to them from remaining depends_on.
fn apply_disabled_builtin_agent_tasks(db: &Db, tasks: Vec<PlannedTask>) -> Vec<PlannedTask> {
    if !db.builtin_agent_disabled("audio") {
        return tasks;
    }
    let mut dropped: HashSet<String> = HashSet::new();
    let mut kept: Vec<PlannedTask> = Vec::new();
    for t in tasks {
        if t.agent_type == "audio" {
            dropped.insert(t.label);
            continue;
        }
        kept.push(t);
    }
    for t in kept.iter_mut() {
        t.depends_on.retain(|d| !dropped.contains(d));
    }
    kept
}

// ---- LLM planning (orchestrator.go Plan path) ----

/// Plan a DAG with the orchestrator LLM: up to 2 attempts, the second with the
/// refinement suffixes appended, each validated + normalized. `script` is the
/// context block (project summary) prepended to the goal, mirroring the Go
/// `PlanWithSystem(goal, projectSummary, system)` composition.
pub async fn plan_with_llm(
    _core: &Core,
    pool: &Pool,
    goal: &str,
    script: &str,
) -> Result<Vec<PlannedTask>, String> {
    let infos = pool.list_info();
    if infos.is_empty() {
        return Err("no agent types available for planning (all agents may be disabled in settings)".to_string());
    }
    let allowed: Vec<String> = infos.iter().map(|a| a.agent_type.clone()).collect();
    let sys = build_orchestrator_system(pool);

    let mut last_err = String::new();
    for attempt in 0..2u32 {
        let (g, s) = if attempt == 1 {
            (
                format!("{goal}{PLAN_REFINEMENT_USER_SUFFIX}"),
                format!("{sys}{PLAN_REFINEMENT_SYSTEM_SUFFIX}"),
            )
        } else {
            (goal.to_string(), sys.clone())
        };
        let prompt = if script.trim().is_empty() { g } else { format!("{script}\n\n{g}") };
        let text = match llm::complete(&s, &prompt, 8000).await {
            Ok((t, _model)) => t,
            Err(e) => {
                last_err = e;
                continue;
            }
        };
        let doc: PlanDoc = match llm::parse_json(&text) {
            Ok(d) => d,
            Err(e) => {
                last_err = format!("orchestrator parse plan: {e}\nraw: {}", llm::truncate(&text, 500));
                continue;
            }
        };
        let mut tasks = doc.tasks;
        for t in tasks.iter_mut() {
            if t.timeout_seconds == 0 {
                t.timeout_seconds = 900;
            }
        }
        normalize_dependencies(&mut tasks, &pool.builtin_order);
        if let Err(e) = validate_plan(&tasks, &allowed) {
            last_err = e;
            continue;
        }
        return Ok(tasks);
    }
    Err(format!("orchestrate pipeline (LLM planning failed after retry): {last_err}"))
}

// ---- manager (manager.go) ----

fn build_project_summary(proj: &db::Row) -> String {
    let name = db::str_of(proj, "name");
    let desc = db::str_of(proj, "description");
    let mut story = db::str_of(proj, "story");
    let mut s = format!("Project: {name}");
    if !desc.is_empty() {
        s.push_str("\nDescription: ");
        s.push_str(&desc);
    }
    if !story.is_empty() {
        if story.chars().count() > 500 {
            story = story.chars().take(500).collect::<String>() + "...";
        }
        s.push_str("\nStory: ");
        s.push_str(&story);
    }
    s
}

fn full_mode_goal(script: &str) -> String {
    format!(
        "Pipeline Mode: FULL (Pre-Production + Production)\n\
         The input below is a raw concept or story idea — NOT a formatted screenplay.\n\
         You MUST build the full pipeline in this order:\n\
         \x20 1. director        → narrative blueprint (scene blocks)\n\
         \x20 2. screenwriter    → Fountain screenplay (depends: director)\n\
         \x20 3. scene_plan      → environment blueprints (depends: screenwriter)\n\
         \x20 4. shot_design     → shot list + synthesis prompts (depends: screenwriter, scene_plan)\n\
         \x20 5. script_parser   → persist scenes and characters to DB (depends: shot_design)\n\
         \x20 6. gen_ref         → synchronize scene entity/location refs after parse (depends: script_parser)\n\
         \x20 7. director_frame  → generate continuity bridge prompts between adjacent scenes (depends: script_parser)\n\
         \x20 8. visual_asset    → Character DNA blueprints (depends: script_parser)\n\
         \x20 9. character → image → video → audio → concat (standard production chain)\n\
         \x2010. media_download  → download all remote media to local storage (depends: concat)\n\n\
         Concept / Story Input:\n\n{script}"
    )
}

fn production_mode_goal(orientation: &str, script: &str) -> String {
    format!(
        "Create a {orientation} video from the following screenplay.\n\
         Standard production chain: script_parser → gen_ref → director_frame → character → image → video → audio → concat → media_download\n\
         ALWAYS include media_download as the final step (depends: concat) to download generated media locally.\n\n{script}"
    )
}

/// Create a pipeline: enforce one active pipeline per project, plan
/// (template or LLM), persist dag_parents + dag_tasks, emit `pipeline:created`.
/// Returns (pipeline_id, task_count).
pub async fn create(
    core: &Core,
    pool: &Pool,
    project_id: &str,
    script: &str,
    orientation: &str,
    goal: &str,
    mode: &str,
) -> Result<(String, usize), String> {
    if project_id.is_empty() {
        return Err("project_id required".to_string());
    }
    let orientation = if orientation.is_empty() { "VERTICAL" } else { orientation };

    // Enforce 1 active pipeline per project.
    if let Some(existing) = core
        .db
        .query_one(
            "SELECT id FROM dag_parents WHERE project_id = ?1 AND status IN ('queued','active') LIMIT 1",
            &[&project_id],
        )
        .map_err(|e| format!("check existing pipeline: {e}"))?
    {
        return Err(format!(
            "project already has an active pipeline ({})",
            db::str_of(&existing, "id")
        ));
    }

    let proj = core
        .db
        .get("project", project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("project {project_id:?} not found"))?;
    let summary = build_project_summary(&proj);

    let computed_goal = if goal.trim().is_empty() {
        if mode == "full" {
            full_mode_goal(script)
        } else {
            production_mode_goal(orientation, script)
        }
    } else {
        goal.to_string()
    };

    let infos = pool.list_info();
    if infos.is_empty() {
        return Err("no agent types available for planning (all agents may be disabled in settings)".to_string());
    }
    let allowed: Vec<String> = infos.iter().map(|a| a.agent_type.clone()).collect();

    // Deterministic template DAG for the standard modes; the LLM planner is
    // used only for "custom" mode or an explicit goal override.
    let use_llm = mode == "custom" || !goal.trim().is_empty();
    let mut plan: Option<Vec<PlannedTask>> = None;
    if !use_llm {
        let tp = build_template_plan(mode, &allowed, &pool.builtin_order)
            .map_err(|e| format!("build template pipeline: {e}"))?;
        match validate_plan(&tp, &allowed) {
            Ok(()) => plan = Some(tp),
            Err(e) => eprintln!("[pipeline] template plan invalid ({e}); falling back to LLM planner"),
        }
    }
    let tasks = match plan {
        Some(p) => p,
        None => plan_with_llm(core, pool, &computed_goal, &summary).await?,
    };
    // Safety net if the model ignores disabled-agent settings.
    let tasks = apply_disabled_builtin_agent_tasks(&core.db, tasks);

    // Persist DAG parent.
    let parent_id = db::new_id();
    let mut prow = Map::new();
    prow.insert("id".into(), json!(parent_id));
    prow.insert("project_id".into(), json!(project_id));
    prow.insert("status".into(), json!("queued"));
    prow.insert("goal".into(), json!(computed_goal));
    prow.insert("orientation".into(), json!(orientation));
    prow.insert("script_md".into(), json!(script));
    core.db.insert("dag_parents", &prow).map_err(|e| format!("create pipeline parent: {e}"))?;

    // Persist tasks.
    for pt in &tasks {
        let mut prompt = pt.prompt.clone();
        // Inject the raw script into script_parser only in production mode; in
        // full mode the screenplay arrives via the screenwriter's result.
        if pt.agent_type == "script_parser" && !script.is_empty() && mode != "full" {
            prompt = format!("{script}\n\n{prompt}");
        }
        let timeout = if pt.timeout_seconds == 0 { 900 } else { pt.timeout_seconds };
        let mut row = Map::new();
        row.insert("id".into(), json!(db::new_id()));
        row.insert("parent_id".into(), json!(parent_id));
        row.insert("label".into(), json!(pt.label));
        row.insert("agent_type".into(), json!(pt.agent_type));
        row.insert("prompt".into(), json!(prompt));
        row.insert("depends_on".into(), json!(dag::encode_depends_on(&pt.depends_on)));
        row.insert("input_from".into(), json!(dag::encode_depends_on(&pt.input_from)));
        row.insert("status".into(), json!("registered"));
        row.insert("timeout_seconds".into(), json!(timeout));
        core.db
            .insert("dag_tasks", &row)
            .map_err(|e| format!("create dag task {:?}: {e}", pt.label))?;
    }

    core.dash.emit("pipeline:created", json!({"pipeline_id": parent_id}));
    Ok((parent_id, tasks.len()))
}

pub fn pause(core: &Core, id: &str) -> Result<(), String> {
    dag::update_parent_status(&core.db, id, "paused")
}

pub fn cancel(core: &Core, id: &str) -> Result<(), String> {
    dag::update_parent_status(&core.db, id, "failed")
}

/// Re-queue a paused/failed pipeline; the engine promotes it back to active.
pub fn start(core: &Core, id: &str) -> Result<(), String> {
    dag::update_parent_status(&core.db, id, "queued")
}

/// Reset a failed/timeout task to registered so the engine re-executes it.
/// A failed/done parent is set back to active.
pub fn retry_task(core: &Core, pipeline_id: &str, task_id: &str) -> Result<(), String> {
    let mut fields = Map::new();
    fields.insert("status".into(), json!("registered"));
    fields.insert("result".into(), Value::Null);
    fields.insert("started_at".into(), Value::Null);
    fields.insert("completed_at".into(), Value::Null);
    core.db.update("dag_tasks", task_id, &fields).map_err(|e| e.to_string())?;
    let parent = dag::load_parent(&core.db, pipeline_id)?;
    if parent.status == "failed" || parent.status == "done" {
        dag::update_parent_status(&core.db, pipeline_id, "active")?;
    }
    Ok(())
}

/// Current state of a pipeline: the dag_parents row plus `tasks` with
/// depends_on/input_from decoded to arrays.
pub fn get_status(core: &Core, id: &str) -> Result<Value, String> {
    let parent = core
        .db
        .get("dag_parents", id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("dag parent {id:?} not found"))?;
    let rows = core
        .db
        .query("SELECT * FROM dag_tasks WHERE parent_id = ?1 ORDER BY rowid", &[&id])
        .map_err(|e| e.to_string())?;
    let tasks: Vec<Value> = rows
        .into_iter()
        .map(|mut r| {
            for k in ["depends_on", "input_from"] {
                let decoded = dag::parse_depends_on(&db::str_of(&r, k));
                r.insert(k.to_string(), json!(decoded));
            }
            Value::Object(r)
        })
        .collect();
    let mut out = parent;
    out.insert("tasks".into(), Value::Array(tasks));
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: &[&str] = KNOWN_DAG_TYPES;

    fn pt(label: &str, agent_type: &str, deps: &[&str]) -> PlannedTask {
        PlannedTask {
            label: label.to_string(),
            agent_type: agent_type.to_string(),
            prompt: String::new(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            input_from: Vec::new(),
            timeout_seconds: 900,
        }
    }

    fn find<'a>(tasks: &'a [PlannedTask], label: &str) -> &'a PlannedTask {
        tasks.iter().find(|t| t.label == label).unwrap()
    }

    #[test]
    fn validate_plan_failures() {
        // empty plan
        assert!(validate_plan(&[], &[]).is_err());
        // duplicate label
        let dup = vec![pt("a", "director", &[]), pt("a", "screenwriter", &[])];
        assert!(validate_plan(&dup, &[]).unwrap_err().contains("duplicate label"));
        // disallowed agent_type
        let bad_type = vec![pt("a", "image", &[])];
        let known = vec!["script_parser".to_string()];
        assert!(validate_plan(&bad_type, &known).unwrap_err().contains("disallowed agent_type"));
        // unknown dependency label
        let bad_dep = vec![pt("a", "director", &["ghost"])];
        assert!(validate_plan(&bad_dep, &[]).unwrap_err().contains("unknown label"));
        // unknown input_from label
        let mut bad_input = vec![pt("a", "director", &[])];
        bad_input[0].input_from = vec!["ghost".to_string()];
        assert!(validate_plan(&bad_input, &[]).unwrap_err().contains("input_from"));
        // cycle
        let cyc = vec![pt("a", "director", &["b"]), pt("b", "screenwriter", &["a"])];
        assert!(validate_plan(&cyc, &[]).unwrap_err().contains("cycle"));
        // valid plan passes
        let ok = vec![pt("a", "director", &[]), pt("b", "screenwriter", &["a"])];
        assert!(validate_plan(&ok, &[]).is_ok());
    }

    #[test]
    fn normalize_promotes_input_from_to_depends_on() {
        let mut tasks = vec![pt("a", "director", &[]), pt("b", "screenwriter", &[])];
        tasks[1].input_from = vec!["a".to_string()];
        normalize_dependencies(&mut tasks, ORDER);
        assert!(find(&tasks, "b").depends_on.contains(&"a".to_string()));
        assert!(find(&tasks, "b").input_from.contains(&"a".to_string()));
    }

    #[test]
    fn normalize_inserts_gen_ref_after_script_parser() {
        let mut tasks = vec![pt("parse", "script_parser", &[])];
        normalize_dependencies(&mut tasks, ORDER);
        assert_eq!(tasks.len(), 2);
        let gr = find(&tasks, "gen_ref_sync");
        assert_eq!(gr.agent_type, "gen_ref");
        assert_eq!(gr.depends_on, vec!["parse".to_string()]);
        assert!(gr.input_from.contains(&"parse".to_string()));
        assert_eq!(gr.timeout_seconds, 300);
        // script_parser never depends on gen_ref
        assert!(!find(&tasks, "parse").depends_on.contains(&"gen_ref_sync".to_string()));
    }

    #[test]
    fn normalize_strips_reverse_order_edges() {
        // image depending on video is a reverse edge against the builtin order.
        let mut tasks = vec![pt("vid", "video", &["img"]), pt("img", "image", &["vid"])];
        normalize_dependencies(&mut tasks, ORDER);
        assert!(!find(&tasks, "img").depends_on.contains(&"vid".to_string()));
        assert!(find(&tasks, "vid").depends_on.contains(&"img".to_string()));
        assert!(validate_plan(&tasks, &[]).is_ok());
    }

    #[test]
    fn normalize_hard_rules_visual_asset_script_parser() {
        let mut tasks = vec![
            pt("va", "visual_asset", &["parse"]),
            pt("parse", "script_parser", &[]),
            pt("gr", "gen_ref", &[]),
        ];
        normalize_dependencies(&mut tasks, ORDER);
        // visual_asset must NOT depend on script_parser; script_parser must
        // depend on visual_asset.
        assert!(!find(&tasks, "va").depends_on.contains(&"parse".to_string()));
        assert!(find(&tasks, "parse").depends_on.contains(&"va".to_string()));
        // gen_ref gains dep + input_from on script_parser.
        assert!(find(&tasks, "gr").depends_on.contains(&"parse".to_string()));
        assert!(find(&tasks, "gr").input_from.contains(&"parse".to_string()));
    }

    #[test]
    fn normalize_character_waits_for_director_frame() {
        let mut tasks = vec![
            pt("parse", "script_parser", &[]),
            pt("gr", "gen_ref", &["parse"]),
            pt("df", "director_frame", &["gr"]),
            pt("char", "character", &[]),
        ];
        normalize_dependencies(&mut tasks, ORDER);
        let c = find(&tasks, "char");
        assert!(c.depends_on.contains(&"df".to_string()));
        assert!(c.depends_on.contains(&"parse".to_string()));
        // director_frame output is excluded from JSON injection.
        assert!(!c.input_from.contains(&"df".to_string()));
        assert!(c.input_from.contains(&"parse".to_string()));
    }

    #[test]
    fn dependency_hints_parse_from_descriptions() {
        assert_eq!(dependency_types_from_description("shot_design"), vec!["screenwriter", "scene_plan"]);
        assert_eq!(dependency_types_from_description("director_frame"), vec!["scene_builder", "script_parser"]);
        assert_eq!(dependency_types_from_description("concat"), vec!["media_download"]);
        assert!(dependency_types_from_description("critic").is_empty());
        assert!(dependency_types_from_description("nonexistent").is_empty());
    }

    #[test]
    fn template_production_plan_shape() {
        let allowed: Vec<String> = KNOWN_DAG_TYPES.iter().map(|s| s.to_string()).collect();
        let tasks = build_template_plan("production", &allowed, ORDER).unwrap();
        let labels: Vec<&str> = tasks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec![
            "script_parser", "gen_ref", "director_frame", "character", "image",
            "video", "audio", "concat", "media_download",
        ]);
        assert!(validate_plan(&tasks, &allowed).is_ok());
        // media_download runs before concat under the pool's canonical order.
        assert_eq!(find(&tasks, "media_download").depends_on, vec!["video".to_string()]);
        let concat = find(&tasks, "concat");
        assert!(concat.depends_on.contains(&"audio".to_string()));
        assert!(concat.depends_on.contains(&"media_download".to_string()));
        // character waits for director_frame, injects only script_parser.
        let c = find(&tasks, "character");
        assert!(c.depends_on.contains(&"director_frame".to_string()));
        assert_eq!(c.input_from, vec!["script_parser".to_string()]);
    }

    #[test]
    fn template_full_plan_shape() {
        let allowed: Vec<String> = KNOWN_DAG_TYPES.iter().map(|s| s.to_string()).collect();
        let tasks = build_template_plan("full", &allowed, ORDER).unwrap();
        let labels: Vec<&str> = tasks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, vec![
            "director", "screenwriter", "scene_plan", "shot_design", "visual_asset",
            "script_parser", "gen_ref", "director_frame", "character", "image",
            "video", "audio", "concat", "media_download",
        ]);
        assert!(validate_plan(&tasks, &allowed).is_ok());
        let sp = find(&tasks, "script_parser");
        assert!(sp.depends_on.contains(&"visual_asset".to_string()));
        assert!(sp.depends_on.contains(&"screenwriter".to_string()));
        let sd = find(&tasks, "shot_design");
        assert!(sd.depends_on.contains(&"screenwriter".to_string()));
        assert!(sd.depends_on.contains(&"scene_plan".to_string()));
    }

    #[test]
    fn template_relinks_across_disabled_gap() {
        // director_frame unavailable: character re-links to gen_ref.
        let allowed: Vec<String> = KNOWN_DAG_TYPES
            .iter()
            .filter(|t| **t != "director_frame")
            .map(|s| s.to_string())
            .collect();
        let tasks = build_template_plan("production", &allowed, ORDER).unwrap();
        assert!(tasks.iter().all(|t| t.agent_type != "director_frame"));
        assert!(find(&tasks, "character").depends_on.contains(&"gen_ref".to_string()));
        assert!(validate_plan(&tasks, &allowed).is_ok());
    }

    #[test]
    fn disabled_audio_dropped_and_deps_stripped() {
        let db = Db::open_memory().unwrap();
        db.kv_set("builtin_agent_disabled:audio", "1").unwrap();
        let tasks = vec![
            pt("vid", "video", &[]),
            pt("aud", "audio", &["vid"]),
            pt("cat", "concat", &["aud", "vid"]),
        ];
        let out = apply_disabled_builtin_agent_tasks(&db, tasks);
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|t| t.agent_type != "audio"));
        assert_eq!(find(&out, "cat").depends_on, vec!["vid".to_string()]);
    }
}
