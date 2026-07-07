//! Human-like input via the Chrome DevTools Protocol `Input.*` domain.
//!
//! Everything a user or the AI does goes through these primitives, so at the DOM
//! level a website cannot tell an AI action apart from a person's: real
//! `mousemove` → `mousedown` → `mouseup` sequences with intermediate motion, and
//! per-character key events with randomized timing (no instant paste).

use anyhow::Result;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    MouseButton,
};
use chromiumoxide::Page;
use rand::Rng;
use std::time::Duration;

async fn jitter(min_ms: u64, max_ms: u64) {
    let d = { rand::thread_rng().gen_range(min_ms..=max_ms) };
    tokio::time::sleep(Duration::from_millis(d)).await;
}

async fn mouse_move(page: &Page, x: f64, y: f64) -> Result<()> {
    let p = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseMoved)
        .x(x)
        .y(y)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(p).await?;
    Ok(())
}

/// Move the cursor to `(x, y)` along a few interpolated steps from a plausible
/// starting point, so the motion isn't a single teleport.
pub async fn human_move(page: &Page, x: f64, y: f64) -> Result<()> {
    let (sx, sy) = {
        let mut r = rand::thread_rng();
        (x - r.gen_range(40.0..160.0), y - r.gen_range(30.0..120.0))
    };
    let steps = 6;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        // ease-in-out for a natural acceleration curve
        let e = if t < 0.5 { 2.0 * t * t } else { -1.0 + (4.0 - 2.0 * t) * t };
        let jx = { rand::thread_rng().gen_range(-1.5..1.5) };
        let jy = { rand::thread_rng().gen_range(-1.5..1.5) };
        mouse_move(page, sx + (x - sx) * e + jx, sy + (y - sy) * e + jy).await?;
        jitter(6, 18).await;
    }
    mouse_move(page, x, y).await?;
    Ok(())
}

/// A full human-like left click at viewport coordinates `(x, y)`.
pub async fn human_click(page: &Page, x: f64, y: f64) -> Result<()> {
    human_move(page, x, y).await?;
    jitter(20, 80).await;
    let down = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MousePressed)
        .x(x)
        .y(y)
        .button(MouseButton::Left)
        .click_count(1)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(down).await?;
    jitter(40, 110).await;
    let up = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseReleased)
        .x(x)
        .y(y)
        .button(MouseButton::Left)
        .click_count(1)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(up).await?;
    Ok(())
}

/// Scroll by dispatching a mouse wheel event at `(x, y)`.
pub async fn scroll(page: &Page, x: f64, y: f64, dx: f64, dy: f64) -> Result<()> {
    let p = DispatchMouseEventParams::builder()
        .r#type(DispatchMouseEventType::MouseWheel)
        .x(x)
        .y(y)
        .delta_x(dx)
        .delta_y(dy)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(p).await?;
    Ok(())
}

/// Type a string one character at a time with randomized inter-key delay.
pub async fn type_text(page: &Page, text: &str) -> Result<()> {
    for ch in text.chars() {
        let s = ch.to_string();
        let ev = DispatchKeyEventParams::builder()
            .r#type(DispatchKeyEventType::Char)
            .text(&s)
            .key(&s)
            .build()
            .map_err(anyhow::Error::msg)?;
        page.execute(ev).await?;
        jitter(40, 150).await;
    }
    Ok(())
}

/// Map a friendly key name to (key, code, windowsVirtualKeyCode).
fn named_key(name: &str) -> Option<(&'static str, &'static str, i64)> {
    Some(match name {
        "Enter" | "Return" => ("Enter", "Enter", 13),
        "Tab" => ("Tab", "Tab", 9),
        "Backspace" => ("Backspace", "Backspace", 8),
        "Delete" => ("Delete", "Delete", 46),
        "Escape" | "Esc" => ("Escape", "Escape", 27),
        "ArrowUp" => ("ArrowUp", "ArrowUp", 38),
        "ArrowDown" => ("ArrowDown", "ArrowDown", 40),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39),
        "Home" => ("Home", "Home", 36),
        "End" => ("End", "End", 35),
        "PageUp" => ("PageUp", "PageUp", 33),
        "PageDown" => ("PageDown", "PageDown", 34),
        "Space" => (" ", "Space", 32),
        _ => return None,
    })
}

/// Press a single named key (keyDown + keyUp). Falls back to typing the string
/// as text if it isn't a recognized special key.
pub async fn press_key(page: &Page, name: &str) -> Result<()> {
    let Some((key, code, vk)) = named_key(name) else {
        return type_text(page, name).await;
    };
    let down = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyDown)
        .key(key)
        .code(code)
        .windows_virtual_key_code(vk)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(down).await?;
    jitter(20, 70).await;
    let up = DispatchKeyEventParams::builder()
        .r#type(DispatchKeyEventType::KeyUp)
        .key(key)
        .code(code)
        .windows_virtual_key_code(vk)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(up).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::named_key;

    #[test]
    fn common_keys_map() {
        assert_eq!(named_key("Enter").unwrap().2, 13);
        assert_eq!(named_key("Escape").unwrap().0, "Escape");
        assert_eq!(named_key("ArrowDown").unwrap().1, "ArrowDown");
        assert_eq!(named_key("Space").unwrap().0, " ");
    }

    #[test]
    fn unknown_key_is_none() {
        assert!(named_key("F13").is_none());
        assert!(named_key("hello").is_none());
    }
}
