//! Xuất tài liệu: gói Markdown và trang HTML tự chứa (tinh thần /export +
//! /preview + /reverse-preview của BA-Kit). HTML render bằng pulldown-cmark;
//! tài liệu format html (wireframe/prototype) nhúng iframe qua data URI base64
//! nên file xuất mở offline vẫn xem được. Sơ đồ mermaid giữ dạng code block
//! trong file xuất (bản render sống nằm ở UI app).

use crate::db::Db;
use crate::templates;
use pulldown_cmark::{html, Options, Parser};
use serde_json::{json, Value};

/// Mermaid UMD vendored (apps/ba/assets) — nhúng inline vào trang preview/export
/// để sơ đồ render thật mà trang vẫn tự chứa 100% (mở offline được, không CDN).
/// Chỉ nhúng khi bộ tài liệu có ít nhất một code fence mermaid (~3.4MB).
const MERMAID_JS: &str = include_str!("../assets/mermaid.min.js");

fn template_order_key(doc_type: &str, subtype: &str) -> (u8, usize) {
    // overview (glossary/convention dùng chung) luôn mở đầu bộ tài liệu, dù
    // template của nó nằm ở giai đoạn 9.
    if doc_type == "overview" {
        return (0, 0);
    }
    for (i, t) in templates::TEMPLATES.iter().enumerate() {
        if t.doc_type == doc_type && t.subtype == subtype {
            return (t.phase, i);
        }
    }
    (10, usize::MAX)
}

/// Gom tài liệu theo thứ tự giai đoạn → thứ tự template.
fn ordered_docs(db: &Db, project_id: i64, feature_id: Option<i64>) -> Vec<Value> {
    let mut docs = match feature_id {
        Some(fid) => {
            // overview cấp project đứng đầu bộ tài liệu feature (glossary/convention).
            let mut v: Vec<Value> = db
                .docs_with_content(project_id, None)
                .into_iter()
                .filter(|d| d["doc_type"] == json!("overview"))
                .collect();
            v.extend(db.docs_with_content(project_id, Some(fid)));
            v
        }
        None => {
            let mut v = db.docs_with_content(project_id, None);
            for f in db.list_features(project_id) {
                if let Some(fid) = f["id"].as_i64() {
                    v.extend(db.docs_with_content(project_id, Some(fid)));
                }
            }
            v
        }
    };
    docs.sort_by_key(|d| {
        template_order_key(
            d["doc_type"].as_str().unwrap_or(""),
            d["subtype"].as_str().unwrap_or(""),
        )
    });
    docs
}

/// Gói markdown: mọi tài liệu nối nhau, doc html bọc trong fence.
pub fn bundle_markdown(db: &Db, project_id: i64, feature_id: Option<i64>) -> Option<(String, String)> {
    let project = db.get_project(project_id)?;
    let scope_name = match feature_id {
        Some(fid) => db.get_feature(fid)?["name"].as_str().unwrap_or("").to_string(),
        None => project["name"].as_str().unwrap_or("").to_string(),
    };
    let docs = ordered_docs(db, project_id, feature_id);
    if docs.is_empty() {
        return None;
    }
    let mut out = format!(
        "# Bộ tài liệu BA — {scope_name}\n\nDự án: {} · Xuất từ BA Studio {}\n\n---\n\n",
        project["name"].as_str().unwrap_or(""),
        chrono::Local::now().format("%Y-%m-%d %H:%M")
    );
    for d in &docs {
        let title = d["title"].as_str().unwrap_or("");
        let status = d["status"].as_str().unwrap_or("");
        let ver = d["version"].as_i64().unwrap_or(1);
        out.push_str(&format!("\n\n<!-- ===== {title} (v{ver}, {status}) ===== -->\n\n"));
        let content = d["content"].as_str().unwrap_or("");
        if d["format"] == json!("html") {
            out.push_str(&format!("# {title}\n\n> Tài liệu HTML — mở bản .html để xem render.\n\n````html\n{content}\n````\n"));
        } else {
            out.push_str(content);
        }
        out.push_str("\n\n---\n");
    }
    Some((scope_name, out))
}

fn md_to_html(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::new();
    html::push_html(&mut out, parser);
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

/// CSS 2 theme qua biến: light mặc định; dark theo `prefers-color-scheme` khi
/// chưa chọn tay, và `data-theme` (nút 🌓, lưu localStorage) thắng cả hai chiều.
const PREVIEW_CSS: &str = r#"
:root {
  color-scheme: light;
  --bg: #f6f7f9; --fg: #1a1f2e; --muted: #667085;
  --card: #ffffff; --border: #e4e7ec; --border2: #d0d5dd;
  --hover: #f2f4f7; --th: #f2f4f7;
  --accent: #7c5cff; --accent-soft: #ede9fe; --accent-fg: #5b21b6;
  --toc-fg: #344054; --heading: #101828;
  --code-bg: #101828; --code-fg: #e4e7ec;
  --inline-code-bg: #f2f4f7; --inline-code-fg: #b42318;
  --quote-bg: #f9f5ff; --quote-fg: #53389e;
}
@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    color-scheme: dark;
    --bg: #101019; --fg: #e6e4f0; --muted: #8f8ba8;
    --card: #191926; --border: #2b2b3d; --border2: #3a3a52;
    --hover: #232336; --th: #232336;
    --accent: #9d85ff; --accent-soft: #2d2452; --accent-fg: #c4b5fd;
    --toc-fg: #c5c2d8; --heading: #f1efff;
    --code-bg: #0b0b12; --code-fg: #d8d6e8;
    --inline-code-bg: #2d2452; --inline-code-fg: #ffb4a8;
    --quote-bg: #221c3a; --quote-fg: #c4b5fd;
  }
}
:root[data-theme="dark"] {
  color-scheme: dark;
  --bg: #101019; --fg: #e6e4f0; --muted: #8f8ba8;
  --card: #191926; --border: #2b2b3d; --border2: #3a3a52;
  --hover: #232336; --th: #232336;
  --accent: #9d85ff; --accent-soft: #2d2452; --accent-fg: #c4b5fd;
  --toc-fg: #c5c2d8; --heading: #f1efff;
  --code-bg: #0b0b12; --code-fg: #d8d6e8;
  --inline-code-bg: #2d2452; --inline-code-fg: #ffb4a8;
  --quote-bg: #221c3a; --quote-fg: #c4b5fd;
}
* { box-sizing: border-box; }
body { font-family: -apple-system, 'Segoe UI', Roboto, sans-serif; margin: 0; color: var(--fg); background: var(--bg); }
header.page { background: #101828; color: #fff; padding: 22px 32px; }
header.page h1 { margin: 0 0 6px; font-size: 22px; }
header.page .sub { color: #98a2b3; font-size: 13px; }
header.page .bar { display: flex; gap: 10px; margin-bottom: 10px; }
.backbtn, .themebtn { display: inline-flex; align-items: center; gap: 6px; background: rgba(255,255,255,0.12); color: #fff; border: 1px solid rgba(255,255,255,0.25); border-radius: 8px; padding: 6px 14px; font-size: 13px; cursor: pointer; text-decoration: none; }
.backbtn:hover, .themebtn:hover { background: rgba(255,255,255,0.22); }
.layout { display: flex; gap: 24px; max-width: 1200px; margin: 0 auto; padding: 24px 16px; }
nav.toc { position: sticky; top: 16px; align-self: flex-start; width: 260px; flex: none; background: var(--card); border: 1px solid var(--border); border-radius: 10px; padding: 14px; font-size: 13px; max-height: 90vh; overflow: auto; }
nav.toc a { display: block; color: var(--toc-fg); text-decoration: none; padding: 3px 6px; border-radius: 6px; }
nav.toc a:hover { background: var(--hover); }
nav.toc a.active { background: var(--accent-soft); color: var(--accent-fg); font-weight: 600; }
nav.toc .phase { font-weight: 700; margin-top: 8px; color: var(--heading); }
pre.mermaid { background: #fff; color: #1a1f2e; border: 1px solid var(--border2); text-align: center; overflow-x: auto; }
pre.mermaid svg { max-width: 100%; height: auto; }
main.docs { flex: 1; min-width: 0; }
article.doc { background: var(--card); border: 1px solid var(--border); border-radius: 10px; padding: 24px 28px; margin-bottom: 22px; overflow-x: auto; }
article.doc h1 { font-size: 20px; border-bottom: 2px solid var(--accent); padding-bottom: 8px; color: var(--heading); }
article.doc h2 { font-size: 16px; margin-top: 22px; border-bottom: 1px solid var(--border); padding-bottom: 4px; color: var(--heading); }
article.doc h3 { font-size: 14px; color: var(--heading); }
table { border-collapse: collapse; width: 100%; font-size: 13px; margin: 10px 0; }
th, td { border: 1px solid var(--border2); padding: 6px 8px; text-align: left; vertical-align: top; }
th { background: var(--th); }
pre { background: var(--code-bg); color: var(--code-fg); padding: 12px; border-radius: 8px; overflow-x: auto; font-size: 12.5px; }
code { font-family: ui-monospace, Menlo, monospace; }
p code, td code, li code { background: var(--inline-code-bg); color: var(--inline-code-fg); padding: 1px 4px; border-radius: 4px; }
.badge { display: inline-block; font-size: 11px; padding: 2px 8px; border-radius: 999px; margin-left: 8px; vertical-align: middle; }
.badge.draft { background: #fef0c7; color: #93370d; }
.badge.in_review { background: #d1e9ff; color: #175cd3; }
.badge.revisions { background: #fee4e2; color: #b42318; }
.badge.approved { background: #d1fadf; color: #027a48; }
.badge.shipped { background: #e9d7fe; color: #6941c6; }
.badge.reverse { background: #fde9c8; color: #92400e; }
iframe.embedded { width: 100%; height: 640px; border: 1px solid var(--border2); border-radius: 8px; background: #fff; }
blockquote { border-left: 3px solid var(--accent); margin: 8px 0; padding: 4px 12px; color: var(--quote-fg); background: var(--quote-bg); }
@media print { nav.toc { display: none; } .layout { display: block; } header.page .bar { display: none; } }
"#;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Chạy TRONG <head>: áp theme đã lưu trước khi vẽ khung (tránh nháy màu).
const THEME_INIT_JS: &str = r#"(function(){try{var t=localStorage.getItem('ba-preview-theme');if(t==='dark'||t==='light'){document.documentElement.dataset.theme=t;}}catch(e){}})();
function baToggleTheme(){var r=document.documentElement;var cur=r.dataset.theme||(window.matchMedia&&window.matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');var next=cur==='dark'?'light':'dark';r.dataset.theme=next;try{localStorage.setItem('ba-preview-theme',next);}catch(e){}}"#;

/// JS trang preview: scrollspy đánh dấu mục TOC đang xem (bấm mục đang active
/// thì thôi không nhảy lại) + biến code fence mermaid thành sơ đồ render thật.
const PREVIEW_JS: &str = r#"(function(){
  var links = Array.prototype.slice.call(document.querySelectorAll('nav.toc a'));
  var articles = Array.prototype.slice.call(document.querySelectorAll('article.doc'));
  function setActive(id){
    links.forEach(function(a){ a.classList.toggle('active', a.getAttribute('href') === '#' + id); });
  }
  // Scrollspy bằng scroll handler thuần (IntersectionObserver không fire trong
  // webview ẩn của desktop app): active = bài có mép trên gần vạch 140px nhất
  // từ phía trên; đầu trang thì bài đầu tiên active ngay.
  function updateActive(){
    if (!articles.length) return;
    var best = null, bestTop = -Infinity;
    articles.forEach(function(a){
      var top = a.getBoundingClientRect().top;
      if (top <= 140 && top > bestTop) { best = a; bestTop = top; }
    });
    if (!best) best = articles[0];
    setActive(best.id);
  }
  // Throttle bằng setTimeout, KHÔNG dùng requestAnimationFrame — rAF không
  // chạy khi webview bị ẩn/không composite (bài học verify trong desktop app).
  var pending = false;
  window.addEventListener('scroll', function(){
    if (pending) return;
    pending = true;
    setTimeout(function(){ pending = false; updateActive(); }, 80);
  }, { passive: true });
  updateActive();
  links.forEach(function(a){
    a.addEventListener('click', function(ev){
      // Đang active mà bấm lại thì thôi — không nhảy/không đổi hash.
      if (a.classList.contains('active')) { ev.preventDefault(); return; }
      // Set active ngay khi bấm, không đợi scroll event (webview ẩn không fire).
      setActive(a.getAttribute('href').slice(1));
    });
  });
  var codes = Array.prototype.slice.call(document.querySelectorAll('code.language-mermaid'));
  if (codes.length && window.mermaid) {
    codes.forEach(function(c){
      var pre = c.parentElement;
      var d = document.createElement('pre');
      d.className = 'mermaid';
      d.textContent = c.textContent;
      pre.parentNode.replaceChild(d, pre);
    });
    window.mermaid.initialize({ startOnLoad: false, theme: 'neutral' });
    window.mermaid.run();
  }
})();"#;

/// Trang HTML tự chứa: TOC theo giai đoạn + toàn bộ tài liệu (giống
/// srs-preview.html của BA-Kit). Doc reverse có badge mức tin cậy.
/// `with_back` — thêm nút Quay lại (dùng khi mở trong app; file export thì không).
pub fn preview_html(db: &Db, project_id: i64, feature_id: Option<i64>, with_back: bool) -> Option<String> {
    let project = db.get_project(project_id)?;
    let scope_name = match feature_id {
        Some(fid) => db.get_feature(fid)?["name"].as_str().unwrap_or("").to_string(),
        None => project["name"].as_str().unwrap_or("").to_string(),
    };
    let docs = ordered_docs(db, project_id, feature_id);
    let mut toc = String::new();
    let mut body = String::new();
    let mut last_phase = 0u8;
    for d in &docs {
        let doc_type = d["doc_type"].as_str().unwrap_or("");
        let subtype = d["subtype"].as_str().unwrap_or("");
        let (phase, _) = template_order_key(doc_type, subtype);
        if phase != last_phase && (1..=9).contains(&phase) {
            toc.push_str(&format!(
                "<div class=\"phase\">{} · {}</div>",
                phase,
                esc(templates::phase_name(phase))
            ));
            last_phase = phase;
        }
        let id = d["id"].as_i64().unwrap_or(0);
        let title = d["title"].as_str().unwrap_or("");
        let status = d["status"].as_str().unwrap_or("draft");
        let anchor = format!("doc-{id}");
        toc.push_str(&format!("<a href=\"#{anchor}\">{}</a>", esc(title)));
        let reverse_badge = if doc_type == "reverse_doc" {
            "<span class=\"badge reverse\">tái lập — soi cột Tin cậy từng mục</span>"
        } else {
            ""
        };
        let head = format!(
            "<div style=\"font-size:12px;color:#667085;margin-bottom:10px\">{} · v{} <span class=\"badge {status}\">{status}</span>{reverse_badge}</div>",
            esc(doc_type),
            d["version"].as_i64().unwrap_or(1),
        );
        let content = d["content"].as_str().unwrap_or("");
        let rendered = if d["format"] == json!("html") {
            let data = format!("data:text/html;base64,{}", base64(content.as_bytes()));
            format!(
                "<h1>{}</h1><iframe class=\"embedded\" src=\"{}\" sandbox=\"allow-scripts\" title=\"{}\"></iframe><details><summary style=\"cursor:pointer;font-size:12px;color:#667085\">Xem mã nguồn HTML</summary><pre><code>{}</code></pre></details>",
                esc(title),
                data,
                esc(title),
                esc(content)
            )
        } else {
            md_to_html(content)
        };
        body.push_str(&format!(
            "<article class=\"doc\" id=\"{anchor}\">{head}{rendered}</article>"
        ));
    }
    if body.is_empty() {
        body = "<article class=\"doc\"><p>Chưa có tài liệu nào — sinh tài liệu trong app trước.</p></article>".into();
    }
    let has_mermaid = docs.iter().any(|d| {
        d["format"] != json!("html") && d["content"].as_str().unwrap_or("").contains("```mermaid")
    });
    // Mermaid ~3.4MB — chỉ nhúng khi thật sự có sơ đồ.
    let mermaid_script = if has_mermaid {
        format!("<script>{MERMAID_JS}</script>")
    } else {
        String::new()
    };
    // Back là LINK thẳng về app UI với deep-link đúng ngữ cảnh — không dùng
    // history.back(): webview desktop điều hướng iframe kiểu replace nên
    // history rỗng, back câm lặng không làm gì (bug người dùng báo).
    let back_btn = if with_back {
        let mut url = format!("/?project={project_id}");
        if let Some(fid) = feature_id {
            url.push_str(&format!("&feature={fid}"));
        }
        format!("<a class=\"backbtn\" href=\"{url}\">← Quay lại app</a>")
    } else {
        String::new()
    };
    Some(format!(
        "<!DOCTYPE html><html lang=\"vi\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{} — BA Preview</title><script>{}</script><style>{}</style></head><body><header class=\"page\"><div class=\"bar\">{}<button class=\"themebtn\" onclick=\"baToggleTheme()\" title=\"Đổi giao diện sáng/tối\">🌓 Sáng/Tối</button></div><h1>{} — Bộ tài liệu BA</h1><div class=\"sub\">Dự án {} · sinh {} · BA Studio</div></header><div class=\"layout\"><nav class=\"toc\">{}</nav><main class=\"docs\">{}</main></div>{}<script>{}</script></body></html>",
        esc(&scope_name),
        THEME_INIT_JS,
        PREVIEW_CSS,
        back_btn,
        esc(&scope_name),
        esc(project["name"].as_str().unwrap_or("")),
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        toc,
        body,
        mermaid_script,
        PREVIEW_JS
    ))
}

/// Xuất file xuống exports dir + trả nội dung để REST cho tải thẳng.
pub fn export_value(db: &Db, project_id: i64, feature_id: Option<i64>, format: &str) -> Value {
    let (scope_name, content, ext) = match format {
        "md" | "markdown" | "" => match bundle_markdown(db, project_id, feature_id) {
            Some((n, c)) => (n, c, "md"),
            None => return json!({ "error": "không có tài liệu để xuất (dự án/tính năng rỗng hoặc không tồn tại)" }),
        },
        "html" => match preview_html(db, project_id, feature_id, false) {
            Some(h) => {
                let name = match feature_id {
                    Some(fid) => db
                        .get_feature(fid)
                        .and_then(|f| f["name"].as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "feature".into()),
                    None => db
                        .get_project(project_id)
                        .and_then(|p| p["name"].as_str().map(|s| s.to_string()))
                        .unwrap_or_else(|| "project".into()),
                };
                (name, h, "html")
            }
            None => return json!({ "error": "không có tài liệu để xuất" }),
        },
        other => return json!({ "error": format!("format '{other}' không hỗ trợ — dùng md | html (PDF/Word: mở bản html rồi in/convert)") }),
    };
    let dir = crate::config::exports_dir();
    std::fs::create_dir_all(&dir).ok();
    let fname = format!(
        "{}-{}.{ext}",
        crate::db::slugify(&scope_name),
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let path = dir.join(&fname);
    if let Err(e) = std::fs::write(&path, &content) {
        return json!({ "error": format!("không ghi được file xuất: {e}") });
    }
    db.log("user", "export", &fname);
    json!({
        "ok": true,
        "file": fname,
        "path": path.to_string_lossy(),
        "bytes": content.len(),
        "format": ext,
    })
}

/// Test-only: trỏ BA_DATA_DIR vào một tempdir DUY NHẤT cho cả process — env
/// var là global, hai test set giá trị khác nhau song song sẽ race.
#[cfg(test)]
pub fn ensure_test_data_dir() {
    use std::sync::OnceLock;
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    let d = DIR.get_or_init(|| tempfile::tempdir().expect("tempdir"));
    std::env::set_var("BA_DATA_DIR", d.path());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Db) -> (i64, i64) {
        let p = db.create_project("Demo", "", "").unwrap();
        let f = db.add_feature(p, "auth", "", "P0").unwrap();
        db.upsert_document(p, Some(f), "srs", "", "SRS — auth", "# SRS\n\n| FR-auth-001 | x |\n", "markdown", "ai", "", "").unwrap();
        db.upsert_document(p, Some(f), "wireframe_html", "", "WF — auth", "<!DOCTYPE html><html><body>wf</body></html>", "html", "ai", "", "").unwrap();
        db.upsert_document(p, None, "overview", "", "Overview", "# Overview chung", "markdown", "ai", "", "").unwrap();
        (p, f)
    }

    #[test]
    fn base64_roundtrip_known_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn bundle_orders_overview_first_and_wraps_html() {
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        let (name, md) = bundle_markdown(&db, p, Some(f)).unwrap();
        assert_eq!(name, "auth");
        let ov = md.find("# Overview chung").unwrap();
        let srs = md.find("# SRS").unwrap();
        let wf = md.find("````html").unwrap();
        assert!(ov < srs, "overview phải đứng trước SRS");
        assert!(srs < wf, "wireframe (giai đoạn 5) sau SRS (giai đoạn 2)");
    }

    #[test]
    fn preview_html_selfcontained_with_iframe() {
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        let html = preview_html(&db, p, Some(f), true).unwrap();
        assert!(html.contains("<!DOCTYPE html"));
        assert!(html.contains("data:text/html;base64,"));
        assert!(html.contains("FR-auth-001"));
        // Tự chứa = không TẢI resource ngoài (mermaid.min.js nhúng inline nên
        // chuỗi http:// xuất hiện trong source của nó là vô hại — cấm là cấm
        // src/href trỏ ra ngoài).
        for pat in ["src=\"http", "src='http", "href=\"http", "href='http", "@import"] {
            assert!(!html.contains(pat), "trang preview tải resource ngoài: {pat}");
        }
        // Có nút back (with_back=true) — là LINK deep-link về app, không phải
        // history.back() (webview replace-navigation làm history rỗng).
        assert!(html.contains("class=\"backbtn\""));
        assert!(html.contains(&format!("href=\"/?project={p}&feature={f}\"")));
        assert!(html.contains("baToggleTheme"));
        assert!(html.contains("updateActive"));
    }

    #[test]
    fn preview_embeds_mermaid_only_when_diagrams_exist() {
        let db = Db::open_memory().unwrap();
        let p = db.create_project("Demo", "", "").unwrap();
        let f = db.add_feature(p, "auth", "", "P0").unwrap();
        db.upsert_document(p, Some(f), "srs", "", "SRS", "# SRS không sơ đồ", "markdown", "ai", "", "").unwrap();
        let plain = preview_html(&db, p, Some(f), false).unwrap();
        // PREVIEW_JS (bé, luôn có) chứa lời gọi mermaid.run có điều kiện — cái
        // được gate là THƯ VIỆN 3.4MB, nên đo bằng kích thước.
        assert!(plain.len() < 1_000_000, "không có sơ đồ thì đừng nhúng mermaid 3.4MB");
        // CSS luôn định nghĩa .backbtn — cái bị gate là MARKUP nút.
        assert!(!plain.contains("class=\"backbtn\""), "file export không có nút back");
        db.upsert_document(
            p, Some(f), "diagram", "erd", "ERD", "# ERD\n```mermaid\nerDiagram\n  A ||--o{ B : has\n```\n",
            "markdown", "ai", "", "",
        )
        .unwrap();
        let with = preview_html(&db, p, Some(f), false).unwrap();
        assert!(with.contains("mermaid.run"));
        assert!(with.len() > plain.len() + 1_000_000, "mermaid.min.js phải được nhúng inline");
    }

    #[test]
    fn export_writes_file() {
        super::ensure_test_data_dir();
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        let out = export_value(&db, p, Some(f), "md");
        assert_eq!(out["ok"], true);
        let path = out["path"].as_str().unwrap();
        assert!(std::path::Path::new(path).exists());
        let bad = export_value(&db, p, Some(f), "pdf");
        assert!(bad["error"].as_str().unwrap().contains("không hỗ trợ"));
    }
}
