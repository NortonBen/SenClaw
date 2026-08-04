//! Human-like input via the Chrome DevTools Protocol `Input.*` domain.
//!
//! Everything a user or the AI does goes through these primitives, so at the DOM
//! level a website cannot tell an AI action apart from a person's: real
//! `mousemove` → `mousedown` → `mouseup` sequences with intermediate motion, and
//! per-character `keydown` → `char` → `keyup` triples with randomized timing.
//!
//! The typing path deserves a note. It used to send only a `Char` event per
//! character, which puts text in the box but never fires `keydown`. Plenty of
//! real UI hangs off `keydown` — search-as-you-type, "type to filter" comboboxes,
//! form validation, keyboard shortcuts — so those pages saw text appear with no
//! keystrokes and did nothing. Sending the full triple is both more compatible
//! and more truthful about what a keyboard actually emits.

use anyhow::Result;
use chromiumoxide::cdp::browser_protocol::input::{
    DispatchKeyEventParams, DispatchKeyEventType, DispatchMouseEventParams, DispatchMouseEventType,
    MouseButton,
};
use chromiumoxide::Page;
use rand::Rng;
use std::time::Duration;
use tokio::sync::Mutex;

/// Where the pointer actually is.
///
/// Without this each gesture invented a starting point 40–160px from its target
/// and jumped there, so the event stream contained a discontinuity before every
/// single movement — an artifact no hand produces. Motion now continues from
/// wherever the pointer was left, which costs nothing and removes the artifact.
#[derive(Debug)]
pub struct Cursor(Mutex<(f64, f64)>);

impl Default for Cursor {
    fn default() -> Self {
        Cursor(Mutex::new((0.0, 0.0)))
    }
}

impl Cursor {
    pub async fn get(&self) -> (f64, f64) {
        *self.0.lock().await
    }
    pub async fn set(&self, x: f64, y: f64) {
        *self.0.lock().await = (x, y);
    }
}

async fn jitter(min_ms: u64, max_ms: u64) {
    let d = { rand::thread_rng().gen_range(min_ms..=max_ms) };
    tokio::time::sleep(Duration::from_millis(d)).await;
}

fn button_of(name: &str) -> MouseButton {
    match name {
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        _ => MouseButton::Left,
    }
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

/// Move the cursor to `(x, y)` from wherever it currently is.
///
/// The path is a plain eased interpolation with a little noise, and that is a
/// deliberate stopping point rather than a to-do. The controlled study on this
/// (Plesner et al., breaking reCAPTCHAv2) found that *having* movement cut
/// challenges roughly in half, while Bezier curves versus straight lines made no
/// significant difference at all — p = 0.57. Meanwhile the vendor data that does
/// exist suggests elaborate "humanizer" libraries score slightly *worse* than
/// crude motion, because a mathematically ideal curve is its own signature.
///
/// So: emit real intermediate moves, keep the timing irregular, start from the
/// true previous position, and stop there.
pub async fn human_move(page: &Page, cursor: &Cursor, x: f64, y: f64) -> Result<()> {
    let (sx, sy) = cursor.get().await;
    let dist = ((x - sx).powi(2) + (y - sy).powi(2)).sqrt();
    if dist < 1.0 {
        cursor.set(x, y).await;
        return Ok(());
    }
    // More steps for longer travel, so speed stays in a plausible band instead
    // of a 20px nudge and a 900px sweep taking the same time.
    let steps = ((dist / 45.0).round() as usize).clamp(3, 18);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        // ease-in-out for a natural acceleration curve
        let e = if t < 0.5 {
            2.0 * t * t
        } else {
            -1.0 + (4.0 - 2.0 * t) * t
        };
        // Noise proportional to speed: human motor noise scales with the size of
        // the control signal, so constant-amplitude jitter is the wrong shape.
        let amp = 0.4 + (dist / 400.0).min(2.0);
        let jx = { rand::thread_rng().gen_range(-amp..amp) };
        let jy = { rand::thread_rng().gen_range(-amp..amp) };
        mouse_move(page, sx + (x - sx) * e + jx, sy + (y - sy) * e + jy).await?;
        jitter(6, 18).await;
    }
    mouse_move(page, x, y).await?;
    cursor.set(x, y).await;
    Ok(())
}

async fn button_event(
    page: &Page,
    kind: DispatchMouseEventType,
    x: f64,
    y: f64,
    button: &str,
    click_count: i64,
) -> Result<()> {
    let p = DispatchMouseEventParams::builder()
        .r#type(kind)
        .x(x)
        .y(y)
        .button(button_of(button))
        .click_count(click_count)
        .build()
        .map_err(anyhow::Error::msg)?;
    page.execute(p).await?;
    Ok(())
}

/// A full human-like click at viewport coordinates `(x, y)`.
pub async fn human_click(
    page: &Page,
    cursor: &Cursor,
    x: f64,
    y: f64,
    button: &str,
    clicks: u32,
) -> Result<()> {
    human_move(page, cursor, x, y).await?;
    jitter(20, 80).await;
    for n in 1..=clicks.max(1) {
        button_event(
            page,
            DispatchMouseEventType::MousePressed,
            x,
            y,
            button,
            n as i64,
        )
        .await?;
        jitter(40, 110).await;
        button_event(
            page,
            DispatchMouseEventType::MouseReleased,
            x,
            y,
            button,
            n as i64,
        )
        .await?;
        if n < clicks {
            // Inside the double-click threshold, or the browser reports two
            // separate clicks instead of a dblclick.
            jitter(40, 90).await;
        }
    }
    Ok(())
}

/// Press at one point, move, release at another.
pub async fn human_drag(
    page: &Page,
    cursor: &Cursor,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
) -> Result<()> {
    human_move(page, cursor, x1, y1).await?;
    jitter(30, 90).await;
    button_event(
        page,
        DispatchMouseEventType::MousePressed,
        x1,
        y1,
        "left",
        1,
    )
    .await?;
    jitter(40, 100).await;
    // Intermediate moves matter here beyond realism: HTML5 drag-and-drop and
    // every JS sortable library start tracking on the first `mousemove` after
    // `mousedown`, so a press-then-release with nothing between drags nothing.
    let steps = 12;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let e = if t < 0.5 {
            2.0 * t * t
        } else {
            -1.0 + (4.0 - 2.0 * t) * t
        };
        mouse_move(page, x1 + (x2 - x1) * e, y1 + (y2 - y1) * e).await?;
        jitter(8, 22).await;
    }
    jitter(50, 120).await;
    button_event(
        page,
        DispatchMouseEventType::MouseReleased,
        x2,
        y2,
        "left",
        1,
    )
    .await?;
    cursor.set(x2, y2).await;
    Ok(())
}

/// Scroll as a train of wheel events with a decaying tail.
///
/// One `mouseWheel` carrying the whole delta is not how any input device
/// behaves: a wheel emits discrete notches, and a trackpad emits a burst that
/// decays roughly exponentially as the flick runs out. A shipped npm library
/// (Lethargy) separates the two by curve shape alone. Splitting the delta also
/// happens to work better in practice — infinite-scroll listeners are written
/// against event streams, and a single jump often fails to trigger a load.
pub async fn scroll(page: &Page, x: f64, y: f64, dx: f64, dy: f64) -> Result<()> {
    let total = dy.abs().max(dx.abs());
    let steps = ((total / 120.0).ceil() as usize).clamp(1, 12);
    let mut sent_x = 0.0;
    let mut sent_y = 0.0;
    // Weights decay so the burst front-loads and tapers, like a flick.
    let weights: Vec<f64> = (0..steps).map(|i| 0.72_f64.powi(i as i32)).collect();
    let sum: f64 = weights.iter().sum();
    for (i, w) in weights.iter().enumerate() {
        let last = i + 1 == steps;
        let (sx, sy) = if last {
            (dx - sent_x, dy - sent_y)
        } else {
            (dx * w / sum, dy * w / sum)
        };
        sent_x += sx;
        sent_y += sy;
        let p = DispatchMouseEventParams::builder()
            .r#type(DispatchMouseEventType::MouseWheel)
            .x(x)
            .y(y)
            .delta_x(sx)
            .delta_y(sy)
            .build()
            .map_err(anyhow::Error::msg)?;
        page.execute(p).await?;
        if !last {
            jitter(10, 26).await;
        }
    }
    Ok(())
}

async fn key_event(
    page: &Page,
    kind: DispatchKeyEventType,
    key: &str,
    code: &str,
    vk: i64,
    text: Option<&str>,
    modifiers: i64,
) -> Result<()> {
    let mut b = DispatchKeyEventParams::builder()
        .r#type(kind)
        .key(key)
        .code(code)
        .windows_virtual_key_code(vk)
        .native_virtual_key_code(vk)
        .modifiers(modifiers);
    if let Some(t) = text {
        b = b.text(t);
    }
    page.execute(b.build().map_err(anyhow::Error::msg)?).await?;
    Ok(())
}

/// `code` and virtual key code for a printable character, so `keydown` carries
/// the physical-key information a real keyboard would report.
fn printable_key(ch: char) -> (String, i64) {
    let upper = ch.to_ascii_uppercase();
    match ch {
        'a'..='z' | 'A'..='Z' => (format!("Key{upper}"), upper as i64),
        '0'..='9' => (format!("Digit{ch}"), ch as i64),
        ' ' => ("Space".to_string(), 32),
        _ => (String::new(), 0),
    }
}

/// How long a key stays down. Measured human mean is ~116ms (SD ~24) and it
/// barely varies with typing speed — fast typists do not press more briefly,
/// they start the next key sooner.
fn sample_dwell() -> u64 {
    rand::thread_rng().gen_range(85..=150)
}

/// Gap between one key going down and the next going down. Right-skewed with a
/// hard floor around 60ms: most keystrokes cluster, a few are much slower
/// because the typist paused to think. A uniform draw would be symmetric, which
/// no measured distribution is.
fn sample_iki() -> u64 {
    let u: f64 = rand::thread_rng().gen_range(0.001..1.0);
    let tail = (-u.ln()).min(3.0); // exponential-ish, clipped
    60 + (95.0 * tail) as u64
}

/// Type a string, one character at a time.
///
/// Two details here are not cosmetic.
///
/// **Dwell.** Every key is held for a realistic interval instead of being
/// released in the same millisecond. The old code sent only a `Char` event, so
/// there was no `keydown` at all — which broke search-as-you-type and any UI
/// built on key handlers, quite apart from what it looked like.
///
/// **Rollover.** Real typists press the next key before releasing the last, so
/// roughly a quarter of keystrokes have a *negative* flight time. A strictly
/// serialized down-up-down-up stream has exactly zero, and fast typing with zero
/// rollover is a combination essentially absent from the 168,000-participant
/// keystroke corpus. Here a key is simply held past the next keydown whenever
/// the sampled inter-key interval lands shorter than its dwell, which reproduces
/// the overlap at about the right rate without special-casing anything.
pub async fn type_text(page: &Page, text: &str) -> Result<()> {
    let mut held: Option<(String, String, i64)> = None;
    for ch in text.chars() {
        if ch == '\n' {
            if let Some((k, c, v)) = held.take() {
                key_event(page, DispatchKeyEventType::KeyUp, &k, &c, v, None, 0).await?;
            }
            // Emitted inline rather than via `press_key`: that would make these
            // two async fns mutually recursive, which Rust cannot size.
            key_event(
                page,
                DispatchKeyEventType::KeyDown,
                "Enter",
                "Enter",
                13,
                Some("\r"),
                0,
            )
            .await?;
            tokio::time::sleep(Duration::from_millis(sample_dwell())).await;
            key_event(
                page,
                DispatchKeyEventType::KeyUp,
                "Enter",
                "Enter",
                13,
                None,
                0,
            )
            .await?;
            continue;
        }
        let s = ch.to_string();
        let (code, vk) = printable_key(ch);
        let dwell = sample_dwell();
        let iki = sample_iki();

        // `keyDown` carrying `text` already produces the keypress and inserts the
        // character. Sending a separate `char` event as well types everything
        // twice — "hello" arrived as "hheelllloo".
        key_event(
            page,
            DispatchKeyEventType::KeyDown,
            &s,
            &code,
            vk,
            Some(&s),
            0,
        )
        .await?;

        // Release the previous key now: it was still down when this one went
        // down, which is what makes the flight time negative.
        if let Some((k, c, v)) = held.take() {
            key_event(page, DispatchKeyEventType::KeyUp, &k, &c, v, None, 0).await?;
        }

        if iki < dwell {
            held = Some((s, code, vk));
            tokio::time::sleep(Duration::from_millis(iki)).await;
        } else {
            tokio::time::sleep(Duration::from_millis(dwell)).await;
            key_event(page, DispatchKeyEventType::KeyUp, &s, &code, vk, None, 0).await?;
            tokio::time::sleep(Duration::from_millis(iki - dwell)).await;
        }
    }
    if let Some((k, c, v)) = held {
        tokio::time::sleep(Duration::from_millis(sample_dwell() / 2)).await;
        key_event(page, DispatchKeyEventType::KeyUp, &k, &c, v, None, 0).await?;
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
        "F1" => ("F1", "F1", 112),
        "F2" => ("F2", "F2", 113),
        "F3" => ("F3", "F3", 114),
        "F4" => ("F4", "F4", 115),
        "F5" => ("F5", "F5", 116),
        "F6" => ("F6", "F6", 117),
        "F7" => ("F7", "F7", 118),
        "F8" => ("F8", "F8", 119),
        "F9" => ("F9", "F9", 120),
        "F10" => ("F10", "F10", 121),
        "F11" => ("F11", "F11", 122),
        "F12" => ("F12", "F12", 123),
        _ => return None,
    })
}

/// Press a single named key (keyDown + keyUp).
///
/// An unrecognized *single character* is typed — `press_key("a")` meaning "type
/// an a" is a reasonable reading. A longer unrecognized name is an error rather
/// than something to type verbatim: `press_key("F13")` used to put the literal
/// text "F13" into the focused field, which is never what anyone wanted and is
/// the kind of failure that looks like the page misbehaving.
pub async fn press_key(page: &Page, name: &str) -> Result<()> {
    let Some((key, code, vk)) = named_key(name) else {
        if name.chars().count() == 1 {
            return type_text(page, name).await;
        }
        anyhow::bail!(
            "unknown key {name:?} — use Enter, Tab, Escape, Backspace, Delete, Arrow*, Home, End, PageUp, PageDown, Space, F1-F12, or a single character"
        );
    };
    // Enter and Tab carry text on a real keyboard; without it some editors
    // never insert the newline.
    let text = match key {
        "Enter" => Some("\r"),
        "Tab" => Some("\t"),
        " " => Some(" "),
        _ => None,
    };
    key_event(page, DispatchKeyEventType::KeyDown, key, code, vk, text, 0).await?;
    jitter(20, 70).await;
    key_event(page, DispatchKeyEventType::KeyUp, key, code, vk, None, 0).await?;
    Ok(())
}

/// CDP modifier bits: Alt=1, Control=2, Meta=4, Shift=8.
const META: i64 = 4;
const CTRL: i64 = 2;

/// Select everything in the focused field, so the next keystroke replaces it.
///
/// Done with a real Cmd/Ctrl+A rather than by assigning `.value = ''`, because
/// a controlled React input ignores a value written from outside and snaps back
/// on the next render.
pub async fn select_all(page: &Page) -> Result<()> {
    let modifier = if cfg!(target_os = "macos") {
        META
    } else {
        CTRL
    };
    key_event(
        page,
        DispatchKeyEventType::KeyDown,
        "a",
        "KeyA",
        65,
        None,
        modifier,
    )
    .await?;
    jitter(20, 50).await;
    key_event(
        page,
        DispatchKeyEventType::KeyUp,
        "a",
        "KeyA",
        65,
        None,
        modifier,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{named_key, printable_key};

    #[test]
    fn common_keys_map() {
        assert_eq!(named_key("Enter").unwrap().2, 13);
        assert_eq!(named_key("Escape").unwrap().0, "Escape");
        assert_eq!(named_key("ArrowDown").unwrap().1, "ArrowDown");
        assert_eq!(named_key("Space").unwrap().0, " ");
        assert_eq!(named_key("F5").unwrap().2, 116);
    }

    #[test]
    fn unknown_key_is_none() {
        assert!(named_key("F13").is_none());
        assert!(named_key("hello").is_none());
    }

    #[test]
    fn printable_characters_get_a_physical_key_code() {
        assert_eq!(printable_key('a'), ("KeyA".to_string(), 65));
        assert_eq!(printable_key('Z'), ("KeyZ".to_string(), 90));
        assert_eq!(printable_key('7'), ("Digit7".to_string(), '7' as i64));
        assert_eq!(printable_key(' '), ("Space".to_string(), 32));
    }

    /// Vietnamese text has no US-keyboard physical key. It must still type —
    /// the `char` event carries the character regardless of `code`.
    #[test]
    fn non_ascii_characters_have_no_code_but_are_not_rejected() {
        let (code, vk) = printable_key('ế');
        assert!(code.is_empty());
        assert_eq!(vk, 0);
    }

    #[test]
    fn dwell_sits_around_the_measured_human_mean() {
        let mean: f64 = (0..2000).map(|_| super::sample_dwell() as f64).sum::<f64>() / 2000.0;
        assert!(
            (90.0..=145.0).contains(&mean),
            "dwell mean {mean} is not human-plausible"
        );
    }

    #[test]
    fn inter_key_intervals_are_right_skewed_with_a_floor() {
        let mut v: Vec<u64> = (0..4000).map(|_| super::sample_iki()).collect();
        v.sort_unstable();
        assert!(
            v[0] >= 60,
            "no keystroke should be faster than the human floor"
        );
        let median = v[v.len() / 2] as f64;
        let mean = v.iter().sum::<u64>() as f64 / v.len() as f64;
        // The defining property: a long right tail drags the mean above the
        // median. A symmetric draw would put them on top of each other.
        assert!(mean > median, "mean {mean} should exceed median {median}");
    }

    /// The rollover rate falls out of dwell and IKI overlapping. If a refactor
    /// ever drives it to zero, fast typing with no key overlap is the single
    /// strongest synthetic-keystroke signal there is.
    #[test]
    fn some_keystrokes_overlap() {
        let overlapping = (0..4000)
            .filter(|_| super::sample_iki() < super::sample_dwell())
            .count();
        let rate = overlapping as f64 / 4000.0;
        assert!(
            (0.10..=0.55).contains(&rate),
            "rollover rate {rate} is outside the measured human band"
        );
    }

    #[tokio::test]
    async fn the_cursor_remembers_where_it_was_left() {
        let c = super::Cursor::default();
        assert_eq!(c.get().await, (0.0, 0.0));
        c.set(120.0, 340.0).await;
        assert_eq!(c.get().await, (120.0, 340.0));
    }
}
