//! `BrowserSession` — the single shared browsing surface driven by BOTH the user
//! (via the live-view WebSocket) and the AI (via MCP). It owns the Chromium
//! instance, a list of tabs, and applies the stealth layer to every new page.

use anyhow::{anyhow, Result};
use base64::Engine;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::cdp::browser_protocol::target::CreateTargetParams;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::{Browser, Page};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;

use crate::input;
use crate::stealth;

pub struct BrowserSession {
    #[allow(dead_code)]
    browser: Browser,
    tabs: Mutex<Vec<Page>>,
    active: AtomicUsize,
    identity: stealth::Identity,
}

impl BrowserSession {
    /// Wrap an already-launched browser + its first page.
    pub async fn new(browser: Browser, first: Page) -> Result<Self> {
        // Read the browser's genuine identity *before* `prepare` installs any
        // override — afterwards it would only report back what we told it.
        let raw = stealth::probe(&first).await?;
        let identity = stealth::correct(&raw);
        if identity.corrected {
            println!("mini-browser: headless build detected — presenting as {}", identity.ua);
        }

        let s = Self {
            browser,
            tabs: Mutex::new(vec![first.clone()]),
            active: AtomicUsize::new(0),
            identity,
        };
        s.prepare(&first).await?;
        Ok(s)
    }

    /// Pin the page's identity: a UA override carrying the browser's real
    /// client-hint metadata plus a matching `Accept-Language`. That is the whole
    /// layer — see `stealth.rs` for why there is no injected JS.
    ///
    /// The override is what keeps `Sec-CH-UA` alive. Setting a UA string without
    /// `userAgentMetadata` (what chromiumoxide's `enable_stealth_mode` does)
    /// silently disables client hints, and a Chrome that sends no `Sec-CH-UA` is
    /// an instant tell — that alone was enough for Google to reject sign-in.
    async fn prepare(&self, page: &Page) -> Result<()> {
        page.execute(stealth::override_params(&self.identity)?).await?;
        Ok(())
    }

    pub async fn active_page(&self) -> Page {
        let tabs = self.tabs.lock().await;
        let i = self.active.load(Ordering::SeqCst).min(tabs.len().saturating_sub(1));
        tabs[i].clone()
    }

    pub async fn navigate(&self, url: &str) -> Result<Value> {
        let url = normalize_url(url);
        let page = self.active_page().await;
        page.goto(&url).await?;
        page.wait_for_navigation().await.ok();
        self.info().await
    }

    pub async fn new_tab(&self, url: Option<&str>) -> Result<Value> {
        let target = url.map(normalize_url).unwrap_or_else(|| "about:blank".to_string());
        let page = self.browser.new_page(CreateTargetParams::new(target)).await?;
        self.prepare(&page).await?;
        let mut tabs = self.tabs.lock().await;
        tabs.push(page);
        let idx = tabs.len() - 1;
        self.active.store(idx, Ordering::SeqCst);
        Ok(json!({ "index": idx, "tabs": tabs.len() }))
    }

    pub async fn list_tabs(&self) -> Result<Value> {
        let tabs = self.tabs.lock().await;
        let active = self.active.load(Ordering::SeqCst);
        let mut out = Vec::new();
        for (i, p) in tabs.iter().enumerate() {
            let url = p.url().await.ok().flatten().unwrap_or_default();
            let title = p.get_title().await.ok().flatten().unwrap_or_default();
            out.push(json!({ "index": i, "url": url, "title": title, "active": i == active }));
        }
        Ok(json!({ "tabs": out, "active": active }))
    }

    pub async fn switch_tab(&self, index: usize) -> Result<Value> {
        let tabs = self.tabs.lock().await;
        if index >= tabs.len() {
            return Err(anyhow!("tab {index} does not exist (have {})", tabs.len()));
        }
        tabs[index].activate().await.ok();
        self.active.store(index, Ordering::SeqCst);
        drop(tabs);
        self.info().await
    }

    pub async fn close_tab(&self, index: usize) -> Result<Value> {
        let mut tabs = self.tabs.lock().await;
        if index >= tabs.len() {
            return Err(anyhow!("tab {index} does not exist"));
        }
        if tabs.len() == 1 {
            return Err(anyhow!("cannot close the last tab"));
        }
        let page = tabs.remove(index);
        let _ = page.close().await;
        let new_active = self.active.load(Ordering::SeqCst).min(tabs.len() - 1);
        self.active.store(new_active, Ordering::SeqCst);
        Ok(json!({ "tabs": tabs.len(), "active": new_active }))
    }

    pub async fn go_back(&self) -> Result<Value> {
        let page = self.active_page().await;
        page.evaluate_expression("history.back()").await.ok();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        self.info().await
    }

    pub async fn go_forward(&self) -> Result<Value> {
        let page = self.active_page().await;
        page.evaluate_expression("history.forward()").await.ok();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        self.info().await
    }

    pub async fn reload(&self) -> Result<Value> {
        let page = self.active_page().await;
        page.reload().await?;
        self.info().await
    }

    /// URL + title of the active page.
    pub async fn info(&self) -> Result<Value> {
        let page = self.active_page().await;
        let url = page.url().await.ok().flatten().unwrap_or_default();
        let title = page.get_title().await.ok().flatten().unwrap_or_default();
        Ok(json!({ "url": url, "title": title }))
    }

    /// Capture a JPEG screenshot of the active page's viewport, base64-encoded.
    pub async fn screenshot_b64(&self) -> Result<String> {
        let page = self.active_page().await;
        let params = ScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Jpeg)
            .quality(55)
            .build();
        let bytes = page.screenshot(params).await?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    /// Run the DOM extractor: assign `data-mb-idx` to interactive elements and
    /// return `{ url, title, count, elements:[…], text }`.
    pub async fn snapshot(&self) -> Result<Value> {
        let page = self.active_page().await;
        let raw: String = page
            .evaluate_expression(SNAPSHOT_JS)
            .await?
            .into_value()
            .map_err(|e| anyhow!("snapshot decode: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| anyhow!("snapshot parse: {e}"))
    }

    /// Viewport coordinates (center) of the element carrying `data-mb-idx=index`.
    async fn coords_of(&self, index: i64) -> Result<(f64, f64)> {
        let page = self.active_page().await;
        let js = format!(
            r#"(() => {{
                const el = document.querySelector('[data-mb-idx="{index}"]');
                if (!el) return JSON.stringify(null);
                el.scrollIntoView({{block:'center', inline:'center'}});
                const r = el.getBoundingClientRect();
                return JSON.stringify({{ x: r.left + r.width/2, y: r.top + r.height/2, w: r.width, h: r.height }});
            }})()"#
        );
        let raw: String = page.evaluate_expression(js).await?.into_value().unwrap_or_default();
        let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        if v.is_null() {
            return Err(anyhow!("element #{index} not found — take a fresh snapshot"));
        }
        // Give the smooth-scroll a moment to settle before reading coordinates.
        tokio::time::sleep(std::time::Duration::from_millis(220)).await;
        let raw2: String = page.evaluate_expression(format!(
            r#"(() => {{ const el=document.querySelector('[data-mb-idx="{index}"]'); if(!el) return JSON.stringify(null); const r=el.getBoundingClientRect(); return JSON.stringify({{x:r.left+r.width/2,y:r.top+r.height/2}}); }})()"#
        )).await?.into_value().unwrap_or_default();
        let v2: Value = serde_json::from_str(&raw2).unwrap_or(v);
        Ok((v2["x"].as_f64().unwrap_or(0.0), v2["y"].as_f64().unwrap_or(0.0)))
    }

    pub async fn click_index(&self, index: i64) -> Result<Value> {
        let (x, y) = self.coords_of(index).await?;
        let page = self.active_page().await;
        input::human_click(&page, x, y).await?;
        Ok(json!({ "clicked": index, "x": x, "y": y }))
    }

    pub async fn click_xy(&self, x: f64, y: f64) -> Result<Value> {
        let page = self.active_page().await;
        input::human_click(&page, x, y).await?;
        Ok(json!({ "clicked_at": [x, y] }))
    }

    pub async fn type_index(&self, index: i64, text: &str, submit: bool) -> Result<Value> {
        let (x, y) = self.coords_of(index).await?;
        let page = self.active_page().await;
        input::human_click(&page, x, y).await?; // focus
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        input::type_text(&page, text).await?;
        if submit {
            input::press_key(&page, "Enter").await?;
        }
        Ok(json!({ "typed_into": index, "submit": submit }))
    }

    pub async fn type_text(&self, text: &str) -> Result<Value> {
        let page = self.active_page().await;
        input::type_text(&page, text).await?;
        Ok(json!({ "typed": text.chars().count() }))
    }

    pub async fn press_key(&self, key: &str) -> Result<Value> {
        let page = self.active_page().await;
        input::press_key(&page, key).await?;
        Ok(json!({ "pressed": key }))
    }

    pub async fn scroll(&self, dx: f64, dy: f64) -> Result<Value> {
        let page = self.active_page().await;
        input::scroll(&page, 400.0, 300.0, dx, dy).await?;
        Ok(json!({ "scrolled": [dx, dy] }))
    }

    pub async fn execute_js(&self, script: &str) -> Result<Value> {
        let page = self.active_page().await;
        // Wrap so both expressions and statement blocks work, and stringify the
        // result so any JSON-serializable value comes back intact.
        let wrapped = format!("(() => {{ try {{ return JSON.stringify((function(){{ {script} }})()); }} catch(e) {{ return JSON.stringify({{__error: String(e)}}); }} }})()");
        let raw: String = page.evaluate_expression(wrapped).await?.into_value().unwrap_or_default();
        let v: Value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
        Ok(v)
    }

    pub async fn extract_text(&self, selector: Option<&str>) -> Result<Value> {
        let page = self.active_page().await;
        let sel = selector.unwrap_or("body");
        let js = format!(
            r#"(() => {{ const el=document.querySelector({sel:?}); return JSON.stringify(el ? (el.innerText||el.textContent||'') : ''); }})()"#,
            sel = sel
        );
        let raw: String = page.evaluate_expression(js).await?.into_value().unwrap_or_default();
        let text: String = serde_json::from_str(&raw).unwrap_or_default();
        Ok(json!({ "text": text }))
    }

    pub async fn extract_links(&self) -> Result<Value> {
        let page = self.active_page().await;
        let raw: String = page.evaluate_expression(LINKS_JS).await?.into_value().unwrap_or_default();
        let v: Value = serde_json::from_str(&raw).unwrap_or(json!([]));
        Ok(v)
    }
}

/// Turn a bare host / query into a URL. A single token with a dot becomes https;
/// anything else becomes a Google search.
pub fn normalize_url(input: &str) -> String {
    let t = input.trim();
    if t.is_empty() {
        return "about:blank".to_string();
    }
    if t.starts_with("http://") || t.starts_with("https://") || t.starts_with("about:") || t.starts_with("file:") {
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
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// DOM extractor. Assigns `data-mb-idx` to visible, interactive elements and
/// returns a compact JSON string the AI can reason over.
const SNAPSHOT_JS: &str = r#"(() => {
  const MAX = 200;
  const out = [];
  const isVisible = (el) => {
    const s = getComputedStyle(el);
    if (s.display === 'none' || s.visibility === 'hidden' || parseFloat(s.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 1 && r.height > 1 && r.bottom > 0 && r.right > 0 &&
           r.top < (innerHeight + 600) && r.left < (innerWidth + 200);
  };
  const interactive = (el) => {
    const tag = el.tagName.toLowerCase();
    if (['a','button','input','textarea','select','summary','option','label'].includes(tag)) return true;
    const role = el.getAttribute('role');
    if (role && ['button','link','checkbox','menuitem','tab','switch','radio','option','searchbox','textbox'].includes(role)) return true;
    if (el.hasAttribute('onclick')) return true;
    if (el.isContentEditable) return true;
    const ti = el.getAttribute('tabindex');
    if (ti !== null && parseInt(ti, 10) >= 0 && tag !== 'body') return true;
    return false;
  };
  const label = (el) => {
    let t = (el.getAttribute('aria-label') || el.getAttribute('placeholder') ||
             (el.tagName.toLowerCase()==='input' ? (el.value||el.getAttribute('name')||el.type) : '') ||
             el.getAttribute('alt') || el.getAttribute('title') || el.innerText || el.textContent || '').trim();
    return t.replace(/\s+/g, ' ').slice(0, 120);
  };
  // clear stale indices
  document.querySelectorAll('[data-mb-idx]').forEach(e => e.removeAttribute('data-mb-idx'));
  let idx = 0;
  const all = document.querySelectorAll('a,button,input,textarea,select,summary,label,[role],[onclick],[tabindex],[contenteditable]');
  for (const el of all) {
    if (idx >= MAX) break;
    if (!interactive(el) || !isVisible(el)) continue;
    el.setAttribute('data-mb-idx', String(idx));
    const r = el.getBoundingClientRect();
    out.push({
      idx, tag: el.tagName.toLowerCase(),
      type: el.getAttribute('type') || undefined,
      role: el.getAttribute('role') || undefined,
      text: label(el),
      x: Math.round(r.left + r.width/2), y: Math.round(r.top + r.height/2),
      w: Math.round(r.width), h: Math.round(r.height),
    });
    idx++;
  }
  const bodyText = (document.body ? (document.body.innerText || '') : '').replace(/\s+\n/g, '\n').trim().slice(0, 4000);
  return JSON.stringify({
    url: location.href, title: document.title, count: out.length,
    elements: out, text: bodyText,
  });
})()"#;

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
    use super::normalize_url;

    #[test]
    fn full_urls_pass_through() {
        assert_eq!(normalize_url("https://a.com/x"), "https://a.com/x");
        assert_eq!(normalize_url("http://a.com"), "http://a.com");
        assert_eq!(normalize_url("about:blank"), "about:blank");
    }

    #[test]
    fn bare_domain_gets_https() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("news.ycombinator.com"), "https://news.ycombinator.com");
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
}
