//! Bridge to the SenClaw daemon's active LLM. The Moltbook app never talks to a
//! provider directly — every completion goes through the daemon's space-app
//! bridge. Also hosts the engine's planner/composer prompts and a tolerant JSON
//! parser for their structured replies.

use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub fn base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}
pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "moltbook".to_string())
}
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(125))
            .build()
            .expect("build http client")
    })
}

/// The LLM **profile** this app composes with — a SenClaw LLM-config label (e.g.
/// "MoltClaw") or id. Empty = follow the daemon's active model.
///
/// Held process-wide rather than threaded through every prompt helper: there is
/// exactly one molty per app instance. `api::make_state` seeds it from the DB and
/// `put_settings` updates it, so a change takes effect on the next completion
/// without a restart.
fn profile_cell() -> &'static std::sync::RwLock<String> {
    static P: OnceLock<std::sync::RwLock<String>> = OnceLock::new();
    P.get_or_init(|| std::sync::RwLock::new(String::new()))
}

pub fn set_profile(p: &str) {
    if let Ok(mut w) = profile_cell().write() {
        *w = p.trim().to_string();
    }
}

pub fn profile() -> String {
    profile_cell().read().map(|r| r.clone()).unwrap_or_default()
}

fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {s}"));
        src = s.source();
    }
    out
}

/// One completion through the daemon bridge. Returns `(text, model, finish)`
/// where `finish == "length"` means the provider cut the output at the cap.
/// Transport errors are retried; application errors are surfaced as-is.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String, String), String> {
    let url = format!("{}/api/space/apps/{}/bridge", base_url().trim_end_matches('/'), app_id());
    let mut payload = json!({ "system": system, "prompt": user, "maxTokens": max_tokens });
    // Compose on our OWN profile when one is chosen — never touch the daemon's
    // global active model. Older daemons ignore the extra field and just use
    // their active model, so this stays backward compatible.
    let p = profile();
    if !p.is_empty() {
        payload["profile"] = json!(p);
    }
    let body = json!({ "action": "llm.request", "payload": payload });
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(700 * attempt as u64)).await;
        }
        let resp = match http().post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("bridge llm.request failed ({url}): {}", describe(&e));
                continue;
            }
        };
        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("invalid bridge response: {}", describe(&e));
                continue;
            }
        };
        return match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => Ok((
                v.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("finish").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            )),
            Some("pending") => Err("bridge LLM chưa được bật trong daemon này".to_string()),
            _ => Err(v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown LLM error")
                .to_string()),
        };
    }
    Err(last_err)
}

/// Convenience: `(text, model)`, dropping the finish reason.
pub async fn complete(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    bridge_llm(system, user, max_tokens).await.map(|(t, m, _)| (t, m))
}

pub async fn list_models() -> Result<Value, String> {
    let url = format!("{}/api/llm-config", base_url().trim_end_matches('/'));
    let v: Value = http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(v)
}

// NOTE: this app deliberately does NOT expose "set the daemon's active model".
// Picking a model here must not change what every other app/chat uses — that's
// what the per-app `llm_profile` setting is for.

// ---- engine planner / composers ----

/// What the molty knows before it speaks: its own memory (knowledge space =
/// trí nhớ) and the shared wiki (kho thông tin). Both are optional — an empty
/// field is simply omitted from the prompt.
#[derive(Default)]
pub struct Grounding {
    /// Recalled from the molty's knowledge space — what it already said/learned.
    pub memory: String,
    /// Excerpts from the user's wiki — the source of truth to speak from.
    pub wiki: String,
}

impl Grounding {
    /// Render as prompt sections. Empty when nothing is grounded.
    fn render(&self) -> String {
        let mut s = String::new();
        if !self.memory.trim().is_empty() {
            s.push_str(&format!(
                "\nTRÍ NHỚ CỦA BẠN (những gì bạn đã nói/học trên Moltbook trước đây — đừng lặp lại, hãy nối tiếp):\n{}\n",
                truncate(self.memory.trim(), 1200)
            ));
        }
        if !self.wiki.trim().is_empty() {
            s.push_str(&format!(
                "\nKHO THÔNG TIN (wiki của Sếp — nguồn sự thật; ưu tiên nói từ đây, KHÔNG bịa ngoài phạm vi này):\n{}\n",
                truncate(self.wiki.trim(), 2000)
            ));
        }
        s
    }

    pub fn is_empty(&self) -> bool {
        self.memory.trim().is_empty() && self.wiki.trim().is_empty()
    }
}

/// One post the planner considered. `id` is the Moltbook post id.
pub struct FeedItem {
    pub id: String,
    pub submolt: String,
    pub author: String,
    pub title: String,
    pub content: String,
    pub score: i64,
}

/// The plan the engine acts on — post ids already resolved.
#[derive(Default)]
pub struct EngagementPlan {
    pub upvotes: Vec<String>,
    pub comments: Vec<PlanComment>,
    pub new_post: Option<PlanPost>,
    pub note: String,
}
#[derive(Clone)]
pub struct PlanComment {
    pub post_id: String,
    pub content: String,
    pub why: String,
}
#[derive(Deserialize, Clone, Default)]
pub struct PlanPost {
    #[serde(default)]
    pub submolt: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub why: String,
    /// 1-based index into the "things my human wants me to post/ask" list this
    /// post came from, so we can stamp it used and rotate to the next one.
    #[serde(default)]
    pub idea: Option<u32>,
}

/// How the human steers the molty: which subjects it should engage with, and
/// what they want it to post/ask about on Moltbook.
#[derive(Default)]
pub struct TopicSteer {
    /// Engage ONLY with posts related to `engage` — ignore the rest of the feed.
    pub focus_only: bool,
    pub engage: Vec<String>,
    /// `(topic_id, text)` — post ideas, least-recently-used first.
    pub ideas: Vec<(i64, String)>,
}

impl TopicSteer {
    fn render(&self) -> String {
        let mut s = String::new();
        if !self.engage.is_empty() {
            s.push_str(&format!("\nCHỦ ĐỀ SẾP QUAN TÂM: {}\n", self.engage.join(" · ")));
            if self.focus_only {
                s.push_str(
                    "CHỈ tương tác với bài liên quan tới các chủ đề trên. Bài không liên quan → bỏ qua \
hoàn toàn (không upvote, không bình luận). Nếu không có bài nào liên quan, trả về upvotes và \
comments RỖNG — đừng cố tương tác cho đủ chỉ tiêu.\n",
                );
            } else {
                s.push_str("Ưu tiên bài thuộc các chủ đề trên; vẫn có thể tương tác bài hay khác.\n");
            }
        }
        if !self.ideas.is_empty() {
            s.push_str("\nĐIỀU SẾP MUỐN BẠN ĐĂNG / HỎI trên Moltbook (chọn TỐI ĐA MỘT nếu bạn đăng bài; mục đầu danh sách là lâu chưa dùng nhất):\n");
            for (i, (_, text)) in self.ideas.iter().enumerate() {
                s.push_str(&format!("  idea {}: {}\n", i + 1, truncate(text, 200)));
            }
            s.push_str("Nếu bạn đăng bài dựa trên một mục, đặt \"idea\": N trong new_post.\n");
        }
        s
    }
}

/// What the model actually returns: 1-based **indices** into the numbered list we
/// showed it — never raw post ids.
///
/// Moltbook post ids are 36-char UUIDs. Making the model echo them blew the token
/// cap and truncated the JSON mid-UUID on essentially every heartbeat, and also
/// invited transcription errors. Indices are 1-3 chars and can't be mistyped into
/// a *different valid* id.
#[derive(Deserialize, Default)]
struct RawPlan {
    #[serde(default)]
    upvotes: Vec<u32>,
    #[serde(default)]
    comments: Vec<RawComment>,
    #[serde(default)]
    new_post: Option<PlanPost>,
    #[serde(default)]
    note: String,
}
#[derive(Deserialize, Clone)]
struct RawComment {
    #[serde(default)]
    post: u32,
    #[serde(default)]
    content: String,
    #[serde(default)]
    why: String,
}

// Encodes Moltbook's OFFICIAL etiquette (from moltbook.com/rules.md +
// heartbeat.md): authenticity, engagement-over-posting, reply-to-your-repliers
// first, quality-over-quantity, and the anti-spam norms.
const PLAN_SYSTEM: &str = "You are an autonomous AI agent ('molty') on Moltbook, the social \
network for AI agents. Plan this heartbeat's engagements following Moltbook's OFFICIAL etiquette:\n\
- Authenticity: engage because you have something to say — never to farm karma or 'be seen'.\n\
- Engagement OVER posting: replying, upvoting and commenting is almost always more valuable than \
a new post.\n\
- TOP PRIORITY: reply to molties who replied to YOU (the 'people replied to you' list) BEFORE \
anything else — people are talking to you.\n\
- Quality over quantity: NO one-word comments, NO emoji spam, NO duplicates, NO low-effort filler. \
A comment must add a real perspective, question, or build (1-3 sentences), in the SAME language as \
the post. Upvote generously but only what you genuinely find good.\n\
- Post at most ONE new thing, and only if it is genuinely valuable; otherwise leave new_post null. \
Never post out of obligation.\n\
- If a TRÍ NHỚ (memory) section is given, it is what you already said before — build on it, never \
repeat it, and stay consistent with it.\n\
- If a KHO THÔNG TIN (wiki) section is given, it is your human's source of truth — ground what you \
say in it and never contradict it. Do not invent facts beyond it.\n\
Refer to posts ONLY by their number (the #N in the list) — NEVER copy a post id.\n\
Return ONLY valid JSON (no prose, no code fences, no markdown) in EXACTLY this shape:\n\
{\"upvotes\":[1,4],\"comments\":[{\"post\":2,\"content\":\"...\",\"why\":\"1 short reason\"}],\"new_post\":{\"submolt\":\"...\",\"title\":\"...\",\"content\":\"...\",\"why\":\"...\",\"idea\":1},\"note\":\"1 sentence summary\"}\n\
Use \"new_post\": null when you have nothing worth posting. Only use numbers shown in the list. \
Respect the engagement budget. Keep it compact — no reasoning outside the JSON.";

/// Ask the LLM to plan this heartbeat's engagements. `priority` is the list of
/// `(post_id, snippet)` where someone replied to one of YOUR posts — the #1
/// heartbeat action per Moltbook, so it's surfaced first and its ids are valid
/// comment targets even though they aren't in the browse feed.
pub async fn plan_engagements(
    voice: &str,
    items: &[FeedItem],
    priority: &[(String, String)],
    grounding: &Grounding,
    topics: &TopicSteer,
    comment_budget: i64,
    default_submolt: &str,
    allow_new_post: bool,
) -> Result<(EngagementPlan, String), String> {
    // One numbered list the model refers to by #N: your repliers first (top
    // priority), then the browse feed. `is_own` marks your own posts — you reply
    // to those but never upvote them.
    struct Target {
        post_id: String,
        is_own: bool,
    }
    let targets: Vec<Target> = priority
        .iter()
        .take(10)
        .map(|(pid, _)| Target { post_id: pid.clone(), is_own: true })
        .chain(items.iter().take(25).map(|it| Target { post_id: it.id.clone(), is_own: false }))
        .collect();

    let mut prompt = String::new();
    prompt.push_str(&format!("Your persona/voice:\n{voice}\n"));
    prompt.push_str(&grounding.render());
    prompt.push_str(&topics.render());
    prompt.push('\n');
    prompt.push_str(&format!(
        "Engagement budget for this check-in: up to {comment_budget} comment(s), a few upvotes, and {} new post. Default submolt for a new post: m/{}.\n\n",
        if allow_new_post { "at most ONE" } else { "ZERO (skip new_post, set it null)" },
        default_submolt.trim_start_matches("m/"),
    ));
    let mut n = 0usize;
    if !priority.is_empty() {
        prompt.push_str("People replied to YOU here — respond to these FIRST (they count against your comment budget):\n");
        for (_, snippet) in priority.iter().take(10) {
            n += 1;
            prompt.push_str(&format!("#{n} (reply on your post): {}\n", truncate(snippet, 300)));
        }
        prompt.push('\n');
    }
    prompt.push_str("Browse feed:\n");
    if items.is_empty() {
        prompt.push_str("(the feed is empty right now)\n");
    }
    for it in items.iter().take(25) {
        n += 1;
        prompt.push_str(&format!(
            "#{n} · {} · by {} · score {}\n{}\n{}\n\n",
            it.submolt,
            it.author,
            it.score,
            it.title,
            truncate(&it.content, 400),
        ));
    }
    prompt.push_str("Return the JSON plan now (refer to posts by #N only).");

    // One retry with a blunt nudge: models occasionally wrap the JSON in prose or
    // get cut mid-value, and a second ask is cheaper than losing the whole tick.
    let mut last_err = String::new();
    let mut last_text = String::new();
    let mut out: Option<(RawPlan, String)> = None;
    for attempt in 0..2u8 {
        let p = if attempt == 0 {
            prompt.clone()
        } else {
            format!("{prompt}\n\nIMPORTANT: your previous reply was not valid JSON ({last_err}). Reply with the JSON object ONLY — no prose, no code fences, and keep it short.")
        };
        let (text, model) = complete(PLAN_SYSTEM, &p, 2000).await?;
        match parse_json::<RawPlan>(&text) {
            Ok(raw) => {
                out = Some((raw, model));
                break;
            }
            Err(e) => {
                last_err = e;
                last_text = text;
            }
        }
    }
    let (raw, model) = out.ok_or_else(|| {
        format!("could not parse engagement plan ({last_err}):\n{}", truncate(&last_text, 300))
    })?;

    // Resolve #N → post_id, dropping anything out of range. Enforce the budget and
    // the new-post rule here rather than trusting the model to respect them.
    let at = |i: u32| -> Option<&Target> {
        if i == 0 { None } else { targets.get((i - 1) as usize) }
    };
    let mut plan = EngagementPlan { note: raw.note, ..Default::default() };
    for i in raw.upvotes {
        if let Some(t) = at(i) {
            if !t.is_own && !plan.upvotes.contains(&t.post_id) {
                plan.upvotes.push(t.post_id.clone());
            }
        }
    }
    for c in raw.comments {
        if c.content.trim().is_empty() {
            continue;
        }
        if let Some(t) = at(c.post) {
            plan.comments.push(PlanComment { post_id: t.post_id.clone(), content: c.content, why: c.why });
        }
    }
    plan.comments.truncate(comment_budget.max(0) as usize);
    plan.new_post = if allow_new_post { raw.new_post } else { None };
    Ok((plan, model))
}

const REPLY_SYSTEM: &str = "You are an AI agent on Moltbook writing a reply to another agent's post. \
Write a single substantive comment (1-4 sentences) that genuinely adds to the conversation — a \
question, a counterpoint, a build, or a specific experience. Match the post's language. No \
sycophancy, no filler, no hashtags, no sign-off. \
If a TRÍ NHỚ (memory) section is given, stay consistent with it and build on it rather than \
repeating yourself. If a KHO THÔNG TIN (wiki) section is given, ground your reply in it — speak \
from that real knowledge and never contradict or invent beyond it. \
Return ONLY the comment text.";

/// Draft a single reply to one post. `instruction` is optional extra steer.
pub async fn compose_reply(
    voice: &str,
    post_title: &str,
    post_content: &str,
    instruction: &str,
    grounding: &Grounding,
) -> Result<(String, String), String> {
    let mut prompt = format!("Your voice:\n{voice}\n{}\nThe post you're replying to:\nTitle: {post_title}\n{post_content}\n", grounding.render());
    if !instruction.trim().is_empty() {
        prompt.push_str(&format!("\nExtra guidance from your human: {}\n", instruction.trim()));
    }
    prompt.push_str("\nWrite the comment now.");
    complete(REPLY_SYSTEM, &prompt, 500).await.map(|(t, m)| (t.trim().to_string(), m))
}

const POST_SYSTEM: &str = "You are an AI agent on Moltbook drafting an original post. Write \
something worth other agents' time: a genuine observation, a lesson learned, a useful pattern, or \
an honest question. \
If a KHO THÔNG TIN (wiki) section is given, build the post from that real knowledge — it is your \
human's source of truth; never invent facts beyond it. If a TRÍ NHỚ (memory) section is given, do \
not re-post something you already said. \
Return ONLY valid JSON (no prose, no code fences): \
{\"title\":\"<max 300 chars, no clickbait>\",\"content\":\"<the body, plain text>\"}. \
Match the language of the topic if one is given, else write in English.";

pub struct DraftedPost {
    pub title: String,
    pub content: String,
}

/// Draft a brand-new post (title + content) for a submolt around an optional topic.
pub async fn compose_post(
    voice: &str,
    submolt: &str,
    topic: &str,
    grounding: &Grounding,
) -> Result<(DraftedPost, String), String> {
    let mut prompt = format!(
        "Your voice:\n{voice}\n{}\nSubmolt: m/{}\n",
        grounding.render(),
        submolt.trim_start_matches("m/")
    );
    if topic.trim().is_empty() {
        prompt.push_str("Topic: your choice — something you genuinely want to share right now.\n");
    } else {
        prompt.push_str(&format!("Topic: {}\n", topic.trim()));
    }
    prompt.push_str("\nReturn the JSON now.");
    let (text, model) = complete(POST_SYSTEM, &prompt, 900).await?;
    #[derive(Deserialize)]
    struct Out {
        #[serde(default)]
        title: String,
        #[serde(default)]
        content: String,
    }
    let out: Out = parse_json(&text)
        .map_err(|e| format!("could not parse drafted post ({e}):\n{}", truncate(&text, 300)))?;
    Ok((DraftedPost { title: out.title.trim().to_string(), content: out.content.trim().to_string() }, model))
}

// ---- trending analysis ----

/// One theme the agent internet is currently talking about.
pub struct TrendingTopic {
    pub name: String,
    pub why: String,
    pub takeaway: String,
    /// Posts belonging to this theme, resolved from the model's `#N` indices.
    pub posts: Vec<usize>,
    /// Matches something the user told us they care about.
    pub relevant: bool,
}

pub struct TrendingReport {
    pub summary: String,
    pub topics: Vec<TrendingTopic>,
}

/// Model output — posts referenced by index, never by UUID (see the planner).
#[derive(Deserialize, Default)]
struct RawTrending {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    topics: Vec<RawTrendingTopic>,
}
#[derive(Deserialize)]
struct RawTrendingTopic {
    #[serde(default)]
    name: String,
    #[serde(default)]
    why: String,
    #[serde(default)]
    takeaway: String,
    #[serde(default)]
    posts: Vec<u32>,
    #[serde(default)]
    relevant: bool,
}

const TRENDING_SYSTEM: &str = "You are an analyst reading a slice of Moltbook — the social network \
where AI agents talk to each other. Given the currently hot/top/rising posts, identify what the \
agent internet is actually TALKING ABOUT right now and turn it into a briefing for your human.\n\
- Group posts into 3-7 real THEMES. A theme is a substantive shared subject (e.g. 'agent memory as \
a process, not a store'), never a generic bucket like 'AI' or 'discussion'.\n\
- For each theme: why it's getting traction now, and the concrete takeaway your human should \
remember. Ground both in the actual posts — do NOT invent claims no post makes.\n\
- Mark relevant=true only if the theme genuinely matches the human's stated interests (given below).\n\
- Refer to posts ONLY by their number (#N). NEVER copy a post id.\n\
- Write in the SAME language as the majority of the posts.\n\
Return ONLY valid JSON (no prose, no code fences) in EXACTLY this shape:\n\
{\"summary\":\"2-3 sentences on the overall mood/direction\",\"topics\":[{\"name\":\"...\",\"why\":\"...\",\"takeaway\":\"...\",\"posts\":[1,5],\"relevant\":false}]}";

/// Cluster a feed slice into trending themes. `interests` are the user's own
/// topics, used only to flag relevance.
pub async fn analyze_trending(
    posts: &[FeedItem],
    interests: &[String],
) -> Result<(TrendingReport, String), String> {
    if posts.is_empty() {
        return Ok((TrendingReport { summary: String::new(), topics: Vec::new() }, String::new()));
    }
    let mut prompt = String::new();
    if interests.is_empty() {
        prompt.push_str("Chủ đề human quan tâm: (chưa khai báo — đặt relevant=false cho tất cả)\n\n");
    } else {
        prompt.push_str(&format!("Chủ đề human quan tâm: {}\n\n", interests.join(" · ")));
    }
    prompt.push_str("Các bài đang nóng trên Moltbook:\n");
    for (i, p) in posts.iter().enumerate() {
        prompt.push_str(&format!(
            "#{} · {} · by {} · {} điểm\n{}\n{}\n\n",
            i + 1,
            p.submolt,
            p.author,
            p.score,
            p.title,
            truncate(&p.content, 200),
        ));
    }
    prompt.push_str("Trả JSON ngay (chỉ tham chiếu bài bằng #N). Viết GỌN — không xuống dòng thừa.");

    // Retry once on a bad/empty parse. Every field is `#[serde(default)]`, so a
    // reply truncated at the token cap can repair down to `{}` and parse
    // "successfully" with zero topics — a silent no-op that looks like "nothing
    // is trending". Treat empty topics as a failure and say why.
    let mut last_err = String::new();
    let mut last_text = String::new();
    // Best result so far, even if imperfect — beats returning nothing.
    let mut best: Option<(RawTrending, String)> = None;
    const ATTEMPTS: u8 = 2;
    for attempt in 0..ATTEMPTS {
        let p = if attempt == 0 {
            prompt.clone()
        } else {
            format!(
                "{prompt}\n\nLƯU Ý: lần trước {last_err}. Trả JSON THẬT NGẮN: tối đa 4 chủ đề, \
mỗi trường tối đa 20 từ, không xuống dòng, không khoảng trắng thừa. Nhớ điền \"posts\":[#N]."
            )
        };
        // `finish == "length"` is the provider telling us it cut the reply at the
        // token cap. Ignoring it is how we ended up with a single half-built
        // theme and no post references; use it to trigger a tighter retry.
        let (text, model, finish) = bridge_llm(TRENDING_SYSTEM, &p, 3200).await?;
        let truncated = finish == "length";
        match parse_json::<RawTrending>(&text) {
            Ok(r) if !r.topics.is_empty() => {
                let complete_enough = !truncated && r.topics.iter().any(|t| !t.posts.is_empty());
                let is_last = attempt == ATTEMPTS - 1;
                // Keep the richer of the two attempts.
                if best.as_ref().map_or(true, |(b, _): &(RawTrending, String)| r.topics.len() > b.topics.len()) {
                    best = Some((r, model));
                }
                if complete_enough || is_last {
                    break;
                }
                last_err = "bị cắt vì hết token nên thiếu chủ đề/bài liên quan".into();
            }
            Ok(_) => {
                last_err = if truncated {
                    "bị cắt vì hết token".into()
                } else {
                    "trả về rỗng".into()
                };
                last_text = text;
            }
            Err(e) => {
                last_err = e;
                last_text = text;
            }
        }
    }
    let (raw, model) = best.ok_or_else(|| {
        format!("không phân tích được xu hướng ({last_err}):\n{}", truncate(&last_text, 300))
    })?;

    // Resolve #N → 0-based indices, dropping anything out of range.
    let topics = raw
        .topics
        .into_iter()
        .filter(|t| !t.name.trim().is_empty())
        .map(|t| TrendingTopic {
            name: t.name,
            why: t.why,
            takeaway: t.takeaway,
            posts: t
                .posts
                .into_iter()
                .filter(|i| *i >= 1 && (*i as usize) <= posts.len())
                .map(|i| (i - 1) as usize)
                .collect(),
            relevant: t.relevant,
        })
        .collect();
    Ok((TrendingReport { summary: raw.summary, topics }, model))
}

const FEEDBACK_SYSTEM: &str = "You are given a post an AI agent published on Moltbook and the \
comments OTHER AI agents left on it. Synthesise the discussion for the author's knowledge base. \
Write in the SAME language as the post. Use this exact markdown skeleton, omitting any section \
that has nothing real in it:\n\
**Tóm tắt:** 1-2 sentences on where the discussion landed.\n\
**Đồng tình:** what others confirmed or built on (cite the agent's name).\n\
**Phản biện:** disagreements or counter-examples raised (cite who).\n\
**Câu hỏi mở:** questions asked that are still unanswered.\n\
**Cần cập nhật:** concrete corrections/additions the original claim needs in light of the feedback \
— or 'không có' if the post holds up.\n\
Rules: ground EVERY point in an actual comment — never invent agreement or objections that aren't \
there. Attribute by the commenter's name. If the comments are trivial (greetings, one-word praise), \
say so plainly instead of inflating them. No preface, no code fences.";

/// Synthesise what other agents said about one of our posts. `comments` is
/// `(author, content)`.
pub async fn synthesize_feedback(
    post_title: &str,
    post_content: &str,
    comments: &[(String, String)],
) -> Result<(String, String), String> {
    if comments.is_empty() {
        return Ok((String::new(), String::new()));
    }
    let mut prompt = format!("Bài đã đăng:\nTitle: {post_title}\n{}\n\n", truncate(post_content, 1500));
    prompt.push_str("Bình luận từ các agent khác:\n");
    for (author, content) in comments.iter().take(40) {
        let c = content.trim();
        if c.is_empty() {
            continue;
        }
        prompt.push_str(&format!("- {author}: {}\n", truncate(c, 600)));
    }
    prompt.push_str("\nViết phần tổng hợp ngay.");
    complete(FEEDBACK_SYSTEM, &prompt, 1200).await.map(|(t, m)| (t.trim().to_string(), m))
}

const CHALLENGE_SYSTEM: &str = "You are given an obfuscated math word problem used by Moltbook to \
verify that a poster is an AI (not a human). Solve it and return ONLY the numeric answer as a \
string with exactly 2 decimal places, e.g. \"15.00\". No words, no units, no explanation.";

/// Solve a Moltbook content-verification challenge. Returns the numeric answer
/// string (2 decimals). Best-effort — the caller decides what to do on failure.
pub async fn solve_challenge(challenge_text: &str) -> Result<(String, String), String> {
    let (text, model) = complete(CHALLENGE_SYSTEM, challenge_text, 60).await?;
    let answer = normalize_answer(&text);
    if answer.is_empty() {
        return Err(format!("không trích được đáp số từ: {}", truncate(&text, 120)));
    }
    Ok((answer, model))
}

/// Pull the first number out of the model's reply and format it with 2 decimals.
fn normalize_answer(text: &str) -> String {
    let mut num = String::new();
    let mut seen_digit = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            seen_digit = true;
        } else if c == '.' && seen_digit && !num.contains('.') {
            num.push(c);
        } else if (c == '-' || c == '+') && num.is_empty() {
            if c == '-' {
                num.push(c);
            }
        } else if seen_digit {
            break;
        }
    }
    match num.parse::<f64>() {
        Ok(n) => format!("{n:.2}"),
        Err(_) => String::new(),
    }
}

// ---- tolerant JSON parsing (shared by every structured prompt) ----

fn parse_json<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, String> {
    let cleaned = strip_fences(text);
    let first_err = match serde_json::from_str::<T>(&cleaned) {
        Ok(v) => return Ok(v),
        Err(e) => e.to_string(),
    };
    // Progressive salvage: try cutting at each complete-element boundary,
    // furthest-first, until one parses. This recovers the common failure (the
    // provider cut the reply at the token cap) and also handles a dangling key
    // (`…,"content":`) by falling back to the boundary before it.
    for cand in repair_candidates(&cleaned) {
        if let Ok(v) = serde_json::from_str::<T>(&cand) {
            return Ok(v);
        }
    }
    Err(first_err)
}

fn strip_fences(t: &str) -> String {
    let t = t.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(start) = t.find(|c| c == '{' || c == '[') {
        let open = t.as_bytes()[start];
        let close = if open == b'{' { b'}' } else { b']' };
        let bytes = &t.as_bytes()[start..];
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes.iter().enumerate() {
            if in_str {
                if esc { esc = false; }
                else if b == b'\\' { esc = true; }
                else if b == b'"' { in_str = false; }
                continue;
            }
            match b {
                b'"' => in_str = true,
                x if x == open => depth += 1,
                x if x == close => {
                    depth -= 1;
                    if depth == 0 {
                        return t[start..=start + i].to_string();
                    }
                }
                _ => {}
            }
        }
    }
    t.to_string()
}

/// Every byte offset where the JSON could be cut and still end on a complete
/// element — furthest-first. Used to salvage a reply the provider truncated.
///
/// The previous version required a already-closed `}`/`]` to cut back to, which
/// meant it could not repair the most common real failure at all: truncation
/// *inside the first array*, e.g. `{"upvotes":["973cbc85-…","d0069771-b12` —
/// there is no closing bracket anywhere, so it gave up and every heartbeat died.
fn repair_candidates(text: &str) -> Vec<String> {
    let Some(start) = text.find(|c| c == '{' || c == '[') else {
        return Vec::new();
    };
    let s = &text[start..];
    let mut points: Vec<usize> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
                points.push(i + 1); // after a complete string
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' | b'}' | b']' => points.push(i + 1),
            b',' => points.push(i), // cut before the comma
            _ => {}
        }
    }
    points.sort_unstable();
    points.dedup();
    points.reverse();
    points.iter().take(60).filter_map(|&p| close_at(s, p)).collect()
}

/// Cut `s` at `cut` and close whatever brackets are still open. `None` when the
/// cut lands inside a string or leaves nothing useful.
fn close_at(s: &str, cut: usize) -> Option<String> {
    let head = s.get(..cut)?.trim_end().trim_end_matches(',').trim_end();
    if head.is_empty() {
        return None;
    }
    let mut stack: Vec<u8> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for &b in head.as_bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    if in_str {
        return None; // cut landed mid-string — a shorter candidate will do better
    }
    let mut out = head.to_string();
    while let Some(c) = stack.pop() {
        out.push(c as char);
    }
    Some(out)
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_answer_formats_two_decimals() {
        assert_eq!(normalize_answer("The answer is 15"), "15.00");
        assert_eq!(normalize_answer("42.5"), "42.50");
        assert_eq!(normalize_answer("= -3.14159 units"), "-3.14");
        assert_eq!(normalize_answer("no number here"), "");
    }

    #[test]
    fn strip_fences_extracts_object() {
        let t = "```json\n{\"a\":1}\n```";
        assert_eq!(strip_fences(t), "{\"a\":1}");
        let t2 = "sure! {\"upvotes\":[]} done";
        assert_eq!(strip_fences(t2), "{\"upvotes\":[]}");
    }

    #[test]
    fn parse_plan_index_based() {
        let good = r#"{"upvotes":[1,4],"comments":[{"post":2,"content":"hi","why":"x"}],"new_post":null,"note":"ok"}"#;
        let p: RawPlan = parse_json(good).unwrap();
        assert_eq!(p.upvotes, vec![1, 4]);
        assert_eq!(p.comments.len(), 1);
        assert_eq!(p.comments[0].post, 2);
        assert!(p.new_post.is_none());
    }

    /// The exact shape that killed every heartbeat: cut mid-UUID inside the first
    /// array, with no closing bracket anywhere to cut back to.
    #[test]
    fn repairs_truncation_inside_first_array() {
        let bad = r#"```json
{ "upvotes": [ "973cbc85-2c3c-4351-b1f5-b8c4104a50da", "d0069771-b121-4c"#;
        let p: RawPlan = parse_json(bad).unwrap();
        // The partial element is dropped; the plan still parses instead of dying.
        assert!(p.comments.is_empty());
    }

    #[test]
    fn repairs_truncation_with_numeric_indices() {
        let bad = r#"{"upvotes":[1,2],"comments":[{"post":3,"content":"một bình luận dài bị cắt giữa chừ"#;
        let p: RawPlan = parse_json(bad).unwrap();
        assert_eq!(p.upvotes, vec![1, 2]);
    }

    /// Cut right after a key (`"content":`) — the furthest cut yields a dangling
    /// key, so it must fall back to the boundary before it.
    #[test]
    fn repairs_dangling_key() {
        let bad = r#"{"upvotes":[1],"comments":[{"post":2,"content":"#;
        let p: RawPlan = parse_json(bad).unwrap();
        assert_eq!(p.upvotes, vec![1]);
    }

    #[test]
    fn repairs_trailing_comma_and_fences() {
        let bad = "```json\n{\"upvotes\":[1,2,],\"note\":\"x\"}\n```";
        let p: RawPlan = parse_json(bad).unwrap();
        assert_eq!(p.upvotes, vec![1, 2]);
    }

    #[test]
    fn unparseable_text_still_errors() {
        assert!(parse_json::<RawPlan>("totally not json").is_err());
    }
}
