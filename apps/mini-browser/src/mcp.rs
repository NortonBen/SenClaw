//! MCP server (JSON-RPC over SSE + POST), exposing the browser to AI agents.
//! Manual protocol impl mirroring the other SenClaw App Spaces.
//!
//! The important design decision here is not the tool list, it is what a tool
//! *returns*. Originally each one answered with just its own result — `{"clicked":
//! 3}` — leaving the agent to guess whether the page had navigated, whether a
//! dialog was now blocking it, whether the click had opened a new tab, or what
//! the page even looked like afterwards. In practice the model would either
//! call `browser_snapshot` after every single action (doubling the round-trips)
//! or skip it and act on a stale mental model (producing confident wrong
//! clicks).
//!
//! So every action now answers with the state that action produced: URL and
//! title, any modal that is blocking, the tab list when there is more than one,
//! new console errors, and a fresh snapshot. Both Playwright's MCP server and
//! Chrome's arrived at the same shape independently, and it is the single
//! biggest reliability difference between an agent that works and one that
//! flails.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::AppState;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };

    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "mini-browser-mcp", "version": "2.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

/// Shorthand for a tool schema.
fn tool(name: &str, description: &str, props: Value, required: &[&str]) -> Value {
    let mut schema = json!({ "type": "object", "properties": props });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    json!({ "name": name, "description": description, "inputSchema": schema })
}

fn s(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}
fn n(desc: &str) -> Value {
    json!({ "type": "number", "description": desc })
}
fn b(desc: &str) -> Value {
    json!({ "type": "boolean", "description": desc })
}

const REF_DESC: &str = "Element ref from the latest browser_snapshot, e.g. \"e12\". Refs stay \
valid while you are on the same page, even after the page re-renders.";

fn tools_list() -> Value {
    json!([
        // ---- observing -------------------------------------------------
        tool("browser_snapshot",
            "Capture the page as an accessibility tree: every element with its role, name, state \
             and a [ref=eN] you use to act on it. This is the primary way to see a page — it \
             includes content inside iframes and shadow DOM, and it is far cheaper than a \
             screenshot. Elements marked with * are new since the last snapshot, which is how you \
             tell what your last action actually changed. Elements shown as `clickable` have no \
             accessible role but the page styles them as pressable — plenty of app UI is built \
             that way, and they are just as actionable as a button. The header says where the \
             viewport sits in the page, and [more below] / [end of page] mark whether there is \
             anything left to scroll to.",
            json!({}), &[]),
        tool("browser_find",
            "Search the page snapshot for text and return only the matching lines with their refs \
             and a little context. Much cheaper than a full snapshot when you already know what \
             you are looking for on a large page.",
            json!({ "text": s("Text to look for (case-insensitive)") }), &["text"]),
        tool("browser_screenshot",
            "Take a JPEG screenshot. Use this only when the visual layout genuinely matters — \
             browser_snapshot is cheaper and more actionable for deciding what to click.",
            json!({ "full_page": b("Capture the whole scrollable page instead of the viewport") }), &[]),
        tool("browser_get_info",
            "The current url and title, plus any dialog that is blocking the page. Cheap — use it \
             when you only need to know where you are, not what is on the page.",
            json!({}), &[]),
        tool("browser_extract_text",
            "Extract visible text from the page, or from one CSS selector. For reading prose; use \
             browser_snapshot when you need to interact.",
            json!({ "selector": s("Optional CSS selector") }), &[]),
        tool("browser_extract_links",
            "Every link on the page as {href, text}. Use this to survey where a page can take you; \
             use browser_snapshot when you intend to click one.",
            json!({}), &[]),

        // ---- navigating ------------------------------------------------
        tool("browser_navigate",
            "Go to a URL. A bare domain gets https://; a phrase becomes a Google search.",
            json!({ "url": s("URL, domain or search phrase") }), &["url"]),
        tool("browser_back",
            "Go back one entry in the browser's own session history. This walks real history \
             rather than calling history.back(), so a single-page app cannot swallow it.",
            json!({}), &[]),
        tool("browser_forward",
            "Go forward one entry in session history. Fails clearly when there is nothing ahead.",
            json!({}), &[]),
        tool("browser_reload",
            "Reload the current page. Useful when a page is stuck mid-render or a spinner never \
             resolves; note every ref is invalidated afterwards.",
            json!({}), &[]),

        // ---- acting ----------------------------------------------------
        tool("browser_click",
            "Click an element with human-like mouse movement. Works on elements inside iframes.",
            json!({
                "ref": s(REF_DESC),
                "button": json!({ "type": "string", "enum": ["left", "right", "middle"], "description": "Default left" }),
                "double": b("Double-click instead of a single click")
            }), &["ref"]),
        tool("browser_type",
            "Type into a field, one key at a time with realistic timing. Clears the field first \
             unless append is set.",
            json!({
                "ref": s(REF_DESC),
                "text": s("Text to type"),
                "submit": b("Press Enter afterwards — use this to run a search"),
                "append": b("Add to the existing value instead of replacing it")
            }), &["ref", "text"]),
        tool("browser_fill_form",
            "Fill several fields in one call. PREFER THIS over one browser_type per field — it is \
             one round-trip instead of many, and avoids the refs going stale mid-form.",
            json!({ "fields": json!({
                "type": "array",
                "description": "Fields to fill, applied in order",
                "items": { "type": "object", "properties": {
                    "ref": { "type": "string", "description": "Element ref" },
                    "type": { "type": "string", "enum": ["textbox", "checkbox", "radio", "combobox"], "description": "Default textbox" },
                    "value": { "type": "string", "description": "Text to type, or \"true\"/\"false\" for checkbox and radio, or the option label for combobox" }
                }, "required": ["ref", "value"] }
            }) }), &["fields"]),
        tool("browser_select_option",
            "Choose one or more options in a <select> dropdown. Native dropdowns render outside \
             the page, so clicking them does not work — use this instead.",
            json!({
                "ref": s(REF_DESC),
                "values": json!({ "type": "array", "items": { "type": "string" }, "description": "Option values or visible labels" })
            }), &["ref", "values"]),
        tool("browser_hover",
            "Move the mouse onto an element, e.g. to open a hover menu.",
            json!({ "ref": s(REF_DESC) }), &["ref"]),
        tool("browser_drag",
            "Drag one element onto another — reordering a list, moving a card between columns, \
             dropping a file onto a zone. Real intermediate mouse movement is sent, which is what \
             HTML5 drag-and-drop and the JS sortable libraries actually listen for.",
            json!({ "from": s("Ref of the element to drag"), "to": s("Ref of the drop target") }), &["from", "to"]),
        tool("browser_press_key",
            "Press one key: Enter, Tab, Escape, Backspace, Delete, ArrowUp/Down/Left/Right, Home, \
             End, PageUp, PageDown, Space, F1-F12, or a single character.",
            json!({ "key": s("Key name") }), &["key"]),
        tool("browser_scroll",
            "Scroll the whole page. Sent as a train of wheel events, so infinite-scroll listeners \
             fire the way they would for a person. To reach something inside a scrollable pane, \
             use browser_scroll_to instead — page scrolling does not move inner panes.",
            json!({
                "direction": json!({ "type": "string", "enum": ["up", "down"], "description": "Default down" }),
                "amount": n("Pixels, default 600")
            }), &[]),
        tool("browser_scroll_to",
            "Scroll a specific element into view. Use this to reach content inside a scrollable \
             pane, which page-level scrolling does not move.",
            json!({ "ref": s(REF_DESC) }), &["ref"]),
        tool("browser_highlight",
            "Briefly outline an element in the live view so the watching user can see what you are \
             about to do. Courteous before anything consequential.",
            json!({ "ref": s(REF_DESC), "ms": n("How long to show it, 200-5000, default 1200") }), &["ref"]),

        // ---- modals, files ---------------------------------------------
        tool("browser_handle_dialog",
            "Answer a JavaScript alert/confirm/prompt. While a dialog is open the page is frozen \
             and every other tool will refuse, so answer it first.",
            json!({
                "accept": b("true = OK/Yes, false = Cancel"),
                "prompt_text": s("Text to enter, for a prompt() dialog")
            }), &["accept"]),
        tool("browser_file_upload",
            "Provide files to a file chooser that a previous click opened.",
            json!({ "paths": json!({ "type": "array", "items": { "type": "string" }, "description": "Absolute file paths" }) }),
            &["paths"]),
        tool("browser_downloads",
            "Files downloaded during this session, with the name each was saved as. Downloads are \
             accepted automatically and written to the app's downloads folder.",
            json!({}), &[]),

        // ---- waiting ---------------------------------------------------
        tool("browser_wait_for",
            "Wait for text to appear, for text to disappear, or for a fixed time. Actions already \
             wait for the page to settle, so reach for this only when something arrives later \
             than that — a slow search result, a progress bar finishing.",
            json!({
                "text": s("Wait until this text is present"),
                "text_gone": s("Wait until this text is gone"),
                "seconds": n("Wait this long instead, max 30")
            }), &[]),

        // ---- diagnostics -----------------------------------------------
        tool("browser_console_messages",
            "Console output for the current page. The fastest way to find out why an action \
             appeared to do nothing.",
            json!({ "errors_only": b("Only errors and uncaught exceptions"), "limit": n("Default 50") }), &[]),
        tool("browser_network_requests",
            "Network requests for the current page, with status codes. Static assets are hidden \
             unless you ask for them. Use this to see what a click actually submitted and what \
             came back.",
            json!({
                "filter": s("Only URLs containing this substring"),
                "include_static": b("Include images, fonts, scripts and stylesheets"),
                "limit": n("Default 50")
            }), &[]),

        // ---- tabs, environment -----------------------------------------
        tool("browser_new_tab",
            "Open a new tab and make it active. Note that tabs opened by the page itself — a \
             target=_blank link, a popup — are adopted automatically and appear in browser_list_tabs.",
            json!({ "url": s("Optional URL to open in it") }), &[]),
        tool("browser_list_tabs",
            "List open tabs with their index, url and title, and which one is active. Worth checking \
             when a click seemed to do nothing — it may have opened a tab.",
            json!({}), &[]),
        tool("browser_switch_tab",
            "Make a tab active. All other tools act on the active tab, and refs do not carry across.",
            json!({ "index": n("Tab index from browser_list_tabs") }), &["index"]),
        tool("browser_close_tab",
            "Close a tab. The last remaining tab cannot be closed.",
            json!({ "index": n("Tab index from browser_list_tabs") }), &["index"]),
        tool("browser_resize",
            "Resize the browser window, e.g. to check a mobile layout.",
            json!({ "width": n("Pixels"), "height": n("Pixels") }), &["width", "height"]),
        tool("browser_execute_js",
            "Run JavaScript in the page and return its result. The snippet is wrapped in a \
             function — use 'return' to yield a value. Prefer the dedicated tools where one \
             exists; this is the escape hatch.",
            json!({ "script": s("JavaScript body") }), &["script"]),

        // ---- handing over to the person --------------------------------
        tool("browser_request_login",
            "Hand the browser to the person so THEY can sign in. Call this the moment a task needs \
             an account — a login form, a 'sign in to continue', an OAuth consent screen, a \
             verification code. Never type a username, password, OTP or recovery code yourself, and \
             never ask the user to paste one into the chat: this tool opens the real browser window \
             so they can use their own password manager, passkey or security key. The agent cannot \
             act until they hand control back, so say plainly what you need signed in and then \
             stop.",
            json!({
                "url": s("Optional page to open for them, e.g. the site's login page"),
                "reason": s("What you need access to, in one line — shown to the user")
            }), &[]),

        // ---- AI --------------------------------------------------------
        tool("browser_act",
            "Carry out a natural-language goal on the live page and do not stop until it is done \
             — e.g. 'open all four articles and read the gold price from each'. Plans the work, \
             executes the steps, then independently checks the page to decide whether the goal was \
             really met, replanning if not (up to the configured plan budget). Returns what it did, \
             what it found, and whether the check passed. Best for multi-step tasks; for a single \
             known action, call the specific tool instead.",
            json!({ "instruction": s("What to accomplish, stated in full") }), &["instruction"]),
        tool("browser_extract",
            "Answer a question about the current page, or pull structured data out of it. Reads \
             only — it never changes the page.",
            json!({
                "request": s("The question, or a description of the data you want"),
                "schema": s("Optional: describe the JSON shape to return, e.g. '[{name, price}]'")
            }), &["request"]),
    ])
}

/// Everything the model should know about the page after an action.
///
/// Assembled in a fixed order so it reads the same way every time.
async fn page_state(state: &Arc<AppState>, with_snapshot: bool) -> String {
    let sess = &state.session;
    let mut out = String::new();

    let info = sess.info().await.unwrap_or_else(|_| json!({}));
    out.push_str(&format!(
        "\n### Page\n{} — {}\n",
        info["url"].as_str().unwrap_or("?"),
        info["title"].as_str().unwrap_or("")
    ));

    // A modal comes first in importance: nothing else will work until it is gone.
    if let Some(d) = info.get("dialog") {
        out.push_str(&format!(
            "\n### Blocking dialog\n{}: {:?}\nAnswer it with browser_handle_dialog before anything else.\n",
            d["type"].as_str().unwrap_or("dialog"),
            d["message"].as_str().unwrap_or("")
        ));
        // No point rendering a snapshot of a frozen page.
        return out;
    }

    let rec = sess.active_recorder().await;
    let errs = rec.console(true, 100);
    if !errs.is_empty() {
        out.push_str(&format!(
            "\n### Console\n{} error(s); latest: {}\nCall browser_console_messages for the rest.\n",
            errs.len(),
            errs.last().map(|c| c.text.as_str()).unwrap_or("")
        ));
    }

    if let Ok(tabs) = sess.list_tabs().await {
        let list = tabs["tabs"].as_array().cloned().unwrap_or_default();
        if list.len() > 1 {
            out.push_str("\n### Open tabs\n");
            for t in list {
                out.push_str(&format!(
                    "{}{}: {} — {}\n",
                    if t["active"].as_bool().unwrap_or(false) {
                        "* "
                    } else {
                        "  "
                    },
                    t["index"],
                    t["title"].as_str().unwrap_or(""),
                    t["url"].as_str().unwrap_or("")
                ));
            }
        }
    }

    if with_snapshot {
        match sess.snapshot().await {
            Ok(snap) => {
                out.push_str(&format!("\n### Snapshot ({} elements", snap.count));
                if snap.new_refs > 0 {
                    out.push_str(&format!(", {} new marked *", snap.new_refs));
                }
                if snap.truncated {
                    out.push_str(", truncated — scroll or use browser_find to narrow");
                }
                out.push_str(")\n");
                out.push_str(&snap.scroll.describe());
                out.push('\n');
                out.push_str(&snap.tree);
            }
            Err(e) => out.push_str(&format!("\n### Snapshot\nunavailable: {e}\n")),
        }
    }
    out
}

/// Render a successful action: what it did, then where that left the page.
async fn acted(state: &Arc<AppState>, summary: String, with_snapshot: bool) -> Value {
    let mut text = format!("### Result\n{summary}\n");
    text.push_str(&page_state(state, with_snapshot).await);
    text_result(text)
}

/// Render a read-only result: no snapshot, because nothing changed.
fn read(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}

/// A one-line account of what an action did: a plain-language label, plus the
/// operation's own detail when there is any worth showing.
fn summarize(label: &str, v: &Value) -> String {
    match v {
        Value::Object(m) if !m.is_empty() => format!("{label} {}", Value::Object(m.clone())),
        _ => label.to_string(),
    }
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let sess = &state.session;
    let arg_ref = |k: &str| -> String {
        args[k]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| match &args[k] {
                Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
    };
    let index = || -> usize {
        args["index"]
            .as_u64()
            .or_else(|| args["index"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0) as usize
    };

    // Anything that can change the page reports the resulting page.
    macro_rules! act {
        ($e:expr, $label:expr) => {
            match $e.await {
                Ok(v) => acted(state, summarize($label, &v), true).await,
                Err(e) => error_result(e.to_string()),
            }
        };
    }

    match name {
        // ---- observing -------------------------------------------------
        "browser_snapshot" => match sess.snapshot().await {
            Ok(snap) => {
                let mut t = format!("{} — {}\n{} elements", snap.url, snap.title, snap.count);
                if snap.new_refs > 0 {
                    t.push_str(&format!(", {} new (marked *)", snap.new_refs));
                }
                if snap.extra_clickables > 0 {
                    t.push_str(&format!(", {} clickable by style", snap.extra_clickables));
                }
                if snap.truncated {
                    t.push_str(", truncated");
                }
                t.push('\n');
                t.push_str(&snap.scroll.describe());
                t.push('\n');
                t.push_str(&snap.tree);
                text_result(t)
            }
            Err(e) => error_result(e.to_string()),
        },
        "browser_find" => match sess.find(args["text"].as_str().unwrap_or("")).await {
            Ok(v) => {
                if v["matches"].is_null() {
                    text_result(format!(
                        "No match for {:?} on {}. Try browser_snapshot to see the whole page.",
                        args["text"].as_str().unwrap_or(""),
                        v["url"].as_str().unwrap_or("")
                    ))
                } else {
                    text_result(v["matches"].as_str().unwrap_or("").to_string())
                }
            }
            Err(e) => error_result(e.to_string()),
        },
        "browser_screenshot" => {
            let full = args["full_page"].as_bool().unwrap_or(false);
            match sess.screenshot_b64(full).await {
                Ok(data) => json!({ "content": [
                    { "type": "image", "data": data, "mimeType": "image/jpeg" }
                ]}),
                Err(e) => error_result(e.to_string()),
            }
        }
        "browser_get_info" => match sess.info().await {
            Ok(v) => read(v),
            Err(e) => error_result(e.to_string()),
        },
        "browser_extract_text" => match sess.extract_text(args["selector"].as_str()).await {
            Ok(v) => text_result(v["text"].as_str().unwrap_or("").to_string()),
            Err(e) => error_result(e.to_string()),
        },
        "browser_extract_links" => match sess.extract_links().await {
            Ok(v) => read(v),
            Err(e) => error_result(e.to_string()),
        },

        // ---- navigating ------------------------------------------------
        "browser_navigate" => {
            let v = sess.navigate(args["url"].as_str().unwrap_or("")).await;
            if let Ok(ref info) = v {
                state
                    .db
                    .add_history(
                        info["url"].as_str().unwrap_or(""),
                        info["title"].as_str().unwrap_or(""),
                        crate::api::now(),
                    )
                    .ok();
            }
            match v {
                Ok(_) => acted(state, "navigated".into(), true).await,
                Err(e) => error_result(e.to_string()),
            }
        }
        "browser_back" => act!(sess.go_back(), "went back"),
        "browser_forward" => act!(sess.go_forward(), "went forward"),
        "browser_reload" => act!(sess.reload(), "reloaded"),

        // ---- acting ----------------------------------------------------
        "browser_click" => {
            let button = args["button"].as_str().unwrap_or("left");
            let clicks = if args["double"].as_bool().unwrap_or(false) {
                2
            } else {
                1
            };
            act!(sess.click_ref(&arg_ref("ref"), button, clicks), "clicked")
        }
        "browser_type" => {
            let submit = args["submit"].as_bool().unwrap_or(false);
            let replace = !args["append"].as_bool().unwrap_or(false);
            act!(
                sess.type_ref(
                    &arg_ref("ref"),
                    args["text"].as_str().unwrap_or(""),
                    submit,
                    replace
                ),
                "typed"
            )
        }
        "browser_fill_form" => {
            let empty = vec![];
            let fields = args["fields"].as_array().unwrap_or(&empty).clone();
            act!(sess.fill_form(&fields), "filled the form")
        }
        "browser_select_option" => {
            let values: Vec<String> = args["values"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            act!(sess.select_option(&arg_ref("ref"), &values), "selected")
        }
        "browser_hover" => act!(sess.hover_ref(&arg_ref("ref")), "hovered"),
        "browser_drag" => act!(sess.drag(&arg_ref("from"), &arg_ref("to")), "dragged"),
        "browser_press_key" => act!(
            sess.press_key(args["key"].as_str().unwrap_or("Enter")),
            "pressed"
        ),
        "browser_scroll" => {
            let amount = args["amount"].as_f64().unwrap_or(600.0);
            let up = args["direction"]
                .as_str()
                .unwrap_or("down")
                .eq_ignore_ascii_case("up");
            act!(
                sess.scroll(0.0, if up { -amount } else { amount }),
                "scrolled"
            )
        }
        "browser_scroll_to" => act!(sess.scroll_to_ref(&arg_ref("ref")), "scrolled to"),
        "browser_highlight" => {
            let ms = args["ms"].as_u64().unwrap_or(1200);
            match sess.highlight_ref(&arg_ref("ref"), ms).await {
                Ok(v) => read(v),
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- modals, files ---------------------------------------------
        "browser_handle_dialog" => {
            let accept = args["accept"].as_bool().unwrap_or(true);
            act!(
                sess.handle_dialog(accept, args["prompt_text"].as_str()),
                "answered the dialog"
            )
        }
        "browser_file_upload" => {
            let paths: Vec<String> = args["paths"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            act!(sess.upload_files(&paths), "uploaded")
        }
        "browser_downloads" => read(json!(sess.downloads().await)),

        // ---- waiting ---------------------------------------------------
        "browser_wait_for" => {
            let text = args["text"].as_str();
            let gone = args["text_gone"].as_str();
            let secs = args["seconds"].as_f64();
            act!(sess.wait_for(text, gone, secs), "waited")
        }

        // ---- diagnostics -----------------------------------------------
        "browser_console_messages" => {
            let only = args["errors_only"].as_bool().unwrap_or(false);
            let limit = args["limit"].as_u64().unwrap_or(50) as usize;
            read(sess.active_recorder().await.console_json(only, limit))
        }
        "browser_network_requests" => {
            let statics = args["include_static"].as_bool().unwrap_or(false);
            let limit = args["limit"].as_u64().unwrap_or(50) as usize;
            read(sess.active_recorder().await.requests_json(
                statics,
                args["filter"].as_str(),
                limit,
            ))
        }

        // ---- tabs, environment -----------------------------------------
        "browser_new_tab" => act!(sess.new_tab(args["url"].as_str()), "opened a tab"),
        "browser_list_tabs" => match sess.list_tabs().await {
            Ok(v) => read(v),
            Err(e) => error_result(e.to_string()),
        },
        "browser_switch_tab" => act!(sess.switch_tab(index()), "switched tab"),
        "browser_close_tab" => act!(sess.close_tab(index()), "closed tab"),
        "browser_resize" => {
            let w = args["width"].as_u64().unwrap_or(1280) as u32;
            let h = args["height"].as_u64().unwrap_or(800) as u32;
            act!(sess.resize(w, h), "resized")
        }
        "browser_execute_js" => {
            match sess.execute_js(args["script"].as_str().unwrap_or("")).await {
                Ok(v) => read(v),
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- handing over ----------------------------------------------
        "browser_request_login" => {
            let reason = args["reason"].as_str().unwrap_or("").trim();
            match sess.set_takeover(true, args["url"].as_str()).await {
                Ok(v) => text_result(format!(
                    "Handed the browser to the user.{}\n{}\n\nStop here and tell them what you need \
                     signed in. Do not attempt the login yourself, and do not ask them for the \
                     credentials — they are signing in directly. You will be able to continue once \
                     they hand control back.",
                    if reason.is_empty() { String::new() } else { format!(" Reason: {reason}.") },
                    v["note"].as_str().unwrap_or("")
                )),
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- AI --------------------------------------------------------
        "browser_act" => {
            let instruction = args["instruction"].as_str().unwrap_or("");
            if instruction.trim().is_empty() {
                return error_result("instruction is required".into());
            }
            // Runs through the same engine, the same replan budget and the same
            // history as the panel — an MCP caller is not a second agent.
            match crate::api::run_agent_for_mcp(state, instruction).await {
                Ok(v) => text_result(crate::llm::format_run(&v)),
                Err(e) => error_result(e),
            }
        }
        "browser_extract" => {
            let request = args["request"].as_str().unwrap_or("");
            if request.trim().is_empty() {
                return error_result("request is required".into());
            }
            match crate::llm::extract(sess, request, args["schema"].as_str()).await {
                Ok((answer, _model)) => text_result(answer),
                Err(e) => error_result(e),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::tools_list;

    #[test]
    fn every_tool_has_a_name_description_and_schema() {
        let tools = tools_list();
        let list = tools.as_array().expect("array");
        assert!(
            list.len() >= 30,
            "expected the full tool set, got {}",
            list.len()
        );
        for t in list {
            let name = t["name"].as_str().expect("name");
            assert!(
                name.starts_with("browser_"),
                "{name} breaks the naming convention"
            );
            assert!(
                t["description"]
                    .as_str()
                    .map(|d| d.len() > 30)
                    .unwrap_or(false),
                "{name} needs a description the model can act on"
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{name} schema");
        }
    }

    /// The README and the manifest both quote a tool count. Pin it so the docs
    /// cannot quietly drift away from the code.
    #[test]
    fn the_documented_tool_count_is_the_real_one() {
        assert_eq!(
            tools_list().as_array().unwrap().len(),
            35,
            "tool count changed — update README.md and senclaw-manifest.json to match"
        );
    }

    #[test]
    fn tool_names_are_unique() {
        let tools = tools_list();
        let mut names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "duplicate tool name");
    }

    /// Every `required` entry must name a property that actually exists, or the
    /// model gets told to send a field the schema does not describe.
    #[test]
    fn required_fields_exist_in_properties() {
        for t in tools_list().as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            let props = &t["inputSchema"]["properties"];
            if let Some(req) = t["inputSchema"]["required"].as_array() {
                for r in req {
                    let key = r.as_str().unwrap();
                    assert!(
                        !props[key].is_null(),
                        "{name}: required '{key}' is not a property"
                    );
                }
            }
        }
    }

    /// The tools that address an element must all spell that argument the same
    /// way. A `ref` here and a `target` there is exactly the kind of drift that
    /// makes a model guess.
    #[test]
    fn element_targeting_argument_is_consistent() {
        let by_ref = [
            "browser_click",
            "browser_type",
            "browser_select_option",
            "browser_hover",
            "browser_scroll_to",
            "browser_highlight",
        ];
        let tools = tools_list();
        for name in by_ref {
            let t = tools
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == name)
                .unwrap_or_else(|| panic!("{name} missing"));
            assert!(
                !t["inputSchema"]["properties"]["ref"].is_null(),
                "{name} should take 'ref'"
            );
        }
    }
}
