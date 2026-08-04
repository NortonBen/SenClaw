//! `PageOps` — the Playwright-like primitive surface the TikTok executors
//! (`browser.rs`) call. Implementors provide only the transport primitives
//! (eval / navigate / url / mouse / keyboard); everything else (visibility,
//! click-by-selector/text, fill, …) is a default method built on top of them,
//! so the same executor logic runs over any transport (the extension bridge, and
//! previously the CDP driver).

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use rand::Rng;
use serde_json::Value;
use std::time::{Duration, Instant};

#[async_trait]
pub trait PageOps: Send + Sync {
    // ---- required transport primitives ----

    /// Current top-level document URL.
    async fn url(&self) -> String;
    /// Navigate and wait (best-effort) for load, up to `timeout_ms`.
    async fn navigate(&self, url: &str, timeout_ms: u64) -> Result<()>;
    /// Evaluate a JS expression in the page, returning its JSON value.
    async fn eval(&self, js: &str) -> Result<Value>;
    /// A trusted left-click at viewport coordinates.
    async fn mouse_click(&self, x: f64, y: f64) -> Result<()>;
    /// Type text one character at a time (trusted key events).
    async fn type_chars(&self, text: &str) -> Result<()>;
    /// Press a single named key (Enter, Tab, …).
    async fn press_named(&self, key: &str) -> Result<()>;
    /// Dispatch a wheel event at `(x, y)`.
    async fn wheel(&self, x: f64, y: f64, dx: f64, dy: f64) -> Result<()>;

    // ---- provided helpers ----

    async fn eval_bool(&self, js: &str) -> bool {
        matches!(self.eval(js).await, Ok(Value::Bool(true)))
    }

    async fn wait_ms(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }

    async fn human_pause(&self, min_ms: u64, max_ms: u64) {
        let max = max_ms.max(min_ms);
        let span = (max - min_ms).max(1);
        let d = min_ms + rand::thread_rng().gen_range(0..span);
        self.wait_ms(d).await;
    }

    /// Alias kept for executor readability.
    async fn goto(&self, url: &str, _wait_until: &str, timeout_ms: u64) -> Result<()> {
        self.navigate(url, timeout_ms.max(1000)).await
    }

    async fn type_text(&self, text: &str) -> Result<()> {
        self.type_chars(text).await
    }

    async fn press_key(&self, key: &str) -> Result<()> {
        self.press_named(key).await
    }

    async fn human_click(&self, x: f64, y: f64) -> Result<()> {
        self.mouse_click(x, y).await
    }

    async fn is_visible_any(&self, selectors: &[String]) -> bool {
        let arr = serde_json::to_string(selectors).unwrap_or_else(|_| "[]".into());
        let js = format!(
            r#"(() => {{ const sels = {arr}; for (const s of sels) {{ try {{ const el = document.querySelector(s); if (el) {{ const r = el.getBoundingClientRect(); if (r.width>0 && r.height>0 && el.offsetParent!==null) return true; }} }} catch(e) {{}} }} return false; }})()"#
        );
        self.eval_bool(&js).await
    }

    async fn find_first_visible(&self, selectors: &[String], timeout_ms: u64) -> Option<String> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(200));
        loop {
            for s in selectors {
                if self.is_visible_any(std::slice::from_ref(s)).await {
                    return Some(s.clone());
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            self.wait_ms(200).await;
        }
    }

    async fn find_visible_center(
        &self,
        selectors: &[String],
        timeout_ms: u64,
    ) -> Option<(f64, f64)> {
        let arr = serde_json::to_string(selectors).unwrap_or_else(|_| "[]".into());
        let js = format!(
            r#"(() => {{
                const sels = {arr};
                for (const s of sels) {{
                    try {{
                        const el = document.querySelector(s);
                        if (!el) continue;
                        el.scrollIntoView({{block:'center', inline:'center'}});
                        const r = el.getBoundingClientRect();
                        if (r.width>0 && r.height>0 && el.offsetParent!==null)
                            return {{x: r.x + r.width/2, y: r.y + r.height/2}};
                    }} catch(e) {{}}
                }}
                return null;
            }})()"#
        );
        self.poll_center(&js, timeout_ms).await
    }

    async fn find_text_center(
        &self,
        base: &str,
        needle: &str,
        timeout_ms: u64,
    ) -> Option<(f64, f64)> {
        let base_j = serde_json::to_string(base).unwrap();
        let needle_j = serde_json::to_string(&needle.to_lowercase()).unwrap();
        let js = format!(
            r#"(() => {{
                const base = {base_j}; const needle = {needle_j};
                const nodes = document.querySelectorAll(base);
                for (const el of nodes) {{
                    try {{
                        const t = (el.innerText||el.textContent||'').toLowerCase();
                        if (!t.includes(needle)) continue;
                        el.scrollIntoView({{block:'center'}});
                        const r = el.getBoundingClientRect();
                        if (r.width>0 && r.height>0 && el.offsetParent!==null)
                            return {{x: r.x + r.width/2, y: r.y + r.height/2}};
                    }} catch(e) {{}}
                }}
                return null;
            }})()"#
        );
        self.poll_center(&js, timeout_ms).await
    }

    async fn poll_center(&self, js: &str, timeout_ms: u64) -> Option<(f64, f64)> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(200));
        loop {
            if let Ok(Value::Object(m)) = self.eval(js).await {
                if let (Some(x), Some(y)) = (
                    m.get("x").and_then(Value::as_f64),
                    m.get("y").and_then(Value::as_f64),
                ) {
                    return Some((x, y));
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            self.wait_ms(200).await;
        }
    }

    async fn click_selectors(&self, selectors: &[String], timeout_ms: u64) -> Result<()> {
        let (x, y) = self
            .find_visible_center(selectors, timeout_ms)
            .await
            .ok_or_else(|| anyhow!("no selector matched: {:?}", selectors))?;
        self.mouse_click(x, y).await
    }

    async fn click_selectors_optional(&self, selectors: &[String], timeout_ms: u64) {
        let _ = self.click_selectors(selectors, timeout_ms).await;
    }

    async fn click_by_text(&self, base: &str, needle: &str, timeout_ms: u64) -> Result<()> {
        let (x, y) = self
            .find_text_center(base, needle, timeout_ms)
            .await
            .ok_or_else(|| anyhow!("no element with text {needle:?} under {base:?}"))?;
        self.mouse_click(x, y).await
    }

    async fn inner_text(&self, selector: &str) -> Option<String> {
        let sel = serde_json::to_string(selector).unwrap();
        let js = format!(
            r#"(() => {{ const el = document.querySelector({sel}); return el ? (el.innerText||el.textContent||'') : null; }})()"#
        );
        match self.eval(&js).await {
            Ok(Value::String(s)) => Some(s),
            _ => None,
        }
    }

    async fn fill(&self, selector: &str, text: &str, timeout_ms: u64) -> Result<()> {
        let sel = vec![selector.to_string()];
        if self.find_visible_center(&sel, timeout_ms).await.is_none() {
            return Err(anyhow!("fill: không thấy {selector:?}"));
        }
        let sel_j = serde_json::to_string(selector).unwrap();
        let text_j = serde_json::to_string(text).unwrap();
        let js = format!(
            r#"(() => {{
                const el = document.querySelector({sel_j});
                if (!el) return false;
                const val = {text_j};
                if (el.isContentEditable) {{ el.focus(); el.textContent = val; el.dispatchEvent(new InputEvent('input', {{bubbles:true}})); return true; }}
                const proto = el.tagName === 'TEXTAREA' ? window.HTMLTextAreaElement.prototype : window.HTMLInputElement.prototype;
                const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
                el.focus();
                if (setter) setter.call(el, val); else el.value = val;
                el.dispatchEvent(new Event('input', {{bubbles:true}}));
                el.dispatchEvent(new Event('change', {{bubbles:true}}));
                return el.value === val || el.textContent === val;
            }})()"#
        );
        if self.eval_bool(&js).await {
            return Ok(());
        }
        self.click_selectors(&sel, timeout_ms).await?;
        self.type_chars(text).await
    }
}
