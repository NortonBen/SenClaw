//! Minimal DOCX read/write.
//!
//! A .docx file is a zip whose `word/document.xml` holds paragraphs of the form
//! <w:p><w:r><w:t>text</w:t></w:r></w:p>. For our editor we treat a document as
//! plain text with paragraphs separated by newlines: extract on load, regenerate
//! on save. Formatting-heavy uploads round-trip as plain text — enough for an AI
//! agent to read, rewrite, and hand a working .docx back to the user.

use anyhow::{anyhow, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Read the raw `word/document.xml` from a .docx zip and collect visible text,
/// preserving paragraph boundaries as `\n\n` and line breaks as `\n`.
pub fn extract_text(docx_bytes: &[u8]) -> Result<String> {
    let mut zip = ZipArchive::new(Cursor::new(docx_bytes))?;
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .map_err(|_| anyhow!("word/document.xml missing — not a docx?"))?
        .read_to_string(&mut xml)?;

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local.ends_with(b":t") || local == b"t" {
                    in_text = true;
                } else if local.ends_with(b":br") || local == b"br" {
                    current.push('\n');
                }
            }
            Ok(Event::Empty(e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local.ends_with(b":br") || local == b"br" {
                    current.push('\n');
                } else if local.ends_with(b":tab") || local == b"tab" {
                    current.push('\t');
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name();
                let local = name.as_ref();
                if local.ends_with(b":t") || local == b"t" {
                    in_text = false;
                } else if local.ends_with(b":p") || local == b"p" {
                    paragraphs.push(std::mem::take(&mut current));
                }
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    let s = t.unescape().unwrap_or_default();
                    current.push_str(&s);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(anyhow!("xml parse: {}", e)),
        }
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }

    for (i, p) in paragraphs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(p);
    }
    Ok(out)
}

/// Build a minimal valid .docx zip from plain text. Each `\n` starts a new
/// paragraph; blank lines become empty paragraphs.
pub fn build_docx(text: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", opts)?;
        zip.write_all(CONTENT_TYPES.as_bytes())?;

        zip.add_directory("_rels/", opts)?;
        zip.start_file("_rels/.rels", opts)?;
        zip.write_all(ROOT_RELS.as_bytes())?;

        zip.add_directory("word/", opts)?;
        zip.add_directory("word/_rels/", opts)?;
        zip.start_file("word/_rels/document.xml.rels", opts)?;
        zip.write_all(DOC_RELS.as_bytes())?;

        zip.start_file("word/document.xml", opts)?;
        zip.write_all(build_document_xml(text).as_bytes())?;

        zip.finish()?;
    }
    Ok(buf)
}

fn build_document_xml(text: &str) -> String {
    let mut body = String::new();
    for para in text.split('\n') {
        body.push_str("<w:p>");
        if !para.is_empty() {
            body.push_str("<w:r><w:t xml:space=\"preserve\">");
            body.push_str(&xml_escape(para));
            body.push_str("</w:t></w:r>");
        }
        body.push_str("</w:p>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
         <w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\
         <w:body>{body}<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/></w:sectPr></w:body>\
         </w:document>",
        body = body
    )
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("</w:t><w:tab/><w:t xml:space=\"preserve\">"),
            c => out.push(c),
        }
    }
    out
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

const DOC_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_plain_text() {
        let src = "Hello world\nSecond line\n\nThird paragraph";
        let docx = build_docx(src).expect("build");
        let text = extract_text(&docx).expect("extract");
        assert_eq!(text, src);
    }

    #[test]
    fn escapes_special_chars() {
        let src = "5 < 10 & \"quoted\"";
        let docx = build_docx(src).expect("build");
        let text = extract_text(&docx).expect("extract");
        assert_eq!(text, src);
    }
}
