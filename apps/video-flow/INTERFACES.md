# video-flow — module interface contract (for parallel porting)

Port of `/Users/benji/Projects/flow-agent-video/backend` (Go) into this Rust
axum Space App. **Read this before writing your module.** Foundation modules
already written (do NOT rewrite them; code against them):

- `src/db.rs` — `Db` (Clone): `open`, `open_memory`, `insert(table,&Row)->String id`,
  `update(table,id,&Row)`, `get(table,id)->Option<Row>`, `delete(table,id)->usize`,
  `query(sql,&[&dyn ToSql])->Vec<Row>`, `query_one`, `execute`, `kv_get(&str)->String`,
  `kv_set`, `builtin_agent_disabled(&str)->bool`, `cascade_after_image(scene_id,ori)`,
  `cascade_after_video`, `delete_pipeline_cascade(pipeline_id,project_id)`.
  `Row = serde_json::Map<String,Value>`. Helpers: `db::now()`, `db::new_id()`,
  `db::str_of(&Row,&str)->String`, `db::i64_of`, `db::ori_prefix(&str)->&'static str`,
  `db::scene_cols(ori)->SceneCols{image_url,image_media_id,image_status,video_url,
  video_media_id,video_status,upscale_url,upscale_media_id,upscale_status,end_scene_media_id}`
  (all String column names).
- `src/llm.rs` — SenClaw daemon bridge (NO provider layer):
  `async complete(system,user,max_tokens)->Result<(String text,String model),String>`,
  `async bridge_llm(...)->Result<(text,model,finish),String>`,
  `async agent_run(system,prompt,space,&[String] tools,timeout)->Result<String,String>`,
  `async list_models()->Result<Value,String>`, `set_profile/profile`,
  `parse_json::<T>(&str)->Result<T,String>` (fence-stripping + truncation repair),
  `parse_value`, `strip_fences`, `truncate(&str,n)`.
- `src/config.rs` — env accessors: `http_port()`, `ws_port()`, `worker_enabled()`,
  `worker_poll_secs()`, `worker_gen_timeout_secs()`, `video_poll_secs()`,
  `video_poll_timeout_secs()`, `google_flow_api()`, `google_api_key()`,
  `default_orientation()`, `exec_allowlist()`, `exec_timeout_secs()`,
  `http_tools_allow_private()`, `data_dir()`, `db_path()`, `media_dir()`,
  `souls_dir()`, `playbooks_dir()`.
- `src/dashws.rs` — `DashHub` (Clone): `emit(type,serde_json::Value)`, `serve(WebSocket)`.
- `src/extbridge.rs` — `ExtBridge` (Clone): `is_connected()`, `stats()->Value`,
  `register_pending(id)->oneshot::Receiver<Value>`, `cancel_pending(id)`,
  `complete_callback(id,Value)->bool`, `send(id,method,params)->Result<(),String>`,
  `async call(method,params,Duration)->Result<Value,String>`,
  `set_event_handler(Fn(Value))`. Server: `extbridge::serve_ws(bridge,port)`.
- `src/souls.rs` — `load(&PathBuf,agent_type)->String` (frontmatter stripped),
  `load_raw`, `write`, `or_default(soul,fallback)`, `canonical_basename`.
- `src/context.rs` — `AgentContext{working: WorkingContext, memory: MemoryManager,
  soul: String, parent_id, project_id}`; `WorkingContext::{set_result,get_result,
  all_results,inject_into_prompt}`; `MemoryManager::{project_summary,list_characters,
  search_scenes}`.
- `src/state.rs` — `Core{db,dash,ext,souls_dir,playbooks_dir,media_dir}`,
  `AppState{core:Arc<Core>, pool:Arc<agents::Pool>, engine:Arc<dag::Engine>,
  mcp_tx: broadcast::Sender<String>}` (Clone).
- `src/agents/mod.rs` — `Task{id,label,agent_type,prompt,timeout_seconds,
  upstream_results:HashMap<String,String>}`, `TaskResult{data:Map<String,Value>,
  summary:String}`, `#[async_trait] trait Agent{agent_type,name,description,
  default_system, async execute(&self,&mut AgentContext,&Task)->Result<TaskResult,String>}`,
  `Pool::{new(Arc<Core>)->Arc<Pool>, register, unregister, get, list_info->Vec<AgentInfo>,
  async execute(&Task,parent_id,project_id)->Result<TaskResult,String>,
  system_prompt(agent_type)->String, builtin_order:Vec<&str>, core:Arc<Core>}`.
  `AgentInfo{agent_type,name,description,kind}`.

## Modules to be written (owners; exact public API expected)

### src/process.rs (owner: process+worker agent)
Port of `internal/agent/process/*`. Google Flow envelope building + execution
over `core.ext` (`api_request` method with `{url,method,headers,body,captchaAction}`
params, awaited via `ext.call`). Public API:
```rust
pub struct GenOutcome { pub media_id: String, pub url: String }
pub async fn entity_image(core:&Core, character_id:&str, project_id:&str, regenerate:bool) -> Result<GenOutcome,String>;
pub async fn scene_image(core:&Core, scene_id:&str, project_id:&str, orientation:&str, regenerate:bool, edit_prompt:Option<&str>) -> Result<GenOutcome,String>;
pub async fn scene_video(core:&Core, scene_id:&str, project_id:&str, orientation:&str, regenerate:bool) -> Result<GenOutcome,String>;
pub async fn upscale_video(core:&Core, scene_id:&str, project_id:&str, orientation:&str) -> Result<GenOutcome,String>;
pub async fn process_all_entities(core:&Core, project_id:&str) -> Result<usize,String>; // count generated
```
Each fn: updates scene/character/request rows + statuses, applies cascades
(`cascade_after_image` / `cascade_after_video`), emits dash events
(`scene_updated{project_id,scene_id}` etc.), per-scene+orientation async lock
(tokio Mutex map) against double-submit. Video is async submit→poll
(`config::video_poll_secs()` interval, `config::video_poll_timeout_secs()`).

### src/worker.rs (owner: process+worker agent)
Port of `internal/worker`. Public API:
```rust
pub fn spawn(core: Arc<Core>);                      // poll loop task
pub fn install_extension_event_handler(core: Arc<Core>); // ext.set_event_handler(...)
```
Poll `request` PENDING rows (types GENERATE_VIDEO, REGENERATE_VIDEO via
process::scene_video; UPSCALE_VIDEO via process::upscale_video or ext method
`upscale_video`), priority order, skip when `!ext.is_connected()`. Event handler
routes `token_captured`, `extension_ready`, `media_urls_refresh` → refresh scene/
character URL columns by media_id; unknown → `dash.emit("extension:event",...)`.

### src/dag.rs (owner: dag+pipeline agent)
Port of `internal/agent/dag`. Public API:
```rust
pub struct Engine { /* core, pool, active map */ }
impl Engine {
  pub fn new(core:Arc<Core>, pool:Arc<Pool>) -> Arc<Engine>;
  pub fn start(self: Arc<Self>);            // 500ms tick loop, max 5 concurrent
  pub fn stop_task(&self, task_id:&str);    // cancel running task
}
```
Lifecycle registered→active→done|error|timeout; blocked tasks (failed dep) →
error; parent queued→active→done. Emits `agent:state` + `pipeline:updated`.
Skips disabled built-ins (`db.builtin_agent_disabled`) as done/skipped.
Upstream results injected from depends_on / input_from labels only.

### src/pipeline.rs (owner: dag+pipeline agent)
Port of `internal/pipeline/manager.go` + orchestrator planning/validation/
normalization from `internal/agent/agents/orchestrator.go`. Public API:
```rust
pub struct PlannedTask { pub label:String, pub agent_type:String, pub prompt:String,
  pub depends_on:Vec<String>, pub input_from:Vec<String>, pub timeout_seconds:i64 }
pub async fn create(core:&Core, pool:&Pool, project_id:&str, script:&str,
  orientation:&str, goal:&str, mode:&str) -> Result<(String pipeline_id, usize task_count),String>;
pub fn pause(core:&Core, id:&str) -> Result<(),String>;
pub fn cancel(core:&Core, id:&str) -> Result<(),String>;
pub fn start(core:&Core, id:&str) -> Result<(),String>;   // re-queue
pub fn retry_task(core:&Core, pipeline_id:&str, task_id:&str) -> Result<(),String>;
pub fn get_status(core:&Core, id:&str) -> Result<serde_json::Value,String>; // parent + tasks[]
pub async fn plan_with_llm(core:&Core, pool:&Pool, goal:&str, script:&str) -> Result<Vec<PlannedTask>,String>;
pub fn normalize_dependencies(tasks:&mut Vec<PlannedTask>, order:&[&str]);
pub fn validate_plan(tasks:&[PlannedTask], known_types:&[String]) -> Result<(),String>;
```
`create`: one active pipeline per project; template DAG for `production` /
`full` modes; LLM plan (2 attempts + validation) for `custom`/goal; persists
dag_parents + dag_tasks; injects raw script into script_parser prompt;
emits `pipeline:created`.

### src/agents/builtin.rs + src/agents/skill_agent.rs (owner: builtin-agents agent)
`pub fn register_builtins(pool:&Arc<Pool>)` registering all 17 built-ins;
`skill_agent.rs`: `pub fn load_skill_agents_from_db(pool:&Arc<Pool>)`,
`pub fn register_skill_agent(pool:&Arc<Pool>, row:&Row)`, ReAct loop (max 10
steps) over `tools::Registry`. Souls carry the real prompts (all 17 files in
`souls/`); `default_system()` may be a concise fallback.

### src/script.rs (owner: builtin-agents agent)
Port of `internal/script`. `ParsedScript{scenes:Vec<ParsedScene>,characters:Vec<ParsedCharacter>}`;
`async parse(system_override:&str, script:&str)->Result<ParsedScript,String>`;
`async parse_blocks(...)` per-scene variant.

### src/tools.rs (owner: builtin-agents agent)
Port of `internal/tool`: `ToolSpec{name,description,input_schema}`, `Registry::
{specs()->Vec<ToolSpec>, async execute(name,args:Value)->Result<Value,String>}`.
Tools: execute_cmd (allowlist ffmpeg/ffprobe, no shell, timeout), file_read/
file_write/file_list/file_read_image (sandbox = data_dir + tmp), http_get/
http_post (SSRF guard), repo_get/repo_list/repo_create/repo_update (table
allowlist: project, character, video, scene, request, dag_parents, dag_tasks).

### src/api.rs + src/media.rs + src/material.rs + src/skillcat.rs (owner: api agent)
`pub fn api_router(state: AppState) -> axum::Router` — FULL route surface from
`internal/api/server.go` (paths WITHOUT the `/api` prefix; main.rs nests under
`/api`). `/health` must ALSO be reachable: add route `/status` (health JSON) since
manifest healthPath = `/api/status`, plus `/health` alias at router root.
`media.rs`: upload (multipart, ≤500MB), serve `/media/:id/file`, probe dims
(image via image bytes sniff or ffprobe). `material.rs`: `pub fn seed(db:&Db)`
(insert-or-ignore builtins from embedded builtin.json), restore, import.
`skillcat.rs`: scan playbooks dir *.md frontmatter → `Vec<PlaybookSkill{id,name,description,body}>`.

### src/mcp.rs (owner: mcp agent)
`mcp_sse` + `mcp_message` axum handlers (JSON-RPC: initialize/ping/
notifications/initialized/tools/list/tools/call), broadcast via `state.mcp_tx`.
Tool set wraps the REST/domain layer.

## Conventions
- Handlers take `axum::extract::State<AppState>`; errors as `(StatusCode, Json<{"error":msg}>)`.
- All timestamps `db::now()` format. IDs `db::new_id()`.
- JSON field names snake_case exactly as the Go API (frontend compatibility).
- Dash event names/payloads exactly as Go (`agent:state`, `pipeline:updated`,
  `pipeline:created`, `scene_updated`, `request_completed`, `request_failed`,
  `extension:token_captured`, `extension:ready`, `extension:event`, `media_urls_refreshed`).
- LLM JSON prompts: use `llm::complete` + `llm::parse_json` (retry once with a
  "JSON only" nudge like the Go side).
- Keep Vietnamese diacritic folding logic where the Go code has it (gen_ref).
