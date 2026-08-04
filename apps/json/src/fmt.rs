//! Order-preserving JSON re-formatting.
//!
//! `serde_json::Value` sorts object keys (the crate is built without the
//! `preserve_order` feature on purpose — enabling it would leak into every
//! other workspace member through feature unification). A formatter that
//! silently reorders the user's keys is a bad formatter, so pretty/minify work
//! directly on the source text: validate with serde_json (for a precise
//! line/column error), then re-indent by scanning the raw bytes.

use serde_json::Value;

/// A parse failure with the position serde_json reported.
#[derive(Debug, Clone)]
pub struct JsonError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for JsonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (line {}, column {})",
            self.message, self.line, self.column
        )
    }
}

/// Parse `src`, returning the value or a positioned error.
pub fn validate(src: &str) -> Result<Value, JsonError> {
    serde_json::from_str::<Value>(src).map_err(|e| JsonError {
        message: e.to_string(),
        line: e.line(),
        column: e.column(),
    })
}

/// Re-indent `src` with `indent` spaces per level, keeping key order intact.
pub fn pretty(src: &str, indent: usize) -> Result<String, JsonError> {
    validate(src)?;
    Ok(reformat(src, Some(indent)))
}

/// Strip all insignificant whitespace, keeping key order intact.
pub fn minify(src: &str) -> Result<String, JsonError> {
    validate(src)?;
    Ok(reformat(src, None))
}

/// Pretty-print with object keys sorted A→Z. This one *does* go through
/// `Value`, because sorting is the whole point — `serde_json::Map` is a
/// BTreeMap here, so serialising it is already alphabetical.
pub fn sorted(src: &str, indent: usize) -> Result<String, JsonError> {
    let value = validate(src)?;
    let compact = serde_json::to_string(&value).map_err(|e| JsonError {
        message: e.to_string(),
        line: 0,
        column: 0,
    })?;
    Ok(reformat(&compact, Some(indent)))
}

/// Scan the (already validated) JSON text and re-emit it.
/// `indent = None` minifies; `Some(n)` pretty-prints with n spaces per level.
fn reformat(src: &str, indent: Option<usize>) -> String {
    let bytes: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len() + src.len() / 4);
    let mut depth = 0usize;
    let mut i = 0usize;

    let newline_indent = |out: &mut String, depth: usize| {
        if let Some(n) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(n * depth));
        }
    };

    while i < bytes.len() {
        let c = bytes[i];
        match c {
            '"' => {
                // Copy the string literal verbatim, honouring backslash escapes.
                out.push('"');
                i += 1;
                while i < bytes.len() {
                    let ch = bytes[i];
                    out.push(ch);
                    i += 1;
                    if ch == '\\' {
                        if i < bytes.len() {
                            out.push(bytes[i]);
                            i += 1;
                        }
                    } else if ch == '"' {
                        break;
                    }
                }
                continue;
            }
            '{' | '[' => {
                out.push(c);
                let close = if c == '{' { '}' } else { ']' };
                match next_significant(&bytes, i + 1) {
                    // Empty container stays on one line: `{}` / `[]`.
                    Some((j, ch)) if ch == close => {
                        out.push(close);
                        i = j + 1;
                        continue;
                    }
                    _ => {
                        depth += 1;
                        newline_indent(&mut out, depth);
                    }
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline_indent(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline_indent(&mut out, depth);
            }
            ':' => {
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
            }
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Index and value of the next non-whitespace char at or after `from`.
fn next_significant(bytes: &[char], from: usize) -> Option<(usize, char)> {
    (from..bytes.len())
        .find(|&j| !bytes[j].is_whitespace())
        .map(|j| (j, bytes[j]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_preserves_key_order() {
        let src = r#"{"zebra":1,"apple":2,"mango":3}"#;
        let out = pretty(src, 2).unwrap();
        let zebra = out.find("zebra").unwrap();
        let apple = out.find("apple").unwrap();
        assert!(zebra < apple, "keys were reordered:\n{out}");
        assert_eq!(
            out,
            "{\n  \"zebra\": 1,\n  \"apple\": 2,\n  \"mango\": 3\n}"
        );
    }

    #[test]
    fn pretty_handles_nesting_and_empties() {
        let src = r#"{"a":[1,{"b":[]},{}],"c":{}}"#;
        let out = pretty(src, 2).unwrap();
        assert_eq!(
            out,
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": []\n    },\n    {}\n  ],\n  \"c\": {}\n}"
        );
    }

    #[test]
    fn strings_with_braces_and_escapes_survive() {
        let src = r#"{"s":"a{b}[c],: \"q\" \\ end"}"#;
        let out = pretty(src, 2).unwrap();
        assert_eq!(out, "{\n  \"s\": \"a{b}[c],: \\\"q\\\" \\\\ end\"\n}");
        assert_eq!(minify(&out).unwrap(), src);
    }

    #[test]
    fn sorted_orders_keys_at_every_level() {
        let out = sorted(r#"{"z":1,"a":{"d":2,"b":3}}"#, 2).unwrap();
        assert_eq!(
            out,
            "{\n  \"a\": {\n    \"b\": 3,\n    \"d\": 2\n  },\n  \"z\": 1\n}"
        );
    }

    #[test]
    fn minify_round_trips() {
        let src = "{\n  \"a\" : [ 1 , 2 ],\n  \"b\": \"x y\"\n}";
        assert_eq!(minify(src).unwrap(), r#"{"a":[1,2],"b":"x y"}"#);
    }

    #[test]
    fn invalid_json_reports_position() {
        let err = pretty("{\"a\": }", 2).unwrap_err();
        assert_eq!(err.line, 1);
        assert!(err.column > 0);
    }

    #[test]
    fn unicode_is_not_split() {
        let src = r#"{"vi":"Tiếng Việt có dấu","n":1}"#;
        let out = pretty(src, 2).unwrap();
        assert!(out.contains("Tiếng Việt có dấu"));
        assert_eq!(minify(&out).unwrap(), src);
    }
}
