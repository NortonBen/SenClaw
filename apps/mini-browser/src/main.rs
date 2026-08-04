mod api;
mod db;
mod events;
mod input;
mod llm;
mod mcp;
mod session;
mod snapshot;
mod stealth;

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::api::AppState;
use crate::db::{default_data_dir, Db};
use crate::session::BrowserSession;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4360".to_string());

    // Emit something to the runtime log immediately — launching Chromium can take
    // a few seconds, and without this the app's log panel shows "(no logs)" during
    // startup.
    println!("SenClaw Mini Browser starting on port {port} — launching Chromium…");

    // ---- Launch the stealth Chromium and build the shared session ----
    let session = match launch_session().await {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("mini-browser: failed to launch Chromium: {e}");
            eprintln!(
                "Ensure Google Chrome / Chromium is installed, or set MB_CHROME to its path."
            );
            std::process::exit(1);
        }
    };

    // The window is hidden, so this stream is the only way anyone sees the page.
    crate::session::spawn_preview_pump(session.clone());

    let db =
        Arc::new(Db::open(&default_data_dir("mini-browser").join("browser.db")).expect("open db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let (agent_tx, _) = tokio::sync::broadcast::channel(256);
    let state = Arc::new(AppState {
        session,
        db,
        mcp_tx,
        agent_tx,
        agent_lock: Arc::new(tokio::sync::Mutex::new(())),
    });

    // Hands the browser back if a takeover is started and then abandoned.
    api::spawn_takeover_watchdog(state.clone());

    let api_router = api::api_router(state);

    // Static web assets — app-specific and packaged paths first, generic last
    // (avoid the static-dir collision with SenClaw's own web/dist).
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/mini-browser/web/dist"),
        std::path::PathBuf::from("web_dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web/dist"),
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web/dist"));

    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    let app = Router::new()
        .nest("/api", api_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Mini Browser running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}

/// Launch Chromium and return a ready `BrowserSession`.
async fn launch_session() -> anyhow::Result<BrowserSession> {
    let profile = default_data_dir("mini-browser").join("profile");
    let downloads = default_data_dir("mini-browser").join("downloads");
    BrowserSession::launch(profile, chrome_path(), want_headless(), downloads).await
}

/// No window unless one is explicitly asked for.
///
/// `MB_HEADFUL=1` (or `MB_HEADLESS=0`) shows the real Chrome window — useful when
/// debugging, or if a site turns out to object to a windowless build.
fn want_headless() -> bool {
    if let Ok(v) = std::env::var("MB_HEADLESS") {
        return v != "0";
    }
    std::env::var("MB_HEADFUL").ok().as_deref() != Some("1")
}

/// Resolve a Chrome/Chromium executable: MB_CHROME env, then common macOS/Linux paths.
fn chrome_path() -> Option<String> {
    if let Ok(p) = std::env::var("MB_CHROME") {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ];
    candidates
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|s| s.to_string())
}

/// Live tests each launch a browser against the shared profile directory, so
/// they must not run concurrently:
///   cargo test -p mini-browser -- --ignored --test-threads=1
/// Running them in parallel makes Chrome fail the profile lock, surfacing as a
/// bare "oneshot canceled".
#[cfg(test)]
mod tests {
    use super::launch_session;

    /// A page exercising everything the snapshot has to cope with: form controls
    /// and their states, a table, an iframe, and an open shadow root.
    const PROBE_PAGE: &str = r#"<!doctype html><html><body>
      <h1>Probe</h1>
      <form>
        <label for="u">Username</label><input id="u" name="user" placeholder="your name">
        <input type="checkbox" id="c" checked><label for="c">Remember me</label>
        <select id="s"><option value="1">One</option><option value="2" selected>Two</option></select>
        <button type="button" id="b" onclick="document.title='clicked'">Sign in</button>
        <button type="button" disabled>Nope</button>
      </form>
      <table><tr><th>H</th></tr><tr><td>cell</td></tr></table>
      <a href="https://example.com/x">Learn more</a>
      <div style="display:none">invisible text</div>
      <iframe srcdoc="&lt;button onclick=&quot;this.textContent='frame-clicked'&quot;&gt;Inside frame&lt;/button&gt;"></iframe>
      <div id="host"></div>
      <!-- Credential-shaped fields: one obvious, one that only a name gives away. -->
      <input type="password" id="pw" aria-label="Password">
      <input type="text" id="otp" name="one-time-code" aria-label="Verification code">
      <!-- The case an accessibility tree cannot see: no role, no ARIA, just a
           click handler and a pointer cursor. Most app UI looks like this. -->
      <div id="styleddiv" style="cursor:pointer" onclick="this.textContent='pressed'">Xem thêm</div>
      <div style="height:3000px"></div>
      <script>
        document.getElementById('host').attachShadow({mode:'open'})
          .innerHTML = '<button>Shadow button</button>';
      </script>
    </body></html>"#;

    /// Live tests get their own profile directory.
    ///
    /// They used to run against `~/.senclaw/space-apps/mini-browser/profile` —
    /// the same profile the *installed* app uses. Two Chromes cannot share a
    /// profile, so the tests were fighting the user's running browser, and
    /// cleaning up after a test could take their session with it. A test must
    /// not be able to touch anything the user is using.
    fn isolate_profile() {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let dir = std::env::temp_dir().join(format!("mb-test-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok();
            std::env::set_var("SENCLAW_DATA_DIR", &dir);
        });
    }

    /// Launch a session against the throwaway test profile.
    async fn test_session() -> crate::session::BrowserSession {
        isolate_profile();
        launch_session().await.expect("launch")
    }

    /// Serve the probe page on a loopback port and return its URL.
    async fn serve_probe() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/",
                axum::routing::get(|| async { axum::response::Html(PROBE_PAGE) }),
            );
            axum::serve(listener, app).await.ok();
        });
        format!("http://127.0.0.1:{}/", addr.port())
    }

    /// The accessibility snapshot is the one part of this app built on an
    /// assumption about what Chrome returns rather than on something observed,
    /// so it gets checked against a real browser. Roles, states, links, iframe
    /// content and shadow DOM all have to survive the render.
    ///   cargo test -p mini-browser -- --ignored snapshot_sees_the_whole_page
    #[tokio::test]
    #[ignore]
    async fn snapshot_sees_the_whole_page() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        let snap = session.snapshot().await.expect("snapshot");
        let tree = &snap.tree;

        // Roles and accessible names.
        assert!(tree.contains("heading \"Probe\""), "no heading:\n{tree}");
        assert!(tree.contains("button \"Sign in\""), "no button:\n{tree}");
        assert!(
            tree.contains("textbox \"Username\""),
            "no labelled textbox:\n{tree}"
        );

        // States that decide whether an action is even worth attempting.
        assert!(
            tree.contains("[checked]"),
            "checkbox state missing:\n{tree}"
        );
        assert!(
            tree.contains("[disabled]"),
            "disabled state missing:\n{tree}"
        );

        // Links carry their destination.
        assert!(
            tree.contains("https://example.com/x"),
            "link url missing:\n{tree}"
        );

        // The two things the old DOM walker could not see at all.
        assert!(
            tree.contains("Inside frame"),
            "iframe content missing:\n{tree}"
        );
        assert!(
            tree.contains("Shadow button"),
            "shadow DOM missing:\n{tree}"
        );

        // And the thing it should not see.
        assert!(
            !tree.contains("invisible text"),
            "hidden text leaked:\n{tree}"
        );

        // Everything addressable got a ref.
        assert!(tree.contains("[ref=e"), "no refs minted:\n{tree}");
        assert!(
            snap.count > 5,
            "suspiciously few elements ({}):\n{tree}",
            snap.count
        );
    }

    /// Clicking by ref must land on the real element — including one inside an
    /// iframe, which the previous `getBoundingClientRect` maths got wrong
    /// because it returned frame-relative coordinates.
    ///   cargo test -p mini-browser -- --ignored clicks_land_on_the_right_element
    #[tokio::test]
    #[ignore]
    async fn clicks_land_on_the_right_element() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        let snap = session.snapshot().await.expect("snapshot");

        let find_ref = |label: &str| -> String {
            let line = snap
                .tree
                .lines()
                .find(|l| l.contains(label))
                .unwrap_or_else(|| panic!("no line for {label} in:\n{}", snap.tree));
            let at = line
                .find("[ref=")
                .unwrap_or_else(|| panic!("no ref on: {line}"));
            line[at + 5..].split(']').next().unwrap().to_string()
        };

        session
            .click_ref(&find_ref("\"Sign in\""), "left", 1)
            .await
            .expect("click button");
        let title = session.info().await.unwrap()["title"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert_eq!(
            title, "clicked",
            "the top-level click did not reach its element"
        );

        // Now the hard one: an element nested inside an iframe. Its effect has to
        // be read back from inside the frame too — the button relabels itself,
        // and the next snapshot picks that up through the same stitching that
        // found it in the first place.
        session
            .click_ref(&find_ref("\"Inside frame\""), "left", 1)
            .await
            .expect("click in frame");
        let after = session.snapshot().await.expect("re-snapshot");
        assert!(
            after.tree.contains("frame-clicked"),
            "the click inside the iframe missed:\n{}",
            after.tree
        );
    }

    /// A `<div onclick>` with a pointer cursor is not an accessibility object, so
    /// the AX tree alone cannot tell the agent it is pressable. The computed-style
    /// query is what closes that gap — and it has to survive a real browser, not
    /// just a unit test with a handcrafted node set.
    ///   cargo test -p mini-browser -- --ignored styled_divs_are_actionable
    #[tokio::test]
    #[ignore]
    async fn styled_divs_are_actionable() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        let snap = session.snapshot().await.expect("snapshot");

        let line = snap
            .tree
            .lines()
            .find(|l| l.contains("Xem thêm"))
            .unwrap_or_else(|| panic!("the styled div is missing entirely:\n{}", snap.tree));
        assert!(
            line.contains("[ref="),
            "the styled div was seen but is not actionable: {line}"
        );
        assert!(
            snap.extra_clickables > 0,
            "no elements were promoted by computed style"
        );

        // And it must actually be pressable, not merely listed.
        let at = line.find("[ref=").unwrap();
        let r = line[at + 5..].split(']').next().unwrap().to_string();
        session
            .click_ref(&r, "left", 1)
            .await
            .expect("click the styled div");
        let after = session.snapshot().await.expect("re-snapshot");
        assert!(
            after.tree.contains("pressed"),
            "the click did not fire:\n{}",
            after.tree
        );
    }

    /// Typing a credential must leave no trace of it in the run log.
    ///   cargo test -p mini-browser -- --ignored secrets_do_not_reach_the_transcript
    #[tokio::test]
    #[ignore]
    async fn secrets_do_not_reach_the_transcript() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        let snap = session.snapshot().await.expect("snapshot");

        let ref_for = |needle: &str| -> String {
            let line = snap.tree.lines().find(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("no line for {needle}:\n{}", snap.tree));
            let at = line.find("[ref=").unwrap_or_else(|| panic!("no ref on: {line}"));
            line[at + 5..].split(']').next().unwrap().to_string()
        };

        // The obvious case.
        let out = session
            .type_ref(&ref_for("\"Password\""), "hunter2-correct-horse", false, true)
            .await
            .expect("type into the password field");
        assert_eq!(out["secret"], serde_json::json!(true), "password field not recognised: {out}");
        assert!(out.get("value").is_none(), "the typed value came back: {out}");
        assert!(!out.to_string().contains("hunter2"), "the secret is in the result: {out}");

        // And the one that is only a secret because of what it is called — a
        // one-time code is a plain text input.
        let out = session
            .type_ref(&ref_for("\"Verification code\""), "123456", false, true)
            .await
            .expect("type into the OTP field");
        assert_eq!(out["secret"], serde_json::json!(true), "OTP field not recognised: {out}");
        assert!(!out.to_string().contains("123456"), "the code is in the result: {out}");

        // An ordinary field is still reported in full — masking everything would
        // make the log useless.
        let out = session
            .type_ref(&ref_for("\"Username\""), "benji", false, true)
            .await
            .expect("type into the username field");
        assert_eq!(out["value"], serde_json::json!("benji"), "ordinary typing should read back: {out}");
    }

    /// A takeover nobody ends must end by itself.
    ///
    /// The failure this prevents is quiet and total: start a handover, close the
    /// tab, and the agent is locked out of the browser until the app restarts.
    ///   cargo test -p mini-browser -- --ignored an_abandoned_takeover_gives_the_browser_back
    #[tokio::test]
    #[ignore]
    async fn an_abandoned_takeover_gives_the_browser_back() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");

        session.set_takeover(true, Some(&url)).await.expect("hand over");
        assert!(session.in_takeover());
        assert!(!session.is_headless());

        // Nothing to expire yet — this must not fire on a live takeover.
        assert!(!session.expire_takeover().await.expect("check"), "a live takeover must be left alone");

        // Simulate the deadline lapsing with nobody around to refresh it.
        session.force_takeover_deadline_for_test().await;
        assert!(!session.in_takeover(), "the deadline should have lapsed");
        assert!(!session.touch_takeover().await, "a lapsed takeover cannot be refreshed");

        assert!(session.expire_takeover().await.expect("expire"), "the watchdog should act");
        assert!(session.is_headless(), "the browser should be back out of sight");
        assert!(session.snapshot().await.is_ok(), "the agent should be able to work again");
    }

    /// Why the takeover relaunches Chrome instead of just hiding the window.
    ///
    /// Relaunching is where every hard problem in this feature lives — the
    /// profile clobber, the flush race, the lost tabs — so the obvious question
    /// is whether a headful window could simply be minimised and restored
    /// instead. If the screencast kept running while it was hidden, the app
    /// could stay headful permanently and the whole class would disappear.
    ///
    /// It cannot. Measured on macOS: 32 frames in three seconds from an
    /// animating page while visible, and exactly **one** while minimised — and
    /// that one is the frame `startScreencast` always emits on start, not a
    /// stream. The window server stops compositing a minimised window, so the
    /// preview goes black and the user is left watching nothing.
    ///
    /// This test exists to keep that answer, because the idea is attractive
    /// enough to be worth re-proposing and the protocol documents nothing about
    /// platform behaviour either way.
    ///   cargo test -p mini-browser -- --ignored can_a_window_be_hidden_instead --nocapture
    #[tokio::test]
    #[ignore]
    async fn can_a_window_be_hidden_instead() {
        use chromiumoxide::cdp::browser_protocol::browser::{
            Bounds, GetWindowForTargetParams, SetWindowBoundsParams, WindowState,
        };
        use chromiumoxide::cdp::browser_protocol::page::{
            EventScreencastFrame, ScreencastFrameAckParams, StartScreencastFormat,
            StartScreencastParams,
        };
        use futures::StreamExt;

        isolate_profile();
        std::env::set_var("MB_HEADFUL", "1");
        let session = launch_session().await.expect("launch headful");
        std::env::remove_var("MB_HEADFUL");

        // An animating page, so "no frames" means the stream stopped rather than
        // the page simply having nothing to redraw.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route("/", axum::routing::get(|| async {
                axum::response::Html(
                    "<body style='margin:0'><div id=b style='width:100vw;height:100vh'></div>\
                     <script>let i=0;setInterval(()=>{i=(i+37)%360;\
                     document.getElementById('b').style.background='hsl('+i+',80%,50%)';},100)</script></body>")
            }));
            axum::serve(listener, app).await.ok();
        });
        session.navigate(&format!("http://127.0.0.1:{}/", addr.port())).await.expect("nav");

        let page = session.active_page().await;
        let win = page.execute(GetWindowForTargetParams::default()).await.expect("window")
            .result.window_id.clone();

        let count_frames = |page: chromiumoxide::Page, label: &'static str| async move {
            let Ok(mut ev) = page.event_listener::<EventScreencastFrame>().await else { return 0 };
            page.execute(
                StartScreencastParams::builder()
                    .format(StartScreencastFormat::Jpeg).quality(50)
                    .max_width(800).max_height(600).every_nth_frame(1).build(),
            ).await.ok();
            let mut n = 0;
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
            while tokio::time::Instant::now() < deadline {
                match tokio::time::timeout(std::time::Duration::from_millis(800), ev.next()).await {
                    Ok(Some(f)) => {
                        n += 1;
                        page.execute(ScreencastFrameAckParams::new(f.session_id.clone())).await.ok();
                    }
                    _ => break,
                }
            }
            println!("  frames while {label}: {n}");
            n
        };

        let visible = count_frames(page.clone(), "visible").await;
        assert!(visible > 0, "no frames even while visible — the test itself is wrong");

        page.execute(SetWindowBoundsParams::new(
            win.clone(),
            Bounds::builder().window_state(WindowState::Minimized).build(),
        ))
        .await
        .expect("minimize");
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;

        let hidden = count_frames(page.clone(), "minimized").await;

        page.execute(SetWindowBoundsParams::new(
            win,
            Bounds::builder().window_state(WindowState::Normal).build(),
        ))
        .await
        .ok();

        println!(
            "\n  VERDICT: minimising {} the preview alive ({visible} visible vs {hidden} minimised)",
            if hidden > 2 { "KEEPS" } else { "KILLS" }
        );
        // One frame is what `startScreencast` emits unconditionally, so anything
        // at or below that is a dead stream, not a slow one.
        assert!(
            hidden <= 2,
            "minimising kept {hidden} frames flowing — if this ever becomes true, the takeover \
             can hide the window instead of relaunching, and `set_takeover` should be rewritten \
             to do that"
        );
    }

    /// Handing control to the person has to do three things: give them a real
    /// window, stop the agent acting, and keep the profile so whatever they
    /// signed into is still there afterwards.
    ///
    /// NOTE: this opens a visible Chrome window for a few seconds.
    ///   cargo test -p mini-browser -- --ignored the_user_can_take_the_browser_over
    #[tokio::test]
    #[ignore]
    async fn the_user_can_take_the_browser_over() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        assert!(!session.in_takeover());

        // Something to prove the profile survived the relaunch.
        session
            .execute_js("localStorage.setItem('mb_probe', 'kept'); return true;")
            .await
            .expect("write localStorage");

        let v = session.set_takeover(true, Some(&url)).await.expect("hand over");
        assert_eq!(v["takeover"], serde_json::json!(true));
        assert!(session.in_takeover());

        // The whole point: while the person holds the browser the agent is
        // refused, so "the AI must not type your password" is enforced rather
        // than merely requested.
        let err = session.snapshot().await.expect_err("the agent must not act during a takeover");
        assert!(err.to_string().contains("hand control back"), "unhelpful refusal: {err}");
        let err = session
            .type_ref("e1", "hunter2", false, true)
            .await
            .expect_err("typing must be refused too");
        assert!(err.to_string().contains("hand control back"), "{err}");

        // Reading has to be refused as well, or "the AI never sees your
        // credentials" is not true: the first cut gated clicking and typing and
        // left these open, so an agent could have read the password field out of
        // the DOM while the user typed it.
        assert!(
            session.execute_js("return document.body.innerHTML;").await.is_err(),
            "execute_js must not work during a takeover — it can read the password field"
        );
        assert!(session.screenshot_b64(false).await.is_err(), "no screenshots during a takeover");
        assert!(session.extract_text(None).await.is_err(), "no page text during a takeover");
        assert!(session.extract_links().await.is_err(), "no page reads during a takeover");

        // Only "where are we" stays available, because the UI needs it.
        assert!(session.info().await.is_ok(), "info() should still answer");

        // An unfinished takeover must not brick the agent forever. There is a
        // deadline, the UI refreshes it while someone is watching, and a
        // watchdog puts the browser back when it lapses.
        assert!(session.takeover_remaining().await.unwrap_or(0) > 0, "no deadline was set");
        assert!(session.touch_takeover().await, "a live takeover should be refreshable");
        assert!(!session.is_headless(), "a takeover means a real window is up");

        // Hand it back and check we can work again, on the same page, with the
        // profile intact.
        session.set_takeover(false, None).await.expect("take it back");
        assert!(!session.in_takeover());
        let snap = session.snapshot().await.expect("the agent should work again");
        assert!(snap.url.starts_with("http://127.0.0.1"), "landed somewhere else: {}", snap.url);

        let kept = session
            .execute_js("return localStorage.getItem('mb_probe');")
            .await
            .expect("read localStorage");
        assert_eq!(
            kept,
            serde_json::json!("kept"),
            "the profile did not survive the relaunch — a login done during takeover would be lost"
        );
    }

    /// Enter in a text field must submit the form it belongs to.
    ///
    /// The run log showed "Typed 'giá vàng hôm nay' into the search box and
    /// pressed Enter. [the page did not navigate]" — the agent had to spend a
    /// whole extra plan finding the search button instead. This pins whether the
    /// keystroke itself reaches the form.
    ///   cargo test -p mini-browser -- --ignored enter_submits_the_form
    #[tokio::test]
    #[ignore]
    async fn enter_submits_the_form() {
        let session = test_session().await;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new()
                .route("/", axum::routing::get(|| async {
                    // A search box shaped like the ones that failed: a GET form
                    // with a submit button, no JS.
                    axum::response::Html(
                        "<form action='/results' method='get'>\
                         <input name='q' aria-label='Search'>\
                         <button type='submit'>Search</button></form>")
                }))
                .route("/results", axum::routing::get(|| async {
                    axum::response::Html("<h1>Results</h1>")
                }));
            axum::serve(listener, app).await.ok();
        });
        let base = format!("http://127.0.0.1:{}/", addr.port());
        session.navigate(&base).await.expect("navigate");

        let snap = session.snapshot().await.expect("snapshot");
        let line = snap.tree.lines().find(|l| l.contains("\"Search\"") && l.contains("textbox"))
            .unwrap_or_else(|| panic!("no search box in:\n{}", snap.tree));
        let at = line.find("[ref=").expect("ref");
        let r = line[at + 5..].split(']').next().unwrap().to_string();

        session.type_ref(&r, "giá vàng hôm nay", true, true).await.expect("type + submit");
        let url = session.info().await.unwrap()["url"].as_str().unwrap_or_default().to_string();
        assert!(
            url.contains("/results"),
            "Enter did not submit the form — still at {url}"
        );
        assert!(url.contains("q=gi"), "the query did not travel with it: {url}");
    }

    /// The root cause of the replan loop in the wild: the agent typed into a
    /// `<div>` that merely wrapped the search box, the tool answered
    /// "typed 20 chars", the step reported "typed the query and submitted", and
    /// the check kept replying "still on the homepage" — for as many plans as the
    /// budget allowed. A tool that cannot type must say so.
    ///   cargo test -p mini-browser -- --ignored typing_into_a_non_field_fails_loudly
    #[tokio::test]
    #[ignore]
    async fn typing_into_a_non_field_fails_loudly() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        let snap = session.snapshot().await.expect("snapshot");

        let ref_for = |needle: &str| -> String {
            let line = snap
                .tree
                .lines()
                .find(|l| l.contains(needle))
                .unwrap_or_else(|| panic!("no line for {needle}:\n{}", snap.tree));
            let at = line
                .find("[ref=")
                .unwrap_or_else(|| panic!("no ref on: {line}"));
            line[at + 5..].split(']').next().unwrap().to_string()
        };

        // The styled div is offered as a target (correctly — it is clickable),
        // but it cannot hold text.
        let err = session
            .type_ref(&ref_for("Xem thêm"), "giá vàng hôm nay", true, true)
            .await
            .expect_err("typing into a div must not report success");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot accept typed text"),
            "unhelpful error: {msg}"
        );
        assert!(
            msg.contains("nothing was typed"),
            "must be explicit that it did not happen: {msg}"
        );

        // And the real field still works, reporting what actually landed.
        let ok = session
            .type_ref(&ref_for("\"Username\""), "giá vàng", false, true)
            .await
            .expect("typing into the real field");
        assert_eq!(
            ok["value"], "giá vàng",
            "the tool must read the field back: {ok}"
        );
    }

    /// A long page must tell the model where it is, or it cannot decide whether
    /// to scroll.
    ///   cargo test -p mini-browser -- --ignored scroll_position_reaches_the_model
    #[tokio::test]
    #[ignore]
    async fn scroll_position_reaches_the_model() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");

        let top = session.snapshot().await.expect("snapshot");
        assert!(
            !top.scroll.fits(),
            "the probe page is 3000px tall: {:?}",
            top.scroll
        );
        assert!(
            top.tree.starts_with("[start of page]"),
            "{}",
            &top.tree[..80.min(top.tree.len())]
        );
        assert!(
            top.tree.contains("[more below"),
            "no hint that there is more to see"
        );
        assert!(
            top.scroll.describe().contains("below"),
            "{}",
            top.scroll.describe()
        );

        session.scroll(0.0, 4000.0).await.expect("scroll down");
        let bottom = session.snapshot().await.expect("snapshot");
        assert!(bottom.scroll.y > top.scroll.y, "the page did not scroll");
        assert!(
            bottom.tree.contains("[more above"),
            "after scrolling the model should be told there is content above:\n{}",
            &bottom.tree[..80.min(bottom.tree.len())]
        );
    }

    /// The safety property the whole ref design exists for: a ref from a page you
    /// have left must FAIL, not land somewhere arbitrary.
    ///
    /// This was observed happening. A click that navigated left the registry
    /// holding the old document's backend node ids; Chrome had since reused those
    /// numbers for unrelated elements, so the stale ref resolved and clicked
    /// something else entirely — and reported success. A wrong click is much worse
    /// than a failed one, so this is a regression test with teeth.
    ///   cargo test -p mini-browser -- --ignored a_ref_from_a_previous_page_is_refused
    #[tokio::test]
    #[ignore]
    async fn a_ref_from_a_previous_page_is_refused() {
        let session = test_session().await;

        // Two pages, so following the link genuinely replaces the document.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new()
                .route(
                    "/",
                    axum::routing::get(|| async {
                        axum::response::Html("<a href='/next' id='go'>Go</a>")
                    }),
                )
                .route(
                    "/next",
                    axum::routing::get(|| async {
                        axum::response::Html("<h1>Next</h1><button>Somewhere else</button>")
                    }),
                );
            axum::serve(listener, app).await.ok();
        });
        let base = format!("http://127.0.0.1:{}/", addr.port());

        session.navigate(&base).await.expect("navigate");
        let snap = session.snapshot().await.expect("snapshot");
        let line = snap
            .tree
            .lines()
            .find(|l| l.contains("\"Go\""))
            .expect("link");
        let at = line.find("[ref=").expect("ref");
        let stale = line[at + 5..].split(']').next().unwrap().to_string();

        // Clicking it navigates — which is exactly what makes the ref stale.
        session
            .click_ref(&stale, "left", 1)
            .await
            .expect("click the link");
        let url = session.info().await.unwrap()["url"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(
            url.ends_with("/next"),
            "expected to have navigated, got {url}"
        );

        let err = session
            .click_ref(&stale, "left", 1)
            .await
            .expect_err("a ref from the previous page must not resolve");
        assert!(
            err.to_string().contains("not on the current page"),
            "unhelpful error: {err}"
        );
    }

    /// With no window, the preview stream is the only way anyone sees the page —
    /// so it has to actually work in the mode the app ships in.
    ///
    /// Also pins the thing that made the first attempt at this misleading: a
    /// screencast emits on compositor commits, so a static page yields exactly one
    /// frame and looks broken. The page here changes continuously on purpose.
    ///   cargo test -p mini-browser -- --ignored preview_streams_with_no_window
    #[tokio::test]
    #[ignore]
    async fn preview_streams_with_no_window() {
        // Deliberately does not set MB_HEADLESS: the point is that *the default*
        // is windowless. Setting it would also leak into every later test in this
        // process, since they share one address space.
        assert!(
            super::want_headless(),
            "the default must be windowless — did MB_HEADFUL leak into this run?"
        );
        let session = std::sync::Arc::new(test_session().await);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route("/", axum::routing::get(|| async {
                axum::response::Html(
                    "<body style='margin:0'><div id=b style='width:100vw;height:100vh'></div>\
                     <script>let i=0;setInterval(()=>{i=(i+37)%360;\
                     document.getElementById('b').style.background='hsl('+i+',80%,50%)';},100)</script></body>")
            }));
            axum::serve(listener, app).await.ok();
        });

        let mut rx = session.frames();
        crate::session::spawn_preview_pump(session.clone());
        session
            .navigate(&format!("http://127.0.0.1:{}/", addr.port()))
            .await
            .expect("navigate");

        let mut n = 0usize;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(6);
        while tokio::time::Instant::now() < deadline && n < 5 {
            match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
                Ok(Ok(data)) => {
                    assert!(!data.is_empty(), "empty frame");
                    n += 1;
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
                _ => break,
            }
        }
        assert!(n >= 5, "only {n} preview frames arrived with no window");
    }

    /// Typing must produce real key events, not just text appearing. A page that
    /// only listens on `keydown` used to see nothing at all.
    ///   cargo test -p mini-browser -- --ignored typing_fires_key_events
    #[tokio::test]
    #[ignore]
    async fn typing_fires_key_events() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");
        session
            .execute_js(
                "window.__keys = 0; window.__ups = 0;
                 document.addEventListener('keydown', () => window.__keys++);
                 document.addEventListener('keyup', () => window.__ups++);
                 return true;",
            )
            .await
            .expect("install listeners");

        let snap = session.snapshot().await.expect("snapshot");
        let line = snap
            .tree
            .lines()
            .find(|l| l.contains("\"Username\""))
            .expect("textbox");
        let at = line.find("[ref=").expect("ref");
        let r = line[at + 5..].split(']').next().unwrap().to_string();

        session
            .type_ref(&r, "hello", false, true)
            .await
            .expect("type");
        let counts = session
            .execute_js("return { keys: window.__keys, ups: window.__ups, value: document.getElementById('u').value };")
            .await
            .expect("read counts");

        assert_eq!(counts["value"], "hello", "text did not land in the field");
        assert!(
            counts["keys"].as_i64().unwrap_or(0) >= 5,
            "expected a keydown per character, got {counts:?}"
        );
        assert!(
            counts["ups"].as_i64().unwrap_or(0) >= 5,
            "expected a keyup per character, got {counts:?}"
        );
    }

    /// A `confirm()` suspends the renderer. The session must notice, refuse to
    /// act, and be able to clear it — otherwise the browser wedges silently and
    /// the live view freezes with it.
    ///   cargo test -p mini-browser -- --ignored a_dialog_blocks_and_can_be_answered
    #[tokio::test]
    #[ignore]
    async fn a_dialog_blocks_and_can_be_answered() {
        let session = test_session().await;
        let url = serve_probe().await;
        session.navigate(&url).await.expect("navigate");

        // Fire the dialog without awaiting it — `confirm` does not return until
        // it is answered, so awaiting the evaluation here would deadlock the test.
        session
            .execute_js("setTimeout(() => { window.__ok = confirm('proceed?'); }, 0); return true;")
            .await
            .ok();
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;

        let info = session.info().await.expect("info");
        assert!(
            info.get("dialog").is_some(),
            "the dialog went unnoticed: {info}"
        );

        let blocked = session.snapshot().await;
        assert!(
            blocked.is_err(),
            "tools must refuse while a dialog is blocking the page"
        );

        session
            .handle_dialog(true, None)
            .await
            .expect("answer the dialog");
        let after = session
            .execute_js("return window.__ok;")
            .await
            .expect("read result");
        assert_eq!(
            after,
            serde_json::json!(true),
            "accept was not delivered to the page"
        );
        assert!(
            session.snapshot().await.is_ok(),
            "the page should work again"
        );
    }

    /// Live identity check — launches Chrome and asserts it presents itself as a
    /// coherent, real browser. Needs a Chrome binary, so ignored by default:
    ///   cargo test -p mini-browser -- --ignored identity_smoke
    #[tokio::test]
    #[ignore]
    async fn identity_smoke() {
        let session = test_session().await;
        // navigator.userAgentData needs a secure context; about:blank is not one.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let app = axum::Router::new().route(
                "/",
                axum::routing::get(|| async {
                    axum::response::Html("<html><body>x</body></html>")
                }),
            );
            axum::serve(listener, app).await.ok();
        });
        session
            .navigate(&format!("http://127.0.0.1:{}/", addr.port()))
            .await
            .expect("navigate");

        let checks = session
            .execute_js(
                "const d = navigator.userAgentData; \
                 const gl = document.createElement('canvas').getContext('webgl'); \
                 const dbg = gl && gl.getExtension('WEBGL_debug_renderer_info'); \
                 return { ua: navigator.userAgent, webdriver: navigator.webdriver, \
                          langs: navigator.languages, chrome: !!window.chrome, \
                          plugins: navigator.plugins.length, \
                          brands: d ? d.brands.map(b => b.brand) : [], \
                          uaPlatform: d ? d.platform : '', \
                          touchPoints: navigator.maxTouchPoints, \
                          hasTouchStart: ('ontouchstart' in window), \
                          orientation: (screen.orientation ? screen.orientation.angle : -1), \
                          renderer: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : '' };",
            )
            .await
            .expect("execute");

        let ua = checks["ua"].as_str().unwrap_or_default();
        let brands: Vec<&str> = checks["brands"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|b| b.as_str())
            .collect();

        assert_eq!(
            checks["webdriver"],
            serde_json::json!(false),
            "real Chrome reports false, not undefined"
        );
        assert_eq!(checks["chrome"], serde_json::json!(true));
        assert!(checks["plugins"].as_i64().unwrap_or(0) > 0, "plugins empty");
        assert_eq!(checks["langs"][0], "vi-VN");

        // Nothing may still claim to be headless…
        assert!(!ua.contains("Headless"), "UA leaks headless: {ua}");
        assert!(
            !brands.iter().any(|b| b.contains("Headless")),
            "brands leak headless: {brands:?}"
        );

        // …and the client hints must exist and agree with the UA. Empty brands
        // means the override dropped Sec-CH-UA — the original sign-in bug.
        assert!(
            !brands.is_empty(),
            "no client-hint brands ⇒ Sec-CH-UA suppressed"
        );
        assert_eq!(checks["uaPlatform"], "macOS");
        assert!(ua.contains("Mac OS X"), "UA/platform disagree: {ua}");

        // The GPU must belong to the platform the UA claims.
        let renderer = checks["renderer"].as_str().unwrap_or_default();
        assert!(
            !renderer.contains("Direct3D"),
            "macOS UA with a Direct3D GPU: {renderer}"
        );

        // A desktop UA must not come with a touchscreen. chromiumoxide turns on
        // `Emulation.setTouchEmulationEnabled` for any configured viewport —
        // hardcoded to true, ignoring `has_touch` — which had this browser
        // reporting a touch-capable Mac. `viewport(None)` in `launch_session` is
        // what prevents it, and this is the assertion that keeps it prevented.
        if !checks["uaPlatform"]
            .as_str()
            .unwrap_or_default()
            .contains("Android")
            && !ua.contains("Mobile")
        {
            assert_eq!(
                checks["touchPoints"],
                serde_json::json!(0),
                "desktop UA reporting a touchscreen — is a viewport being emulated again?"
            );
            assert_eq!(
                checks["hasTouchStart"],
                serde_json::json!(false),
                "ontouchstart present on a desktop UA"
            );
        }

        // Real desktop landscape is angle 0; the emulation layer pinned it to 90.
        let angle = checks["orientation"].as_i64().unwrap_or(-1);
        assert!(
            angle == 0 || angle == -1,
            "unnatural screen orientation angle: {angle}"
        );

        // The header and JS must name the same languages. They are set from one
        // source (`--accept-lang`), and this proves the two ends agree.
        let want = crate::stealth::accept_language();
        let first = want.split(',').next().unwrap_or("");
        assert_eq!(
            checks["langs"][0].as_str().unwrap_or_default(),
            first,
            "navigator.languages disagrees with the Accept-Language we set ({want})"
        );
    }

    /// The regression test for the bug this layer exists to fix: Google used to
    /// bounce us to `/v3/signin/rejected` ("Không thể đăng nhập cho bạn"), which
    /// is how a fabricated identity gets treated. Asserts the sign-in form is
    /// actually served — no credentials involved, only the landing URL.
    /// Needs network + a Chrome binary:
    ///   cargo test -p mini-browser -- --ignored google_serves_signin_form
    #[tokio::test]
    #[ignore]
    async fn google_serves_signin_form() {
        let session = test_session().await;
        session
            .navigate("https://accounts.google.com/ServiceLogin?hl=vi")
            .await
            .expect("navigate");
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;

        let url = session.info().await.expect("info")["url"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let body = session.extract_text(None).await.unwrap_or_default();
        let text = body["text"].as_str().unwrap_or_default();

        assert!(
            !url.contains("/signin/rejected") && !text.contains("Không thể đăng nhập"),
            "Google rejected the browser as insecure — landed at {url}"
        );
        assert!(
            url.contains("/signin/identifier"),
            "expected the sign-in form, got {url}"
        );
    }
}
