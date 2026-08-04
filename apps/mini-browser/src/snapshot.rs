//! The page representation the AI reasons about.
//!
//! This used to be a DOM walk that wrote `data-mb-idx="7"` onto every
//! interactive element and handed the model a flat list of them. That had three
//! problems, and all three are structural rather than cosmetic:
//!
//!   1. **It edited the live page.** The user is signed into this profile and
//!      watching the same tab. Stamping attributes into their DOM can trip
//!      attribute selectors, mutation observers and React reconciliation, and it
//!      is trivially visible to any script on the page — including an anti-bot
//!      one looking for exactly this kind of marking.
//!   2. **It could not see inside anything.** `document.querySelectorAll` does
//!      not cross a shadow root or an iframe, so half of a modern page was
//!      simply invisible to the agent.
//!   3. **It threw the structure away.** A flat list of clickables cannot tell
//!      the model that a button lives inside the third row of a table, or that a
//!      heading introduces the paragraph under it.
//!
//! So the snapshot now comes from Chrome's own accessibility tree
//! (`Accessibility.getFullAXTree`). The browser has already computed the
//! accessible role, name and state of every node — including through shadow DOM
//! and same-process iframes — using the real accname algorithm. We read it, we
//! do not write anything, and the page cannot tell we looked.
//!
//! The rendering deliberately matches the shape Playwright's MCP server emits,
//! because that is the page format today's models have seen the most of:
//!
//! ```text
//! - heading "Example Domain" [level=1] [ref=e3]
//! - paragraph [ref=e4]: This domain is for use in documentation examples.
//! - link "Learn more" [ref=e6]:
//!   - /url: https://iana.org/domains/example
//! ```
//!
//! Each `ref` maps to a CDP `backendNodeId`, which is what `session.rs` feeds to
//! `DOM.getContentQuads` to get real viewport coordinates — correct even for an
//! element nested inside an iframe, which the old `getBoundingClientRect` math
//! got silently wrong.

use std::borrow::Cow;
use std::collections::HashMap;

use chromiumoxide::cdp::browser_protocol::dom::BackendNodeId;
use chromiumoxide::{Command, Method};
use serde::{Deserialize, Serialize};

/// `Accessibility.getFullAXTree`, issued and decoded by hand.
///
/// chromiumoxide ships generated types for this, and using them was a bug: the
/// generated `AXPropertyName` and `AXValueType` are closed enums built from the
/// protocol snapshot the crate was published with, so the moment Chrome emits a
/// value newer than that — `"uninteresting"`, on the Chrome this was developed
/// against — serde rejects it and the *entire* tree fails to decode. Not one
/// property: the whole page, with the error `Serde(Error("uninteresting"))`,
/// which points at nothing useful.
///
/// Chrome ships a new stable every four weeks and the pinned protocol will
/// always lag it, so this is a recurring failure by construction rather than a
/// one-off. Decoding into plain strings makes an unrecognised value cost exactly
/// what it should — one property we do not render — instead of the whole
/// snapshot.
#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetFullAxTree {
    /// Which document to read. `None` means the top frame — and only the top
    /// frame, which is why `session::stitch_frames` has to ask again per iframe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
}

impl Method for GetFullAxTree {
    fn identifier(&self) -> Cow<'static, str> {
        Cow::Borrowed("Accessibility.getFullAXTree")
    }
}

impl Command for GetFullAxTree {
    type Response = AxTree;
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct AxTree {
    #[serde(default)]
    pub nodes: Vec<AxNode>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct AxNode {
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default)]
    pub role: Option<AxValue>,
    #[serde(default)]
    pub name: Option<AxValue>,
    #[serde(default)]
    pub value: Option<AxValue>,
    #[serde(default)]
    pub properties: Option<Vec<AxProperty>>,
    #[serde(default)]
    pub child_ids: Option<Vec<String>>,
    #[serde(default, rename = "backendDOMNodeId")]
    pub backend_dom_node_id: Option<i64>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct AxValue {
    #[serde(default)]
    pub value: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct AxProperty {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub value: AxValue,
}

/// Refs that survive re-snapshotting.
///
/// The naive thing is to number elements 1..n on every capture. That quietly
/// breaks the most natural way to use an agent loop: observe, think, act. If the
/// page re-renders between the snapshot and the click — and something always
/// re-renders — every ref shifts by one and the agent clicks the wrong control
/// while believing it clicked the right one. A wrong click is much worse than a
/// failed one.
///
/// So a ref is bound to the element's `backendNodeId`, which Chrome keeps stable
/// for as long as that node exists. The same button keeps `e7` across every
/// snapshot of the document, and a ref from three turns ago still resolves.
/// Navigation replaces the document, so `reset()` drops the lot — a ref from the
/// previous page must fail loudly rather than land somewhere arbitrary.
#[derive(Debug, Default)]
pub struct RefRegistry {
    by_node: HashMap<i64, String>,
    by_ref: HashMap<String, BackendNodeId>,
    next: usize,
}

impl RefRegistry {
    pub fn reset(&mut self) {
        self.by_node.clear();
        self.by_ref.clear();
        self.next = 0;
    }

    /// The ref for this node, minting one if it is new. Returns
    /// `(ref, is_new)` — `is_new` drives the `*` marker in the rendering.
    fn intern(&mut self, backend: i64) -> (String, bool) {
        if let Some(r) = self.by_node.get(&backend) {
            return (r.clone(), false);
        }
        self.next += 1;
        let r = format!("e{}", self.next);
        self.by_node.insert(backend, r.clone());
        self.by_ref.insert(r.clone(), BackendNodeId::new(backend));
        (r, true)
    }

    /// Look a ref up, tolerating the `#e12` / `e12` / `12` spellings a model
    /// might produce.
    pub fn resolve(&self, r: &str) -> Option<BackendNodeId> {
        let t = r.trim().trim_start_matches('#');
        if let Some(id) = self.by_ref.get(t) {
            return Some(id.clone());
        }
        let with_e = format!("e{}", t.trim_start_matches('e'));
        self.by_ref.get(&with_e).cloned()
    }
}

/// Where the viewport sits in the document.
///
/// The accessibility tree is position-blind: it describes the whole document
/// with no hint of what is on screen or how much is left. An agent reading it
/// cannot tell whether it has seen the page, and either stops early on a long
/// one or scrolls a short one forever. Three numbers fix that.
#[derive(Debug, Clone, Default)]
pub struct Scroll {
    pub y: f64,
    pub height: f64,
    pub viewport: f64,
}

impl Scroll {
    fn pages_above(&self) -> f64 {
        if self.viewport <= 0.0 {
            0.0
        } else {
            self.y / self.viewport
        }
    }
    fn pages_below(&self) -> f64 {
        if self.viewport <= 0.0 {
            0.0
        } else {
            ((self.height - self.y - self.viewport).max(0.0)) / self.viewport
        }
    }
    fn percent(&self) -> u32 {
        let scrollable = (self.height - self.viewport).max(0.0);
        if scrollable <= 1.0 {
            100
        } else {
            ((self.y / scrollable) * 100.0).round().clamp(0.0, 100.0) as u32
        }
    }
    /// True when the whole document already fits on screen.
    pub fn fits(&self) -> bool {
        self.height <= self.viewport + 1.0
    }

    /// One line telling the model where it is and how much is left.
    pub fn describe(&self) -> String {
        if self.fits() {
            return "Whole page fits on screen — nothing to scroll.".to_string();
        }
        format!(
            "Viewport {:.0}px of a {:.0}px page — {:.1} pages above, {:.1} below, at {}%.",
            self.viewport,
            self.height,
            self.pages_above(),
            self.pages_below(),
            self.percent()
        )
    }
}

/// A captured page: the text the model reads, plus what changed since last time.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub url: String,
    pub title: String,
    /// The YAML-ish tree handed to the model.
    pub tree: String,
    /// Number of nodes that carry a ref.
    pub count: usize,
    /// Refs appearing for the first time in this capture.
    pub new_refs: usize,
    /// True when the tree was cut off by the node budget.
    pub truncated: bool,
    /// Where the viewport sits in the document.
    pub scroll: Scroll,
    /// Elements found actionable by computed style that the a11y tree gave no
    /// role to — see `session::clickable_backends`.
    pub extra_clickables: usize,
}

/// Nodes that carry no information a model can use.
fn is_noise(role: &str) -> bool {
    matches!(
        role,
        "none" | "presentation" | "InlineTextBox" | "LineBreak" | "IframePresentational"
    )
}

/// Chrome reports a handful of internal role names that no ARIA author would
/// recognise. Translate the common ones and pass everything else through, so a
/// role we have never seen still shows up rather than disappearing.
fn friendly_role(role: &str) -> &str {
    match role {
        "RootWebArea" | "WebArea" => "document",
        "StaticText" => "text",
        "genericContainer" | "GenericContainer" => "generic",
        "Iframe" | "iframe" => "iframe",
        "image" => "img",
        "LabelText" => "label",
        "DescriptionListDetail" => "definition",
        "DescriptionListTerm" => "term",
        "ListMarker" => "listmarker",
        "PluginObject" | "EmbeddedObject" => "embed",
        "textFieldWithComboBox" => "combobox",
        "SvgRoot" => "graphics-document",
        other => other,
    }
}

/// Roles whose subtree is pure decoration once we have the node itself.
fn is_leafish(role: &str) -> bool {
    matches!(role, "text" | "listmarker" | "img")
}

fn ax_string(v: Option<&AxValue>) -> String {
    let Some(v) = v else { return String::new() };
    match v.value.as_ref() {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        _ => String::new(),
    }
}

/// Collapse runs of whitespace and clip, so one `<pre>` block cannot blow the
/// whole snapshot budget. Char-based, because a byte slice would panic on the
/// Vietnamese text this browser mostly reads.
fn clean(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len().min(max * 4));
    let mut space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(ch);
        if out.chars().count() >= max {
            out.push('…');
            break;
        }
    }
    out
}

/// The state flags worth showing. Chrome reports dozens of properties; most are
/// either always-present defaults or irrelevant to deciding what to click, and
/// every one we print costs tokens on every snapshot.
fn interesting_props(node: &AxNode) -> Vec<String> {
    let mut out = Vec::new();
    let Some(props) = node.properties.as_ref() else {
        return out;
    };
    for p in props {
        let raw = ax_string(Some(&p.value));
        let truthy = match p.value.value.as_ref() {
            Some(serde_json::Value::Bool(b)) => *b,
            Some(serde_json::Value::String(s)) => !s.is_empty() && s != "false" && s != "none",
            Some(serde_json::Value::Number(_)) => true,
            _ => false,
        };
        match p.name.as_str() {
            // Tristates: "true" / "false" / "mixed".
            "checked" | "selected" | "pressed" => {
                if raw == "true" {
                    out.push(p.name.clone());
                } else if raw == "mixed" {
                    out.push("mixed".into());
                }
            }
            "disabled" if truthy => out.push("disabled".into()),
            "required" if truthy => out.push("required".into()),
            "readonly" if truthy => out.push("readonly".into()),
            "focused" if truthy => out.push("active".into()),
            "expanded" => out.push(if truthy {
                "expanded".into()
            } else {
                "collapsed".into()
            }),
            "modal" if truthy => out.push("modal".into()),
            "level" => {
                if !raw.is_empty() && raw != "0" {
                    out.push(format!("level={raw}"));
                }
            }
            "invalid" => {
                if !raw.is_empty() && raw != "false" {
                    out.push("invalid".into());
                }
            }
            "multiselectable" if truthy => out.push("multiselectable".into()),
            _ => {}
        }
    }
    out
}

fn url_prop(node: &AxNode) -> Option<String> {
    let props = node.properties.as_ref()?;
    for p in props {
        if p.name == "url" {
            let s = ax_string(Some(&p.value));
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

/// How many ref-bearing nodes one snapshot may contain. A big search-results
/// page runs to a few hundred; past that the model is being flooded rather than
/// informed, and the caller should scroll or narrow instead.
const MAX_NODES: usize = 600;
/// Guard against a pathological tree (or a cycle in malformed `childIds`).
const MAX_DEPTH: usize = 40;

/// Build the model-facing snapshot from a flat list of AX nodes.
///
/// CDP returns the tree flattened with parent/child ids, so the first job is to
/// index it and find the root — the node nobody claims as a child.
pub fn render(
    nodes: &[AxNode],
    url: &str,
    title: &str,
    registry: &mut RefRegistry,
    clickables: &std::collections::HashSet<i64>,
    scroll: Scroll,
) -> Snapshot {
    let by_id: HashMap<&str, &AxNode> = nodes.iter().map(|n| (n.node_id.as_str(), n)).collect();
    let claimed: std::collections::HashSet<&str> = nodes
        .iter()
        .filter_map(|n| n.child_ids.as_ref())
        .flatten()
        .map(|c| c.as_str())
        .collect();
    let roots: Vec<&AxNode> = nodes
        .iter()
        .filter(|n| !claimed.contains(n.node_id.as_str()))
        .collect();

    let mut st = Render {
        by_id,
        out: String::new(),
        registry,
        clickables,
        count: 0,
        new_refs: 0,
        extra: 0,
        truncated: false,
        seen: std::collections::HashSet::new(),
    };
    for r in roots {
        st.walk(r, 0);
    }

    let extra = st.extra;
    let truncated = st.truncated;
    let (count, new_refs) = (st.count, st.new_refs);
    let mut tree = if st.out.is_empty() {
        "(page is empty)".to_string()
    } else {
        st.out
    };

    // Bracket the tree so the model can tell "this is all of it" from "this is
    // the part you can see". Without the markers a long page reads exactly like
    // a short one.
    if !scroll.fits() {
        let above = if scroll.y > 1.0 {
            "[more above — scroll up]"
        } else {
            "[start of page]"
        };
        let below = if scroll.height - scroll.y - scroll.viewport > 1.0 {
            "[more below — scroll down]"
        } else {
            "[end of page]"
        };
        tree = format!("{above}\n{tree}{below}\n");
    }

    Snapshot {
        url: url.to_string(),
        title: title.to_string(),
        count,
        new_refs,
        truncated,
        tree,
        scroll,
        extra_clickables: extra,
    }
}

struct Render<'a, 'r> {
    by_id: HashMap<&'a str, &'a AxNode>,
    out: String,
    registry: &'r mut RefRegistry,
    clickables: &'r std::collections::HashSet<i64>,
    count: usize,
    new_refs: usize,
    extra: usize,
    truncated: bool,
    seen: std::collections::HashSet<&'a str>,
}

impl<'a, 'r> Render<'a, 'r> {
    fn walk(&mut self, node: &'a AxNode, depth: usize) {
        if depth > MAX_DEPTH || !self.seen.insert(node.node_id.as_str()) {
            return;
        }
        if self.count >= MAX_NODES {
            self.truncated = true;
            return;
        }

        let raw_role = ax_string(node.role.as_ref());
        let mut role = friendly_role(&raw_role).to_string();

        // Does the page's own styling say this element is actionable?
        //
        // This is the one thing the accessibility tree genuinely cannot tell us.
        // A `<div onclick>` with no role and no ARIA is not an accessibility
        // object — Chrome reports it as `generic`, or ignores it outright — yet
        // it is what a great many application UIs are actually built from. The
        // agent could read the label and never learn it was a target.
        //
        // `session::clickable_backends` asks Chrome which elements compute to an
        // interactive cursor, which is the same signal a sighted user acts on.
        let clickable = node
            .backend_dom_node_id
            .map(|b| self.clickables.contains(&b))
            .unwrap_or(false);

        // An ignored or noise node contributes nothing itself, but its children
        // may — a `display:contents` wrapper is ignored while everything under
        // it is real. So we skip the node and hoist its children to this level.
        // Unless the page says it is clickable, in which case dropping it is
        // exactly the mistake this branch is here to avoid.
        if (node.ignored || is_noise(&role) || role.is_empty()) && !clickable {
            self.children(node, depth);
            return;
        }

        let mut name = clean(&ax_string(node.name.as_ref()), 160);
        let value = clean(&ax_string(node.value.as_ref()), 160);

        // Promote an anonymous clickable to something the model will act on, and
        // give it a label from the text it contains — the same text a person
        // would read off the control.
        if clickable && (role == "generic" || role.is_empty() || role == "text") {
            role = "clickable".to_string();
            self.extra += 1;
            if name.is_empty() {
                name = clean(&self.text_of(node, 0), 80);
            }
        }

        // Plain text: no ref, no nesting — it is content, not a target.
        if role == "text" && !clickable {
            if !name.is_empty() {
                self.line(depth, &format!("- text: {name}"));
            }
            return;
        }

        // A "generic" with nothing to say is pure markup scaffolding. Drop it and
        // pull its children up, or the tree becomes mostly indentation.
        if role == "generic" && name.is_empty() && value.is_empty() && !clickable {
            self.children(node, depth);
            return;
        }

        let mut head = format!("- {role}");
        if !name.is_empty() {
            head.push_str(&format!(" \"{}\"", name.replace('"', "'")));
        }
        for p in interesting_props(node) {
            head.push_str(&format!(" [{p}]"));
        }

        if let Some(backend) = node.backend_dom_node_id {
            let (key, is_new) = self.registry.intern(backend);
            self.count += 1;
            // A leading `*` flags an element that was not in the previous
            // snapshot. After a click that opens a menu, that one character
            // tells the model which handful of lines are the consequence of
            // what it just did, without diffing two full trees in its head.
            head.push_str(&format!(" [ref={key}]"));
            if is_new {
                self.new_refs += 1;
                head.insert_str(2, "*");
            }
        }

        let url = url_prop(node);
        let kids: Vec<&'a AxNode> = if is_leafish(&role) {
            Vec::new()
        } else {
            self.child_nodes(node)
        };
        let has_body = url.is_some() || !kids.is_empty();

        if !value.is_empty() && !has_body {
            self.line(depth, &format!("{head}: {value}"));
            return;
        }
        if !has_body {
            self.line(depth, &head);
            return;
        }

        self.line(depth, &format!("{head}:"));
        if !value.is_empty() {
            self.line(depth + 1, &format!("- /value: {value}"));
        }
        if let Some(u) = url {
            self.line(depth + 1, &format!("- /url: {}", clean(&u, 200)));
        }
        for k in kids {
            self.walk(k, depth + 1);
        }
    }

    /// The text a control contains, for labelling an element the page styled as
    /// clickable but gave no accessible name.
    fn text_of(&self, node: &'a AxNode, depth: usize) -> String {
        if depth > 4 {
            return String::new();
        }
        let mut out = String::new();
        for k in self.child_nodes(node) {
            let role = friendly_role(&ax_string(k.role.as_ref())).to_string();
            let name = ax_string(k.name.as_ref());
            if role == "text" && !name.is_empty() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&name);
            } else {
                let deeper = self.text_of(k, depth + 1);
                if !deeper.is_empty() {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(&deeper);
                }
            }
            if out.chars().count() > 120 {
                break;
            }
        }
        out
    }

    fn child_nodes(&self, node: &AxNode) -> Vec<&'a AxNode> {
        node.child_ids
            .as_ref()
            .map(|ids| {
                ids.iter()
                    .filter_map(|c| self.by_id.get(c.as_str()).copied())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn children(&mut self, node: &'a AxNode, depth: usize) {
        for k in self.child_nodes(node) {
            self.walk(k, depth);
        }
    }

    fn line(&mut self, depth: usize, text: &str) {
        for _ in 0..depth {
            self.out.push_str("  ");
        }
        self.out.push_str(text);
        self.out.push('\n');
    }
}

/// Search a rendered tree for lines matching `needle`, returning each hit with a
/// little surrounding context. Lets an agent locate one control on a huge page
/// without paying for the whole snapshot.
pub fn find(tree: &str, needle: &str, context: usize) -> String {
    let lines: Vec<&str> = tree.lines().collect();
    let lower = needle.to_lowercase();
    let mut keep: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for (i, l) in lines.iter().enumerate() {
        if l.to_lowercase().contains(&lower) {
            let from = i.saturating_sub(context);
            let to = (i + context).min(lines.len().saturating_sub(1));
            for k in from..=to {
                keep.insert(k);
            }
        }
    }
    if keep.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let mut prev: Option<usize> = None;
    for i in keep {
        if let Some(p) = prev {
            if i > p + 1 {
                out.push_str("  …\n");
            }
        }
        out.push_str(lines[i]);
        out.push('\n');
        prev = Some(i);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> AxValue {
        AxValue {
            value: Some(serde_json::Value::String(s.to_string())),
        }
    }

    fn node(id: &str, role: &str, name: &str, kids: &[&str], backend: Option<i64>) -> AxNode {
        AxNode {
            node_id: id.to_string(),
            ignored: false,
            role: Some(v(role)),
            name: if name.is_empty() { None } else { Some(v(name)) },
            value: None,
            properties: None,
            child_ids: if kids.is_empty() {
                None
            } else {
                Some(kids.iter().map(|k| k.to_string()).collect())
            },
            backend_dom_node_id: backend,
        }
    }

    fn no_clicks() -> std::collections::HashSet<i64> {
        std::collections::HashSet::new()
    }

    /// A short page: no scroll markers, so assertions stay about the tree.
    fn fitting() -> Scroll {
        Scroll {
            y: 0.0,
            height: 800.0,
            viewport: 800.0,
        }
    }

    /// Render with a throwaway registry, for tests that do not care about it.
    fn once(nodes: &[AxNode]) -> Snapshot {
        let mut reg = RefRegistry::default();
        render(nodes, "u", "t", &mut reg, &no_clicks(), fitting())
    }

    /// The regression test for the bug that made this module decode by hand.
    ///
    /// Chrome sent a property named `uninteresting`, which the crate's generated
    /// enum had never heard of, and serde threw out the whole tree — every page
    /// came back as `Serde(Error("uninteresting"))` and the browser could not see
    /// anything at all. An unknown property must cost one property.
    #[test]
    fn a_property_name_chrome_invented_later_does_not_destroy_the_tree() {
        let raw = r#"{"nodes":[
            {"nodeId":"1","ignored":false,
             "role":{"type":"role","value":"button"},
             "name":{"type":"computedString","value":"Sign in"},
             "properties":[
                {"name":"uninteresting","value":{"type":"boolean","value":false}},
                {"name":"somethingFromChrome999","value":{"type":"token","value":"x"}},
                {"name":"disabled","value":{"type":"booleanOrUndefined","value":true}}],
             "backendDOMNodeId":42}
        ]}"#;
        let tree: AxTree = serde_json::from_str(raw).expect("unknown properties must not fail");
        assert_eq!(tree.nodes.len(), 1);

        let s = once(&tree.nodes);
        assert!(s.tree.contains("button \"Sign in\""), "{}", s.tree);
        assert!(
            s.tree.contains("[disabled]"),
            "known properties still render: {}",
            s.tree
        );
        assert!(
            !s.tree.contains("uninteresting"),
            "unknown ones are dropped: {}",
            s.tree
        );
    }

    /// Fields Chrome omits must not be required, or a node without a name or
    /// children takes the whole page down with it.
    #[test]
    fn sparse_nodes_decode() {
        let tree: AxTree =
            serde_json::from_str(r#"{"nodes":[{"nodeId":"1","ignored":true}]}"#).expect("decode");
        assert_eq!(tree.nodes.len(), 1);
        assert!(tree.nodes[0].backend_dom_node_id.is_none());
        assert!(
            serde_json::from_str::<AxTree>(r#"{}"#).is_ok(),
            "an empty response is not an error"
        );
    }

    #[test]
    fn renders_a_nested_tree_with_refs() {
        let nodes = vec![
            node("1", "RootWebArea", "Probe", &["2", "3"], Some(10)),
            node("2", "heading", "Hello", &[], Some(11)),
            node("3", "button", "Sign in", &[], Some(12)),
        ];
        let s = once(&nodes);
        assert!(s.tree.contains("heading \"Hello\" [ref=e2]"), "{}", s.tree);
        assert!(s.tree.contains("button \"Sign in\" [ref=e3]"), "{}", s.tree);
        // Children are indented under the document node.
        assert!(s.tree.contains("\n  - *heading"), "{}", s.tree);
        assert_eq!(s.count, 3);
    }

    #[test]
    fn refs_resolve_to_backend_ids_and_tolerate_spelling() {
        let mut reg = RefRegistry::default();
        render(
            &[node("1", "button", "Go", &[], Some(42))],
            "u",
            "t",
            &mut reg,
            &no_clicks(),
            fitting(),
        );
        let want = BackendNodeId::new(42);
        assert_eq!(reg.resolve("e1"), Some(want.clone()));
        assert_eq!(reg.resolve("#e1"), Some(want.clone()));
        assert_eq!(reg.resolve("1"), Some(want));
        assert_eq!(reg.resolve("e99"), None);
    }

    /// The whole point of the registry: the same element keeps its ref even
    /// when the page re-renders and shifts everything around it.
    #[test]
    fn a_ref_survives_re_snapshotting_and_reordering() {
        let mut reg = RefRegistry::default();
        let first = vec![
            node("a", "button", "Alpha", &[], Some(100)),
            node("b", "button", "Beta", &[], Some(200)),
        ];
        let s1 = render(&first, "u", "t", &mut reg, &no_clicks(), fitting());
        assert!(
            s1.tree.contains("*button \"Alpha\" [ref=e1]"),
            "{}",
            s1.tree
        );
        assert!(s1.tree.contains("*button \"Beta\" [ref=e2]"), "{}", s1.tree);

        // A banner appears at the top and Beta moves down. Naive numbering would
        // renumber Beta to e3 and the agent would click the banner.
        let second = vec![
            node("z", "button", "Banner", &[], Some(50)),
            node("a", "button", "Alpha", &[], Some(100)),
            node("b", "button", "Beta", &[], Some(200)),
        ];
        let s2 = render(&second, "u", "t", &mut reg, &no_clicks(), fitting());
        assert!(s2.tree.contains("button \"Beta\" [ref=e2]"), "{}", s2.tree);
        assert_eq!(reg.resolve("e2"), Some(BackendNodeId::new(200)));
        // Only the banner is new.
        assert_eq!(s2.new_refs, 1);
        assert!(s2.tree.contains("*button \"Banner\""), "{}", s2.tree);
        assert!(
            !s2.tree.contains("*button \"Beta\""),
            "Beta is not new: {}",
            s2.tree
        );
    }

    #[test]
    fn navigation_invalidates_every_ref() {
        let mut reg = RefRegistry::default();
        render(
            &[node("a", "button", "Old", &[], Some(7))],
            "u",
            "t",
            &mut reg,
            &no_clicks(),
            fitting(),
        );
        assert!(reg.resolve("e1").is_some());
        reg.reset();
        assert_eq!(
            reg.resolve("e1"),
            None,
            "a ref from the previous page must not resolve"
        );
    }

    #[test]
    fn ignored_nodes_are_skipped_but_their_children_survive() {
        let mut wrapper = node("1", "generic", "", &["2"], Some(1));
        wrapper.ignored = true;
        let nodes = vec![wrapper, node("2", "link", "Deep", &[], Some(2))];
        let s = once(&nodes);
        assert!(s.tree.contains("link \"Deep\""), "{}", s.tree);
        // The hoisted child sits at the top level, not indented under a ghost.
        assert!(s.tree.starts_with("- *link"), "{}", s.tree);
    }

    #[test]
    fn empty_generic_wrappers_do_not_add_indentation() {
        let nodes = vec![
            node("1", "generic", "", &["2"], Some(1)),
            node("2", "generic", "", &["3"], Some(2)),
            node("3", "button", "Deep", &[], Some(3)),
        ];
        let s = once(&nodes);
        assert_eq!(s.tree.trim(), "- *button \"Deep\" [ref=e1]", "{}", s.tree);
    }

    #[test]
    fn static_text_becomes_content_not_a_target() {
        let nodes = vec![
            node("1", "paragraph", "", &["2"], Some(1)),
            node("2", "StaticText", "Some words", &[], Some(2)),
        ];
        let s = once(&nodes);
        assert!(s.tree.contains("- text: Some words"), "{}", s.tree);
        // Only the paragraph is addressable — clicking a text run is meaningless.
        assert_eq!(s.count, 1);
    }

    #[test]
    fn links_expose_their_url() {
        let mut n = node("1", "link", "Learn more", &[], Some(1));
        n.properties = Some(vec![AxProperty {
            name: "url".into(),
            value: v("https://example.com/x"),
        }]);
        let s = once(&[n]);
        assert!(
            s.tree.contains("- /url: https://example.com/x"),
            "{}",
            s.tree
        );
    }

    #[test]
    fn checked_and_disabled_states_are_shown() {
        let mut n = node("1", "checkbox", "Remember me", &[], Some(1));
        n.properties = Some(vec![
            AxProperty {
                name: "checked".into(),
                value: v("true"),
            },
            AxProperty {
                name: "disabled".into(),
                value: AxValue {
                    value: Some(serde_json::Value::Bool(true)),
                },
            },
        ]);
        let s = once(&[n]);
        assert!(s.tree.contains("[checked]"), "{}", s.tree);
        assert!(s.tree.contains("[disabled]"), "{}", s.tree);
    }

    #[test]
    fn a_cycle_in_child_ids_cannot_hang_the_walk() {
        let nodes = vec![
            node("1", "generic", "A", &["2"], Some(1)),
            node("2", "generic", "B", &["1"], Some(2)),
        ];
        let s = once(&nodes);
        assert!(s.count <= 2);
    }

    #[test]
    fn node_budget_is_enforced() {
        let mut nodes = vec![node("root", "document", "", &[], Some(0))];
        let kids: Vec<String> = (0..MAX_NODES + 50).map(|i| i.to_string()).collect();
        nodes[0].child_ids = Some(kids.clone());
        for (i, k) in kids.iter().enumerate() {
            nodes.push(node(k, "button", "b", &[], Some(i as i64 + 1)));
        }
        let s = once(&nodes);
        assert!(s.truncated, "expected truncation");
        assert!(s.count <= MAX_NODES + 1);
    }

    #[test]
    fn multibyte_text_is_clipped_without_panicking() {
        let long = "Chào bạn ".repeat(60);
        let nodes = vec![node("1", "paragraph", &long, &[], Some(1))];
        let s = once(&nodes);
        assert!(s.tree.contains('…'));
    }

    /// The gap an accessibility tree cannot close on its own: a styled div that
    /// the page treats as a button. Chrome calls it `generic`, so without the
    /// computed-cursor signal the agent reads the words and never learns it can
    /// press them.
    #[test]
    fn a_styled_div_becomes_actionable() {
        let nodes = vec![
            node("1", "generic", "", &["2"], Some(50)),
            node("2", "StaticText", "Xem thêm", &[], Some(51)),
        ];

        let plain = once(&nodes);
        assert!(
            !plain.tree.contains("[ref="),
            "no role, no ref: {}",
            plain.tree
        );
        assert!(plain.tree.contains("- text: Xem thêm"), "{}", plain.tree);

        let mut reg = RefRegistry::default();
        let clicks: std::collections::HashSet<i64> = [50].into_iter().collect();
        let promoted = render(&nodes, "u", "t", &mut reg, &clicks, fitting());
        assert!(
            promoted.tree.contains("clickable \"Xem thêm\""),
            "the styled div should be actionable and labelled: {}",
            promoted.tree
        );
        assert!(promoted.tree.contains("[ref=e1]"), "{}", promoted.tree);
        assert_eq!(promoted.extra_clickables, 1);
    }

    /// A node Chrome ignores outright must still surface if the page styles it
    /// as clickable — that is precisely the case the a11y tree gets wrong.
    #[test]
    fn an_ignored_node_that_is_clickable_is_kept() {
        let mut n = node("1", "generic", "Buy", &[], Some(9));
        n.ignored = true;
        let mut reg = RefRegistry::default();
        let clicks: std::collections::HashSet<i64> = [9].into_iter().collect();
        let s = render(&[n], "u", "t", &mut reg, &clicks, fitting());
        assert!(s.tree.contains("[ref=e1]"), "{}", s.tree);
    }

    #[test]
    fn scroll_position_is_described_for_the_model() {
        let s = Scroll {
            y: 800.0,
            height: 5000.0,
            viewport: 1000.0,
        };
        let d = s.describe();
        assert!(d.contains("0.8 pages above"), "{d}");
        assert!(d.contains("3.2 below"), "{d}");
        assert!(d.contains("20%"), "{d}");
        assert!(!s.fits());

        let short = Scroll {
            y: 0.0,
            height: 700.0,
            viewport: 900.0,
        };
        assert!(short.fits());
        assert!(
            short.describe().contains("fits on screen"),
            "{}",
            short.describe()
        );
    }

    /// A long page must be bracketed, or it reads exactly like a short one and
    /// the agent stops without scrolling.
    #[test]
    fn a_long_page_is_bracketed_with_scroll_markers() {
        let mut reg = RefRegistry::default();
        let nodes = vec![node("1", "heading", "Top", &[], Some(1))];
        let mid = render(
            &nodes,
            "u",
            "t",
            &mut reg,
            &no_clicks(),
            Scroll {
                y: 900.0,
                height: 5000.0,
                viewport: 900.0,
            },
        );
        assert!(mid.tree.starts_with("[more above"), "{}", mid.tree);
        assert!(
            mid.tree.trim_end().ends_with("[more below — scroll down]"),
            "{}",
            mid.tree
        );

        let mut reg2 = RefRegistry::default();
        let bottom = render(
            &nodes,
            "u",
            "t",
            &mut reg2,
            &no_clicks(),
            Scroll {
                y: 4100.0,
                height: 5000.0,
                viewport: 900.0,
            },
        );
        assert!(
            bottom.tree.trim_end().ends_with("[end of page]"),
            "{}",
            bottom.tree
        );

        // A page that fits gets no markers at all — they would be noise.
        assert!(
            !once(&nodes).tree.contains("[more"),
            "short page should not be bracketed"
        );
    }

    #[test]
    fn find_returns_matching_lines_with_context() {
        let tree = "- a\n- b\n- button \"Submit\" [ref=e9]\n- d\n- e\n";
        let hit = find(tree, "submit", 1);
        assert!(hit.contains("ref=e9"));
        assert!(hit.contains("- b"));
        assert!(!hit.contains("- e"));
        assert!(find(tree, "nothing here", 1).is_empty());
    }
}
