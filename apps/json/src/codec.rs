//! Data encodings: base64 (standard + URL-safe), hex, percent/URL, JSON string
//! escaping, MessagePack and JWT. One implementation, shared by the REST API,
//! the Ant Design UI and the MCP server — so an agent and a human always get
//! byte-identical results.

use base64::Engine;
use percent_encoding::{percent_decode_str, utf8_percent_encode, NON_ALPHANUMERIC};
use serde_json::{json, Value};

/// Every codec the app understands. `jwt` is decode-only.
pub const CODECS: [&str; 6] = ["base64", "base64url", "hex", "url", "escape", "msgpack"];

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;
const B64URL: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

pub fn encode(codec: &str, input: &str) -> Result<String, String> {
    match codec.trim().to_lowercase().as_str() {
        "base64" => Ok(B64.encode(input.as_bytes())),
        "base64url" => Ok(B64URL.encode(input.as_bytes())),
        "hex" => Ok(hex_encode(input.as_bytes())),
        "url" => Ok(utf8_percent_encode(input, NON_ALPHANUMERIC).to_string()),
        "escape" => Ok(escape_json_string(input)),
        "msgpack" => msgpack_encode(input),
        "jwt" => Err("JWT chỉ hỗ trợ giải mã (decode), không ký token".into()),
        other => Err(unsupported(other, "encode")),
    }
}

pub fn decode(codec: &str, input: &str) -> Result<String, String> {
    match codec.trim().to_lowercase().as_str() {
        "base64" => bytes_to_utf8(&base64_decode_bytes(input)?),
        "base64url" => bytes_to_utf8(&base64url_decode_bytes(input)?),
        "hex" => bytes_to_utf8(&hex_decode(input)?),
        "url" => percent_decode_str(input)
            .decode_utf8()
            .map(|c| c.to_string())
            .map_err(|e| e.to_string()),
        "escape" => unescape_json_string(input),
        "msgpack" => msgpack_decode(input).map(|v| pretty(&v)),
        "jwt" => jwt_decode(input).map(|v| pretty(&v)),
        other => Err(unsupported(other, "decode")),
    }
}

fn unsupported(codec: &str, dir: &str) -> String {
    format!(
        "codec `{codec}` không hỗ trợ {dir} — dùng một trong: {}{}",
        CODECS.join(", "),
        if dir == "decode" { ", jwt" } else { "" }
    )
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn bytes_to_utf8(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|_| "dữ liệu giải mã không phải UTF-8 hợp lệ".to_string())
}

// ---------------------------------------------------------------- base64 / hex

fn base64_decode_bytes(s: &str) -> Result<Vec<u8>, String> {
    B64.decode(s.trim())
        .map_err(|e| format!("không phải base64 hợp lệ: {e}"))
}

/// Accepts URL-safe base64 with or without `=` padding.
fn base64url_decode_bytes(s: &str) -> Result<Vec<u8>, String> {
    let t = s.trim().trim_end_matches('=');
    B64URL
        .decode(t)
        .map_err(|e| format!("không phải base64url hợp lệ: {e}"))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Tolerates whitespace and `0x` prefixes so pasted hex dumps just work.
fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s
        .replace("0x", "")
        .replace("0X", "")
        .chars()
        .filter(|c| !c.is_whitespace() && *c != ',' && *c != ':' && *c != '-')
        .collect();
    if cleaned.len() % 2 != 0 {
        return Err("chuỗi hex phải có số ký tự chẵn".into());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| format!("ký tự hex không hợp lệ tại vị trí {i}"))
        })
        .collect()
}

// ---------------------------------------------------------------- JSON string

/// Escape text so it can be pasted inside a JSON string literal (no quotes).
pub fn escape_json_string(s: &str) -> String {
    let quoted = Value::String(s.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

/// Inverse of [`escape_json_string`]; also accepts an already-quoted literal.
pub fn unescape_json_string(s: &str) -> Result<String, String> {
    let t = s.trim();
    let quoted = if t.starts_with('"') && t.ends_with('"') && t.len() >= 2 {
        t.to_string()
    } else {
        format!("\"{t}\"")
    };
    serde_json::from_str::<String>(&quoted).map_err(|e| format!("chuỗi escape không hợp lệ: {e}"))
}

// ---------------------------------------------------------------- MessagePack

/// JSON → MessagePack, returned base64-encoded (MCP carries text only).
pub fn msgpack_encode(src: &str) -> Result<String, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    let bytes = rmp_serde::to_vec_named(&v).map_err(|e| e.to_string())?;
    Ok(B64.encode(bytes))
}

/// base64-encoded MessagePack → JSON value. Trailing bytes are rejected:
/// rmp-serde stops at the end of the first value, so without this check a
/// blob of arbitrary bytes could "decode" to whatever its first byte means.
pub fn msgpack_decode(b64: &str) -> Result<Value, String> {
    use serde::Deserialize;

    let bytes = base64_decode_bytes(b64)?;
    let mut de = rmp_serde::Deserializer::new(std::io::Cursor::new(&bytes[..]));
    let value = Value::deserialize(&mut de).map_err(|e| format!("không phải MessagePack: {e}"))?;
    let consumed = de.into_inner().position() as usize;
    if consumed != bytes.len() {
        return Err(format!(
            "không phải MessagePack: thừa {} byte sau giá trị đầu tiên",
            bytes.len() - consumed
        ));
    }
    Ok(value)
}

// ---------------------------------------------------------------- JWT

/// Decode a JWT **without verifying the signature** — inspection only.
/// The result carries `header`, `payload`, the raw signature, and a
/// `signature_verified: false` flag so no caller can mistake this for auth.
pub fn jwt_decode(token: &str) -> Result<Value, String> {
    let parts: Vec<&str> = token.trim().split('.').collect();
    if parts.len() != 3 {
        return Err("JWT phải có 3 phần ngăn cách bằng dấu chấm (header.payload.signature)".into());
    }
    let part = |i: usize, name: &str| -> Result<Value, String> {
        let bytes = base64url_decode_bytes(parts[i])?;
        let text = bytes_to_utf8(&bytes)?;
        serde_json::from_str::<Value>(&text).map_err(|e| format!("{name} không phải JSON: {e}"))
    };
    let header = part(0, "header")?;
    let payload = part(1, "payload")?;
    Ok(json!({
        "header": header,
        "payload": payload,
        "signature": parts[2],
        "signature_verified": false,
        "note": "Chữ ký KHÔNG được xác minh — chỉ dùng để xem nội dung token.",
        "claims": readable_claims(&payload),
    }))
}

/// Render the well-known numeric-date claims as ISO-8601 for humans.
fn readable_claims(payload: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for key in ["iat", "exp", "nbf"] {
        if let Some(ts) = payload.get(key).and_then(|v| v.as_i64()) {
            out.insert(key.to_string(), json!(iso_from_unix(ts)));
        }
    }
    Value::Object(out)
}

fn iso_from_unix(ts: i64) -> String {
    // Days-since-epoch → civil date (Howard Hinnant's algorithm), no chrono dep.
    let (days, rem) = (ts.div_euclid(86_400), ts.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_codecs_round_trip() {
        let s = "Tiếng Việt & co?= \n<tag>";
        for codec in ["base64", "base64url", "hex", "url", "escape"] {
            let enc = encode(codec, s).unwrap();
            assert_eq!(decode(codec, &enc).unwrap(), s, "codec {codec}");
        }
    }

    #[test]
    fn hex_tolerates_formatting_and_rejects_junk() {
        assert_eq!(hex_encode(b"hi"), "6869");
        assert_eq!(decode("hex", "68 69").unwrap(), "hi");
        assert_eq!(decode("hex", "0x68:0x69").unwrap(), "hi");
        assert!(decode("hex", "686").is_err());
        assert!(decode("hex", "zz").is_err());
    }

    #[test]
    fn base64url_accepts_padded_and_unpadded() {
        assert_eq!(decode("base64url", "aGVsbG8").unwrap(), "hello");
        assert_eq!(decode("base64url", "aGVsbG8=").unwrap(), "hello");
        assert!(decode("base64", "not base64!!").is_err());
    }

    #[test]
    fn escape_produces_a_pastable_fragment() {
        assert_eq!(encode("escape", "a\"b\n").unwrap(), "a\\\"b\\n");
        assert_eq!(decode("escape", "a\\\"b\\n").unwrap(), "a\"b\n");
        // A fully quoted literal is accepted too.
        assert_eq!(decode("escape", "\"x\\ty\"").unwrap(), "x\ty");
    }

    #[test]
    fn msgpack_round_trip_and_trailing_bytes() {
        let b64 = encode("msgpack", r#"{"a":[1,2],"b":"x"}"#).unwrap();
        let back: Value = serde_json::from_str(&decode("msgpack", &b64).unwrap()).unwrap();
        assert_eq!(back, json!({"a": [1, 2], "b": "x"}));
        assert!(msgpack_decode("////").is_err(), "trailing bytes must fail");
    }

    #[test]
    fn jwt_decodes_without_claiming_verification() {
        // {"alg":"HS256","typ":"JWT"} . {"sub":"1","iat":1700000000} . sig
        let token =
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIiwiaWF0IjoxNzAwMDAwMDAwfQ.abc";
        let v = jwt_decode(token).unwrap();
        assert_eq!(v["header"]["alg"], json!("HS256"));
        assert_eq!(v["payload"]["sub"], json!("1"));
        assert_eq!(v["signature_verified"], json!(false));
        assert_eq!(v["claims"]["iat"], json!("2023-11-14T22:13:20Z"));
        assert!(jwt_decode("a.b").is_err());
        assert!(encode("jwt", token).is_err(), "signing must be refused");
    }

    #[test]
    fn unknown_codec_is_reported() {
        assert!(encode("rot13", "x").unwrap_err().contains("rot13"));
        assert!(decode("rot13", "x").unwrap_err().contains("rot13"));
    }
}
