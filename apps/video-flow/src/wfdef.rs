//! Build a SenClaw workflow definition for one project.
//!
//! Why this exists: the app's own DAG engine ran each stage as a single task,
//! so `video` meant "render all 9 clips one after another" — 20–45 minutes of
//! wall clock with four idle concurrency slots. The workflow engine runs
//! independent nodes in parallel (cap 5 by default), so the pipeline is emitted
//! with **one node per scene**:
//!
//! ```text
//!   parse → refs → bridge ─┬─ img_0 ─ vid_0 ─┐
//!                          ├─ img_1 ─ vid_1 ─┼─ download → concat
//!                          └─ img_N ─ vid_N ─┘
//! ```
//!
//! `vid_k` depends on `img_k` alone — never on "all images" — so scene 0 starts
//! rendering while scene 3's still image is being generated, and one bad scene
//! fails its own node instead of sinking the stage.

use crate::db::{self, Row};

/// How the definition reaches back into this app.
pub struct StepEndpoint {
    /// e.g. `http://127.0.0.1:4460`
    pub base_url: String,
}

impl StepEndpoint {
    /// `curl` line for a blocking step call. `--fail-with-body` makes a 500
    /// exit non-zero *and* still print the error, which is what shows up on the
    /// failed node in the workflow UI.
    fn curl(&self, path: &str, body: &str) -> String {
        format!(
            "curl -sS --fail-with-body -X POST {}/api/steps/{} -H 'content-type: application/json' -d '{}'",
            self.base_url.trim_end_matches('/'),
            path,
            body.replace('\'', "'\\''")
        )
    }

    fn agent(&self, agent_type: &str, project_id: &str, video_id: &str) -> String {
        let body = serde_json::json!({
            "agent_type": agent_type,
            "project_id": project_id,
            "video_id": video_id,
        })
        .to_string();
        self.curl("agent", &body)
    }

    /// An agent whose input is the project's own story text (the `director`
    /// blueprint starts from the concept, not from an upstream stage).
    fn agent_story(&self, agent_type: &str, project_id: &str, video_id: &str) -> String {
        let body = serde_json::json!({
            "agent_type": agent_type,
            "project_id": project_id,
            "video_id": video_id,
            "use_project_story": true,
        })
        .to_string();
        self.curl("agent", &body)
    }

    /// `script_parser` fed the project's real screenplay rather than a stub.
    fn parse_story(&self, project_id: &str, video_id: &str) -> String {
        let body = serde_json::json!({
            "agent_type": "script_parser",
            "project_id": project_id,
            "video_id": video_id,
            "use_project_story": true,
        })
        .to_string();
        self.curl("agent", &body)
    }

    /// Address the scene by position, not id: `script_parser` recreates scenes
    /// during the run, so ids captured at definition time are already stale.
    fn scene(&self, op: &str, video_id: &str, index: usize, project_id: &str, orientation: &str) -> String {
        let body = serde_json::json!({
            "op": op,
            "video_id": video_id,
            "scene_index": index,
            "project_id": project_id,
            "orientation": orientation,
        })
        .to_string();
        self.curl("scene", &body)
    }

    fn catchup(&self, video_id: &str, project_id: &str, orientation: &str, from_index: usize) -> String {
        let body = serde_json::json!({
            "video_id": video_id,
            "project_id": project_id,
            "orientation": orientation,
            "from_index": from_index,
        })
        .to_string();
        self.curl("catchup", &body)
    }

    fn entities(&self, project_id: &str) -> String {
        let body = serde_json::json!({ "project_id": project_id }).to_string();
        self.curl("entity", &body)
    }
}

pub struct BuildOpts<'a> {
    pub project: &'a Row,
    pub video: &'a Row,
    pub scenes: &'a [Row],
    pub orientation: &'a str,
    pub endpoint: &'a StepEndpoint,
    /// Workspace dir for the run (per project — the engine allows only one run
    /// per workspace at a time, so sharing one would serialize projects).
    pub workspace: String,
    /// Include the audio (TTS narration) stage.
    pub with_audio: bool,
    /// Include the critic QA stage.
    pub with_critic: bool,
    /// Scene slots to provision. Defaults to the current scene count; raise it
    /// when `script_parser` is expected to produce more scenes than exist now.
    pub scene_slots: usize,
}

/// Workflow name for a project — stable, so re-running overwrites the same def.
pub fn workflow_name(project_id: &str) -> String {
    format!("video-flow-{}", &project_id.replace('-', "")[..project_id.replace('-', "").len().min(12)])
}

fn yaml_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Emit the workflow markdown. Steps are all `script` kind: the work is
/// mechanical (HTTP calls into this app) and script nodes are not charged
/// against the engine's LLM-parallelism budget, so scenes really do run
/// concurrently. The LLM lives inside the app's agents, which keep their souls.
pub fn build(opts: &BuildOpts) -> String {
    let project_id = db::str_of(opts.project, "id");
    let video_id = db::str_of(opts.video, "id");
    let project_name = db::str_of(opts.project, "name");
    let ep = opts.endpoint;

    let mut steps = String::new();
    let mut push = |s: String| steps.push_str(&s);

    // ---- planning / synthesis (sequential by nature) ----
    //
    // `script_parser` DELETES every scene before repopulating, and scene rows
    // carry all rendered media. So it runs only to create scenes that do not
    // exist yet; once a video has scenes they are the input, and re-parsing
    // would destroy both the user's edits and every generated image/clip.
    // Starting from a bare story runs full pre-production first: a screenplay
    // parsed straight from a synopsis skips the narrative and shot work, which
    // is most of what makes the scenes worth rendering. Once scenes exist those
    // stages are done, so the run picks up from continuity.
    let parse_first = opts.scenes.is_empty();
    if parse_first {
        push(format!(
            "  - id: blueprint\n    kind: script\n    run: {}\n    timeout: 600\n",
            yaml_quote(&ep.agent_story("director", &project_id, &video_id))
        ));
        push(format!(
            "  - id: draft\n    kind: script\n    dependsOn: [blueprint]\n    run: {}\n    timeout: 1800\n",
            yaml_quote(&ep.agent("screenwriter", &project_id, &video_id))
        ));
        push(format!(
            "  - id: environments\n    kind: script\n    dependsOn: [draft]\n    run: {}\n    timeout: 900\n",
            yaml_quote(&ep.agent("scene_plan", &project_id, &video_id))
        ));
        push(format!(
            "  - id: shot_list\n    kind: script\n    dependsOn: [environments]\n    run: {}\n    timeout: 1200\n",
            yaml_quote(&ep.agent("shot_design", &project_id, &video_id))
        ));
        // Character DNA before parsing, so the parser's entities land on top of
        // real appearance blueprints rather than inventing their own.
        push(format!(
            "  - id: dna\n    kind: script\n    dependsOn: [draft]\n    run: {}\n    timeout: 900\n",
            yaml_quote(&ep.agent("visual_asset", &project_id, &video_id))
        ));
        push(format!(
            "  - id: parse\n    kind: script\n    dependsOn: [shot_list, dna]\n    run: {}\n    timeout: 1800\n",
            yaml_quote(&ep.parse_story(&project_id, &video_id))
        ));
    }
    let after_parse = if parse_first { "\n    dependsOn: [parse]" } else { "" };
    push(format!(
        "  - id: refs\n    kind: script{after_parse}\n    run: {}\n    timeout: 900\n",
        yaml_quote(&ep.agent("gen_ref", &project_id, &video_id))
    ));
    push(format!(
        "  - id: bridge\n    kind: script\n    dependsOn: [refs]\n    run: {}\n    timeout: 900\n",
        yaml_quote(&ep.agent("director_frame", &project_id, &video_id))
    ));
    // Reference images for every entity — must exist before scene stills.
    push(format!(
        "  - id: entities\n    kind: script\n    dependsOn: [bridge]\n    run: {}\n    timeout: 3600\n",
        yaml_quote(&ep.entities(&project_id))
    ));

    // Pre-flight GATE, not a post-mortem: rendering is the expensive step, so
    // the check that inputs are sound runs before the fan-out and fails the run
    // in seconds instead of after N clips have been paid for.
    let render_gate = if opts.with_critic {
        push(format!(
            "  - id: preflight\n    kind: script\n    dependsOn: [entities]\n    run: {}\n    timeout: 300\n",
            yaml_quote(&ep.agent("critic", &project_id, &video_id))
        ));
        "preflight"
    } else {
        "entities"
    };

    // ---- fan-out: one image + one video node per scene slot ----
    //
    // Slots are provisioned by POSITION. The count comes from the scenes that
    // exist now, but `script_parser` may produce a different number during the
    // run; a slot with no scene behind it reports "skipped" instead of failing,
    // so over-provisioning is free.
    let slots = opts.scene_slots.max(opts.scenes.len());
    let mut video_ids: Vec<String> = Vec::new();
    for i in 0..slots {
        let img = format!("img_{i}");
        let vid = format!("vid_{i}");
        push(format!(
            "  - id: {img}\n    kind: script\n    dependsOn: [{render_gate}]\n    run: {}\n    timeout: 1200\n",
            yaml_quote(&ep.scene("image", &video_id, i, &project_id, opts.orientation))
        ));
        // Depends on ITS OWN image only — this is what unlocks the parallelism.
        push(format!(
            "  - id: {vid}\n    kind: script\n    dependsOn: [{img}]\n    run: {}\n    timeout: 1800\n",
            yaml_quote(&ep.scene("video", &video_id, i, &project_id, opts.orientation))
        ));
        video_ids.push(vid);
    }

    // ---- catch-up: scenes beyond the provisioned slots ----
    //
    // `script_parser` runs inside this workflow, so it can return more scenes
    // than there are slots. Those scenes would otherwise never be rendered
    // while the run still reported success, so one serial node sweeps them up.
    let rendered = if video_ids.is_empty() {
        render_gate.to_string()
    } else {
        video_ids.join(", ")
    };
    push(format!(
        "  - id: catchup\n    kind: script\n    dependsOn: [{rendered}]\n    run: {}\n    timeout: 3600\n",
        yaml_quote(&ep.catchup(&video_id, &project_id, opts.orientation, slots))
    ));

    // ---- post: narration, local download, concat, QA ----
    let all_videos = "catchup".to_string();

    let mut post_deps = all_videos.clone();
    if opts.with_audio {
        push(format!(
            "  - id: audio\n    kind: script\n    dependsOn: [{all_videos}]\n    run: {}\n    timeout: 1800\n",
            yaml_quote(&ep.agent("audio", &project_id, &video_id))
        ));
        post_deps = "audio".to_string();
    }
    push(format!(
        "  - id: download\n    kind: script\n    dependsOn: [{post_deps}]\n    run: {}\n    timeout: 1800\n",
        yaml_quote(&ep.agent("media_download", &project_id, &video_id))
    ));
    push(format!(
        "  - id: concat\n    kind: script\n    dependsOn: [download]\n    run: {}\n    timeout: 1800\n",
        yaml_quote(&ep.agent("concat", &project_id, &video_id))
    ));

    let name = workflow_name(&project_id);
    format!(
        "---\nname: {name}\ndescription: {}\nworkspace: {}\nsteps:\n{steps}---\n\n\
# {}\n\nSinh tự động bởi Video Flow cho project `{project_id}` (video `{video_id}`, \
{} cảnh, {}).\nMỗi cảnh là một node riêng: `vid_k` chỉ phụ thuộc `img_k`, nên các cảnh \
render song song và một cảnh lỗi không kéo sập cả pipeline.\n\n\
Chạy lại workflow này sẽ bỏ qua phần đã xong (các bước đều idempotent).\n",
        yaml_quote(&format!("Video Flow — {project_name}")),
        yaml_quote(&opts.workspace),
        project_name,
        opts.scenes.len(),
        opts.orientation,
    )
}

/// Options for a user-authored (custom-stage) workflow.
pub struct CustomBuildOpts<'a> {
    pub project: &'a Row,
    pub video: &'a Row,
    pub scenes: &'a [Row],
    pub orientation: &'a str,
    pub endpoint: &'a StepEndpoint,
    pub workspace: String,
    /// Per-scene slots for `image`/`video` fan-out (usually scene count, floored).
    pub scene_slots: usize,
}

/// Agent types the builder knows how to place. Anything here is safe to drop
/// into a stage; unknown names are rejected by the caller.
pub fn known_agent_types() -> &'static [&'static str] {
    &[
        "director", "screenwriter", "scene_plan", "shot_design", "visual_asset",
        "scene_builder", "script_parser", "gen_ref", "director_frame", "character",
        "image", "video", "audio", "media_download", "concat", "critic",
    ]
}

/// Build a workflow from user-chosen **stages**. Agents inside one stage run in
/// PARALLEL; each stage depends on the whole previous stage, so stages run in
/// SEQUENCE. The scene-level agents `image`/`video` fan out one node per scene
/// slot; a `video` node also waits on its own scene's `image` node when one
/// exists (even within the same stage), keeping per-scene correctness.
pub fn build_custom(stages: &[Vec<String>], opts: &CustomBuildOpts) -> String {
    let project_id = db::str_of(opts.project, "id");
    let video_id = db::str_of(opts.video, "id");
    let project_name = db::str_of(opts.project, "name");
    let ep = opts.endpoint;
    let slots = opts.scene_slots.max(opts.scenes.len()).max(1);

    let dep_clause = |ids: &[String]| -> String {
        if ids.is_empty() {
            String::new()
        } else {
            format!("\n    dependsOn: [{}]", ids.join(", "))
        }
    };

    let mut steps = String::new();
    let mut prev_ids: Vec<String> = Vec::new();
    let mut img_by_scene: std::collections::HashMap<usize, String> = Default::default();
    let mut stage_count = 0usize;

    for (si, stage) in stages.iter().enumerate() {
        let mut this_ids: Vec<String> = Vec::new();
        // Within a stage, emit image nodes before video nodes so a video can bind
        // its matching image; everything else is order-independent (parallel).
        // Dedup within the stage first: the node id is keyed only by (agent,
        // stage), so a repeated agent would emit a colliding id and the daemon
        // rejects the whole definition ("duplicate step id"). Running the same
        // agent twice in one parallel stage is meaningless anyway.
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut ordered: Vec<&String> = stage
            .iter()
            .filter(|a| {
                let t = a.trim();
                !t.is_empty() && seen.insert(t)
            })
            .collect();
        ordered.sort_by_key(|a| match a.trim() {
            "image" => 0,
            "video" => 2,
            _ => 1,
        });

        for agent in ordered {
            let a = agent.trim();
            if a.is_empty() {
                continue;
            }
            match a {
                "image" | "video" => {
                    for i in 0..slots {
                        let id = format!("{a}_{si}_{i}");
                        let mut deps = prev_ids.clone();
                        if a == "video" {
                            if let Some(img) = img_by_scene.get(&i) {
                                deps.push(img.clone());
                            }
                        }
                        steps.push_str(&format!(
                            "  - id: {id}\n    kind: script{}\n    run: {}\n    timeout: 1800\n",
                            dep_clause(&deps),
                            yaml_quote(&ep.scene(a, &video_id, i, &project_id, opts.orientation)),
                        ));
                        if a == "image" {
                            img_by_scene.insert(i, id.clone());
                        }
                        this_ids.push(id);
                    }
                }
                "character" => {
                    let id = format!("character_{si}");
                    steps.push_str(&format!(
                        "  - id: {id}\n    kind: script{}\n    run: {}\n    timeout: 3600\n",
                        dep_clause(&prev_ids),
                        yaml_quote(&ep.entities(&project_id)),
                    ));
                    this_ids.push(id);
                }
                "script_parser" => {
                    let id = format!("script_parser_{si}");
                    steps.push_str(&format!(
                        "  - id: {id}\n    kind: script{}\n    run: {}\n    timeout: 1800\n",
                        dep_clause(&prev_ids),
                        yaml_quote(&ep.parse_story(&project_id, &video_id)),
                    ));
                    this_ids.push(id);
                }
                "director" => {
                    let id = format!("director_{si}");
                    steps.push_str(&format!(
                        "  - id: {id}\n    kind: script{}\n    run: {}\n    timeout: 900\n",
                        dep_clause(&prev_ids),
                        yaml_quote(&ep.agent_story("director", &project_id, &video_id)),
                    ));
                    this_ids.push(id);
                }
                other => {
                    let id = format!("{other}_{si}");
                    steps.push_str(&format!(
                        "  - id: {id}\n    kind: script{}\n    run: {}\n    timeout: 1800\n",
                        dep_clause(&prev_ids),
                        yaml_quote(&ep.agent(other, &project_id, &video_id)),
                    ));
                    this_ids.push(id);
                }
            }
        }
        if !this_ids.is_empty() {
            prev_ids = this_ids;
            stage_count += 1;
        }
    }

    let name = workflow_name(&project_id);
    format!(
        "---\nname: {name}\ndescription: {}\nworkspace: {}\nsteps:\n{steps}---\n\n\
# {}\n\nWorkflow tùy chỉnh do người dùng dựng cho project `{project_id}` (video \
`{video_id}`, {stage_count} stage, {} cảnh, {}).\nMỗi stage chạy song song bên trong, \
các stage nối tiếp nhau. `video_*` chờ `image_*` cùng cảnh.\n\n\
Chạy lại sẽ bỏ qua phần đã xong (các bước idempotent).\n",
        yaml_quote(&format!("Video Flow (tùy chỉnh) — {project_name}")),
        yaml_quote(&opts.workspace),
        project_name,
        opts.scenes.len(),
        opts.orientation,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(pairs: &[(&str, &str)]) -> Row {
        let mut m = Row::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), json!(v));
        }
        m
    }

    fn sample(n: usize) -> String {
        let project = row(&[("id", "1126ef9e-99e3-449b-af47-a30565f8d60c"), ("name", "Khói Bếp")]);
        let video = row(&[("id", "v-1")]);
        let scenes: Vec<Row> = (0..n).map(|i| row(&[("id", &format!("s-{i}")[..])])).collect();
        let ep = StepEndpoint { base_url: "http://127.0.0.1:4460".into() };
        build(&BuildOpts {
            project: &project,
            video: &video,
            scenes: &scenes,
            orientation: "VERTICAL",
            endpoint: &ep,
            workspace: "/tmp/wf/p1".into(),
            with_audio: false,
            with_critic: true,
            scene_slots: n,
        })
    }

    fn custom(stages: &[&[&str]], n: usize) -> String {
        let project = row(&[("id", "1126ef9e-99e3-449b-af47-a30565f8d60c"), ("name", "Khói Bếp")]);
        let video = row(&[("id", "v-1")]);
        let scenes: Vec<Row> = (0..n).map(|i| row(&[("id", &format!("s-{i}")[..])])).collect();
        let ep = StepEndpoint { base_url: "http://127.0.0.1:4460".into() };
        let stages: Vec<Vec<String>> = stages
            .iter()
            .map(|s| s.iter().map(|a| a.to_string()).collect())
            .collect();
        build_custom(
            &stages,
            &CustomBuildOpts {
                project: &project,
                video: &video,
                scenes: &scenes,
                orientation: "VERTICAL",
                endpoint: &ep,
                workspace: "/tmp/wf/p1".into(),
                scene_slots: n,
            },
        )
    }

    /// Stages: agents inside one stage run in parallel (no deps between them);
    /// each stage depends on the whole previous stage.
    #[test]
    fn custom_stages_sequence_and_parallel() {
        // director → (screenwriter ∥ scene_plan) → critic
        let def = custom(&[&["director"], &["screenwriter", "scene_plan"], &["critic"]], 2);
        // Stage 0 has no deps.
        assert!(def.contains("- id: director_0\n    kind: script\n    run:"));
        // Stage 1 agents both depend on stage 0's only node, and NOT on each other.
        assert!(def.contains("- id: screenwriter_1\n    kind: script\n    dependsOn: [director_0]"));
        assert!(def.contains("- id: scene_plan_1\n    kind: script\n    dependsOn: [director_0]"));
        // Stage 2 depends on BOTH stage-1 nodes.
        assert!(def.contains("dependsOn: [screenwriter_1, scene_plan_1]"));
    }

    /// A repeated agent within one stage must be deduped, not emit colliding ids
    /// (which the daemon rejects with "duplicate step id", failing the whole run).
    #[test]
    fn custom_dedups_repeated_agent_in_stage() {
        let def = custom(&[&["critic", "critic"]], 2);
        assert_eq!(def.matches("- id: critic_0").count(), 1, "duplicate agent not deduped");
        let vid = custom(&[&["image", "image"]], 2);
        // Two "image" collapse to one fan-out (image_0_0, image_0_1), not four.
        assert_eq!(vid.matches("- id: image_0_").count(), 2);
    }

    /// `image`/`video` fan out per scene; a `video` node waits on its own image.
    #[test]
    fn custom_scene_fanout_binds_video_to_image() {
        let def = custom(&[&["image", "video"]], 3);
        for i in 0..3 {
            assert!(def.contains(&format!("- id: image_0_{i}")), "missing image_0_{i}");
            assert!(def.contains(&format!("- id: video_0_{i}")), "missing video_0_{i}");
            // video_0_i depends on image_0_i (same scene), even in the same stage.
            assert!(
                def.contains(&format!("- id: video_0_{i}\n    kind: script\n    dependsOn: [image_0_{i}]")),
                "video_0_{i} not bound to its image"
            );
        }
    }

    /// `script_parser` purges every scene before repopulating, and scene rows
    /// hold the rendered media — so a video that already has scenes must never
    /// be re-parsed by a pipeline run.
    /// Starting from a bare story must run pre-production, not jump straight to
    /// parsing a synopsis — that skipped the narrative and shot work entirely.
    #[test]
    fn bare_story_runs_full_preproduction() {
        let def = sample(0);
        for stage in ["blueprint", "draft", "environments", "shot_list", "dna", "parse"] {
            assert!(def.contains(&format!("- id: {stage}")), "missing stage {stage}");
        }
        // Ordering that matters: shots need environments; parse needs both the
        // shot list and the character DNA.
        assert!(def.contains("- id: environments\n    kind: script\n    dependsOn: [draft]"));
        assert!(def.contains("- id: shot_list\n    kind: script\n    dependsOn: [environments]"));
        assert!(def.contains("dependsOn: [shot_list, dna]"));
        // The director works from the project story, not an upstream stage.
        assert!(def.contains("use_project_story"));
    }

    #[test]
    fn does_not_reparse_when_scenes_exist() {
        let def = sample(3);
        assert!(!def.contains("- id: parse"), "parse must be skipped when scenes exist");
        assert!(def.contains("- id: refs\n    kind: script\n    run:"), "refs should be the entry node");
        assert!(!def.contains("dependsOn: [parse]"));
    }

    /// An empty video is the one case where parsing is correct — and it must be
    /// fed the project's real story, not the `{"video_id":…}` stub.
    #[test]
    fn parses_only_when_there_are_no_scenes() {
        let def = sample(0);
        assert!(def.contains("- id: parse"));
        assert!(def.contains("use_project_story"));
        assert!(def.contains("dependsOn: [parse]"));
    }

    #[test]
    fn emits_one_node_per_scene() {
        let def = sample(3);
        for i in 0..3 {
            assert!(def.contains(&format!("- id: img_{i}")), "missing img_{i}");
            assert!(def.contains(&format!("- id: vid_{i}")), "missing vid_{i}");
        }
        assert!(!def.contains("img_3"));
    }

    /// `script_parser` can produce more scenes than there are slots; a `catchup`
    /// node past the last slot renders the surplus so nothing sits at PENDING
    /// while the run reports success. It must gate the whole post-production tail.
    #[test]
    fn catchup_covers_scenes_beyond_slots() {
        let def = sample(3);
        assert!(def.contains("- id: catchup"), "missing catchup node");
        // Body is JSON embedded in a yaml-quoted curl line, so quotes are
        // escaped; keys are alphabetical (from_index first). 3 = the slot count.
        assert!(def.contains(r#"from_index\":3"#), "catchup must start at the slot count (3)");
        // Everything downstream of the fan-out hangs off catchup, not the last vid.
        assert!(def.contains("- id: download\n    kind: script\n    dependsOn: [catchup]"));
    }

    /// The whole point of the refactor: a scene's video waits on its own image,
    /// not on every image, so scenes overlap instead of running in lockstep.
    #[test]
    fn video_depends_only_on_its_own_image() {
        let def = sample(3);
        assert!(def.contains("- id: vid_1\n    kind: script\n    dependsOn: [img_1]"));
        assert!(!def.contains("dependsOn: [img_0, img_1"));
        // …and every image fans out from the same single upstream gate.
        assert_eq!(def.matches("dependsOn: [preflight]").count(), 3);
    }

    #[test]
    fn concat_waits_for_every_scene() {
        let def = sample(3);
        assert!(def.contains("dependsOn: [vid_0, vid_1, vid_2]"));
        assert!(def.contains("- id: concat"));
    }

    /// Checking the inputs AFTER paying for N clips is worthless — the gate
    /// must sit between the reference images and the render fan-out.
    #[test]
    fn preflight_gates_the_render_fanout() {
        let def = sample(3);
        assert!(def.contains("- id: preflight"), "preflight node missing");
        assert!(def.contains("- id: preflight\n    kind: script\n    dependsOn: [entities]"));
        // Every image node waits on the gate, not on `entities` directly.
        assert_eq!(def.matches("dependsOn: [preflight]").count(), 3);
        assert!(!def.contains("dependsOn: [concat]"), "critic must not run after concat");
    }

    #[test]
    fn audio_stage_is_optional() {
        let def = sample(2);
        assert!(!def.contains("- id: audio"));
    }

    #[test]
    fn no_scenes_still_produces_a_valid_chain() {
        let def = sample(0);
        assert!(def.contains("- id: download"));
        assert!(def.contains("dependsOn: [entities]"));
    }

    /// Single quotes in a prompt must not break out of the shell string.
    #[test]
    fn curl_body_is_shell_safe() {
        let ep = StepEndpoint { base_url: "http://127.0.0.1:4460".into() };
        let cmd = ep.curl("agent", r#"{"prompt":"Hậu's home"}"#);
        assert!(cmd.contains(r#"'\''"#), "single quote not escaped: {cmd}");
    }

    #[test]
    fn workflow_name_is_stable_and_safe() {
        let a = workflow_name("1126ef9e-99e3-449b-af47-a30565f8d60c");
        assert_eq!(a, workflow_name("1126ef9e-99e3-449b-af47-a30565f8d60c"));
        assert!(!a.contains('-') || a.starts_with("video-flow-"));
    }
}
