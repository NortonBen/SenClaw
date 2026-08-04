//! `BrowserSession` — the single shared browsing surface driven by BOTH the user
//! (via the live-view WebSocket) and the AI (via MCP). It owns the Chromium
//! instance, the list of tabs, and the per-tab recorder that remembers what each
//! page did.
//!
//! Two things changed shape here relative to the first version, and both came
//! from reading how the mature open-source browser agents actually work:
//!
//! * **Elements are addressed by `ref`, resolved through CDP.** A ref comes from
//!   the accessibility snapshot and maps to a `backendNodeId`; coordinates come
//!   from `DOM.getContentQuads`, which reports viewport coordinates the browser
//!   computed itself. The old path — `querySelector('[data-mb-idx=…]')` plus
//!   `getBoundingClientRect()` — could not see into an iframe, and inside one it
//!   returned coordinates relative to the *frame*, so every click landed in the
//!   wrong place. That class of bug is now gone by construction.
//!
//! * **Actions wait for the page instead of sleeping at it.** `settle()` watches
//!   in-flight requests and `document.readyState`. The fixed `sleep(500ms)` it
//!   replaces was simultaneously too long for a static page and far too short
//!   for a real one.

use anyhow::{anyhow, Result};
use base64::Engine;
use chromiumoxide::cdp::browser_protocol::browser::{
    Bounds, GetWindowForTargetParams, SetWindowBoundsParams,
};
use chromiumoxide::cdp::browser_protocol::dom::{
    BackendNodeId, GetContentQuadsParams, ResolveNodeParams, ScrollIntoViewIfNeededParams,
    SetFileInputFilesParams,
};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, GetNavigationHistoryParams, HandleJavaScriptDialogParams,
    NavigateToHistoryEntryParams, SetInterceptFileChooserDialogParams,
};
use chromiumoxide::cdp::browser_protocol::target::CreateTargetParams;
use chromiumoxide::cdp::js_protocol::runtime::CallFunctionOnParams;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::{Browser, Page};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::events::{PendingDialog, Recorder};
use crate::input;
use crate::snapshot::{self, GetFullAxTree, RefRegistry, Snapshot};
use crate::stealth;

/// A JavaScript dialog blocks the renderer — including screenshots, so the live
/// view freezes too. We hold it long enough for a person or the agent to answer,
/// then dismiss it rather than leave the browser wedged forever.
const DIALOG_AUTO_DISMISS: Duration = Duration::from_secs(30);

/// How long an action waits for the page to go quiet before proceeding anyway.
const SETTLE_ACTION: Duration = Duration::from_millis(2500);
const SETTLE_NAVIGATION: Duration = Duration::from_millis(12_000);

pub struct Tab {
    pub page: Page,
    pub rec: Recorder,
}

pub struct BrowserSession {
    /// Behind a lock because a takeover replaces it: Chrome decides at launch
    /// whether it has a window, and there is no CDP command that changes its
    /// mind. Handing the user a real window therefore means relaunching against
    /// the same profile, which is also what carries the login they just did back
    /// into every later automated run.
    browser: tokio::sync::RwLock<Browser>,
    profile: std::path::PathBuf,
    chrome: Option<String>,
    headless: std::sync::atomic::AtomicBool,
    /// While the user is driving. The agent refuses to act, so "the AI must not
    /// type your password" is enforced by a lock rather than by asking the model
    /// nicely.
    ///
    /// Stored as a deadline rather than a flag, because a flag only ever fails
    /// one way: someone starts a takeover, gets distracted or closes the tab,
    /// and the agent is bricked with no way back except restarting the app. A
    /// deadline fails the other way — control comes back on its own. Refreshed
    /// while the user is visibly still there, so a real sign-in is never cut off
    /// mid-flow.
    takeover_until: Mutex<Option<Instant>>,
    tabs: Mutex<Vec<Tab>>,
    active: AtomicUsize,
    identity: stealth::Identity,
    /// The most recent capture, reused when nothing has happened since.
    last: Mutex<Option<Snapshot>>,
    /// ref → element, stable for the life of the document.
    refs: Mutex<RefRegistry>,
    /// (target, loaderId) the current refs belong to.
    doc: Mutex<Option<(String, String)>>,
    /// Where the pointer was left, so motion continues rather than teleports.
    cursor: input::Cursor,
    viewport: Mutex<(u32, u32)>,
    downloads: std::path::PathBuf,
    /// Preview frames (base64 JPEG) for the live view.
    frames: tokio::sync::broadcast::Sender<String>,
}

impl BrowserSession {
    /// Launch Chromium and build the shared session around it.
    pub async fn launch(
        profile: std::path::PathBuf,
        chrome: Option<String>,
        headless: bool,
        downloads: std::path::PathBuf,
    ) -> Result<Self> {
        let (browser, first) = launch_browser(&profile, chrome.as_deref(), headless, true).await?;
        Self::new(browser, first, profile, chrome, headless, downloads).await
    }

    /// Wrap an already-launched browser + its first page.
    async fn new(
        browser: Browser,
        first: Page,
        profile: std::path::PathBuf,
        chrome: Option<String>,
        headless: bool,
        downloads: std::path::PathBuf,
    ) -> Result<Self> {
        // Read the browser's genuine identity *before* `prepare` installs any
        // override — afterwards it would only report back what we told it.
        let raw = stealth::probe(&first).await?;
        let identity = stealth::correct(&raw);
        if identity.corrected {
            println!(
                "mini-browser: headless build detected — presenting as {}",
                identity.ua
            );
        }
        std::fs::create_dir_all(&downloads).ok();

        let s = Self {
            browser: tokio::sync::RwLock::new(browser),
            profile,
            chrome,
            headless: std::sync::atomic::AtomicBool::new(headless),
            takeover_until: Mutex::new(None),
            tabs: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
            identity,
            last: Mutex::new(None),
            refs: Mutex::new(RefRegistry::default()),
            doc: Mutex::new(None),
            cursor: input::Cursor::default(),
            viewport: Mutex::new((1280, 800)),
            downloads,
            // Small buffer: a slow viewer should see the newest frame, not a
            // backlog of stale ones.
            frames: tokio::sync::broadcast::channel(4).0,
        };
        let tab = s.prepare(first.clone()).await?;
        s.tabs.lock().await.push(tab);
        // Prime the tab list before anything can act, so pages Chrome opened for
        // itself are known but do not steal focus from ours.
        s.sync_tabs_inner(false).await.ok();
        let ours = s
            .tabs
            .lock()
            .await
            .iter()
            .position(|t| t.page.target_id() == first.target_id())
            .unwrap_or(0);
        s.active.store(ours, Ordering::SeqCst);
        Ok(s)
    }

    /// Pin the page's identity and start recording what it does.
    ///
    /// The UA override carries the browser's real client-hint metadata plus a
    /// matching `Accept-Language`. That is the whole identity layer — see
    /// `stealth.rs` for why there is no injected JS. Setting a UA string without
    /// `userAgentMetadata` (what chromiumoxide's `enable_stealth_mode` does)
    /// silently disables client hints, and a Chrome that sends no `Sec-CH-UA` is
    /// an instant tell.
    async fn prepare(&self, page: Page) -> Result<Tab> {
        page.execute(stealth::override_params(&self.identity)?)
            .await?;

        let rec = Recorder::new();
        wire_events(&page, rec.clone(), &self.downloads).await;

        Ok(Tab { page, rec })
    }


    // ------------------------------------------------------------ takeover ---

    /// How long a takeover lasts without a sign of life.
    ///
    /// Long enough for a real sign-in — finding a phone, waiting for an SMS,
    /// unlocking a password manager — and short enough that a forgotten one does
    /// not strand the agent for the rest of the day.
    const TAKEOVER_IDLE: Duration = Duration::from_secs(15 * 60);

    pub fn is_headless(&self) -> bool {
        self.headless.load(Ordering::SeqCst)
    }

    pub fn in_takeover(&self) -> bool {
        self.takeover_until
            .try_lock()
            .map(|d| d.map(|t| Instant::now() < t).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Push the deadline out. Called whenever the UI shows the user is still
    /// there, so a slow login is never interrupted.
    pub async fn touch_takeover(&self) -> bool {
        let mut d = self.takeover_until.lock().await;
        match *d {
            Some(t) if Instant::now() < t => {
                *d = Some(Instant::now() + Self::TAKEOVER_IDLE);
                true
            }
            _ => false,
        }
    }

    /// Expire the deadline immediately. Test-only: waiting fifteen minutes to
    /// find out whether the watchdog works is not a test anyone runs.
    #[cfg(test)]
    pub async fn force_takeover_deadline_for_test(&self) {
        let mut d = self.takeover_until.lock().await;
        if d.is_some() {
            *d = Some(Instant::now() - Duration::from_secs(1));
        }
    }

    /// Seconds left before control returns on its own, if a takeover is running.
    pub async fn takeover_remaining(&self) -> Option<u64> {
        let d = self.takeover_until.lock().await;
        d.and_then(|t| t.checked_duration_since(Instant::now())).map(|d| d.as_secs())
    }

    /// Hand the browser to the person, or take it back.
    ///
    /// This exists because some things must not be automated. Signing in is the
    /// clearest: the agent must never type a password, and a rule the model is
    /// merely *told* is not the same as one it cannot break. During a takeover
    /// the session refuses every acting tool, so the guarantee is structural.
    ///
    /// It also has to be a real window, not the live view. A password manager,
    /// a passkey prompt and a hardware key all live in browser and OS chrome
    /// that no screencast can show and no synthetic `Input` event can reach.
    /// Chrome fixes whether it has a window at launch, so this relaunches
    /// against the same profile — which is exactly what makes the login persist
    /// into later automated runs.
    pub async fn set_takeover(&self, on: bool, url: Option<&str>) -> Result<Value> {
        if self.in_takeover() == on {
            return Ok(json!({ "takeover": on, "changed": false }));
        }
        // Where to land afterwards: what the user asked for, else wherever we
        // already were, so handing control back does not lose their place.
        let target = match url.map(normalize_url) {
            Some(u) if u != "about:blank" => u,
            _ => self
                .info()
                .await
                .ok()
                .and_then(|i| i["url"].as_str().map(String::from))
                .filter(|u| !u.is_empty() && u != "about:blank")
                .unwrap_or_else(|| "about:blank".to_string()),
        };

        // Flip the flag *before* relaunching, not after. `relaunch` navigates,
        // and navigation is one of the things a takeover forbids — so handing
        // control back with the flag still set had the guard block the restore
        // and drop the user back on a blank page.
        let was = *self.takeover_until.lock().await;
        *self.takeover_until.lock().await =
            if on { Some(Instant::now() + Self::TAKEOVER_IDLE) } else { None };
        if let Err(e) = self.relaunch(!on, &target).await {
            *self.takeover_until.lock().await = was;
            return Err(e);
        }
        Ok(json!({
            "takeover": on,
            "changed": true,
            "url": target,
            "note": if on {
                "The real browser window is open. Sign in there — the agent will not act until you hand control back."
            } else {
                "Control returned to the app. Anything you signed into is kept in the profile."
            }
        }))
    }

    /// End a takeover that timed out, putting the browser back as it was.
    ///
    /// Returns whether there was anything to end, so the caller can report it
    /// rather than silently changing the world.
    pub async fn expire_takeover(&self) -> Result<bool> {
        // Expired means the deadline has passed but the browser is still the
        // one we opened for the user — a window is still up and the agent is
        // still locked out of a session nobody is using.
        if self.in_takeover() || self.is_headless() {
            return Ok(false);
        }
        let target = self
            .info()
            .await
            .ok()
            .and_then(|i| i["url"].as_str().map(String::from))
            .filter(|u| !u.is_empty() && u != "about:blank")
            .unwrap_or_else(|| "about:blank".to_string());
        *self.takeover_until.lock().await = None;
        self.relaunch(true, &target).await?;
        Ok(true)
    }

    /// Replace the running browser with one launched the other way round.
    async fn relaunch(&self, headless: bool, url: &str) -> Result<()> {
        // Order matters, and the obvious order is wrong. Launching the
        // replacement first looks safer — a failed launch would leave a working
        // browser — but two Chromes cannot share a profile, and the singleton
        // lock that enforces that had to be cleared to let the second one start.
        // Both then wrote the profile, and the old one's orderly shutdown
        // clobbered the new one's `Preferences` and cookie flush. So: stop the
        // old browser completely, let it finish writing, and only then start.
        {
            let mut guard = self.browser.write().await;
            // Ask it to close, then *wait for the process to actually exit*.
            //
            // Killing straight after `close()` returns loses data: the CDP reply
            // comes back before Chrome has finished writing, so the SIGKILL lands
            // mid-flush. Measured — localStorage written seconds earlier was gone
            // after the relaunch, and cookies would go the same way, which is the
            // one thing a takeover exists to preserve. `close()` is bounded
            // because a wedged browser never replies; the wait is bounded for the
            // same reason; `kill()` is the last resort, not the first move.
            let _ = tokio::time::timeout(Duration::from_secs(8), guard.close()).await;
            let _ = tokio::time::timeout(Duration::from_secs(8), guard.wait()).await;
            let _ = guard.kill().await;

            match launch_browser(&self.profile, self.chrome.as_deref(), headless, true).await {
                Ok((fresh, first)) => {
                    *guard = fresh;
                    drop(guard);
                    self.adopt(first, headless).await?;
                }
                Err(e) => {
                    // No browser at all now. Try to come back the way we were so
                    // the app is still usable, and report what happened either way.
                    let back = self.headless.load(Ordering::SeqCst);
                    if let Ok((fresh, first)) =
                        launch_browser(&self.profile, self.chrome.as_deref(), back, true).await
                    {
                        *guard = fresh;
                        drop(guard);
                        self.adopt(first, back).await.ok();
                    }
                    return Err(anyhow!("could not restart the browser: {e}"));
                }
            }
        }

        if url != "about:blank" {
            // Deliberately not `navigate()`: that is the *agent's* entry point
            // and is gated on the takeover flag, which is mid-flip here. This is
            // the session restoring its own position, not an action anyone
            // requested, so it goes straight to the page.
            let page = self.active_page().await;
            self.active_recorder().await.reset_for_navigation();
            page.goto(url).await.ok();
            self.settle(SETTLE_NAVIGATION).await;
            self.reset_refs().await;
            self.sync_tabs_inner(false).await.ok();
        }
        Ok(())
    }

    /// Rebuild the session's view of a freshly launched browser.
    async fn adopt(&self, first: Page, headless: bool) -> Result<()> {
        self.tabs.lock().await.clear();
        self.active.store(0, Ordering::SeqCst);
        let tab = self.prepare(first).await?;
        self.tabs.lock().await.push(tab);
        self.reset_refs().await;
        self.headless.store(headless, Ordering::SeqCst);
        Ok(())
    }

    /// Refuse to touch the page at all while the person is driving.
    ///
    /// Deny by default, not just for the obviously-acting tools. The first cut
    /// gated clicking and typing and left `execute_js`, `screenshot_b64` and
    /// `extract_text` open — so an agent that had just handed over could read the
    /// password field out of the DOM while the user typed it. "The AI never sees
    /// your credentials" has to cover *reading*, or it is not a claim worth
    /// making. Only `info()` (url and title) stays available, because the UI
    /// needs to know where things are.
    fn ensure_not_taken_over(&self) -> Result<()> {
        if self.in_takeover() {
            return Err(anyhow!(
                "you have control of the browser right now — the agent can neither act on the page nor read it until you hand control back"
            ));
        }
        Ok(())
    }

    // ---------------------------------------------------------------- tabs ---

    /// Reconcile our tab list with the browser's real target list.
    ///
    /// Without this, a `target="_blank"` link or a `window.open()` popup creates
    /// a page we never learn about: the live view keeps streaming the old tab and
    /// the agent keeps acting on it, with no clue that its click "did nothing".
    /// Every checkout and OAuth flow hits this, so it is not an edge case.
    pub async fn sync_tabs(&self) -> Result<()> {
        self.sync_tabs_inner(true).await
    }

    /// `activate_new` decides whether adopting a target is *allowed* to focus it.
    ///
    /// Focus should follow a popup: the user clicked something, a window came up,
    /// and that is now what they are looking at. It must not follow a tab the
    /// browser opened for its own reasons.
    ///
    /// Telling those apart matters more than it sounds. A headful Chrome creates
    /// its new-tab page asynchronously, a moment *after* launch, so it was not
    /// there to be adopted at startup and instead got picked up by the first sync
    /// after the first navigation — stealing focus, and making every subsequent
    /// command act on Chrome's start page. The symptom was a browser that
    /// appeared to ignore instructions while reporting success.
    ///
    /// The distinguishing fact is `openerId`: a window opened *by a page* records
    /// which page opened it. Chrome's own new tab has no opener.
    async fn sync_tabs_inner(&self, activate_new: bool) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::target::GetTargetsParams;

        let real = self.browser.read().await.pages().await.unwrap_or_default();
        let opened_by_a_page: std::collections::HashSet<String> = self
            .browser
            .read()
            .await
            .execute(GetTargetsParams::default())
            .await
            .map(|r| {
                r.result
                    .target_infos
                    .iter()
                    .filter(|t| t.opener_id.is_some())
                    .map(|t| t.target_id.inner().clone())
                    .collect()
            })
            .unwrap_or_default();

        let mut tabs = self.tabs.lock().await;

        // Drop tabs whose target is gone.
        let alive: std::collections::HashSet<_> =
            real.iter().map(|p| p.target_id().clone()).collect();
        let before = tabs.len();
        tabs.retain(|t| alive.contains(t.page.target_id()));
        let dropped = before != tabs.len();

        let known: std::collections::HashSet<_> =
            tabs.iter().map(|t| t.page.target_id().clone()).collect();
        let mut focus: Option<usize> = None;
        for p in real {
            if known.contains(p.target_id()) {
                continue;
            }
            let is_popup = opened_by_a_page.contains(p.target_id().inner());
            if let Ok(tab) = self.prepare(p).await {
                tabs.push(tab);
                if activate_new && is_popup {
                    focus = Some(tabs.len() - 1);
                }
            }
        }
        if let Some(i) = focus {
            self.active.store(i, Ordering::SeqCst);
        } else if dropped || self.active.load(Ordering::SeqCst) >= tabs.len() {
            self.active
                .store(tabs.len().saturating_sub(1), Ordering::SeqCst);
        }
        Ok(())
    }

    pub async fn active_page(&self) -> Page {
        let tabs = self.tabs.lock().await;
        let i = self
            .active
            .load(Ordering::SeqCst)
            .min(tabs.len().saturating_sub(1));
        tabs[i].page.clone()
    }

    pub async fn active_recorder(&self) -> Recorder {
        let tabs = self.tabs.lock().await;
        let i = self
            .active
            .load(Ordering::SeqCst)
            .min(tabs.len().saturating_sub(1));
        tabs[i].rec.clone()
    }

    pub async fn new_tab(&self, url: Option<&str>) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let target = url
            .map(normalize_url)
            .unwrap_or_else(|| "about:blank".to_string());
        let page = self
            .browser
            .read()
            .await
            .new_page(CreateTargetParams::new(target))
            .await?;
        let tab = self.prepare(page).await?;
        let mut tabs = self.tabs.lock().await;
        tabs.push(tab);
        let idx = tabs.len() - 1;
        self.active.store(idx, Ordering::SeqCst);
        drop(tabs);
        self.reset_refs().await;
        Ok(json!({ "index": idx }))
    }

    pub async fn list_tabs(&self) -> Result<Value> {
        self.sync_tabs().await.ok();
        let tabs = self.tabs.lock().await;
        let active = self.active.load(Ordering::SeqCst);
        let mut out = Vec::new();
        for (i, t) in tabs.iter().enumerate() {
            let url = t.page.url().await.ok().flatten().unwrap_or_default();
            let title = t.page.get_title().await.ok().flatten().unwrap_or_default();
            out.push(json!({ "index": i, "url": url, "title": title, "active": i == active }));
        }
        Ok(json!({ "tabs": out, "active": active }))
    }

    pub async fn switch_tab(&self, index: usize) -> Result<Value> {
        self.ensure_not_taken_over()?;
        {
            let tabs = self.tabs.lock().await;
            if index >= tabs.len() {
                return Err(anyhow!("tab {index} does not exist (have {})", tabs.len()));
            }
            tabs[index].page.activate().await.ok();
        }
        self.active.store(index, Ordering::SeqCst);
        self.reset_refs().await;
        self.info().await
    }

    pub async fn close_tab(&self, index: usize) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let mut tabs = self.tabs.lock().await;
        if index >= tabs.len() {
            return Err(anyhow!("tab {index} does not exist"));
        }
        if tabs.len() == 1 {
            return Err(anyhow!("cannot close the last tab"));
        }
        let tab = tabs.remove(index);
        let _ = tab.page.close().await;
        let new_active = self.active.load(Ordering::SeqCst).min(tabs.len() - 1);
        self.active.store(new_active, Ordering::SeqCst);
        drop(tabs);
        self.reset_refs().await;
        Ok(json!({ "tabs_open": self.tabs.lock().await.len(), "active": new_active }))
    }

    // ---------------------------------------------------------- navigation ---

    pub async fn navigate(&self, url: &str) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let url = normalize_url(url);
        let page = self.active_page().await;
        self.active_recorder().await.reset_for_navigation();
        self.reset_refs().await;
        page.goto(&url).await?;
        self.settle(SETTLE_NAVIGATION).await;
        self.sync_tabs().await.ok();
        self.info().await
    }

    /// Walk the real session history rather than calling `history.back()`.
    ///
    /// `history.back()` is a request to the *page*, which a single-page app is
    /// free to intercept, and it tells us nothing about whether anything
    /// happened. `Page.navigateToHistoryEntry` moves the browser itself, and the
    /// entry list tells us up front whether there is anywhere to go.
    async fn history_go(&self, delta: i64) -> Result<Value> {
        let page = self.active_page().await;
        let hist = page.execute(GetNavigationHistoryParams::default()).await?;
        let cur = hist.result.current_index;
        let target = cur + delta;
        if target < 0 || target as usize >= hist.result.entries.len() {
            return Err(anyhow!(
                "nothing to go {} to",
                if delta < 0 { "back" } else { "forward" }
            ));
        }
        let id = hist.result.entries[target as usize].id;
        self.reset_refs().await;
        page.execute(NavigateToHistoryEntryParams::new(id)).await?;
        self.settle(SETTLE_NAVIGATION).await;
        self.info().await
    }

    pub async fn go_back(&self) -> Result<Value> {
        self.ensure_not_taken_over()?;
        self.history_go(-1).await
    }
    pub async fn go_forward(&self) -> Result<Value> {
        self.ensure_not_taken_over()?;
        self.history_go(1).await
    }

    pub async fn reload(&self) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        self.active_recorder().await.reset_for_navigation();
        self.reset_refs().await;
        page.reload().await?;
        self.settle(SETTLE_NAVIGATION).await;
        self.info().await
    }

    /// URL + title of the active page, plus anything the caller must know about
    /// before it tries to act — a modal dialog being the main one.
    pub async fn info(&self) -> Result<Value> {
        let page = self.active_page().await;
        // `url()` is answered from the handler's cached frame state, so it is
        // safe even when the renderer is suspended. `get_title()` is not — it
        // evaluates in the page.
        let url = page.url().await.ok().flatten().unwrap_or_default();

        if let Some(d) = self.active_recorder().await.dialog() {
            // Asking the page anything now would block until the dialog is
            // answered. This is not hypothetical: it is what made a `confirm()`
            // hang every tool result for the full 30-second dismissal timeout,
            // and by the time the call returned the dialog had been cleared, so
            // the report said there had never been one.
            let mut dv = json!({ "type": d.kind, "message": d.message });
            if !d.default_prompt.is_empty() {
                dv["defaultText"] = json!(d.default_prompt);
            }
            return Ok(json!({ "url": url, "title": "", "dialog": dv }));
        }

        // Belt and braces: a dialog can open between the check above and the
        // call below, and a page can be wedged for reasons of its own.
        let title = tokio::time::timeout(Duration::from_secs(3), page.get_title())
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten()
            .unwrap_or_default();
        Ok(json!({ "url": url, "title": title }))
    }

    /// Wait for the page to stop working.
    ///
    /// The old code slept 400–500ms after every action and hoped. That is the
    /// wrong shape twice over: on a static page it wastes half a second, and on
    /// a real one the XHR that the click fired has not even been *sent* yet, so
    /// the next snapshot shows the page as it was and the agent concludes its
    /// click did nothing.
    ///
    /// Instead: wait for the network to go quiet, the document to be ready, and
    /// the DOM to stop mutating. Bail out immediately on a dialog — the renderer
    /// is suspended, nothing will ever settle, and every evaluation below would
    /// block until the budget expired.
    pub async fn settle(&self, budget: Duration) {
        let started = Instant::now();
        let rec = self.active_recorder().await;
        let page = self.active_page().await;
        let mut quiet_since: Option<Instant> = None;
        while started.elapsed() < budget {
            if rec.dialog().is_some() {
                return;
            }
            let busy = rec.in_flight() > 0;
            let ready = matches!(
                page.evaluate_expression("document.readyState")
                    .await
                    .ok()
                    .and_then(|r| r.into_value::<String>().ok())
                    .as_deref(),
                Some("interactive") | Some("complete")
            );
            if !busy && ready {
                match quiet_since {
                    // Two consecutive quiet polls ~120ms apart: enough to let a
                    // click's XHR actually start before we call the page settled.
                    Some(t) if t.elapsed() >= Duration::from_millis(120) => break,
                    Some(_) => {}
                    None => quiet_since = Some(Instant::now()),
                }
            } else {
                quiet_since = None;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }

        // A quiet network says nothing about a framework still re-rendering from
        // data it already has. Wait for the DOM itself to hold still.
        if rec.dialog().is_none() {
            let remaining = budget
                .saturating_sub(started.elapsed())
                .as_millis()
                .min(1500)
                .max(200);
            let js = format!(
                r#"new Promise(res => {{
                    let t = setTimeout(() => {{ ob.disconnect(); res('quiet'); }}, 120);
                    const ob = new MutationObserver(() => {{
                        clearTimeout(t);
                        t = setTimeout(() => {{ ob.disconnect(); res('quiet'); }}, 120);
                    }});
                    if (document.body) ob.observe(document.body, {{ childList: true, subtree: true, attributes: true }});
                    setTimeout(() => {{ ob.disconnect(); res('cap'); }}, {remaining});
                }})"#
            );
            let params = chromiumoxide::cdp::js_protocol::runtime::EvaluateParams::builder()
                .expression(js)
                .await_promise(true)
                .return_by_value(true)
                .build();
            if let Ok(p) = params {
                let _ = tokio::time::timeout(Duration::from_millis(2500), page.evaluate(p)).await;
            }
        }
    }

    // ------------------------------------------------------------ snapshot ---

    /// Drop the cached capture. Refs stay valid — they are bound to nodes, not
    /// to a particular capture.
    async fn invalidate(&self) {
        *self.last.lock().await = None;
    }

    /// A new document: every ref describes an element that no longer exists.
    async fn reset_refs(&self) {
        self.refs.lock().await.reset();
        *self.last.lock().await = None;
    }

    /// Drop the refs if the active tab is showing a different document than the
    /// one they were minted against.
    ///
    /// Note the deliberate absence of a "helpful" fallback in `resolve`: an
    /// unknown ref used to trigger a fresh snapshot and a second lookup, on the
    /// theory that the ref might simply predate any capture. On a page that had
    /// navigated, that re-minted `e1..eN` and found `e5` again — a *different*
    /// element — so the stale ref quietly became a wrong click instead of an
    /// error. An unknown ref must fail.
    ///
    /// Clearing refs at the *call sites* that navigate is not enough, and the gap
    /// is dangerous rather than merely untidy: a click on a link navigates too,
    /// and after it the old refs still pointed at backend node ids that Chrome had
    /// since handed out to unrelated elements in the new document. Observed live —
    /// a ref from example.com resolved on iana.org and clicked something arbitrary,
    /// reporting success. A wrong click is far worse than a failed one, which is
    /// the whole reason refs are pinned to nodes in the first place.
    ///
    /// `loaderId` changes exactly when the document is replaced, which is the
    /// signal this needs. Same-document SPA route changes keep it, so refs stay
    /// stable where they legitimately can.
    async fn ensure_document(&self) {
        use chromiumoxide::cdp::browser_protocol::page::GetFrameTreeParams;

        let page = self.active_page().await;
        let ident = match page.execute(GetFrameTreeParams::default()).await {
            Ok(r) => Some((
                page.target_id().inner().clone(),
                r.result.frame_tree.frame.loader_id.inner().clone(),
            )),
            // If we cannot tell, assume the worst rather than risk a wrong click.
            Err(_) => None,
        };
        let mut cur = self.doc.lock().await;
        if *cur != ident {
            *cur = ident;
            drop(cur);
            self.reset_refs().await;
        }
    }

    /// Capture the page as an accessibility tree. See `snapshot.rs` for why this
    /// replaced the DOM-marking extractor.
    pub async fn snapshot(&self) -> Result<Snapshot> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        self.ensure_document().await;
        let page = self.active_page().await;
        let mut nodes = match page.execute(GetFullAxTree::default()).await {
            Ok(r) => r.result.nodes.clone(),
            Err(_) => {
                // Chrome wants the domain switched on before it will compute a
                // tree for some documents; enable and retry once.
                page.execute(
                    chromiumoxide::cdp::browser_protocol::accessibility::EnableParams::default(),
                )
                .await
                .ok();
                page.execute(GetFullAxTree::default())
                    .await?
                    .result
                    .nodes
                    .clone()
            }
        };
        stitch_frames(&page, &mut nodes).await;
        let clickables = clickable_backends(&page).await;
        let scroll = read_scroll(&page).await;
        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title = page.get_title().await.ok().flatten().unwrap_or_default();

        let snap = {
            let mut reg = self.refs.lock().await;
            snapshot::render(&nodes, &url, &title, &mut reg, &clickables, scroll)
        };
        *self.last.lock().await = Some(snap.clone());
        Ok(snap)
    }

    /// The current capture, taking a fresh one if the page has moved on.
    pub async fn current(&self) -> Result<Snapshot> {
        if let Some(s) = self.last.lock().await.clone() {
            return Ok(s);
        }
        self.snapshot().await
    }

    pub async fn find(&self, needle: &str) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let snap = self.current().await?;
        let hits = snapshot::find(&snap.tree, needle, 2);
        Ok(json!({
            "url": snap.url, "title": snap.title,
            "matches": if hits.is_empty() { Value::Null } else { json!(hits) },
        }))
    }

    async fn resolve(&self, r: &str) -> Result<BackendNodeId> {
        self.ensure_document().await;
        self.refs.lock().await.resolve(r).ok_or_else(|| {
            anyhow!("ref '{r}' is not on the current page — take a fresh browser_snapshot and use a ref from it")
        })
    }

    /// Refuse to act while a modal dialog is up.
    ///
    /// A JavaScript dialog suspends the renderer: clicks queue, evaluations
    /// hang, screenshots never return. Playwright's MCP server treats this as a
    /// modal state that gates every other tool, and it is right to — without the
    /// gate the agent issues a click, waits out the full timeout, sees nothing
    /// change, and concludes the *page* is broken.
    async fn ensure_no_dialog(&self) -> Result<()> {
        if let Some(d) = self.active_recorder().await.dialog() {
            return Err(anyhow!(
                "a {} dialog is open and blocking the page: {:?}. Answer it with browser_handle_dialog before doing anything else.",
                d.kind, d.message
            ));
        }
        Ok(())
    }

    /// Viewport coordinates of an element, scrolling it into view first.
    ///
    /// `DOM.getContentQuads` returns quads the browser computed in *main-frame*
    /// viewport space, so this is correct for an element nested inside an
    /// iframe — which the old `getBoundingClientRect` math was not.
    async fn point_of(&self, backend: &BackendNodeId) -> Result<(f64, f64)> {
        let page = self.active_page().await;
        page.execute(
            ScrollIntoViewIfNeededParams::builder()
                .backend_node_id(backend.clone())
                .build(),
        )
        .await
        .map_err(|e| {
            anyhow!("element is not in the page any more ({e}) — take a fresh snapshot")
        })?;

        let quads = page
            .execute(
                GetContentQuadsParams::builder()
                    .backend_node_id(backend.clone())
                    .build(),
            )
            .await?
            .result
            .quads
            .clone();

        // An inline element wraps across lines and reports one quad per line.
        // Aim at the largest, which is the piece a person would actually hit.
        let best = quads
            .iter()
            .map(|q| q.inner().clone())
            .filter(|q| q.len() == 8)
            .max_by(|a, b| {
                quad_area(a)
                    .partial_cmp(&quad_area(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .ok_or_else(|| {
                anyhow!("element has no visible box — it may be hidden or zero-sized")
            })?;

        let x = (best[0] + best[2] + best[4] + best[6]) / 4.0;
        let y = (best[1] + best[3] + best[5] + best[7]) / 4.0;
        Ok((x, y))
    }

    /// Run a function with the element as `this`, in the page's own world.
    async fn call_on(&self, backend: &BackendNodeId, decl: &str) -> Result<Value> {
        let page = self.active_page().await;
        let obj = page
            .execute(
                ResolveNodeParams::builder()
                    .backend_node_id(backend.clone())
                    .build(),
            )
            .await?
            .result
            .object
            .clone();
        let object_id = obj
            .object_id
            .ok_or_else(|| anyhow!("could not get a handle on the element"))?;
        let res = page
            .execute(
                CallFunctionOnParams::builder()
                    .object_id(object_id)
                    .function_declaration(decl.to_string())
                    .return_by_value(true)
                    .await_promise(true)
                    .build()
                    .map_err(anyhow::Error::msg)?,
            )
            .await?;
        Ok(res.result.result.value.clone().unwrap_or(Value::Null))
    }

    // ------------------------------------------------------------- actions ---

    pub async fn click_ref(&self, r: &str, button: &str, click_count: u32) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let backend = self.resolve(r).await?;
        let (x, y) = self.point_of(&backend).await?;
        let page = self.active_page().await;
        input::human_click(&page, &self.cursor, x, y, button, click_count).await?;
        self.after_action().await;
        Ok(json!({ "clicked": r, "at": [x.round(), y.round()] }))
    }

    pub async fn click_xy(&self, x: f64, y: f64) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        input::human_click(&page, &self.cursor, x, y, "left", 1).await?;
        self.after_action().await;
        Ok(json!({ "clicked_at": [x, y] }))
    }

    pub async fn hover_ref(&self, r: &str) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let backend = self.resolve(r).await?;
        let (x, y) = self.point_of(&backend).await?;
        let page = self.active_page().await;
        input::human_move(&page, &self.cursor, x, y).await?;
        // Hover menus animate open; give them a beat before the next snapshot.
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.invalidate().await;
        Ok(json!({ "hovered": r, "at": [x.round(), y.round()] }))
    }

    /// Type into a field — and confirm the text actually arrived.
    ///
    /// This used to dispatch keystrokes at whatever coordinates the ref resolved
    /// to and report success unconditionally. When the ref was not a text field
    /// — a wrapper `<div>` around the input, which the clickable-by-style pass
    /// makes it easier to pick — the keys went to the document and were lost,
    /// while the tool answered `{"typed_into":"e12","chars":14}`. The agent then
    /// told the planner it had typed and submitted, the check said the page had
    /// not moved, and the two disagreed for as many plans as the budget allowed.
    /// That loop was the whole failure.
    ///
    /// So: refuse a target that cannot hold text, and read the value back
    /// afterwards. A tool that reports what happened is worth far more to the
    /// loop above it than one that always claims success.
    pub async fn type_ref(
        &self,
        r: &str,
        text: &str,
        submit: bool,
        replace: bool,
    ) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let backend = self.resolve(r).await?;

        let probe = self
            .call_on(
                &backend,
                r#"function () {
                    const tag = this.tagName ? this.tagName.toLowerCase() : '';
                    const type = (this.getAttribute && this.getAttribute('type') || '').toLowerCase();
                    const role = (this.getAttribute && this.getAttribute('role') || '').toLowerCase();
                    const editable = (tag === 'textarea')
                        || (tag === 'input' && !['button','submit','reset','checkbox','radio','file','image','range','color'].includes(type))
                        || !!this.isContentEditable
                        || ['textbox','searchbox','combobox'].includes(role);
                    // If this is a wrapper, point at the field it wraps so the
                    // error can say what to use instead.
                    let inner = null;
                    if (!editable && this.querySelector) {
                        const el = this.querySelector('input:not([type=hidden]),textarea,[contenteditable=""],[contenteditable=true]');
                        if (el) inner = (el.tagName || '').toLowerCase() + (el.type ? '[type=' + el.type + ']' : '');
                    }
                    // Is this field credential-shaped? `type=password` is the
                    // easy case and not the only one: a one-time code, a CVV or
                    // a PIN is a plain text input, and a "show password" toggle
                    // turns a password field into one. Anything matching gets
                    // masked before it reaches the stored transcript.
                    const hint = [
                        this.getAttribute && this.getAttribute('name'),
                        this.id,
                        this.getAttribute && this.getAttribute('autocomplete'),
                        this.getAttribute && this.getAttribute('aria-label'),
                        this.getAttribute && this.getAttribute('placeholder'),
                    ].filter(Boolean).join(' ').toLowerCase();
                    const secret = type === 'password'
                        || /pass|pwd|otp|one-?time|verification|2fa|mfa|cvv|cvc|csc|\bpin\b|secret|token|security[- ]?code|mã\s*(otp|xác|bí)/.test(hint);
                    return { tag, type, role, editable, inner, secret,
                             label: (this.getAttribute && (this.getAttribute('aria-label') || this.getAttribute('placeholder'))) || '' };
                }"#,
            )
            .await?;

        if !probe["editable"].as_bool().unwrap_or(false) {
            let what = probe["tag"].as_str().unwrap_or("element").to_string();
            let hint = match probe["inner"].as_str() {
                Some(inner) => format!(
                    " It contains a <{inner}> — take a fresh snapshot and use that element's ref instead."
                ),
                None => " Use a ref whose role is textbox, searchbox or combobox.".to_string(),
            };
            return Err(anyhow!(
                "ref {r} is a <{what}>, which cannot accept typed text — nothing was typed.{hint}"
            ));
        }

        let (x, y) = self.point_of(&backend).await?;
        let page = self.active_page().await;
        input::human_click(&page, &self.cursor, x, y, "left", 1).await?;
        tokio::time::sleep(Duration::from_millis(60)).await;
        if replace {
            // Select-all then type: a plain `value = ''` would not fire the
            // events a React-controlled input listens for.
            input::select_all(&page).await?;
        }
        input::type_text(&page, text).await?;

        // Read the field back *before* submitting: once the page navigates the
        // element is gone and there is nothing left to check.
        let secret = probe["secret"].as_bool().unwrap_or(false);
        let got = self
            .call_on(&backend, "function () { return this.value !== undefined ? String(this.value) : (this.innerText || ''); }")
            .await
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        let landed = got.contains(text.trim()) || text.trim().is_empty();

        if !landed {
            // The diagnostic is useful and the contents are not ours to repeat.
            if secret {
                return Err(anyhow!(
                    "typed into {r} but the field did not take it — the keystrokes did not reach it. Click the field first, or use a different ref."
                ));
            }
            return Err(anyhow!(
                "typed into {r} but the field now reads {:?} instead of {:?} — the keystrokes did not reach it. Click the field first, or use a different ref.",
                truncate_for_msg(&got),
                truncate_for_msg(text)
            ));
        }

        if submit {
            input::press_key(&page, "Enter").await?;
        }
        self.after_action().await;

        // A credential-shaped field reports that it was filled and nothing more.
        // The run log is written to disk and fed back into later prompts, so
        // what goes in it is a lasting decision, not a debugging convenience.
        let mut out = json!({ "typed_into": r, "chars": text.chars().count(), "submit": submit });
        if secret {
            out["secret"] = json!(true);
        } else {
            out["value"] = json!(got);
        }
        Ok(out)
    }

    /// Type into whatever currently has focus (the live view's keystroke path).
    pub async fn type_text(&self, text: &str) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        input::type_text(&page, text).await?;
        self.invalidate().await;
        Ok(json!({ "typed": text.chars().count() }))
    }

    /// Pick option(s) in a `<select>`.
    ///
    /// Done in the page rather than by clicking, because a native select popup
    /// is an OS-level widget that renders outside the page — there is nothing on
    /// the surface for a synthetic mouse event to hit.
    pub async fn select_option(&self, r: &str, values: &[String]) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let backend = self.resolve(r).await?;
        let wanted = serde_json::to_string(values)?;
        let decl = format!(
            r#"function () {{
                if (this.tagName !== 'SELECT') return {{ error: 'ref is a <' + this.tagName.toLowerCase() + '>, not a <select> — click it instead' }};
                const want = {wanted};
                let hit = 0;
                for (const o of this.options) {{
                    const on = want.includes(o.value) || want.includes(o.label) || want.includes(o.text.trim());
                    o.selected = on;
                    if (on) hit++;
                }}
                if (!hit) return {{ error: 'no option matched', available: [...this.options].map(o => o.text.trim()).slice(0, 50) }};
                this.dispatchEvent(new Event('input', {{ bubbles: true }}));
                this.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return {{ selected: [...this.selectedOptions].map(o => o.text.trim()) }};
            }}"#
        );
        let out = self.call_on(&backend, &decl).await?;
        if let Some(err) = out.get("error").and_then(|e| e.as_str()) {
            let extra = out
                .get("available")
                .map(|a| format!(" (available: {a})"))
                .unwrap_or_default();
            return Err(anyhow!("{err}{extra}"));
        }
        self.after_action().await;
        Ok(out)
    }

    /// Fill several fields in one go.
    ///
    /// Worth having as its own operation rather than a loop the model writes:
    /// a login or checkout form is the single most common multi-step task, and
    /// doing it field-by-field costs one model round-trip per field, each one
    /// another chance to mis-ref after a re-render. Both Playwright's and
    /// Chrome's MCP servers ship this and tell the model to prefer it.
    ///
    /// Fields are applied in order and the first failure stops the run, with the
    /// successes reported — a half-filled form the model can see is far easier
    /// to recover from than an opaque failure.
    pub async fn fill_form(&self, fields: &[Value]) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let mut done = Vec::new();
        for f in fields {
            let Some(r) = f["ref"].as_str().or_else(|| f["target"].as_str()) else {
                return Err(anyhow!("each field needs a 'ref'"));
            };
            let value = match &f["value"] {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            };
            let kind = f["type"].as_str().unwrap_or("textbox");
            let outcome = match kind {
                "checkbox" | "radio" => {
                    let want = value != "false" && !value.is_empty();
                    self.set_checked(r, want).await
                }
                "combobox" | "select" => self.select_option(r, &[value.clone()]).await,
                _ => self.type_ref(r, &value, false, true).await,
            };
            match outcome {
                Ok(_) => done.push(json!({ "ref": r, "type": kind, "ok": true })),
                Err(e) => {
                    return Err(anyhow!(
                        "filled {} of {} fields, then '{}' failed: {e}",
                        done.len(),
                        fields.len(),
                        r
                    ))
                }
            }
        }
        Ok(json!({ "filled": done }))
    }

    /// Click a checkbox/radio only if it is not already in the wanted state —
    /// blindly clicking a checked box unchecks it.
    async fn set_checked(&self, r: &str, want: bool) -> Result<Value> {
        let backend = self.resolve(r).await?;
        let is = self
            .call_on(&backend, "function () { return !!this.checked; }")
            .await?
            .as_bool()
            .unwrap_or(false);
        if is == want {
            return Ok(json!({ "ref": r, "already": want }));
        }
        self.click_ref(r, "left", 1).await
    }

    /// Outline an element in the live view.
    ///
    /// This exists because of what makes this browser unusual: a person is
    /// watching the same tab the AI is driving. Showing them what it is about to
    /// touch turns an opaque automation into something supervisable — they can
    /// stop it before the click rather than read about it afterwards. The
    /// overlay is `pointer-events: none` and removes itself, so it never
    /// intercepts the click it is describing.
    pub async fn highlight_ref(&self, r: &str, ms: u64) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let backend = self.resolve(r).await?;
        let ms = ms.clamp(200, 5000);
        let decl = format!(
            r#"function () {{
                const r = this.getBoundingClientRect();
                const d = document.createElement('div');
                d.style.cssText = 'position:fixed;pointer-events:none;z-index:2147483647;' +
                  'border:2px solid #ff3b30;border-radius:4px;box-shadow:0 0 0 4px rgba(255,59,48,.25);' +
                  'transition:opacity .2s;left:' + (r.left-2) + 'px;top:' + (r.top-2) + 'px;' +
                  'width:' + r.width + 'px;height:' + r.height + 'px;';
                document.documentElement.appendChild(d);
                setTimeout(() => {{ d.style.opacity = '0'; setTimeout(() => d.remove(), 250); }}, {ms});
                return {{ x: Math.round(r.left), y: Math.round(r.top), w: Math.round(r.width), h: Math.round(r.height) }};
            }}"#
        );
        let box_ = self.call_on(&backend, &decl).await?;
        Ok(json!({ "highlighted": r, "box": box_ }))
    }

    pub async fn drag(&self, from: &str, to: &str) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let a = self.resolve(from).await?;
        let b = self.resolve(to).await?;
        let (x1, y1) = self.point_of(&a).await?;
        let (x2, y2) = self.point_of(&b).await?;
        let page = self.active_page().await;
        input::human_drag(&page, &self.cursor, x1, y1, x2, y2).await?;
        self.after_action().await;
        Ok(json!({ "dragged": from, "onto": to }))
    }

    pub async fn press_key(&self, key: &str) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        input::press_key(&page, key).await?;
        self.after_action().await;
        Ok(json!({ "pressed": key }))
    }

    pub async fn scroll(&self, dx: f64, dy: f64) -> Result<Value> {
        self.ensure_no_dialog().await?;
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        let (w, h) = *self.viewport.lock().await;
        input::scroll(&page, w as f64 / 2.0, h as f64 / 2.0, dx, dy).await?;
        // Lazy lists need a moment to render what just came into view.
        tokio::time::sleep(Duration::from_millis(300)).await;
        self.invalidate().await;
        Ok(json!({ "scrolled": [dx, dy] }))
    }

    /// Scroll an element into view — the reliable way to reach content inside a
    /// scrollable pane, which page-level scrolling never touches.
    pub async fn scroll_to_ref(&self, r: &str) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let backend = self.resolve(r).await?;
        let page = self.active_page().await;
        page.execute(
            ScrollIntoViewIfNeededParams::builder()
                .backend_node_id(backend)
                .build(),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(250)).await;
        self.invalidate().await;
        Ok(json!({ "scrolled_to": r }))
    }

    /// Answer the file chooser that a previous click opened.
    pub async fn upload_files(&self, paths: &[String]) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let rec = self.active_recorder().await;
        let backend = rec
            .file_chooser()
            .ok_or_else(|| anyhow!("no file chooser is open — click the upload control first"))?;
        for p in paths {
            if !std::path::Path::new(p).exists() {
                return Err(anyhow!("file does not exist: {p}"));
            }
        }
        let page = self.active_page().await;
        page.execute(
            SetFileInputFilesParams::builder()
                .files(paths.to_vec())
                .backend_node_id(BackendNodeId::new(backend))
                .build()
                .map_err(anyhow::Error::msg)?,
        )
        .await?;
        rec.set_file_chooser(None);
        self.after_action().await;
        Ok(json!({ "uploaded": paths }))
    }

    pub async fn handle_dialog(&self, accept: bool, prompt_text: Option<&str>) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let rec = self.active_recorder().await;
        let d = rec.dialog().ok_or_else(|| anyhow!("no dialog is open"))?;
        let page = self.active_page().await;
        let mut b = HandleJavaScriptDialogParams::builder().accept(accept);
        if let Some(t) = prompt_text {
            b = b.prompt_text(t.to_string());
        }
        page.execute(b.build().map_err(anyhow::Error::msg)?).await?;
        rec.set_dialog(None);
        self.after_action().await;
        Ok(json!({ "dialog": d.kind, "accepted": accept }))
    }

    /// Wait for text to appear, for text to disappear, or for a fixed time.
    pub async fn wait_for(
        &self,
        text: Option<&str>,
        text_gone: Option<&str>,
        seconds: Option<f64>,
    ) -> Result<Value> {
        self.ensure_not_taken_over()?;
        if let Some(s) = seconds {
            tokio::time::sleep(Duration::from_secs_f64(s.clamp(0.0, 30.0))).await;
            self.invalidate().await;
            return Ok(json!({ "waited": s }));
        }
        let needle = text
            .or(text_gone)
            .ok_or_else(|| anyhow!("give text, textGone or time"))?;
        let want_present = text.is_some();
        let deadline = Instant::now() + Duration::from_secs(15);
        let page = self.active_page().await;
        while Instant::now() < deadline {
            let body: String = page
                .evaluate_expression("document.body ? document.body.innerText : ''")
                .await
                .ok()
                .and_then(|r| r.into_value().ok())
                .unwrap_or_default();
            if body.contains(needle) == want_present {
                self.invalidate().await;
                return Ok(json!({ "ok": true, "text": needle, "present": want_present }));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        Err(anyhow!(
            "timed out after 15s waiting for {:?} to {}",
            needle,
            if want_present { "appear" } else { "disappear" }
        ))
    }

    /// Resize by moving the real OS window, not by overriding device metrics.
    ///
    /// `Emulation.setDeviceMetricsOverride` would be the obvious call and it is
    /// the wrong one. It makes `window.screen` disagree with the actual display,
    /// pins a `screenOrientation`, and puts us in the `Emulation.*` domain that
    /// anti-bot vendors specifically watch. Moving the window keeps every
    /// dimension the page can read genuinely consistent, because it genuinely is.
    pub async fn resize(&self, width: u32, height: u32) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let w = width.clamp(320, 3840);
        let h = height.clamp(240, 2160);
        let page = self.active_page().await;
        let win = page
            .execute(GetWindowForTargetParams::default())
            .await?
            .result
            .window_id
            .clone();
        page.execute(SetWindowBoundsParams::new(
            win,
            Bounds::builder().width(w as i64).height(h as i64).build(),
        ))
        .await?;
        // The window is not the viewport — subtract whatever the browser's own
        // chrome takes — so read back what the page actually got.
        tokio::time::sleep(Duration::from_millis(150)).await;
        let (vw, vh) = self.measure_viewport().await;
        self.invalidate().await;
        Ok(json!({ "window": [w, h], "viewport": [vw, vh] }))
    }

    /// The page's real `innerWidth`/`innerHeight`.
    ///
    /// Worth measuring rather than assuming: the live view maps a click in the
    /// browser UI onto page coordinates, and if the assumed size is off by the
    /// height of a bookmarks bar, every click the user makes lands slightly
    /// above where they aimed.
    async fn measure_viewport(&self) -> (u32, u32) {
        let page = self.active_page().await;
        let got: Option<(u32, u32)> = page
            .evaluate_expression("[innerWidth, innerHeight]")
            .await
            .ok()
            .and_then(|r| r.into_value::<Vec<f64>>().ok())
            .filter(|v| v.len() == 2 && v[0] > 0.0 && v[1] > 0.0)
            .map(|v| (v[0] as u32, v[1] as u32));
        if let Some(v) = got {
            *self.viewport.lock().await = v;
            return v;
        }
        *self.viewport.lock().await
    }

    pub async fn viewport(&self) -> (u32, u32) {
        self.measure_viewport().await
    }

    /// After anything that can change the page: let it settle and drop the refs,
    /// which now describe a document that may no longer exist.
    async fn after_action(&self) {
        self.settle(SETTLE_ACTION).await;
        self.invalidate().await;
        self.sync_tabs().await.ok();
    }

    // ---------------------------------------------------------- inspection ---

    pub async fn screenshot_b64(&self, full_page: bool) -> Result<String> {
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Jpeg)
            .quality(55)
            .full_page(full_page)
            .build();
        let bytes = page.screenshot(params).await?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub async fn execute_js(&self, script: &str) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        // Wrap so both expressions and statement blocks work, and stringify the
        // result so any JSON-serializable value comes back intact.
        let wrapped = format!(
            "(() => {{ try {{ return JSON.stringify((function(){{ {script} }})()); }} catch(e) {{ return JSON.stringify({{__error: String(e)}}); }} }})()"
        );
        let raw: String = page
            .evaluate_expression(wrapped)
            .await?
            .into_value()
            .unwrap_or_default();
        let v: Value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
        self.invalidate().await;
        Ok(v)
    }

    pub async fn extract_text(&self, selector: Option<&str>) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        let sel = selector.unwrap_or("body");
        let js = format!(
            r#"(() => {{ const el=document.querySelector({sel:?}); return JSON.stringify(el ? (el.innerText||el.textContent||'') : ''); }})()"#
        );
        let raw: String = page
            .evaluate_expression(js)
            .await?
            .into_value()
            .unwrap_or_default();
        let text: String = serde_json::from_str(&raw).unwrap_or_default();
        Ok(json!({ "text": text }))
    }

    pub async fn extract_links(&self) -> Result<Value> {
        self.ensure_not_taken_over()?;
        let page = self.active_page().await;
        let raw: String = page
            .evaluate_expression(LINKS_JS)
            .await?
            .into_value()
            .unwrap_or_default();
        Ok(serde_json::from_str(&raw).unwrap_or(json!([])))
    }

    pub async fn downloads(&self) -> Vec<Value> {
        self.active_recorder().await.downloads()
    }

    /// Subscribe to the preview stream.
    pub fn frames(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.frames.subscribe()
    }

    fn active_target(&self) -> Option<String> {
        self.tabs.try_lock().ok().and_then(|t| {
            let i = self
                .active
                .load(Ordering::SeqCst)
                .min(t.len().saturating_sub(1));
            t.get(i).map(|t| t.page.target_id().inner().clone())
        })
    }
}

/// Feed the live view.
///
/// This matters more than it used to. The browser now runs with no visible
/// window by default, so this stream *is* the browser as far as the user is
/// concerned — a laggy preview is no longer a cosmetic issue.
///
/// It uses CDP screencast rather than polling `captureScreenshot` on a timer.
/// Chrome pushes a frame when the page actually composites something, which is
/// both smoother while things are moving (measured ~10 fps against ~3) and free
/// while they are not — a timer pays the full cost of encoding a JPEG several
/// times a second to show a page that has not changed.
///
/// Screencast is per-target, so the pump restarts whenever the active tab
/// changes, and falls back to polling if it cannot be started at all.
pub fn spawn_preview_pump(session: std::sync::Arc<BrowserSession>) {
    use chromiumoxide::cdp::browser_protocol::page::{
        EventScreencastFrame, ScreencastFrameAckParams, StartScreencastFormat,
        StartScreencastParams, StopScreencastParams,
    };
    use futures_util::StreamExt;

    tokio::spawn(async move {
        loop {
            let page = session.active_page().await;
            let target = page.target_id().inner().clone();

            // Nothing is streamed while the person holds the browser. They are
            // typing a password into the real window; encoding that page to JPEG
            // and broadcasting it over a socket would make "the AI is not
            // watching" false in the most literal way available.
            if session.in_takeover() {
                tokio::time::sleep(Duration::from_millis(400)).await;
                continue;
            }

            let started = page
                .event_listener::<EventScreencastFrame>()
                .await
                .ok()
                .map(|ev| (ev, page.clone()));

            let Some((mut ev, page)) = started else {
                poll_preview(&session, &target).await;
                continue;
            };

            let start = |p: Page| async move {
                p.execute(
                    StartScreencastParams::builder()
                        .format(StartScreencastFormat::Jpeg)
                        .quality(60)
                        .max_width(1600)
                        .max_height(1000)
                        .every_nth_frame(1)
                        .build(),
                )
                .await
                .is_ok()
            };
            if !start(page.clone()).await {
                poll_preview(&session, &target).await;
                continue;
            }

            let mut idle = tokio::time::interval(Duration::from_millis(1500));
            idle.tick().await;
            loop {
                tokio::select! {
                    frame = ev.next() => {
                        let Some(frame) = frame else { break };
                        // Ack first: Chrome will not send another frame until the
                        // previous one is acknowledged, so a slow consumer stalls
                        // the stream rather than dropping frames.
                        // Events arrive behind an Arc, so copy the payload out.
                        let data: String = AsRef::<str>::as_ref(&frame.data).to_string();
                        page.execute(ScreencastFrameAckParams::new(frame.session_id.clone())).await.ok();
                        let _ = session.frames.send(data);
                    }
                    _ = idle.tick() => {
                        if session.in_takeover()
                            || session.active_target().as_deref() != Some(target.as_str())
                        {
                            break;
                        }
                        // A navigation can quietly end the cast. Re-issuing it is
                        // cheap and stops the preview freezing on the old page.
                        if !start(page.clone()).await {
                            break;
                        }
                    }
                }
            }
            page.execute(StopScreencastParams::default()).await.ok();
        }
    });
}

/// Fallback for when screencast will not start: the original timer.
async fn poll_preview(session: &std::sync::Arc<BrowserSession>, target: &str) {
    let mut tick = tokio::time::interval(Duration::from_millis(330));
    while !session.in_takeover() && session.active_target().as_deref() == Some(target) {
        tick.tick().await;
        if let Ok(data) = session.screenshot_b64(false).await {
            let _ = session.frames.send(data);
        }
    }
}

/// A page can style a great many things as clickable; past this we are flooding
/// the model rather than informing it.
const MAX_CLICKABLES: usize = 250;

/// Ask Chrome which elements the page itself styles as actionable.
///
/// This closes the one real blind spot of an accessibility-tree snapshot. A
/// `<div onclick>` carrying no role and no ARIA is not an accessibility object,
/// so Chrome reports it as `generic` or ignores it — and an enormous amount of
/// application UI is built exactly that way. The agent would read the label and
/// have no idea it was a target.
///
/// The signal used is computed `cursor`, which is the same cue a sighted person
/// acts on, and it is queried through `DOM.getNodesForSubtreeByStyle` — so
/// Chrome does the traversal (piercing shadow roots) and we still never touch
/// the page. The in-page libraries that pioneered this heuristic have to inject
/// a script to get the same answer.
async fn clickable_backends(page: &Page) -> std::collections::HashSet<i64> {
    use chromiumoxide::cdp::browser_protocol::dom::{
        CssComputedStyleProperty, DescribeNodeParams, GetDocumentParams,
        GetNodesForSubtreeByStyleParams,
    };

    let mut out = std::collections::HashSet::new();
    let Ok(doc) = page
        .execute(GetDocumentParams::builder().depth(0).build())
        .await
    else {
        return out;
    };
    // `pointer` alone covers the overwhelming majority. The rarer grab/resize
    // cursors mark controls a person can act on too, and they cost nothing to
    // ask for in the same call.
    let styles: Vec<CssComputedStyleProperty> = ["pointer", "grab", "cell", "context-menu"]
        .iter()
        .map(|v| CssComputedStyleProperty::new("cursor", *v))
        .collect();

    let Ok(found) = page
        .execute(
            GetNodesForSubtreeByStyleParams::builder()
                .node_id(doc.result.root.node_id.clone())
                .computed_styles(styles)
                .pierce(true)
                .build()
                .expect("style query"),
        )
        .await
    else {
        return out;
    };

    for id in found.result.node_ids.iter().take(MAX_CLICKABLES) {
        if let Ok(d) = page
            .execute(DescribeNodeParams::builder().node_id(id.clone()).build())
            .await
        {
            out.insert(*d.result.node.backend_node_id.inner());
        }
    }
    out
}

/// Where the viewport sits in the document.
async fn read_scroll(page: &Page) -> snapshot::Scroll {
    let v: Vec<f64> = page
        .evaluate_expression(
            "(() => { const d = document.documentElement, b = document.body;              return [window.scrollY || 0,                      Math.max(d ? d.scrollHeight : 0, b ? b.scrollHeight : 0, innerHeight),                      innerHeight]; })()",
        )
        .await
        .ok()
        .and_then(|r| r.into_value().ok())
        .unwrap_or_default();
    if v.len() == 3 {
        snapshot::Scroll {
            y: v[0],
            height: v[1],
            viewport: v[2],
        }
    } else {
        snapshot::Scroll::default()
    }
}


/// Build and start Chromium.
///
/// `window_size` is a real Chrome flag — the OS window genuinely becomes this
/// size, so `screen`, `outerWidth`, `innerWidth` and `devicePixelRatio` all stay
/// native and agree with each other.
///
/// `viewport(None)` is load-bearing. Passing a viewport makes chromiumoxide run
/// `Emulation.setDeviceMetricsOverride` plus — hardcoded, regardless of the
/// `has_touch` we ask for — `Emulation.setTouchEmulationEnabled(true)`. That
/// gave this browser `navigator.maxTouchPoints >= 1` behind a `Macintosh`
/// user-agent: a Mac claiming a touchscreen, and a `screenOrientation` angle no
/// desktop reports. Cross-attribute impossibilities like that are the strongest
/// published signal for spotting an evasive browser, so the fix is to stop
/// emulating rather than to emulate more carefully.
pub async fn launch_browser(
    profile: &std::path::Path,
    chrome: Option<&str>,
    headless: bool,
    clear_locks: bool,
) -> Result<(Browser, Page)> {
    use chromiumoxide::BrowserConfig;
    use futures_util::StreamExt;

    std::fs::create_dir_all(profile).ok();
    // A profile left by an instance killed uncleanly keeps its lock files and
    // the next launch aborts on them — so cold start clears them.
    //
    // A *relaunch* must not, and the distinction matters: those files are the
    // only thing stopping two Chromes writing one profile at once. Clearing
    // them while the previous browser was still alive meant both wrote the
    // profile, and then the old one's clean shutdown overwrote what the new one
    // had saved — which is exactly the login the takeover exists to capture.
    if clear_locks {
        for lock in ["SingletonLock", "SingletonSocket", "SingletonCookie"] {
            std::fs::remove_file(profile.join(lock)).ok();
        }
    }

    let mut builder = BrowserConfig::builder()
        .disable_default_args()
        .args(stealth::chrome_args())
        .user_data_dir(profile)
        .window_size(1280, 800)
        .viewport(None);

    builder = if headless { builder.new_headless_mode() } else { builder.with_head() };
    if let Some(path) = chrome {
        builder = builder.chrome_executable(path);
    }

    let config = builder.build().map_err(anyhow::Error::msg)?;
    let (browser, mut handler) = Browser::launch(config).await?;

    // The handler MUST be polled for the connection to work. Keep draining it
    // regardless of transient per-event errors — only stop when the stream ends
    // (browser actually gone).
    tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = browser.new_page(CreateTargetParams::new("about:blank")).await?;
    Ok((browser, page))
}

/// How many frames one snapshot will descend into. An ad-heavy page can carry
/// dozens of tracking iframes, and each one is a round-trip.
const MAX_FRAMES: usize = 12;

/// Splice each iframe's accessibility tree in under the iframe element.
///
/// `Accessibility.getFullAXTree` returns the tree for *one* document. An
/// `<iframe>` therefore comes back as a childless leaf, and everything inside it
/// — which on a real page means the payment form, the OAuth prompt, the CAPTCHA,
/// the embedded video player — is invisible to the agent. Verified against a
/// real Chrome: the parent tree ends at `- iframe [ref=e19]` with nothing under
/// it.
///
/// So each iframe node is resolved to its frame through `DOM.describeNode`, that
/// frame's tree is fetched separately, and its root is attached as a child.
/// Node ids are namespaced per frame first, because ids are only unique within
/// one document and a collision would silently graft one frame's subtree onto
/// another's.
async fn stitch_frames(page: &Page, nodes: &mut Vec<crate::snapshot::AxNode>) {
    use chromiumoxide::cdp::browser_protocol::dom::DescribeNodeParams;

    let mut pending: Vec<usize> = (0..nodes.len()).collect();
    let mut fetched = 0usize;

    while let Some(i) = pending.pop() {
        if fetched >= MAX_FRAMES {
            break;
        }
        let is_iframe = nodes[i]
            .role
            .as_ref()
            .and_then(|r| r.value.as_ref())
            .and_then(|v| v.as_str())
            .map(|r| r.eq_ignore_ascii_case("iframe"))
            .unwrap_or(false);
        if !is_iframe {
            continue;
        }
        let Some(backend) = nodes[i].backend_dom_node_id else {
            continue;
        };

        // An iframe element's DOM node carries the id of the frame it hosts.
        let frame_id = match page
            .execute(
                DescribeNodeParams::builder()
                    .backend_node_id(BackendNodeId::new(backend))
                    .build(),
            )
            .await
        {
            Ok(r) => r.result.node.frame_id.clone(),
            Err(_) => None,
        };
        let Some(frame_id) = frame_id else { continue };

        let sub = match page
            .execute(GetFullAxTree {
                frame_id: Some(frame_id.inner().clone()),
            })
            .await
        {
            // A cross-origin frame lives in another process and answers with an
            // error. Nothing to do about that here; the iframe still shows up as
            // a leaf, which at least tells the agent something is embedded.
            Ok(r) => r.result.nodes.clone(),
            Err(_) => continue,
        };
        if sub.is_empty() {
            continue;
        }
        fetched += 1;

        let prefix = format!("f{fetched}:");
        let claimed: std::collections::HashSet<String> = sub
            .iter()
            .filter_map(|n| n.child_ids.clone())
            .flatten()
            .collect();
        let root_ids: Vec<String> = sub
            .iter()
            .filter(|n| !claimed.contains(&n.node_id))
            .map(|n| format!("{prefix}{}", n.node_id))
            .collect();

        let base = nodes.len();
        for mut n in sub {
            n.node_id = format!("{prefix}{}", n.node_id);
            if let Some(kids) = n.child_ids.as_mut() {
                for k in kids.iter_mut() {
                    *k = format!("{prefix}{k}");
                }
            }
            nodes.push(n);
        }
        nodes[i]
            .child_ids
            .get_or_insert_with(Vec::new)
            .extend(root_ids);
        // A frame can itself contain frames.
        pending.extend(base..nodes.len());
    }
}

/// Clip a value for an error message without splitting a multi-byte character.
fn truncate_for_msg(s: &str) -> String {
    if s.chars().count() <= 60 {
        s.to_string()
    } else {
        s.chars().take(60).collect::<String>() + "…"
    }
}

fn quad_area(q: &[f64]) -> f64 {
    // Shoelace over the four corners.
    let mut a = 0.0;
    for i in 0..4 {
        let (x1, y1) = (q[i * 2], q[i * 2 + 1]);
        let (x2, y2) = (q[(i + 1) % 4 * 2], q[(i + 1) % 4 * 2 + 1]);
        a += x1 * y2 - x2 * y1;
    }
    (a / 2.0).abs()
}

/// Subscribe to everything a page can tell us. Each stream gets its own task;
/// dropping the stream would silently unsubscribe, so the task owns it for the
/// life of the page.
async fn wire_events(page: &Page, rec: Recorder, downloads: &std::path::Path) {
    use chromiumoxide::cdp::browser_protocol::browser::EventDownloadWillBegin;
    use chromiumoxide::cdp::browser_protocol::network::{
        EventLoadingFailed, EventRequestWillBeSent, EventResponseReceived,
    };
    use chromiumoxide::cdp::browser_protocol::page::{
        EventFileChooserOpened, EventJavascriptDialogOpening,
    };
    use chromiumoxide::cdp::js_protocol::runtime::{EventConsoleApiCalled, EventExceptionThrown};
    use futures_util::StreamExt;

    page.execute(chromiumoxide::cdp::browser_protocol::network::EnableParams::default())
        .await
        .ok();

    // Route downloads to a known directory so `browser_downloads` can name real
    // files, instead of Chrome's default "ask the user" behaviour which, with no
    // one at the keyboard, silently cancels.
    page.execute(
        chromiumoxide::cdp::browser_protocol::browser::SetDownloadBehaviorParams::builder()
            .behavior(
                chromiumoxide::cdp::browser_protocol::browser::SetDownloadBehaviorBehavior::Allow,
            )
            .download_path(downloads.to_string_lossy().to_string())
            .build()
            .expect("download behavior"),
    )
    .await
    .ok();

    // Without this Chrome opens the OS file picker, which nothing can answer and
    // which blocks the click that opened it.
    page.execute(SetInterceptFileChooserDialogParams::new(true))
        .await
        .ok();

    if let Ok(mut s) = page.event_listener::<EventConsoleApiCalled>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                let text = e
                    .args
                    .iter()
                    .map(|a| match a.value.as_ref() {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(v) => v.to_string(),
                        None => a.description.clone().unwrap_or_default(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                rec.push_console(&format!("{:?}", e.r#type).to_lowercase(), text);
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventExceptionThrown>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                let d = &e.exception_details;
                let msg = d
                    .exception
                    .as_ref()
                    .and_then(|x| x.description.clone())
                    .unwrap_or_else(|| d.text.clone());
                rec.push_console("exception", msg);
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventRequestWillBeSent>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                rec.start_request(
                    e.request_id.inner().clone(),
                    e.request.method.clone(),
                    e.request.url.clone(),
                    e.r#type
                        .as_ref()
                        .map(|t| format!("{t:?}"))
                        .unwrap_or_default(),
                );
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventResponseReceived>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                rec.finish_request(
                    e.request_id.inner(),
                    e.response.status,
                    e.response.status_text.clone(),
                    e.response.mime_type.clone(),
                );
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventLoadingFailed>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                rec.fail_request(e.request_id.inner(), e.error_text.clone());
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventJavascriptDialogOpening>().await {
        let rec = rec.clone();
        let page = page.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                let d = PendingDialog {
                    kind: format!("{:?}", e.r#type).to_lowercase(),
                    message: e.message.clone(),
                    default_prompt: e.default_prompt.clone().unwrap_or_default(),
                    at: crate::events::now_ms(),
                };
                let stamp = d.at;
                rec.set_dialog(Some(d));

                // Backstop: a dialog nobody answers blocks the renderer, and with
                // it every screenshot — the live view would just freeze. Give a
                // person or the agent time to decide, then dismiss.
                let rec2 = rec.clone();
                let page2 = page.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(DIALOG_AUTO_DISMISS).await;
                    if rec2.dialog().map(|d| d.at) == Some(stamp) {
                        page2
                            .execute(HandleJavaScriptDialogParams::new(false))
                            .await
                            .ok();
                        rec2.set_dialog(None);
                        rec2.push_console(
                            "warning",
                            "a JavaScript dialog went unanswered for 30s and was dismissed".into(),
                        );
                    }
                });
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventFileChooserOpened>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                rec.set_file_chooser(e.backend_node_id.as_ref().map(|b| *b.inner()));
            }
        });
    }

    if let Ok(mut s) = page.event_listener::<EventDownloadWillBegin>().await {
        let rec = rec.clone();
        tokio::spawn(async move {
            while let Some(e) = s.next().await {
                rec.push_download(json!({
                    "url": e.url, "filename": e.suggested_filename, "at": crate::events::now_ms()
                }));
            }
        });
    }
}

/// Turn a bare host / query into a URL. A single token with a dot becomes https;
/// anything else becomes a Google search.
pub fn normalize_url(input: &str) -> String {
    let t = input.trim();
    if t.is_empty() {
        return "about:blank".to_string();
    }
    if t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with("about:")
        || t.starts_with("file:")
    {
        return t.to_string();
    }
    let looks_like_domain = !t.contains(' ') && t.contains('.');
    if looks_like_domain {
        format!("https://{t}")
    } else {
        format!("https://www.google.com/search?q={}", urlencode(t))
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

const LINKS_JS: &str = r#"(() => {
  const seen = new Set(); const out = [];
  for (const a of document.querySelectorAll('a[href]')) {
    const href = a.href; if (!href || seen.has(href)) continue; seen.add(href);
    const text = (a.innerText||a.textContent||'').replace(/\s+/g,' ').trim().slice(0,100);
    out.push({ href, text }); if (out.length >= 300) break;
  }
  return JSON.stringify(out);
})()"#;

#[cfg(test)]
mod tests {
    use super::{normalize_url, quad_area};

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(normalize_url("https://a.com/x"), "https://a.com/x");
        assert_eq!(normalize_url("http://a.com"), "http://a.com");
        assert_eq!(normalize_url("about:blank"), "about:blank");
    }

    #[test]
    fn bare_domain_gets_https() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(
            normalize_url("news.ycombinator.com"),
            "https://news.ycombinator.com"
        );
    }

    #[test]
    fn phrase_becomes_google_search() {
        let u = normalize_url("eiffel tower height");
        assert!(u.starts_with("https://www.google.com/search?q="));
        assert!(u.contains("eiffel+tower+height"));
    }

    #[test]
    fn empty_is_blank() {
        assert_eq!(normalize_url("   "), "about:blank");
    }

    #[test]
    fn quad_area_measures_the_box() {
        // 100x20 rectangle, corners clockwise from top-left.
        let q = vec![0.0, 0.0, 100.0, 0.0, 100.0, 20.0, 0.0, 20.0];
        assert!((quad_area(&q) - 2000.0).abs() < 0.001);
    }

    #[test]
    fn quad_area_is_orientation_independent() {
        let cw = vec![0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let ccw = vec![0.0, 0.0, 0.0, 10.0, 10.0, 10.0, 10.0, 0.0];
        assert!((quad_area(&cw) - quad_area(&ccw)).abs() < 0.001);
    }
}
