//! LLM auto-draw pipeline. Everything goes through the app-space-sdk bridge
//! (`llm.request`) — the app never contacts a provider directly. Two modes:
//!
//! * **mermaid** — the LLM emits Mermaid source (~10× cheaper); the editor
//!   converts it into editable shapes client-side via the embed protocol's
//!   `load` descriptor. UI-only: headless callers can't run the conversion.
//! * **xml** — the LLM emits uncompressed `<mxGraphModel>` XML directly,
//!   following draw.io's official diagram-generation rules. Works headless
//!   (MCP), so it is the default for `drawio_generate`.
//!
//! The bridge has no `temperature` and a hard output ceiling; reasoning models
//! spend budget on hidden traces first, so `finish == "length"` means the XML
//! is chopped mid-tag and MUST be treated as an error (we retry once with a
//! smaller shape budget, then repair or fail).

use app_space_sdk::SpaceClient;

pub struct GenOutcome {
    pub content: String,
    pub model: String,
    pub finish: String,
}

const XML_SYSTEM: &str = r#"You generate draw.io diagrams as mxGraph XML. Return ONLY the XML — no prose, no markdown fences, no XML comments, never compressed.

Output exactly this structure:
<mxGraphModel dx="800" dy="600" grid="1" gridSize="10" page="1" pageWidth="1169" pageHeight="826">
  <root>
    <mxCell id="0" />
    <mxCell id="1" parent="0" />
    ...diagram cells...
  </root>
</mxGraphModel>

Rules:
1. Cells id="0" and id="1" are mandatory exactly as shown; every other cell uses parent="1".
2. Every id is unique and short: n1, n2, ... for shapes, e1, e2, ... for connectors.
3. A shape: vertex="1" plus <mxGeometry x="..." y="..." width="..." height="..." as="geometry"/>. A connector: edge="1" plus source/target referencing shape ids and <mxGeometry relative="1" as="geometry"/>. Never both on one cell.
4. style is semicolon-separated key=value pairs, e.g. "rounded=1;whiteSpace=wrap;html=1;fillColor=#DAE8FC;strokeColor=#6C8EBF;".
5. Escape &, <, >, " inside value attributes as &amp; &lt; &gt; &quot;.
6. Coordinates: origin top-left, x grows right, y grows down. Space shapes ~200px apart horizontally and ~110px vertically; no overlaps. Typical shape size 160x60.
7. Palette by role — process: fillColor=#DAE8FC;strokeColor=#6C8EBF; start/end (ellipse;): #D5E8D4/#82B366; decision (rhombus;): #FFF2CC/#D6B656; error/risk: #F8CECC/#B85450; external system: #E1D5E7/#9673A6.
8. Connectors: style "edgeStyle=orthogonalEdgeStyle;rounded=1;html=1;". Label a connector via its value attribute when it clarifies (e.g. value="Yes").
9. For groups/containers use style "swimlane;" with child cells whose parent is the container id and coordinates relative to it.
10. Keep it under 40 shapes. Write every label in the same language as the user's request."#;

const MERMAID_SYSTEM: &str = r#"You generate diagrams as Mermaid source. Return ONLY the Mermaid code — no prose, no markdown fences.
Pick the syntax that fits the request: flowchart TD (or LR), sequenceDiagram, classDiagram, erDiagram, stateDiagram-v2, gantt, journey, pie.
Keep labels short; wrap labels containing special characters in double quotes. Maximum ~40 nodes.
Write every label in the same language as the user's request."#;

const EDIT_SYSTEM: &str = r#"You edit an existing draw.io diagram. You get its current mxGraphModel XML and an instruction. Return ONLY the FULL updated mxGraphModel XML — uncompressed, no prose, no markdown fences, no XML comments.
Keep the ids and positions of untouched cells exactly as they are; add, remove or restyle only what the instruction requires (new ids must not collide). Follow mxGraph rules: cells id="0" and id="1" mandatory, unique ids, vertex="1" xor edge="1", entities escaped in value attributes, style as key=value; pairs."#;

/// Character cap for sending an existing diagram back for AI editing. Beyond
/// this the model would have to reproduce more XML than the bridge output
/// ceiling allows, so we refuse with a clear message instead of truncating.
const EDIT_XML_CHAR_CAP: usize = 60_000;

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "drawio");
    }
    SpaceClient::from_env()
}

/// One-shot completion returning (text, model, finish). `finish == "length"`
/// is the caller's signal that the output hit the cap and is incomplete.
async fn bridge_llm_full(system: &str, user: &str, max_tokens: u32) -> Result<GenOutcome, String> {
    let (content, model, finish) = client()
        .llm_request_full(system, user, max_tokens, None)
        .await
        .map_err(|e| e.to_string())?;
    Ok(GenOutcome {
        content,
        model,
        finish,
    })
}

/// Generate mxGraphModel XML for `prompt`. Retries once with a smaller shape
/// budget when the output is truncated, then extracts/validates/repairs.
pub async fn generate_xml(prompt: &str, kind: &str) -> Result<(String, String), String> {
    let user = format!("Diagram type: {kind}\nRequest: {prompt}\n\nReturn the XML now.");
    let mut out = bridge_llm_full(XML_SYSTEM, &user, 16_000).await?;
    if out.finish == "length" {
        let retry = format!(
            "{user}\n\nIMPORTANT: your previous attempt exceeded the output budget and was cut off. \
             Regenerate the whole diagram with at most 20 coarser shapes and minimal styling."
        );
        out = bridge_llm_full(XML_SYSTEM, &retry, 16_000).await?;
        if out.finish == "length" {
            return Err(
                "model output truncated twice (finish=length) — ask for a simpler diagram".into(),
            );
        }
    }
    let xml = extract_mxgraph(&out.content).ok_or_else(|| {
        format!(
            "no <mxGraphModel> found in model output:\n{}",
            truncate(&out.content, 400)
        )
    })?;
    validate_mxgraph(&xml)?;
    Ok((xml, out.model))
}

/// Generate Mermaid source for `prompt` (UI mode — the editor does the
/// conversion to shapes).
pub async fn generate_mermaid(prompt: &str, kind: &str) -> Result<(String, String), String> {
    let user = format!("Diagram type: {kind}\nRequest: {prompt}\n\nReturn the Mermaid code now.");
    let mut out = bridge_llm_full(MERMAID_SYSTEM, &user, 4_000).await?;
    if out.finish == "length" {
        let retry = format!(
            "{user}\n\nIMPORTANT: keep it under 20 nodes — your previous attempt was cut off."
        );
        out = bridge_llm_full(MERMAID_SYSTEM, &retry, 4_000).await?;
        if out.finish == "length" {
            return Err(
                "model output truncated twice (finish=length) — ask for a simpler diagram".into(),
            );
        }
    }
    let code = strip_fences(&out.content).trim().to_string();
    if code.is_empty() {
        return Err("model returned empty Mermaid source".into());
    }
    Ok((code, out.model))
}

/// AI-edit an existing diagram: full-XML rewrite guided by `instruction`.
pub async fn edit_xml(current_xml: &str, instruction: &str) -> Result<(String, String), String> {
    if current_xml.chars().count() > EDIT_XML_CHAR_CAP {
        return Err(format!(
            "diagram too large for AI editing (> {EDIT_XML_CHAR_CAP} chars) — edit it manually or split it"
        ));
    }
    let user = format!(
        "Current diagram XML:\n{current_xml}\n\nInstruction: {instruction}\n\nReturn the full updated XML now."
    );
    let out = bridge_llm_full(EDIT_SYSTEM, &user, 24_000).await?;
    if out.finish == "length" {
        return Err(
            "model output truncated (finish=length) — the diagram is too large for this edit"
                .into(),
        );
    }
    let xml = extract_mxgraph(&out.content).ok_or_else(|| {
        format!(
            "no <mxGraphModel> found in model output:\n{}",
            truncate(&out.content, 400)
        )
    })?;
    validate_mxgraph(&xml)?;
    Ok((xml, out.model))
}

// ---------------------------------------------------------------------------
// Extraction / validation / repair
// ---------------------------------------------------------------------------

/// Pull an `<mxGraphModel>…</mxGraphModel>` block out of possibly-fenced,
/// possibly-chatty, possibly-truncated model output. Repairs a truncated tail
/// by dropping any half-written tag and closing the still-open elements.
pub fn extract_mxgraph(text: &str) -> Option<String> {
    let cleaned = strip_fences(text);
    let start = cleaned.find("<mxGraphModel")?;
    let body = &cleaned[start..];

    // Fast path: a balanced document ending at the model's close tag.
    if let Some(end) = body.find("</mxGraphModel>") {
        let candidate = &body[..end + "</mxGraphModel>".len()];
        if scan_tags(candidate)
            .map(|open| open.is_empty())
            .unwrap_or(false)
        {
            return Some(candidate.to_string());
        }
    }

    // Truncated: drop a trailing half-open tag (a '<' never closed by '>'),
    // then drop anything after the last complete tag, then close open elements.
    let mut kept: &str = body;
    if let Some(last_lt) = kept.rfind('<') {
        if !kept[last_lt..].contains('>') {
            kept = &kept[..last_lt];
        }
    }
    let last_gt = kept.rfind('>')?;
    let kept = &kept[..=last_gt];
    let open = scan_tags(kept)?;
    let mut out = kept.to_string();
    for name in open.iter().rev() {
        out.push_str(&format!("</{name}>"));
    }
    Some(out)
}

/// Walk the tags of an XML fragment and return the stack of still-open element
/// names (empty = balanced). Returns None on hopeless nesting (a close tag that
/// matches nothing). String-aware: quotes inside attribute values may contain
/// `>` and are skipped.
fn scan_tags(xml: &str) -> Option<Vec<String>> {
    let bytes = xml.as_bytes();
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // Comments / processing instructions / doctype.
        if xml[i..].starts_with("<!--") {
            i = xml[i..].find("-->").map(|p| i + p + 3)?;
            continue;
        }
        if xml[i..].starts_with("<?") {
            i = xml[i..].find("?>").map(|p| i + p + 2)?;
            continue;
        }
        if xml[i..].starts_with("<!") {
            i = xml[i..].find('>').map(|p| i + p + 1)?;
            continue;
        }
        // Find the tag's closing '>' respecting quoted attribute values.
        let mut j = i + 1;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let b = bytes[j];
            match quote {
                Some(q) => {
                    if b == q {
                        quote = None;
                    }
                }
                None => match b {
                    b'"' | b'\'' => quote = Some(b),
                    b'>' => break,
                    _ => {}
                },
            }
            j += 1;
        }
        if j >= bytes.len() {
            return None; // tag never closed — caller should have trimmed it
        }
        let tag = &xml[i + 1..j]; // between '<' and '>'
        if let Some(name) = tag.strip_prefix('/') {
            let name = name.trim();
            let top = stack.pop()?;
            if top != name {
                return None; // mismatched nesting — not repairable here
            }
        } else if !tag.trim_end().ends_with('/') {
            let name: String = tag
                .trim_start()
                .chars()
                .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
                .collect();
            if !name.is_empty() {
                stack.push(name);
            }
        }
        i = j + 1;
    }
    Some(stack)
}

/// Index of a tag's closing `>` respecting quoted attribute values (which may
/// legally contain `>`).
fn find_tag_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'>' => return Some(i),
                _ => {}
            },
        }
    }
    None
}

/// Extract an attribute value from a raw tag body (double or single quotes).
fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(pos) = tag.find(&needle) {
            let rest = &tag[pos + needle.len()..];
            if let Some(end) = rest.find(quote) {
                return Some(&rest[..end]);
            }
        }
    }
    None
}

/// Structural validation per draw.io's official generation rules: mandatory
/// root cells 0/1, unique ids, vertex xor edge on every diagram cell, edge
/// endpoints resolving to known cells, at least one shape.
pub fn validate_mxgraph(xml: &str) -> Result<(), String> {
    match scan_tags(xml) {
        Some(open) if open.is_empty() => {}
        Some(open) => {
            return Err(format!(
                "XML not balanced — still open: {}",
                open.join(", ")
            ))
        }
        None => return Err("XML is malformed (mismatched or unterminated tags)".into()),
    }

    let mut ids: Vec<String> = Vec::new();
    let mut vertices = 0usize;
    let mut edges: Vec<(String, Option<String>, Option<String>)> = Vec::new();

    let mut rest = xml;
    while let Some(pos) = rest.find("<mxCell") {
        let tag_body = &rest[pos..];
        let end = find_tag_end(tag_body).ok_or("unterminated <mxCell tag")?;
        let tag = &tag_body[..end];
        let id = attr_value(tag, "id").unwrap_or("").to_string();
        if id.is_empty() {
            return Err("an <mxCell> is missing its id attribute".into());
        }
        if ids.contains(&id) {
            return Err(format!("duplicate cell id: {id}"));
        }
        let is_vertex = attr_value(tag, "vertex") == Some("1");
        let is_edge = attr_value(tag, "edge") == Some("1");
        if is_vertex && is_edge {
            return Err(format!("cell {id} is both vertex and edge"));
        }
        if is_vertex {
            vertices += 1;
        }
        if is_edge {
            edges.push((
                id.clone(),
                attr_value(tag, "source").map(str::to_string),
                attr_value(tag, "target").map(str::to_string),
            ));
        }
        if !is_vertex && !is_edge && id != "0" && id != "1" {
            return Err(format!("cell {id} is neither vertex=\"1\" nor edge=\"1\""));
        }
        ids.push(id);
        rest = &rest[pos + end..];
    }

    if !ids.iter().any(|i| i == "0") || !ids.iter().any(|i| i == "1") {
        return Err("mandatory root cells id=\"0\" and id=\"1\" are missing".into());
    }
    if vertices == 0 {
        return Err("diagram has no shapes (no vertex cells)".into());
    }
    for (eid, source, target) in &edges {
        for (label, endpoint) in [("source", source), ("target", target)] {
            if let Some(ep) = endpoint {
                if !ids.iter().any(|i| i == ep) {
                    return Err(format!("edge {eid} {label} references unknown cell {ep}"));
                }
            }
        }
    }
    Ok(())
}

/// The daemon's configured LLMs via the SDK → { activeId, configs:[…] }.
pub async fn list_models() -> Result<serde_json::Value, String> {
    let (active, configs) = client().list_models().await.map_err(|e| e.to_string())?;
    let configs: Vec<serde_json::Value> = configs
        .into_iter()
        .map(|m| serde_json::json!({ "id": m.id, "modelName": m.model_name, "provider": m.provider }))
        .collect();
    Ok(serde_json::json!({ "activeId": active, "configs": configs }))
}

pub async fn set_active_model(id: &str) -> Result<(), String> {
    client()
        .set_active_model(id)
        .await
        .map_err(|e| e.to_string())
}

fn strip_fences(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // drop an optional language tag on the first line
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").to_string();
    }
    t.to_string()
}

/// Char-safe truncation — byte slicing panics on Vietnamese multibyte labels.
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

    const VALID: &str = r#"<mxGraphModel dx="800" dy="600">
  <root>
    <mxCell id="0" />
    <mxCell id="1" parent="0" />
    <mxCell id="n1" value="Start" style="ellipse;" vertex="1" parent="1">
      <mxGeometry x="40" y="40" width="120" height="60" as="geometry" />
    </mxCell>
    <mxCell id="n2" value="End" style="ellipse;" vertex="1" parent="1">
      <mxGeometry x="40" y="200" width="120" height="60" as="geometry" />
    </mxCell>
    <mxCell id="e1" style="edgeStyle=orthogonalEdgeStyle;" edge="1" source="n1" target="n2" parent="1">
      <mxGeometry relative="1" as="geometry" />
    </mxCell>
  </root>
</mxGraphModel>"#;

    #[test]
    fn extract_plain_and_fenced() {
        assert!(extract_mxgraph(VALID).is_some());
        let fenced = format!("Here is the diagram:\n```xml\n{VALID}\n```\nDone.");
        let got = extract_mxgraph(&fenced).unwrap();
        assert!(got.starts_with("<mxGraphModel"));
        assert!(got.ends_with("</mxGraphModel>"));
        assert!(validate_mxgraph(&got).is_ok());
    }

    #[test]
    fn repair_truncated_output() {
        // Cut mid-attribute inside the last cell — repair must drop the half
        // tag and close root + mxGraphModel.
        let cut = &VALID[..VALID.find("<mxCell id=\"e1\"").unwrap() + 20];
        let repaired = extract_mxgraph(cut).unwrap();
        assert!(repaired.ends_with("</root></mxGraphModel>"));
        assert!(validate_mxgraph(&repaired).is_ok());
    }

    #[test]
    fn validate_catches_structural_errors() {
        assert!(
            validate_mxgraph("<mxGraphModel><root><mxCell id=\"0\"/></root></mxGraphModel>")
                .unwrap_err()
                .contains("id=\"0\" and id=\"1\"")
        );

        let dup = VALID.replace("id=\"n2\"", "id=\"n1\"");
        assert!(validate_mxgraph(&dup).unwrap_err().contains("duplicate"));

        let bad_edge = VALID.replace("target=\"n2\"", "target=\"nope\"");
        assert!(validate_mxgraph(&bad_edge)
            .unwrap_err()
            .contains("unknown cell"));

        let both = VALID.replace("edge=\"1\" source", "edge=\"1\" vertex=\"1\" source");
        assert!(validate_mxgraph(&both).unwrap_err().contains("both"));
    }

    #[test]
    fn scan_handles_quoted_gt_and_entities() {
        let xml = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>
          <mxCell id="n1" value="a &gt; b" style="fontSize=12;" vertex="1" parent="1">
            <mxGeometry x="0" y="0" width="10" height="10" as="geometry"/>
          </mxCell></root></mxGraphModel>"#;
        assert!(validate_mxgraph(xml).is_ok());
        let tricky = r#"<a b="x > y"><c/></a>"#;
        assert_eq!(scan_tags(tricky).unwrap().len(), 0);
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("sơ đồ tiếng Việt", 5), "sơ đồ…");
    }
}
